//! GPU-accelerated SDF Raymarcher for ALICE Browser.
//!
//! Uses WebGPU compute shaders to perform full per-pixel raymarching,
//! shading, and compositing on the GPU. Falls back to CPU if unavailable.
//!
//! Architecture:
//! - SDF union tree is transpiled to WGSL via ALICE-SDF's WgslShader
//! - Per-primitive SDFs are generated inline for color lookup
//! - A single compute dispatch renders all pixels in parallel

use alice_sdf::compiled::WgslShader;
use alice_sdf::prelude::*;
use wgpu::util::DeviceExt;

use crate::render::sdf_renderer::CameraParams;
use crate::render::sdf_ui::{SdfPrimitive, SdfScene};

// ── Uniform structs (must match WGSL layout exactly) ──

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    // Camera (4 × vec4 = 64 bytes)
    cam_origin: [f32; 3],
    cam_fov_factor: f32,
    cam_forward: [f32; 3],
    cam_aspect: f32,
    cam_right: [f32; 3],
    cam_max_march_dist: f32,
    cam_up: [f32; 3],
    _pad0: f32,
    // Render params (3 × vec4 = 48 bytes)
    light_dir: [f32; 3],
    fog_start: f32,
    bg_color: [f32; 3],
    fog_end: f32,
    width: u32,
    height: u32,
    _pad1: u32,
    _pad2: u32,
}

// ── GPU Renderer ──

/// Persistent GPU renderer that caches device/queue and recompiles
/// the pipeline only when the scene changes.
pub struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    cached: Option<CachedPipeline>,
    /// Number of primitives in the cached scene (used to detect changes)
    cached_prim_count: usize,
}

struct CachedPipeline {
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
}

impl GpuRenderer {
    /// Try to initialise the GPU renderer. Returns None if no GPU is available.
    pub fn new() -> Option<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))?;

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("ALICE-Browser GPU"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .ok()?;

        log::info!(
            "GPU renderer initialised: {:?}",
            adapter.get_info().name
        );

        Some(Self {
            device,
            queue,
            cached: None,
            cached_prim_count: 0,
        })
    }

    /// Render the scene to an RGBA pixel buffer using the GPU.
    pub fn render(
        &mut self,
        scene: &SdfScene,
        width: usize,
        height: usize,
        cam: &CameraParams,
    ) -> Option<Vec<u8>> {
        if scene.primitives.is_empty() {
            return None;
        }

        // Rebuild pipeline when scene changes
        if self.cached.is_none() || self.cached_prim_count != scene.primitives.len() {
            self.rebuild_pipeline(scene);
        }
        let cached = self.cached.as_ref()?;

        // Compute camera vectors
        let target = Vec3::new(cam.target[0], cam.target[1], cam.target[2]);
        let eye = target
            + Vec3::new(
                cam.distance * cam.azimuth.sin() * cam.elevation.cos(),
                cam.distance * cam.elevation.sin(),
                cam.distance * cam.azimuth.cos() * cam.elevation.cos(),
            );
        let forward = (target - eye).normalize();
        let world_up = Vec3::Y;
        let right = forward.cross(world_up).normalize();
        let up = right.cross(forward);
        let fov_factor = (50.0f32.to_radians() * 0.5).tan();

        // Scene bounds for fog / march distance
        let (mn, mx) = scene_bounds(scene);
        let extent = mx - mn;
        let max_extent = extent.x.max(extent.y.max(extent.z)).max(0.5);
        let max_march_dist = max_extent * 5.0;

        let light_dir = Vec3::new(0.5, 0.8, 0.3).normalize();

        let uniforms = Uniforms {
            cam_origin: eye.into(),
            cam_fov_factor: fov_factor,
            cam_forward: forward.into(),
            cam_aspect: width as f32 / height as f32,
            cam_right: right.into(),
            cam_max_march_dist: max_march_dist,
            cam_up: up.into(),
            _pad0: 0.0,
            light_dir: light_dir.into(),
            fog_start: max_extent * 1.5,
            bg_color: [
                scene.background_color[0],
                scene.background_color[1],
                scene.background_color[2],
            ],
            fog_end: max_extent * 4.0,
            width: width as u32,
            height: height as u32,
            _pad1: 0,
            _pad2: 0,
        };

        let pixel_count = width * height;

        // Create buffers
        let uniform_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Uniforms"),
                contents: bytemuck::bytes_of(&uniforms),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        let output_size = (pixel_count * 4) as u64; // u32 per pixel
        let output_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Output Pixels"),
            size: output_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let staging_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Staging"),
            size: output_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Bind group
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Render Bind Group"),
            layout: &cached.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output_buf.as_entire_binding(),
                },
            ],
        });

        // Dispatch
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Raymarch Pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&cached.pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            let wg_x = (width as u32 + 15) / 16;
            let wg_y = (height as u32 + 15) / 16;
            pass.dispatch_workgroups(wg_x, wg_y, 1);
        }

        encoder.copy_buffer_to_buffer(&output_buf, 0, &staging_buf, 0, output_size);
        self.queue.submit(std::iter::once(encoder.finish()));

        // Read back
        let buffer_slice = staging_buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::Maintain::Wait);

        if rx.recv().ok()?.is_err() {
            return None;
        }

        let data = buffer_slice.get_mapped_range();
        let packed: &[u32] = bytemuck::cast_slice(&data);

        // Convert packed u32 (RGBA) to [u8; 4] per pixel
        let mut pixels = vec![0u8; pixel_count * 4];
        for (i, &px) in packed.iter().enumerate() {
            let off = i * 4;
            pixels[off] = (px & 0xFF) as u8;
            pixels[off + 1] = ((px >> 8) & 0xFF) as u8;
            pixels[off + 2] = ((px >> 16) & 0xFF) as u8;
            pixels[off + 3] = ((px >> 24) & 0xFF) as u8;
        }

        drop(data);
        staging_buf.unmap();

        Some(pixels)
    }

    /// Invalidate the cached pipeline so it will be rebuilt on next render.
    pub fn invalidate(&mut self) {
        self.cached = None;
        self.cached_prim_count = 0;
    }

    // ── Pipeline construction ──

    fn rebuild_pipeline(&mut self, scene: &SdfScene) {
        let wgsl = generate_shader(scene);

        let shader_module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Raymarch Shader"),
                source: wgpu::ShaderSource::Wgsl(wgsl.into()),
            });

        let bind_group_layout =
            self.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("Render BGL"),
                    entries: &[
                        // Uniforms
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                        // Output pixels
                        wgpu::BindGroupLayoutEntry {
                            binding: 1,
                            visibility: wgpu::ShaderStages::COMPUTE,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Storage { read_only: false },
                                has_dynamic_offset: false,
                                min_binding_size: None,
                            },
                            count: None,
                        },
                    ],
                });

        let pipeline_layout =
            self.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("Render PL"),
                    bind_group_layouts: &[&bind_group_layout],
                    push_constant_ranges: &[],
                });

        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("Raymarch Pipeline"),
                layout: Some(&pipeline_layout),
                module: &shader_module,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            });

        self.cached = Some(CachedPipeline {
            pipeline,
            bind_group_layout,
        });
        self.cached_prim_count = scene.primitives.len();
        log::info!(
            "GPU pipeline rebuilt for {} primitives",
            scene.primitives.len()
        );
    }
}

// ── WGSL Shader Generation ──

/// Generate the complete WGSL compute shader for a given scene.
fn generate_shader(scene: &SdfScene) -> String {
    // 1. Build the union tree and transpile to WGSL
    let nodes: Vec<SdfNode> = scene
        .primitives
        .iter()
        .map(|p| primitive_to_node(p).0)
        .collect();
    let union_tree = build_balanced_union(&nodes);
    let sdf_shader = WgslShader::transpile(&union_tree);
    let sdf_eval_src = sdf_shader.source; // contains helpers + fn sdf_eval(...)

    // 2. Generate per-primitive SDF functions for color lookup
    let mut prim_fns = String::new();
    let mut color_body = String::new();
    color_body.push_str("    var min_d = 1e10;\n");
    color_body.push_str("    var col = vec3<f32>(0.0);\n");
    color_body.push_str("    var unlit = 0.0;\n");
    color_body.push_str("    var d: f32;\n");

    for (i, prim) in scene.primitives.iter().enumerate() {
        let (_, color) = primitive_to_node(prim);
        prim_fns.push_str(&prim_to_wgsl(prim, i));
        prim_fns.push('\n');
        let is_unlit = matches!(prim, SdfPrimitive::TextLabel { .. } | SdfPrimitive::Billboard { .. });
        let unlit_val = if is_unlit { 1.0 } else { 0.0 };
        use std::fmt::Write;
        write!(
            color_body,
            "    d = sdf_prim_{i}(p);\n    if (d < min_d) {{ min_d = d; col = vec3<f32>({:.6}, {:.6}, {:.6}); unlit = {:.1}; }}\n",
            color[0], color[1], color[2], unlit_val
        )
        .unwrap();
    }

    // 3. Compose the full shader
    format!(
        r#"// ALICE Browser — GPU Raymarcher (auto-generated)

struct Uniforms {{
    cam_origin: vec3<f32>,
    cam_fov_factor: f32,
    cam_forward: vec3<f32>,
    cam_aspect: f32,
    cam_right: vec3<f32>,
    cam_max_march_dist: f32,
    cam_up: vec3<f32>,
    _pad0: f32,
    light_dir: vec3<f32>,
    fog_start: f32,
    bg_color: vec3<f32>,
    fog_end: f32,
    width: u32,
    height: u32,
    _pad1: u32,
    _pad2: u32,
}}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<storage, read_write> output_pixels: array<u32>;

// ── SDF evaluation (transpiled by ALICE-SDF) ──
{sdf_eval_src}

// ── Per-primitive SDF for color lookup ──
{prim_fns}

fn closest_color(p: vec3<f32>) -> vec4<f32> {{
{color_body}    return vec4<f32>(col, unlit);
}}

// ── Normal estimation (central differences) ──
fn calc_normal(p: vec3<f32>) -> vec3<f32> {{
    let e = vec2<f32>(0.001, 0.0);
    return normalize(vec3<f32>(
        sdf_eval(p + e.xyy) - sdf_eval(p - e.xyy),
        sdf_eval(p + e.yxy) - sdf_eval(p - e.yxy),
        sdf_eval(p + e.yyx) - sdf_eval(p - e.yyx)
    ));
}}

// ── Toon shading step function ──
fn toon_step(n_dot_l: f32) -> f32 {{
    return smoothstep(0.48, 0.52, n_dot_l);
}}

// ── Rim lighting ──
fn rim_light(n: vec3<f32>, v: vec3<f32>) -> f32 {{
    let rim = 1.0 - max(dot(n, v), 0.0);
    return pow(rim, 3.0) * 0.6;
}}

// ── Sky color (Cyber-White: pure white with subtle gradient) ──
fn sky_color(dir: vec3<f32>) -> vec3<f32> {{
    let t = clamp(dir.y * 0.5 + 0.5, 0.0, 1.0);
    let horizon = u.bg_color;
    let zenith = u.bg_color * 0.95;
    return horizon * (1.0 - t) + zenith * t;
}}

// ── Main compute kernel: one thread per pixel ──
@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {{
    let px = gid.x;
    let py = gid.y;
    if (px >= u.width || py >= u.height) {{
        return;
    }}

    let uf = (f32(px) + 0.5) / f32(u.width) * 2.0 - 1.0;
    let vf = -((f32(py) + 0.5) / f32(u.height) * 2.0 - 1.0);

    let ray_dir = normalize(
        u.cam_forward
        + u.cam_right * (uf * u.cam_fov_factor * u.cam_aspect)
        + u.cam_up * (vf * u.cam_fov_factor)
    );

    // Sphere trace
    var t = 0.0;
    var hit = false;
    for (var i = 0u; i < 80u; i++) {{
        let p = u.cam_origin + ray_dir * t;
        let d = sdf_eval(p);
        if (d < 0.001) {{
            hit = true;
            break;
        }}
        t += d;
        if (t > u.cam_max_march_dist) {{
            break;
        }}
    }}

    var r: f32;
    var g: f32;
    var b: f32;

    if (hit) {{
        let hit_pos = u.cam_origin + ray_dir * t;
        let n = calc_normal(hit_pos);
        let light_dir = normalize(u.light_dir);
        let n_dot_l = max(dot(n, light_dir), 0.0);
        let view_dir = normalize(u.cam_origin - hit_pos);

        // Material color + unlit flag
        let mat_info = closest_color(hit_pos);
        let mat = mat_info.xyz;
        let is_unlit = mat_info.w;

        // Branch: unlit primitives (TextLabel/Billboard) skip toon shading
        var col_rim: vec3<f32>;
        if (is_unlit > 0.5) {{
            col_rim = mat;
        }} else {{
            // Toon: 2-tone shading (hard light/shadow boundary)
            let toon = toon_step(n_dot_l);

            // Shadow color: complementary dark (not black)
            let shadow_col = mat * 0.35 + vec3<f32>(0.05, 0.03, 0.08);

            // Lit = bright material, shadow = complementary dark
            let col = mat * toon + shadow_col * (1.0 - toon);

            // Rim lighting: edge glow
            let rim = rim_light(n, view_dir);
            let rim_col = mat * 0.5 + vec3<f32>(0.5, 0.5, 0.5);
            col_rim = col + rim_col * rim;
        }}

        // Distance fog (gentle, into white)
        let fog_t = clamp((t - u.fog_start) / (u.fog_end - u.fog_start), 0.0, 1.0);
        let sky = sky_color(ray_dir);
        let final_col = col_rim * (1.0 - fog_t) + sky * fog_t;

        r = clamp(final_col.x, 0.0, 1.0);
        g = clamp(final_col.y, 0.0, 1.0);
        b = clamp(final_col.z, 0.0, 1.0);
    }} else {{
        let sky = sky_color(ray_dir);
        r = clamp(sky.x, 0.0, 1.0);
        g = clamp(sky.y, 0.0, 1.0);
        b = clamp(sky.z, 0.0, 1.0);
    }}

    let idx = py * u.width + px;
    output_pixels[idx] = u32(r * 255.0)
                       | (u32(g * 255.0) << 8u)
                       | (u32(b * 255.0) << 16u)
                       | (255u << 24u);
}}
"#,
    )
}

// ── Scene helpers (duplicated from sdf_renderer to avoid pub exposure) ──

fn primitive_to_node(prim: &SdfPrimitive) -> (SdfNode, [f32; 3]) {
    match prim {
        SdfPrimitive::RoundedBox {
            center,
            size,
            radius,
            color,
        } => {
            let node = if *radius > 0.001 {
                let w = (size[0] - 2.0 * radius).max(0.001);
                let h = (size[1] - 2.0 * radius).max(0.001);
                let d = (size[2] - 2.0 * radius).max(0.001);
                SdfNode::box3d(w, h, d)
                    .round(*radius)
                    .translate(center[0], center[1], center[2])
            } else {
                SdfNode::box3d(size[0], size[1], size[2])
                    .translate(center[0], center[1], center[2])
            };
            (node, [color[0], color[1], color[2]])
        }
        SdfPrimitive::Plane {
            center,
            size,
            color,
        } => {
            let node = SdfNode::box3d(size[0], size[1], 0.04)
                .translate(center[0], center[1], center[2]);
            (node, [color[0], color[1], color[2]])
        }
        SdfPrimitive::TextLabel {
            position,
            font_size,
            color,
            text,
        } => {
            let w = text.len().min(40) as f32 * font_size * 0.5;
            let h = *font_size;
            let node = SdfNode::box3d(w, h, 0.01)
                .translate(position[0], position[1], position[2]);
            (node, [color[0], color[1], color[2]])
        }
        SdfPrimitive::Line {
            start,
            end,
            thickness,
            color,
        } => {
            let a = Vec3::new(start[0], start[1], start[2]);
            let b = Vec3::new(end[0], end[1], end[2]);
            let node = SdfNode::capsule(a, b, *thickness * 0.5);
            (node, [color[0], color[1], color[2]])
        }
        SdfPrimitive::Sphere {
            center,
            radius,
            color,
        } => {
            let node = SdfNode::sphere(*radius)
                .translate(center[0], center[1], center[2]);
            (node, [color[0], color[1], color[2]])
        }
        SdfPrimitive::Billboard {
            position,
            size,
            color,
            ..
        } => {
            let node = SdfNode::box3d(size[0], size[1], 0.005)
                .translate(position[0], position[1], position[2]);
            (node, [color[0], color[1], color[2]])
        }
        SdfPrimitive::Torus {
            center,
            major_radius,
            minor_radius,
            color,
            ..
        } => {
            let node = SdfNode::torus(*major_radius, *minor_radius)
                .translate(center[0], center[1], center[2]);
            (node, [color[0], color[1], color[2]])
        }
    }
}

fn build_balanced_union(nodes: &[SdfNode]) -> SdfNode {
    match nodes.len() {
        0 => SdfNode::sphere(0.001),
        1 => nodes[0].clone(),
        2 => nodes[0].clone().union(nodes[1].clone()),
        _ => {
            let mid = nodes.len() / 2;
            let left = build_balanced_union(&nodes[..mid]);
            let right = build_balanced_union(&nodes[mid..]);
            left.union(right)
        }
    }
}

fn scene_bounds(scene: &SdfScene) -> (Vec3, Vec3) {
    let mut mn = Vec3::splat(f32::MAX);
    let mut mx = Vec3::splat(f32::MIN);

    for prim in &scene.primitives {
        let (c, ext) = match prim {
            SdfPrimitive::RoundedBox { center, size, .. } => (
                Vec3::new(center[0], center[1], center[2]),
                Vec3::new(size[0] / 2.0, size[1] / 2.0, size[2] / 2.0),
            ),
            SdfPrimitive::Plane { center, size, .. } => (
                Vec3::new(center[0], center[1], center[2]),
                Vec3::new(size[0] / 2.0, size[1] / 2.0, 0.1),
            ),
            SdfPrimitive::TextLabel {
                position,
                font_size,
                text,
                ..
            } => {
                let w = text.len().min(40) as f32 * font_size * 0.5;
                (
                    Vec3::new(position[0], position[1], position[2]),
                    Vec3::new(w / 2.0, *font_size, 0.1),
                )
            }
            SdfPrimitive::Line { start, end, .. } => {
                let a = Vec3::new(start[0], start[1], start[2]);
                let b = Vec3::new(end[0], end[1], end[2]);
                let c = (a + b) * 0.5;
                let ext = (a - b).abs() * 0.5 + Vec3::splat(0.1);
                (c, ext)
            }
            SdfPrimitive::Sphere { center, radius, .. } => (
                Vec3::new(center[0], center[1], center[2]),
                Vec3::splat(*radius),
            ),
            SdfPrimitive::Billboard { position, size, .. } => (
                Vec3::new(position[0], position[1], position[2]),
                Vec3::new(size[0] / 2.0, size[1] / 2.0, 0.1),
            ),
            SdfPrimitive::Torus {
                center,
                major_radius,
                minor_radius,
                ..
            } => (
                Vec3::new(center[0], center[1], center[2]),
                Vec3::new(
                    major_radius + minor_radius,
                    *minor_radius,
                    major_radius + minor_radius,
                ),
            ),
        };
        mn = mn.min(c - ext);
        mx = mx.max(c + ext);
    }

    if mn.x > mx.x {
        mn = Vec3::splat(-1.0);
        mx = Vec3::splat(1.0);
    }
    (mn, mx)
}

// ── Per-primitive WGSL code generation ──

/// Generate a standalone WGSL SDF function for a single primitive.
fn prim_to_wgsl(prim: &SdfPrimitive, idx: usize) -> String {
    match prim {
        SdfPrimitive::RoundedBox {
            center,
            size,
            radius,
            ..
        } => {
            if *radius > 0.001 {
                let hx = (size[0] - 2.0 * radius).max(0.001) * 0.5;
                let hy = (size[1] - 2.0 * radius).max(0.001) * 0.5;
                let hz = (size[2] - 2.0 * radius).max(0.001) * 0.5;
                format!(
                    "fn sdf_prim_{idx}(p: vec3<f32>) -> f32 {{\
                    \n    let lp = p - vec3<f32>({cx:.6}, {cy:.6}, {cz:.6});\
                    \n    let q = abs(lp) - vec3<f32>({hx:.6}, {hy:.6}, {hz:.6});\
                    \n    return length(max(q, vec3<f32>(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0) - {r:.6};\
                    \n}}\n",
                    cx = center[0],
                    cy = center[1],
                    cz = center[2],
                    hx = hx,
                    hy = hy,
                    hz = hz,
                    r = radius,
                )
            } else {
                let hx = size[0] * 0.5;
                let hy = size[1] * 0.5;
                let hz = size[2] * 0.5;
                format!(
                    "fn sdf_prim_{idx}(p: vec3<f32>) -> f32 {{\
                    \n    let lp = p - vec3<f32>({cx:.6}, {cy:.6}, {cz:.6});\
                    \n    let q = abs(lp) - vec3<f32>({hx:.6}, {hy:.6}, {hz:.6});\
                    \n    return length(max(q, vec3<f32>(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0);\
                    \n}}\n",
                    cx = center[0],
                    cy = center[1],
                    cz = center[2],
                    hx = hx,
                    hy = hy,
                    hz = hz,
                )
            }
        }
        SdfPrimitive::Plane { center, size, .. } => {
            let hx = size[0] * 0.5;
            let hy = size[1] * 0.5;
            format!(
                "fn sdf_prim_{idx}(p: vec3<f32>) -> f32 {{\
                \n    let lp = p - vec3<f32>({cx:.6}, {cy:.6}, {cz:.6});\
                \n    let q = abs(lp) - vec3<f32>({hx:.6}, {hy:.6}, 0.020000);\
                \n    return length(max(q, vec3<f32>(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0);\
                \n}}\n",
                cx = center[0],
                cy = center[1],
                cz = center[2],
                hx = hx,
                hy = hy,
            )
        }
        SdfPrimitive::TextLabel {
            position,
            font_size,
            text,
            ..
        } => {
            let w = text.len().min(40) as f32 * font_size * 0.5;
            let hx = w * 0.5;
            let hy = font_size * 0.5;
            format!(
                "fn sdf_prim_{idx}(p: vec3<f32>) -> f32 {{\
                \n    let lp = p - vec3<f32>({px:.6}, {py:.6}, {pz:.6});\
                \n    let q = abs(lp) - vec3<f32>({hx:.6}, {hy:.6}, 0.005000);\
                \n    return length(max(q, vec3<f32>(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0);\
                \n}}\n",
                px = position[0],
                py = position[1],
                pz = position[2],
                hx = hx,
                hy = hy,
            )
        }
        SdfPrimitive::Line {
            start,
            end,
            thickness,
            ..
        } => {
            let r = thickness * 0.5;
            format!(
                "fn sdf_prim_{idx}(p: vec3<f32>) -> f32 {{\
                \n    let pa = p - vec3<f32>({ax:.6}, {ay:.6}, {az:.6});\
                \n    let ba = vec3<f32>({bx:.6}, {by:.6}, {bz:.6});\
                \n    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);\
                \n    return length(pa - ba * h) - {r:.6};\
                \n}}\n",
                ax = start[0],
                ay = start[1],
                az = start[2],
                bx = end[0] - start[0],
                by = end[1] - start[1],
                bz = end[2] - start[2],
                r = r,
            )
        }
        SdfPrimitive::Sphere {
            center,
            radius,
            ..
        } => {
            format!(
                "fn sdf_prim_{idx}(p: vec3<f32>) -> f32 {{\
                \n    return length(p - vec3<f32>({cx:.6}, {cy:.6}, {cz:.6})) - {r:.6};\
                \n}}\n",
                cx = center[0],
                cy = center[1],
                cz = center[2],
                r = radius,
            )
        }
        SdfPrimitive::Billboard {
            position,
            size,
            ..
        } => {
            let hx = size[0] * 0.5;
            let hy = size[1] * 0.5;
            format!(
                "fn sdf_prim_{idx}(p: vec3<f32>) -> f32 {{\
                \n    let lp = p - vec3<f32>({px:.6}, {py:.6}, {pz:.6});\
                \n    let q = abs(lp) - vec3<f32>({hx:.6}, {hy:.6}, 0.002500);\
                \n    return length(max(q, vec3<f32>(0.0))) + min(max(q.x, max(q.y, q.z)), 0.0);\
                \n}}\n",
                px = position[0],
                py = position[1],
                pz = position[2],
                hx = hx,
                hy = hy,
            )
        }
        SdfPrimitive::Torus {
            center,
            major_radius,
            minor_radius,
            ..
        } => {
            // SDF torus: length(vec2(length(p.xz) - R, p.y)) - r
            format!(
                "fn sdf_prim_{idx}(p: vec3<f32>) -> f32 {{\
                \n    let lp = p - vec3<f32>({cx:.6}, {cy:.6}, {cz:.6});\
                \n    let q = vec2<f32>(length(lp.xz) - {R:.6}, lp.y);\
                \n    return length(q) - {r:.6};\
                \n}}\n",
                cx = center[0],
                cy = center[1],
                cz = center[2],
                R = major_radius,
                r = minor_radius,
            )
        }
    }
}
