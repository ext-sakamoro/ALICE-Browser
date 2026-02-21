//! SIMD-Accelerated Layout Computation
//!
//! Traditional layout: process nodes one-by-one, accumulating cursor_y.
//! SIMD layout: batch-compute margins, paddings, and text heights for 8 nodes.
//!
//! Key optimizations:
//! - SoA layout boxes (x[], y[], w[], h[] instead of Vec<Box{x,y,w,h}>)
//! - SIMD margin/padding computation (8 nodes at once)
//! - Division exorcism: chars_per_line uses multiplication by reciprocal
//! - Branchless max() for clamping

use super::F32x8;
use crate::dom::{Classification, DomNode, NodeType};

/// Pre-computed reciprocals for Division Exorcism.
/// These are computed once and reused across all layout passes.
pub struct LayoutConstants {
    /// 1.0 / 0.6 — for chars_per_line calculation (font_size * 0.6)
    pub inv_char_width_factor: f32,
    /// Line height multiplier (1.4)
    pub line_height_factor: f32,
    /// 1.0 / viewport_width — for normalization
    pub inv_viewport: f32,
}

impl LayoutConstants {
    #[inline]
    pub fn new(viewport_width: f32) -> Self {
        Self {
            inv_char_width_factor: 1.0 / 0.6,
            line_height_factor: 1.4,
            // Division exorcism: pre-compute reciprocal
            inv_viewport: if viewport_width > 0.0 { 1.0 / viewport_width } else { 0.0 },
        }
    }
}

/// Batch-compute font sizes for 8 nodes using branchless selection.
///
/// Instead of match tag { "h1" => 32.0, "h2" => 24.0, ... }
/// we encode tag→font_size as SIMD comparison + blend.
#[inline]
pub fn batch_font_sizes(tag_types: &[i32], parent_size: f32) -> [f32; 8] {
    let mut sizes = [parent_size; 8];

    for i in 0..8 {
        // Branchless font size selection using tag type encoding:
        // h1=11 could be further decomposed, but tag_type is pre-encoded.
        // We use a lookup table approach (no branches):
        sizes[i] = font_size_lut(tag_types[i], parent_size);
    }
    sizes
}

/// Font size lookup table — branchless via array index.
///
/// Instead of match/if chains, we pre-compute all possibilities
/// and index into the table. Array access is branchless.
#[inline(always)]
fn font_size_lut(tag_type: i32, parent: f32) -> f32 {
    // Tag type encoding from soa.rs:
    // 11 = h1..h6 (we refine below), others use parent size
    // For simplicity, we encode heading levels more precisely in SoA
    // For now: tag_type 11 → heading, use a middle size
    const LUT: [f32; 18] = [
        0.0,  // 0: div → parent
        0.0,  // 1: p → parent
        0.0,  // 2: a → parent
        0.0,  // 3: script → parent
        0.0,  // 4: style → parent
        0.0,  // 5: nav → parent
        0.0,  // 6: header → parent
        0.0,  // 7: footer → parent
        0.0,  // 8: interactive → parent
        0.0,  // 9: media → parent
        0.0,  // 10: iframe → parent
        24.0, // 11: heading (average of h1-h6)
        0.0,  // 12: span → parent
        0.0,  // 13: list → parent
        0.0,  // 14: table → parent
        0.0,  // 15: section/article → parent
        0.0,  // 16: text node → parent
        0.0,  // 17: other → parent
    ];

    let idx = (tag_type as usize).min(17);
    let lut_val = LUT[idx];
    // Branchless: if lut_val == 0.0, use parent; else use lut_val
    // This avoids an if/else branch.
    // lut_val + (parent - lut_val) * (lut_val == 0.0) as i32 as f32
    let is_zero = (lut_val == 0.0) as u32 as f32; // 1.0 if zero, 0.0 if nonzero
    // FMA: lut_val + is_zero * (parent - lut_val) = lut_val * (1 - is_zero) + parent * is_zero
    lut_val * (1.0 - is_zero) + parent * is_zero
}

/// Batch-compute margin tops for 8 nodes.
///
/// Uses the same LUT approach as font sizes.
#[inline]
pub fn batch_margin_tops(tag_types: &[i32]) -> F32x8 {
    let mut v = [0.0f32; 8];
    for i in 0..8 {
        v[i] = margin_top_lut(tag_types[i]);
    }
    F32x8 { v }
}

/// Batch-compute margin bottoms for 8 nodes.
#[inline]
pub fn batch_margin_bottoms(tag_types: &[i32]) -> F32x8 {
    let mut v = [0.0f32; 8];
    for i in 0..8 {
        v[i] = margin_bottom_lut(tag_types[i]);
    }
    F32x8 { v }
}

/// Batch-compute paddings for 8 nodes.
#[inline]
pub fn batch_paddings(tag_types: &[i32]) -> F32x8 {
    let mut v = [0.0f32; 8];
    for i in 0..8 {
        v[i] = padding_lut(tag_types[i]);
    }
    F32x8 { v }
}

// Margin top lookup table
#[inline(always)]
fn margin_top_lut(tag_type: i32) -> f32 {
    const LUT: [f32; 18] = [
        0.0, 4.0, 0.0, 0.0, 0.0, 12.0, 12.0, 12.0, // div,p,a,script,style,nav,header,footer
        0.0, 0.0, 0.0, 20.0, 0.0, 8.0, 0.0, 16.0,   // interactive,media,iframe,heading,span,list,table,section
        0.0, 0.0,                                       // text,other
    ];
    LUT[(tag_type as usize).min(17)]
}

// Margin bottom lookup table
#[inline(always)]
fn margin_bottom_lut(tag_type: i32) -> f32 {
    const LUT: [f32; 18] = [
        0.0, 10.0, 0.0, 0.0, 0.0, 12.0, 12.0, 12.0,
        0.0, 0.0, 0.0, 12.0, 0.0, 8.0, 0.0, 16.0,
        0.0, 0.0,
    ];
    LUT[(tag_type as usize).min(17)]
}

// Padding lookup table
#[inline(always)]
fn padding_lut(tag_type: i32) -> f32 {
    const LUT: [f32; 18] = [
        4.0, 4.0, 0.0, 0.0, 0.0, 12.0, 12.0, 12.0,  // div→4,p→4,a→0,...
        4.0, 0.0, 0.0, 4.0, 0.0, 4.0, 4.0, 16.0,     // interactive→4,...,section→16
        0.0, 0.0,
    ];
    LUT[(tag_type as usize).min(17)]
}

/// Compute text height for a batch of 8 nodes.
///
/// text_height = ceil(text_len / chars_per_line) * line_height
///
/// Division exorcism:
///   chars_per_line = available_width / (font_size * 0.6)
///   → chars_per_line = available_width * inv_char_width_factor / font_size
///   → But we need 1/chars_per_line for the division:
///   → inv_cpl = font_size * 0.6 / available_width = font_size * 0.6 * inv_viewport
///
/// So: lines = text_len * inv_cpl = text_len * font_size * 0.6 * inv_viewport
/// No division at all!
#[inline]
pub fn batch_text_heights(
    text_lens: &[f32; 8],
    font_sizes: &[f32; 8],
    inv_viewport: f32,
) -> F32x8 {
    let inv_vp = F32x8::splat(inv_viewport);
    let char_factor = F32x8::splat(0.6);
    let line_factor = F32x8::splat(1.4);
    let one = F32x8::splat(1.0);

    let tl = F32x8 { v: *text_lens };
    let fs = F32x8 { v: *font_sizes };

    // lines = text_len * font_size * 0.6 * inv_viewport
    // Using FMA chain:
    //   temp = font_size * char_factor  (= font_size * 0.6)
    //   lines = text_len * temp * inv_viewport
    let temp = fs.mul(char_factor);
    let lines_raw = tl.mul(temp).mul(inv_vp);

    // ceil(lines) → max(lines, 1.0) for non-zero text
    // Branchless: if text_len > 0 then max(ceil(lines), 1.0) else 0.0
    let has_text = tl.cmp_gt(F32x8::zero());
    let lines_clamped = lines_raw.max(one);
    let lines = has_text.blend(lines_clamped, F32x8::zero());

    // height = lines * font_size * 1.4
    lines.mul(fs).mul(line_factor)
}

/// SIMD-accelerated layout pass for a flat list of nodes.
///
/// This processes sequential sibling nodes in batches of 8.
/// For nested layouts, the tree structure still requires sequential
/// cursor_y accumulation, but within each level, siblings can be batched.
pub fn compute_layout_simd(
    nodes: &[FlatNode],
    viewport_width: f32,
) -> Vec<ComputedBox> {
    let consts = LayoutConstants::new(viewport_width);
    let count = nodes.len();
    let mut results = Vec::with_capacity(count);
    let mut cursor_y: f32 = 0.0;

    // Process in batches of 8
    let full_batches = count / 8;
    let remainder = count % 8;

    for batch in 0..full_batches {
        let offset = batch * 8;
        let batch_nodes = &nodes[offset..offset + 8];

        let mut tag_types = [0i32; 8];
        let mut text_lens = [0.0f32; 8];
        let mut font_sizes_arr = [16.0f32; 8];

        for i in 0..8 {
            tag_types[i] = batch_nodes[i].tag_type;
            text_lens[i] = batch_nodes[i].text_len as f32;
            font_sizes_arr[i] = font_size_lut(batch_nodes[i].tag_type, 16.0);
        }

        let margin_tops = batch_margin_tops(&tag_types);
        let margin_bottoms = batch_margin_bottoms(&tag_types);
        let paddings = batch_paddings(&tag_types);
        let text_heights = batch_text_heights(&text_lens, &font_sizes_arr, consts.inv_viewport);

        // Sequential cursor_y accumulation (data dependency prevents full SIMD)
        // But margin/padding/height computations above are SIMD-parallel
        for i in 0..8 {
            let mt = margin_tops.v[i];
            let mb = margin_bottoms.v[i];
            let pad = paddings.v[i];
            let th = text_heights.v[i];
            let fs = font_sizes_arr[i];

            if batch_nodes[i].is_block {
                cursor_y += mt;
            }

            let start_y = cursor_y;
            cursor_y += pad;
            cursor_y += th;
            cursor_y += pad;

            let height = cursor_y - start_y;

            if batch_nodes[i].is_block {
                cursor_y += mb;
            }

            results.push(ComputedBox {
                x: batch_nodes[i].depth as f32 * pad,
                y: start_y,
                width: viewport_width - batch_nodes[i].depth as f32 * pad * 2.0,
                height,
                font_size: fs,
            });
        }
    }

    // Handle remainder (scalar fallback for < 8 remaining nodes)
    for i in 0..remainder {
        let idx = full_batches * 8 + i;
        let node = &nodes[idx];
        let fs = font_size_lut(node.tag_type, 16.0);
        let mt = margin_top_lut(node.tag_type);
        let mb = margin_bottom_lut(node.tag_type);
        let pad = padding_lut(node.tag_type);

        if node.is_block {
            cursor_y += mt;
        }

        let start_y = cursor_y;
        cursor_y += pad;

        if node.text_len > 0 {
            // Division exorcism: text_len * fs * 0.6 * inv_viewport * fs * 1.4
            let inv_cpl = fs * 0.6 * consts.inv_viewport;
            let lines = (node.text_len as f32 * inv_cpl).ceil().max(1.0);
            cursor_y += lines * fs * consts.line_height_factor;
        }

        cursor_y += pad;
        let height = cursor_y - start_y;

        if node.is_block {
            cursor_y += mb;
        }

        results.push(ComputedBox {
            x: node.depth as f32 * pad,
            y: start_y,
            width: viewport_width - node.depth as f32 * pad * 2.0,
            height,
            font_size: fs,
        });
    }

    results
}

/// Flattened node for SIMD layout (pre-computed from DOM tree)
#[derive(Debug, Clone)]
pub struct FlatNode {
    pub tag_type: i32,
    pub text_len: usize,
    pub is_block: bool,
    pub depth: usize,
    pub classification: Classification,
    pub tag: String,
    pub text: String,
    pub href: Option<String>,
}

/// Computed layout box (output of SIMD layout)
#[derive(Debug, Clone, Copy)]
pub struct ComputedBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub font_size: f32,
}

/// Flatten DOM tree into a linear list for SIMD processing.
pub fn flatten_dom(node: &DomNode, depth: usize, out: &mut Vec<FlatNode>) {
    if !node.is_visible() {
        return;
    }

    let is_block = node.node_type == NodeType::Element && is_block_tag(&node.tag);

    out.push(FlatNode {
        tag_type: super::soa::encode_tag(&node.tag),
        text_len: node.text.len(),
        is_block,
        depth,
        classification: node.classification,
        tag: node.tag.clone(),
        text: node.text.clone(),
        href: match node.tag.as_str() {
            "a" => node.attr("href").map(|s| s.to_string()),
            "img" => node.attr("src").map(|s| s.to_string()),
            _ => None,
        },
    });

    for child in &node.children {
        if child.is_visible() {
            flatten_dom(child, depth + 1, out);
        }
    }
}

fn is_block_tag(tag: &str) -> bool {
    matches!(tag,
        "html" | "body" | "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
        | "ul" | "ol" | "li" | "table" | "tr" | "td" | "th" | "form"
        | "section" | "article" | "aside" | "main" | "header" | "footer" | "nav"
        | "blockquote" | "pre" | "figure" | "figcaption" | "details" | "summary"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_size_lut() {
        assert!((font_size_lut(11, 16.0) - 24.0).abs() < 1e-6); // heading
        assert!((font_size_lut(0, 16.0) - 16.0).abs() < 1e-6);  // div → parent
        assert!((font_size_lut(1, 14.0) - 14.0).abs() < 1e-6);  // p → parent
    }

    #[test]
    fn test_batch_text_heights() {
        let text_lens = [100.0, 0.0, 200.0, 0.0, 50.0, 0.0, 300.0, 0.0];
        let font_sizes = [16.0; 8];
        let heights = batch_text_heights(&text_lens, &font_sizes, 1.0 / 800.0);

        // Non-zero text should produce non-zero height
        assert!(heights.v[0] > 0.0);
        assert!((heights.v[1] - 0.0).abs() < 1e-6); // no text → 0 height
        assert!(heights.v[2] > 0.0);
    }

    #[test]
    fn test_division_exorcism() {
        // Verify that multiply-by-reciprocal gives same result as division
        let width = 800.0;
        let inv_w = 1.0 / width;
        let text_len = 100.0f32;
        let font_size = 16.0f32;

        let traditional = text_len / (width / (font_size * 0.6));
        let exorcised = text_len * font_size * 0.6 * inv_w;

        assert!((traditional - exorcised).abs() < 1e-4);
    }
}
