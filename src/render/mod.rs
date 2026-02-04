pub mod layout;
pub mod sdf_ui;
pub mod sdf_paint;
pub mod spatial;
pub mod text;
pub mod animator;
pub mod stream;

#[cfg(feature = "sdf-render")]
pub mod sdf_renderer;

#[cfg(feature = "sdf-render")]
pub mod gpu_renderer;

/// Rendering mode for the browser
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    /// Standard 2D rendering (egui widgets)
    Flat,
    /// SDF-based 2D rendering (ALICE-SDF)
    Sdf2D,
    /// 3D spatial web (ALICE-SDF + VRChat mode)
    Spatial3D,
    /// OZ Mode: orbital/planetary info-space (Cyber-White aesthetic)
    OzMode,
}

impl Default for RenderMode {
    fn default() -> Self {
        Self::Flat
    }
}
