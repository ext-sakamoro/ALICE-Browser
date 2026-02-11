//! ALICE-Browser × ALICE-View bridge
//!
//! SDF-based UI rendering for resolution-independent browser elements.
//!
//! Author: Moroya Sakamoto

/// UI element type for SDF rendering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UiElementKind {
    RoundedRect = 0,
    Circle = 1,
    Shadow = 2,
    Border = 3,
    Text = 4,
}

/// SDF UI render command
#[derive(Debug, Clone)]
pub struct SdfUiCommand {
    pub kind: UiElementKind,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub corner_radius: f32,
    pub color_rgba: [u8; 4],
}

/// SDF UI render batch
pub struct SdfUiBatch {
    commands: Vec<SdfUiCommand>,
    pub total_elements: u64,
}

impl SdfUiBatch {
    pub fn new() -> Self {
        Self { commands: Vec::new(), total_elements: 0 }
    }

    /// Add a rounded rectangle (CSS border-radius equivalent)
    pub fn add_rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: [u8; 4]) {
        self.commands.push(SdfUiCommand {
            kind: UiElementKind::RoundedRect,
            x, y, width: w, height: h,
            corner_radius: radius,
            color_rgba: color,
        });
        self.total_elements += 1;
    }

    /// Add a circle element
    pub fn add_circle(&mut self, cx: f32, cy: f32, radius: f32, color: [u8; 4]) {
        self.commands.push(SdfUiCommand {
            kind: UiElementKind::Circle,
            x: cx - radius, y: cy - radius,
            width: radius * 2.0, height: radius * 2.0,
            corner_radius: radius,
            color_rgba: color,
        });
        self.total_elements += 1;
    }

    /// Add a box-shadow
    pub fn add_shadow(&mut self, x: f32, y: f32, w: f32, h: f32, blur: f32, color: [u8; 4]) {
        self.commands.push(SdfUiCommand {
            kind: UiElementKind::Shadow,
            x, y, width: w, height: h,
            corner_radius: blur,
            color_rgba: color,
        });
        self.total_elements += 1;
    }

    /// Get all commands for GPU submission
    pub fn commands(&self) -> &[SdfUiCommand] {
        &self.commands
    }

    /// Clear batch for next frame
    pub fn clear(&mut self) {
        self.commands.clear();
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }
}

/// Evaluate rounded-rect SDF at a point
pub fn sdf_rounded_rect(px: f32, py: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let dx = (px - cx).abs() - hw + r;
    let dy = (py - cy).abs() - hh + r;
    let outside = (dx.max(0.0) * dx.max(0.0) + dy.max(0.0) * dy.max(0.0)).sqrt();
    let inside = dx.max(dy).min(0.0);
    outside + inside - r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_add_elements() {
        let mut batch = SdfUiBatch::new();
        batch.add_rounded_rect(0.0, 0.0, 100.0, 50.0, 8.0, [255, 255, 255, 255]);
        batch.add_circle(50.0, 50.0, 25.0, [255, 0, 0, 255]);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch.total_elements, 2);
    }

    #[test]
    fn test_batch_clear() {
        let mut batch = SdfUiBatch::new();
        batch.add_rounded_rect(0.0, 0.0, 10.0, 10.0, 2.0, [0; 4]);
        batch.clear();
        assert!(batch.is_empty());
        assert_eq!(batch.total_elements, 1); // total_elements persists
    }

    #[test]
    fn test_sdf_rounded_rect_center() {
        // Center of a 100x100 rect should be inside (negative SDF)
        let d = sdf_rounded_rect(50.0, 50.0, 50.0, 50.0, 50.0, 50.0, 0.0);
        assert!(d < 0.0);
    }

    #[test]
    fn test_sdf_rounded_rect_outside() {
        // Point far outside should be positive
        let d = sdf_rounded_rect(200.0, 200.0, 50.0, 50.0, 50.0, 50.0, 0.0);
        assert!(d > 0.0);
    }

    #[test]
    fn test_shadow_element() {
        let mut batch = SdfUiBatch::new();
        batch.add_shadow(10.0, 10.0, 100.0, 50.0, 4.0, [0, 0, 0, 128]);
        assert_eq!(batch.commands()[0].kind, UiElementKind::Shadow);
    }
}
