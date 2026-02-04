//! SDF Renderer for ALICE Browser — powered by ALICE-SDF engine.
//!
//! Converts SdfScene primitives into alice_sdf::SdfNode trees and renders
//! via sphere-tracing with compiled SIMD evaluation + rayon parallel rows.

use alice_sdf::prelude::*;
use rayon::prelude::*;

use crate::render::sdf_ui::{SdfPrimitive, SdfScene};

// ── Camera parameters (public API, unchanged) ──

/// Camera parameters for interactive 3D navigation.
#[derive(Debug, Clone, Copy)]
pub struct CameraParams {
    /// Horizontal orbit angle in radians (0 = front)
    pub azimuth: f32,
    /// Vertical orbit angle in radians (0 = level, positive = looking down)
    pub elevation: f32,
    /// Distance from the camera to the target point
    pub distance: f32,
    /// Target point the camera looks at [x, y, z]
    pub target: [f32; 3],
}

impl Default for CameraParams {
    fn default() -> Self {
        Self {
            azimuth: 0.3,
            elevation: 0.6,
            distance: 3.0,
            target: [0.0, 0.0, 0.0],
        }
    }
}

// ── Compiled scene ──

/// A scene compiled for fast rendering: SIMD bytecode + per-primitive color map.
struct CompiledScene {
    /// Individual SdfNodes per primitive (for color lookup on hit)
    nodes: Vec<SdfNode>,
    /// Colors per primitive [r, g, b]
    colors: Vec<[f32; 3]>,
    /// Per-primitive unlit flag (true = TextLabel/Billboard, skip toon shading)
    unlit: Vec<bool>,
    /// Full scene union as a single SdfNode (for normals / shadows)
    union_tree: SdfNode,
    /// Compiled bytecode of the union tree (for fast SIMD raymarching)
    compiled: CompiledSdf,
    /// Background color
    background: [f32; 4],
}

/// Convert an SdfPrimitive to an alice_sdf::SdfNode + color.
fn primitive_to_node(prim: &SdfPrimitive) -> (SdfNode, [f32; 3]) {
    match prim {
        SdfPrimitive::RoundedBox {
            center,
            size,
            radius,
            color,
        } => {
            let node = if *radius > 0.001 {
                // Shrink box so that .round(r) brings it back to the original outer size
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
            // Billboard rendered as a thin box (camera-facing handled at scene level)
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

/// Build a compiled scene from an SdfScene.
fn compile_scene(scene: &SdfScene) -> Option<CompiledScene> {
    if scene.primitives.is_empty() {
        return None;
    }

    let mut nodes = Vec::with_capacity(scene.primitives.len());
    let mut colors = Vec::with_capacity(scene.primitives.len());
    let mut unlit = Vec::with_capacity(scene.primitives.len());

    for prim in &scene.primitives {
        let (node, color) = primitive_to_node(prim);
        nodes.push(node);
        colors.push(color);
        unlit.push(matches!(prim, SdfPrimitive::TextLabel { .. } | SdfPrimitive::Billboard { .. }));
    }

    // Build balanced union tree for better traversal
    let union_tree = build_balanced_union(&nodes);
    let compiled = CompiledSdf::compile(&union_tree);

    Some(CompiledScene {
        nodes,
        colors,
        unlit,
        union_tree,
        compiled,
        background: scene.background_color,
    })
}

/// Build a balanced binary union tree (reduces max depth for better perf).
fn build_balanced_union(nodes: &[SdfNode]) -> SdfNode {
    match nodes.len() {
        0 => SdfNode::sphere(0.001), // should not happen
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

/// Find the closest primitive color at a point. Returns (color, is_unlit).
fn closest_color(p: Vec3, scene: &CompiledScene) -> ([f32; 3], bool) {
    let mut min_d = f32::MAX;
    let mut col = [0.0f32; 3];
    let mut is_unlit = false;
    for (i, node) in scene.nodes.iter().enumerate() {
        let d = eval(node, p);
        if d < min_d {
            min_d = d;
            col = scene.colors[i];
            is_unlit = scene.unlit[i];
        }
    }
    (col, is_unlit)
}

// ── Camera ──

struct Camera {
    origin: Vec3,
    forward: Vec3,
    right: Vec3,
    up: Vec3,
    fov_factor: f32,
}

impl Camera {
    fn look_at(eye: Vec3, target: Vec3, fov_deg: f32) -> Self {
        let forward = (target - eye).normalize();
        let world_up = Vec3::Y;
        let right = forward.cross(world_up).normalize();
        let up = right.cross(forward);
        let fov_factor = (fov_deg.to_radians() * 0.5).tan();
        Self {
            origin: eye,
            forward,
            right,
            up,
            fov_factor,
        }
    }

    fn ray(&self, u: f32, v: f32, aspect: f32) -> Vec3 {
        (self.forward
            + self.right * (u * self.fov_factor * aspect)
            + self.up * (v * self.fov_factor))
            .normalize()
    }
}

// ── Scene bounds ──

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

// ── Sky ──

fn sky_color(dir: Vec3, bg: [f32; 4]) -> [f32; 3] {
    let t = (dir.y * 0.5 + 0.5).clamp(0.0, 1.0);
    let horizon = [bg[0], bg[1], bg[2]];
    let zenith = [
        (bg[0] * 0.5).min(0.4),
        (bg[1] * 0.6).min(0.5),
        (bg[2] * 0.8).min(0.9),
    ];
    [
        horizon[0] * (1.0 - t) + zenith[0] * t,
        horizon[1] * (1.0 - t) + zenith[1] * t,
        horizon[2] * (1.0 - t) + zenith[2] * t,
    ]
}

// ── Public rendering API ──

/// Render an SDF scene with interactive camera parameters.
pub fn render_sdf_interactive(
    scene: &SdfScene,
    width: usize,
    height: usize,
    cam: &CameraParams,
) -> Option<Vec<u8>> {
    if scene.primitives.is_empty() {
        return None;
    }

    let target = Vec3::new(cam.target[0], cam.target[1], cam.target[2]);
    let eye = target
        + Vec3::new(
            cam.distance * cam.azimuth.sin() * cam.elevation.cos(),
            cam.distance * cam.elevation.sin(),
            cam.distance * cam.azimuth.cos() * cam.elevation.cos(),
        );

    let camera = Camera::look_at(eye, target, 50.0);
    render_scene(scene, width, height, &camera)
}

/// Render an SDF scene to an RGBA pixel buffer (auto-framing).
pub fn render_sdf_image(
    scene: &SdfScene,
    width: usize,
    height: usize,
    spatial: bool,
) -> Option<Vec<u8>> {
    if scene.primitives.is_empty() {
        return None;
    }

    let (mn, mx) = scene_bounds(scene);
    let center = (mn + mx) * 0.5;
    let extent = mx - mn;
    let max_extent = extent.x.max(extent.y.max(extent.z)).max(0.5);

    let camera = if spatial {
        let eye = center + Vec3::new(max_extent * 0.3, max_extent * 0.8, max_extent * 1.2);
        Camera::look_at(eye, center, 50.0)
    } else {
        let eye = center + Vec3::new(0.0, max_extent * 0.4, max_extent * 1.8);
        Camera::look_at(eye, center, 45.0)
    };

    render_scene(scene, width, height, &camera)
}

/// Compute initial camera params that auto-frame the scene.
pub fn auto_camera(scene: &SdfScene) -> CameraParams {
    let (mn, mx) = scene_bounds(scene);
    let center = (mn + mx) * 0.5;
    let extent = mx - mn;
    let max_ext = extent.x.max(extent.y.max(extent.z)).max(0.5);

    CameraParams {
        azimuth: 0.3,
        elevation: 0.5,
        distance: max_ext * 1.8,
        target: [center.x, center.y, center.z],
    }
}

// ── Core rendering (rayon-parallel rows, compiled SIMD eval) ──

fn render_scene(
    scene_data: &SdfScene,
    width: usize,
    height: usize,
    camera: &Camera,
) -> Option<Vec<u8>> {
    let compiled = compile_scene(scene_data)?;

    let (mn, mx) = scene_bounds(scene_data);
    let extent = mx - mn;
    let max_extent = extent.x.max(extent.y.max(extent.z)).max(0.5);
    let max_march_dist = max_extent * 5.0;

    let light_dir = Vec3::new(0.5, 0.8, 0.3).normalize();
    let light_col = Vec3::new(1.0, 0.98, 0.95);
    let ambient = Vec3::new(0.15, 0.17, 0.22);

    let aspect = width as f32 / height as f32;

    let mut pixels = vec![0u8; width * height * 4];
    let row_size = width * 4;

    // Parallel row rendering via rayon
    pixels
        .par_chunks_exact_mut(row_size)
        .enumerate()
        .for_each(|(py, row_buf)| {
            let v = -((py as f32 + 0.5) / height as f32 * 2.0 - 1.0);

            for px in 0..width {
                let u = (px as f32 + 0.5) / width as f32 * 2.0 - 1.0;
                let ray_dir = camera.ray(u, v, aspect);

                // Sphere-trace using compiled SIMD evaluation
                let mut t = 0.0f32;
                let mut hit = false;
                let mut hit_color = [0.0f32; 3];
                let mut hit_unlit = false;

                for _ in 0..80 {
                    let p = camera.origin + ray_dir * t;
                    let d = eval_compiled(&compiled.compiled, p);
                    if d < 0.001 {
                        hit = true;
                        let (c, u) = closest_color(p, &compiled);
                        hit_color = c;
                        hit_unlit = u;
                        break;
                    }
                    t += d;
                    if t > max_march_dist {
                        break;
                    }
                }

                let (r, g, b) = if hit {
                    let hit_pos = camera.origin + ray_dir * t;
                    let mat = Vec3::new(hit_color[0], hit_color[1], hit_color[2]);

                    let col_rim = if hit_unlit {
                        // Unlit: TextLabel/Billboard — use base color directly
                        mat
                    } else {
                        // Surface normal
                        let n = normal(&compiled.union_tree, hit_pos, 0.001);
                        let n_dot_l = n.dot(light_dir).max(0.0);
                        let view_dir = (camera.origin - hit_pos).normalize();

                        // Toon: 2-tone hard boundary
                        let toon = if n_dot_l > 0.5 { 1.0 } else { 0.0 };

                        // Shadow color: complementary dark
                        let shadow_col = mat * 0.35 + Vec3::new(0.05, 0.03, 0.08);

                        let col = mat * toon + shadow_col * (1.0 - toon);

                        // Rim lighting
                        let rim = (1.0 - n.dot(view_dir).max(0.0)).powf(3.0) * 0.6;
                        let rim_col = mat * 0.5 + Vec3::splat(0.5);
                        col + rim_col * rim
                    };

                    // Distance fog
                    let fog_start = max_extent * 1.5;
                    let fog_end = max_extent * 4.0;
                    let fog_t = ((t - fog_start) / (fog_end - fog_start)).clamp(0.0, 1.0);
                    let sky = sky_color(ray_dir, compiled.background);
                    let fog_col = Vec3::new(sky[0], sky[1], sky[2]);
                    let final_col = col_rim * (1.0 - fog_t) + fog_col * fog_t;

                    (
                        (final_col.x.clamp(0.0, 1.0) * 255.0) as u8,
                        (final_col.y.clamp(0.0, 1.0) * 255.0) as u8,
                        (final_col.z.clamp(0.0, 1.0) * 255.0) as u8,
                    )
                } else {
                    let sky = sky_color(ray_dir, compiled.background);
                    (
                        (sky[0].clamp(0.0, 1.0) * 255.0) as u8,
                        (sky[1].clamp(0.0, 1.0) * 255.0) as u8,
                        (sky[2].clamp(0.0, 1.0) * 255.0) as u8,
                    )
                };

                let idx = px * 4;
                row_buf[idx] = r;
                row_buf[idx + 1] = g;
                row_buf[idx + 2] = b;
                row_buf[idx + 3] = 255;
            }
        });

    Some(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::sdf_ui::{SdfPrimitive, SdfScene};

    #[test]
    fn renders_single_box() {
        let scene = SdfScene {
            primitives: vec![SdfPrimitive::RoundedBox {
                center: [0.0, 0.0, 0.0],
                size: [1.0, 1.0, 1.0],
                radius: 0.1,
                color: [0.8, 0.2, 0.2, 1.0],
            }],
            background_color: [0.1, 0.1, 0.1, 1.0],
        };
        let pixels = render_sdf_image(&scene, 64, 48, false).unwrap();
        assert_eq!(pixels.len(), 64 * 48 * 4);
        let has_bright = pixels.chunks(4).any(|px| px[0] > 50);
        assert!(has_bright, "Should have rendered something visible");
    }

    #[test]
    fn renders_spatial_mode() {
        let scene = SdfScene {
            primitives: vec![
                SdfPrimitive::RoundedBox {
                    center: [0.0, 0.0, 0.0],
                    size: [2.0, 0.1, 2.0],
                    radius: 0.0,
                    color: [0.9, 0.9, 0.9, 1.0],
                },
                SdfPrimitive::RoundedBox {
                    center: [0.5, 1.0, -0.5],
                    size: [0.5, 2.0, 0.1],
                    radius: 0.02,
                    color: [0.2, 0.6, 1.0, 0.8],
                },
            ],
            background_color: [0.6, 0.8, 1.0, 1.0],
        };
        let pixels = render_sdf_image(&scene, 64, 48, true).unwrap();
        assert_eq!(pixels.len(), 64 * 48 * 4);
    }

    #[test]
    fn empty_scene_returns_none() {
        let scene = SdfScene {
            primitives: vec![],
            background_color: [0.0; 4],
        };
        assert!(render_sdf_image(&scene, 64, 48, false).is_none());
    }

    #[test]
    fn auto_camera_frames_scene() {
        let scene = SdfScene {
            primitives: vec![SdfPrimitive::RoundedBox {
                center: [1.0, 0.5, -1.0],
                size: [2.0, 1.0, 2.0],
                radius: 0.1,
                color: [0.5, 0.5, 0.5, 1.0],
            }],
            background_color: [0.5, 0.7, 0.9, 1.0],
        };
        let cam = auto_camera(&scene);
        assert!(cam.distance > 0.5, "Camera should be at reasonable distance");
        assert!((cam.target[0] - 1.0).abs() < 0.5, "Target should be near scene center");
    }

    #[test]
    fn interactive_render_works() {
        let scene = SdfScene {
            primitives: vec![SdfPrimitive::RoundedBox {
                center: [0.0, 0.0, 0.0],
                size: [1.0, 1.0, 1.0],
                radius: 0.0,
                color: [0.8, 0.2, 0.2, 1.0],
            }],
            background_color: [0.1, 0.1, 0.1, 1.0],
        };
        let cam = CameraParams::default();
        let pixels = render_sdf_interactive(&scene, 32, 24, &cam).unwrap();
        assert_eq!(pixels.len(), 32 * 24 * 4);
    }
}
