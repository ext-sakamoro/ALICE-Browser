/// MSDF text rendering data structures for ALICE Browser.
///
/// Defines the data model for Multi-channel Signed Distance Field font rendering.
/// Actual GPU rendering is handled by the shader pipeline; this module provides
/// layout computation and glyph metadata.

/// A single glyph in the MSDF atlas.
#[derive(Debug, Clone, Copy)]
pub struct MsdfGlyph {
    /// Unicode codepoint
    pub codepoint: char,
    /// UV coordinates in the atlas texture [u_min, v_min, u_max, v_max]
    pub uv: [f32; 4],
    /// Horizontal advance (em units)
    pub advance: f32,
    /// Glyph size in em units [width, height]
    pub size: [f32; 2],
    /// Bearing offset [x, y] from baseline
    pub bearing: [f32; 2],
}

/// MSDF font atlas metadata.
#[derive(Debug, Clone)]
pub struct MsdfAtlas {
    /// Atlas texture dimensions [width, height] in pixels
    pub texture_size: [u32; 2],
    /// Font size used to generate the atlas (pixels per em)
    pub font_size: f32,
    /// Distance field range in pixels
    pub distance_range: f32,
    /// Glyph table (ASCII subset for now)
    pub glyphs: Vec<MsdfGlyph>,
    /// Line height multiplier
    pub line_height: f32,
}

impl MsdfAtlas {
    /// Create a default monospace-approximation atlas for basic ASCII.
    /// Real MSDF atlas would be loaded from a generated font texture.
    pub fn default_ascii() -> Self {
        let mut glyphs = Vec::with_capacity(96);
        for i in 32u8..=126 {
            let ch = i as char;
            let col = (i - 32) % 16;
            let row = (i - 32) / 16;
            let cell_w = 1.0 / 16.0;
            let cell_h = 1.0 / 6.0;
            glyphs.push(MsdfGlyph {
                codepoint: ch,
                uv: [
                    col as f32 * cell_w,
                    row as f32 * cell_h,
                    (col + 1) as f32 * cell_w,
                    (row + 1) as f32 * cell_h,
                ],
                advance: 0.6,
                size: [0.55, 1.0],
                bearing: [0.025, 0.0],
            });
        }
        Self {
            texture_size: [512, 192],
            font_size: 32.0,
            distance_range: 4.0,
            glyphs,
            line_height: 1.2,
        }
    }

    /// Look up a glyph by character.
    pub fn glyph(&self, ch: char) -> Option<&MsdfGlyph> {
        self.glyphs.iter().find(|g| g.codepoint == ch)
    }
}

/// A positioned text node in 3D space.
#[derive(Debug, Clone)]
pub struct SdfTextNode {
    /// Center position in world space
    pub position: [f32; 3],
    /// Text content
    pub text: String,
    /// Font size in world units (meters)
    pub font_size: f32,
    /// Text color [r, g, b, a]
    pub color: [f32; 4],
    /// If true, text always faces the camera (billboard mode)
    pub billboard: bool,
}

/// Computed quad for a single character, ready for rendering.
#[derive(Debug, Clone, Copy)]
pub struct TextQuad {
    /// World-space center of this character quad
    pub center: [f32; 3],
    /// Size of the quad [width, height] in world units
    pub size: [f32; 2],
    /// UV coordinates in the atlas [u_min, v_min, u_max, v_max]
    pub uv: [f32; 4],
    /// Color [r, g, b, a]
    pub color: [f32; 4],
}

/// Generate character quads for a text string.
///
/// Each character becomes a positioned quad with atlas UV coordinates.
/// The text is laid out left-to-right starting from `origin`.
pub fn generate_text_quads(
    node: &SdfTextNode,
    atlas: &MsdfAtlas,
) -> Vec<TextQuad> {
    let mut quads = Vec::with_capacity(node.text.len());
    let scale = node.font_size;
    let total_width: f32 = node
        .text
        .chars()
        .filter_map(|ch| atlas.glyph(ch))
        .map(|g| g.advance * scale)
        .sum();
    let mut cursor_x = node.position[0] - total_width * 0.5;

    for ch in node.text.chars() {
        if let Some(glyph) = atlas.glyph(ch) {
            let w = glyph.size[0] * scale;
            let h = glyph.size[1] * scale;
            let x = cursor_x + glyph.bearing[0] * scale + w * 0.5;
            let y = node.position[1] + glyph.bearing[1] * scale;

            quads.push(TextQuad {
                center: [x, y, node.position[2]],
                size: [w, h],
                uv: glyph.uv,
                color: node.color,
            });

            cursor_x += glyph.advance * scale;
        }
    }

    quads
}
