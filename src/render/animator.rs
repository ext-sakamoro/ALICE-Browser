/// OZ Mode Celestial Mechanics Animator.
///
/// Transforms a static OZ scene into an animated one based on elapsed time.
/// - Orbit Revolution: planets/satellites revolve around their parents
/// - Floating: gentle sine-wave vertical drift
/// - Ring Rotation: Torus axis slowly rotates
/// - Ticker: headline texts flow around the outer Data Ring

use crate::render::sdf_ui::{SdfPrimitive, SdfScene};

/// Per-primitive animation metadata
#[derive(Debug, Clone)]
pub struct OzAnimMeta {
    /// DOM depth (0=sun, 1=planet, 2+=satellite)
    pub depth: u32,
    /// Orbit radius from parent
    pub orbit_radius: f32,
    /// Parent center (static, pre-animation)
    pub parent_center: [f32; 3],
    /// Base angle offset on the orbit
    pub angle_offset: f32,
    /// Orbital inclination angle
    pub inclination: f32,
    /// Kind of primitive for animation purposes
    pub kind: AnimKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnimKind {
    /// Central sun — only floats
    Sun,
    /// Orbiting body (planet, satellite, micro) — revolves + floats
    Orbiter,
    /// Connector line — endpoints recomputed from parent/child
    Connector { child_index: usize },
    /// Orbit ring (Torus) — axis slowly rotates
    Ring,
    /// Billboard label — follows its parent body
    Label { body_index: usize },
    /// Ticker text on the outer Data Ring — constant speed flow
    Ticker { ring_radius: f32 },
}

/// Animation state for an OZ scene
#[derive(Debug, Clone)]
pub struct OzAnimState {
    pub meta: Vec<OzAnimMeta>,
}

impl OzAnimState {
    pub fn new() -> Self {
        Self { meta: Vec::new() }
    }

    pub fn push(&mut self, m: OzAnimMeta) {
        self.meta.push(m);
    }
}

/// Animate the OZ scene at time `t` (seconds since start).
///
/// Returns a new SdfScene with updated positions.
pub fn animate_oz(
    base_scene: &SdfScene,
    state: &OzAnimState,
    t: f32,
    _cam_origin: [f32; 3],
) -> SdfScene {
    let mut prims = base_scene.primitives.clone();
    let float_y = (t * 0.5).sin() * 0.08;

    // First pass: compute animated positions for all orbiters
    let mut animated_centers: Vec<Option<[f32; 3]>> = vec![None; prims.len()];

    for (i, meta) in state.meta.iter().enumerate() {
        if i >= prims.len() {
            break;
        }
        match meta.kind {
            AnimKind::Sun => {
                if let SdfPrimitive::Sphere { ref mut center, .. } = prims[i] {
                    center[1] += float_y;
                    animated_centers[i] = Some(*center);
                }
            }
            AnimKind::Orbiter => {
                let speed = if meta.orbit_radius > 0.1 {
                    0.3 / meta.orbit_radius.sqrt()
                } else {
                    0.5
                };
                let angle = meta.angle_offset + t * speed;
                let incl = meta.inclination;

                let lx = meta.orbit_radius * angle.cos();
                let ly = meta.orbit_radius * incl.sin() * angle.sin();
                let lz = meta.orbit_radius * incl.cos() * angle.sin();

                let parent = resolve_parent_center(
                    meta.parent_center,
                    &animated_centers,
                    &state.meta,
                    meta.depth,
                    i,
                );

                let new_center = [
                    parent[0] + lx,
                    parent[1] + ly + float_y,
                    parent[2] + lz,
                ];
                animated_centers[i] = Some(new_center);

                match prims[i] {
                    SdfPrimitive::Sphere { ref mut center, .. } => *center = new_center,
                    _ => {}
                }
            }
            AnimKind::Ring => {
                if let SdfPrimitive::Torus {
                    ref mut center,
                    ref mut axis,
                    ..
                } = prims[i]
                {
                    let parent = resolve_parent_center(
                        meta.parent_center,
                        &animated_centers,
                        &state.meta,
                        meta.depth,
                        i,
                    );
                    *center = [parent[0], parent[1] + float_y, parent[2]];

                    let rot_speed = 0.05;
                    let base_incl = meta.inclination;
                    let wobble = t * rot_speed;
                    axis[0] = (base_incl + wobble).sin();
                    axis[1] = (base_incl + wobble).cos();
                    axis[2] = (wobble * 0.3).sin() * 0.2;
                    let len =
                        (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
                    if len > 0.001 {
                        axis[0] /= len;
                        axis[1] /= len;
                        axis[2] /= len;
                    }
                }
            }
            AnimKind::Connector { child_index } => {
                if let SdfPrimitive::Line {
                    ref mut start,
                    ref mut end,
                    ..
                } = prims[i]
                {
                    let parent = resolve_parent_center(
                        meta.parent_center,
                        &animated_centers,
                        &state.meta,
                        meta.depth,
                        i,
                    );
                    *start = parent;

                    if let Some(child_pos) =
                        animated_centers.get(child_index).and_then(|c| *c)
                    {
                        *end = child_pos;
                    }
                }
            }
            AnimKind::Label { body_index } => {
                if let Some(body_pos) =
                    animated_centers.get(body_index).and_then(|c| *c)
                {
                    match prims[i] {
                        SdfPrimitive::Billboard {
                            ref mut position, ..
                        } => {
                            position[0] = body_pos[0];
                            position[1] = body_pos[1] + 0.12;
                            position[2] = body_pos[2];
                        }
                        _ => {}
                    }
                }
            }
            AnimKind::Ticker { ring_radius } => {
                // Constant-speed flow around the outer Data Ring
                // All ticker texts rotate at the same speed (news ticker effect)
                let ticker_speed = 0.08; // slow, readable
                let angle = meta.angle_offset + t * ticker_speed;
                let hx = ring_radius * angle.cos();
                let hz = ring_radius * angle.sin();

                match prims[i] {
                    SdfPrimitive::Billboard {
                        ref mut position, ..
                    } => {
                        position[0] = hx;
                        position[1] = float_y; // gentle float
                        position[2] = hz;
                    }
                    _ => {}
                }
                animated_centers[i] = Some([hx, float_y, hz]);
            }
        }
    }

    SdfScene {
        primitives: prims,
        background_color: base_scene.background_color,
    }
}

/// Resolve the animated parent center. Falls back to static parent_center.
fn resolve_parent_center(
    static_parent: [f32; 3],
    animated: &[Option<[f32; 3]>],
    metas: &[OzAnimMeta],
    my_depth: u32,
    my_index: usize,
) -> [f32; 3] {
    if my_depth == 0 {
        return static_parent;
    }
    for j in (0..my_index).rev() {
        if j >= metas.len() {
            continue;
        }
        let m = &metas[j];
        if m.depth == my_depth.saturating_sub(1) {
            if let (AnimKind::Orbiter | AnimKind::Sun, Some(pos)) =
                (m.kind, animated.get(j).and_then(|c| *c))
            {
                let dx = (m.parent_center[0] - static_parent[0]).abs()
                    + (m.parent_center[1] - static_parent[1]).abs()
                    + (m.parent_center[2] - static_parent[2]).abs();
                if my_depth == 1 || dx < 0.01 {
                    return pos;
                }
            }
        }
    }
    static_parent
}
