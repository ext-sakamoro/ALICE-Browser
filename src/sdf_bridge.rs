//! ALICE-Browser × ALICE-SDF bridge
//!
//! 3D web content: SDF evaluation for WebGL/WebGPU scene rendering in-browser.
//!
//! Author: Moroya Sakamoto

/// 3D scene descriptor for browser rendering
#[derive(Debug, Clone)]
pub struct WebSdfScene {
    pub primitives: Vec<WebSdfPrimitive>,
    pub camera_pos: [f32; 3],
    pub camera_target: [f32; 3],
}

/// SDF primitive for web rendering
#[derive(Debug, Clone)]
pub enum WebSdfPrimitive {
    Sphere { center: [f32; 3], radius: f32 },
    Box { center: [f32; 3], half_extents: [f32; 3] },
    Cylinder { base: [f32; 3], radius: f32, height: f32 },
}

impl WebSdfPrimitive {
    /// Evaluate SDF at a point
    pub fn eval(&self, p: [f32; 3]) -> f32 {
        match self {
            Self::Sphere { center, radius } => {
                let dx = p[0] - center[0];
                let dy = p[1] - center[1];
                let dz = p[2] - center[2];
                (dx * dx + dy * dy + dz * dz).sqrt() - radius
            }
            Self::Box { center, half_extents } => {
                let dx = (p[0] - center[0]).abs() - half_extents[0];
                let dy = (p[1] - center[1]).abs() - half_extents[1];
                let dz = (p[2] - center[2]).abs() - half_extents[2];
                let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2) + dz.max(0.0).powi(2)).sqrt();
                let inside = dx.max(dy).max(dz).min(0.0);
                outside + inside
            }
            Self::Cylinder { base, radius, height } => {
                let dx = p[0] - base[0];
                let dz = p[2] - base[2];
                let dist_xz = (dx * dx + dz * dz).sqrt() - radius;
                let dist_y = (p[1] - base[1] - height * 0.5).abs() - height * 0.5;
                dist_xz.max(dist_y)
            }
        }
    }
}

/// Evaluate scene SDF (union of all primitives)
pub fn eval_scene(scene: &WebSdfScene, p: [f32; 3]) -> f32 {
    scene.primitives.iter()
        .map(|prim| prim.eval(p))
        .fold(f32::MAX, f32::min)
}

/// Simple sphere-trace for hit detection
pub fn sphere_trace(scene: &WebSdfScene, origin: [f32; 3], dir: [f32; 3], max_steps: u32) -> Option<f32> {
    let mut t = 0.0f32;
    for _ in 0..max_steps {
        let p = [
            origin[0] + dir[0] * t,
            origin[1] + dir[1] * t,
            origin[2] + dir[2] * t,
        ];
        let d = eval_scene(scene, p);
        if d < 0.001 { return Some(t); }
        if t > 100.0 { return None; }
        t += d;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sphere_sdf() {
        let s = WebSdfPrimitive::Sphere { center: [0.0; 3], radius: 1.0 };
        assert!((s.eval([0.0, 0.0, 0.0]) - (-1.0)).abs() < 0.001);
        assert!((s.eval([2.0, 0.0, 0.0]) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_box_sdf() {
        let b = WebSdfPrimitive::Box { center: [0.0; 3], half_extents: [1.0, 1.0, 1.0] };
        assert!(b.eval([0.0, 0.0, 0.0]) < 0.0); // Inside
        assert!(b.eval([2.0, 0.0, 0.0]) > 0.0); // Outside
    }

    #[test]
    fn test_scene_union() {
        let scene = WebSdfScene {
            primitives: vec![
                WebSdfPrimitive::Sphere { center: [0.0; 3], radius: 1.0 },
                WebSdfPrimitive::Sphere { center: [3.0, 0.0, 0.0], radius: 1.0 },
            ],
            camera_pos: [0.0, 0.0, 5.0],
            camera_target: [0.0; 3],
        };
        assert!(eval_scene(&scene, [0.0, 0.0, 0.0]) < 0.0);
        assert!(eval_scene(&scene, [3.0, 0.0, 0.0]) < 0.0);
        assert!(eval_scene(&scene, [1.5, 0.0, 0.0]) > 0.0);
    }

    #[test]
    fn test_sphere_trace_hit() {
        let scene = WebSdfScene {
            primitives: vec![
                WebSdfPrimitive::Sphere { center: [0.0, 0.0, 0.0], radius: 1.0 },
            ],
            camera_pos: [0.0, 0.0, 5.0],
            camera_target: [0.0; 3],
        };
        let hit = sphere_trace(&scene, [0.0, 0.0, 5.0], [0.0, 0.0, -1.0], 64);
        assert!(hit.is_some());
        assert!((hit.unwrap() - 4.0).abs() < 0.01);
    }

    #[test]
    fn test_sphere_trace_miss() {
        let scene = WebSdfScene {
            primitives: vec![
                WebSdfPrimitive::Sphere { center: [0.0, 0.0, 0.0], radius: 1.0 },
            ],
            camera_pos: [0.0, 0.0, 5.0],
            camera_target: [0.0; 3],
        };
        let hit = sphere_trace(&scene, [0.0, 0.0, 5.0], [0.0, 1.0, 0.0], 64);
        assert!(hit.is_none());
    }
}
