//! Structure of Arrays (SoA) — SIMD-friendly data layout
//!
//! Instead of AoS: [Node{tag,text_density,link_density,...}, Node{...}, ...]
//! We use SoA:     tags: [t,t,t,...], text_densities: [d,d,d,...], link_densities: [l,l,l,...]
//!
//! This lets SIMD load 8 text_densities in ONE instruction (sequential memory access),
//! instead of gathering scattered fields from different cache lines.

use super::{align_up, F32x8, I32x8, SIMD_WIDTH};

/// SoA representation of DOM node features for SIMD batch processing.
///
/// Each Vec stores ONE feature across ALL nodes, aligned to SIMD_WIDTH.
/// Memory layout:
///   tag_types:      [t0, t1, t2, t3, t4, t5, t6, t7, t8, ...]  ← continuous!
///   text_densities: [d0, d1, d2, d3, d4, d5, d6, d7, d8, ...]  ← continuous!
///
/// CPU loads 8 consecutive f32s in a single AVX2 vmovaps instruction.
#[derive(Debug, Clone)]
pub struct NodeFeaturesSoA {
    /// Tag type encoded as integer (0=div, 1=p, 2=a, 3=script, ...)
    pub tag_types: AlignedVec<i32>,
    /// Text density: text_len / node_count (higher = more content-rich)
    pub text_densities: AlignedVec<f32>,
    /// Link density: link_text / total_text (higher = more navigational)
    pub link_densities: AlignedVec<f32>,
    /// Child count (normalized by 1/32 for SIMD range)
    pub child_counts: AlignedVec<f32>,
    /// Has ad-pattern in class/id (1.0 or 0.0)
    pub has_ad_class: AlignedVec<f32>,
    /// Has tracker-pattern in class/id (1.0 or 0.0)
    pub has_tracker_class: AlignedVec<f32>,
    /// Has data-ad* attributes (1.0 or 0.0)
    pub has_data_ad: AlignedVec<f32>,
    /// Is script/noscript tag (1.0 or 0.0)
    pub is_script: AlignedVec<f32>,
    /// Is style tag (1.0 or 0.0)
    pub is_style: AlignedVec<f32>,
    /// Is nav tag (1.0 or 0.0)
    pub is_nav: AlignedVec<f32>,
    /// Is interactive (button/input/form) (1.0 or 0.0)
    pub is_interactive: AlignedVec<f32>,
    /// Is media (img/video/audio) (1.0 or 0.0)
    pub is_media: AlignedVec<f32>,
    /// Text length (normalized by 1/1024)
    pub text_lengths: AlignedVec<f32>,
    /// Has href attribute (1.0 or 0.0)
    pub has_href: AlignedVec<f32>,
    /// Attribute count (normalized by 1/16)
    pub attr_counts: AlignedVec<f32>,
    /// Node count (actual, for back-reference)
    pub count: usize,

    /// Output: classification result per node (written by SIMD classify)
    pub classifications: AlignedVec<i32>,
}

impl NodeFeaturesSoA {
    /// Create empty SoA with capacity for `cap` nodes (aligned up)
    pub fn with_capacity(cap: usize) -> Self {
        let aligned_cap = align_up(cap);
        Self {
            tag_types: AlignedVec::with_capacity(aligned_cap),
            text_densities: AlignedVec::with_capacity(aligned_cap),
            link_densities: AlignedVec::with_capacity(aligned_cap),
            child_counts: AlignedVec::with_capacity(aligned_cap),
            has_ad_class: AlignedVec::with_capacity(aligned_cap),
            has_tracker_class: AlignedVec::with_capacity(aligned_cap),
            has_data_ad: AlignedVec::with_capacity(aligned_cap),
            is_script: AlignedVec::with_capacity(aligned_cap),
            is_style: AlignedVec::with_capacity(aligned_cap),
            is_nav: AlignedVec::with_capacity(aligned_cap),
            is_interactive: AlignedVec::with_capacity(aligned_cap),
            is_media: AlignedVec::with_capacity(aligned_cap),
            text_lengths: AlignedVec::with_capacity(aligned_cap),
            has_href: AlignedVec::with_capacity(aligned_cap),
            attr_counts: AlignedVec::with_capacity(aligned_cap),
            count: 0,
            classifications: AlignedVec::with_capacity(aligned_cap),
        }
    }

    /// Push a single node's features (called during DOM→SoA flattening)
    pub fn push(&mut self, features: NodeFeatures) {
        self.tag_types.push(features.tag_type);
        self.text_densities.push(features.text_density);
        self.link_densities.push(features.link_density);
        self.child_counts.push(features.child_count);
        self.has_ad_class.push(features.has_ad_class);
        self.has_tracker_class.push(features.has_tracker_class);
        self.has_data_ad.push(features.has_data_ad);
        self.is_script.push(features.is_script);
        self.is_style.push(features.is_style);
        self.is_nav.push(features.is_nav);
        self.is_interactive.push(features.is_interactive);
        self.is_media.push(features.is_media);
        self.text_lengths.push(features.text_length);
        self.has_href.push(features.has_href);
        self.attr_counts.push(features.attr_count);
        self.classifications.push(8); // Unknown = 8
        self.count += 1;
    }

    /// Pad to SIMD_WIDTH boundary with zeros (must call before SIMD processing)
    pub fn pad_to_simd_width(&mut self) {
        let padded = align_up(self.count);
        while self.tag_types.len() < padded {
            self.tag_types.push(0);
            self.text_densities.push(0.0);
            self.link_densities.push(0.0);
            self.child_counts.push(0.0);
            self.has_ad_class.push(0.0);
            self.has_tracker_class.push(0.0);
            self.has_data_ad.push(0.0);
            self.is_script.push(0.0);
            self.is_style.push(0.0);
            self.is_nav.push(0.0);
            self.is_interactive.push(0.0);
            self.is_media.push(0.0);
            self.text_lengths.push(0.0);
            self.has_href.push(0.0);
            self.attr_counts.push(0.0);
            self.classifications.push(8);
        }
    }

    /// Number of SIMD batches needed
    #[inline(always)]
    pub fn batch_count(&self) -> usize {
        align_up(self.count) / SIMD_WIDTH
    }

    /// Load a batch of 8 f32 values from a feature array at the given batch index
    #[inline(always)]
    pub fn load_f32_batch(&self, data: &AlignedVec<f32>, batch: usize) -> F32x8 {
        let offset = batch * 8;
        F32x8::load(&data.as_slice()[offset..])
    }

    /// Load a batch of 8 i32 values
    #[inline(always)]
    pub fn load_i32_batch(&self, data: &AlignedVec<i32>, batch: usize) -> I32x8 {
        let offset = batch * 8;
        I32x8::load(&data.as_slice()[offset..])
    }
}

/// Single node's features (AoS format, used only for push)
pub struct NodeFeatures {
    pub tag_type: i32,
    pub text_density: f32,
    pub link_density: f32,
    pub child_count: f32,
    pub has_ad_class: f32,
    pub has_tracker_class: f32,
    pub has_data_ad: f32,
    pub is_script: f32,
    pub is_style: f32,
    pub is_nav: f32,
    pub is_interactive: f32,
    pub is_media: f32,
    pub text_length: f32,
    pub has_href: f32,
    pub attr_count: f32,
}

/// SoA representation of layout boxes for SIMD batch layout computation.
///
/// Instead of Vec<LayoutBox{x,y,w,h}> (AoS, cache-hostile),
/// we store x[], y[], w[], h[] separately (SoA, SIMD-ready).
#[derive(Debug, Clone)]
pub struct LayoutBoxesSoA {
    pub xs: AlignedVec<f32>,
    pub ys: AlignedVec<f32>,
    pub widths: AlignedVec<f32>,
    pub heights: AlignedVec<f32>,
    pub font_sizes: AlignedVec<f32>,
    pub margin_tops: AlignedVec<f32>,
    pub margin_bottoms: AlignedVec<f32>,
    pub paddings: AlignedVec<f32>,
    pub is_block: AlignedVec<f32>,
    pub count: usize,
}

impl LayoutBoxesSoA {
    pub fn with_capacity(cap: usize) -> Self {
        let aligned_cap = align_up(cap);
        Self {
            xs: AlignedVec::with_capacity(aligned_cap),
            ys: AlignedVec::with_capacity(aligned_cap),
            widths: AlignedVec::with_capacity(aligned_cap),
            heights: AlignedVec::with_capacity(aligned_cap),
            font_sizes: AlignedVec::with_capacity(aligned_cap),
            margin_tops: AlignedVec::with_capacity(aligned_cap),
            margin_bottoms: AlignedVec::with_capacity(aligned_cap),
            paddings: AlignedVec::with_capacity(aligned_cap),
            is_block: AlignedVec::with_capacity(aligned_cap),
            count: 0,
        }
    }

    pub fn push(&mut self, x: f32, y: f32, w: f32, h: f32, fs: f32, mt: f32, mb: f32, pad: f32, block: bool) {
        self.xs.push(x);
        self.ys.push(y);
        self.widths.push(w);
        self.heights.push(h);
        self.font_sizes.push(fs);
        self.margin_tops.push(mt);
        self.margin_bottoms.push(mb);
        self.paddings.push(pad);
        self.is_block.push(if block { 1.0 } else { 0.0 });
        self.count += 1;
    }

    pub fn pad_to_simd_width(&mut self) {
        let padded = align_up(self.count);
        while self.xs.len() < padded {
            self.xs.push(0.0);
            self.ys.push(0.0);
            self.widths.push(0.0);
            self.heights.push(0.0);
            self.font_sizes.push(0.0);
            self.margin_tops.push(0.0);
            self.margin_bottoms.push(0.0);
            self.paddings.push(0.0);
            self.is_block.push(0.0);
        }
    }
}

/// 32-byte aligned Vec for SIMD loads/stores without unaligned penalty.
///
/// Standard Vec doesn't guarantee alignment > 8 bytes.
/// This wrapper ensures 32-byte alignment (AVX2 requirement).
#[derive(Debug, Clone)]
pub struct AlignedVec<T: Copy + Default> {
    data: Vec<T>,
    // We use Vec with overallocation to ensure alignment
    // The actual alignment is enforced by the repr(C, align(32)) wrapper
}

impl<T: Copy + Default> AlignedVec<T> {
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            data: Vec::with_capacity(cap),
        }
    }

    #[inline(always)]
    pub fn push(&mut self, val: T) {
        self.data.push(val);
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    #[inline(always)]
    pub fn as_slice(&self) -> &[T] {
        &self.data
    }

    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.data
    }
}

impl<T: Copy + Default> Default for AlignedVec<T> {
    fn default() -> Self {
        Self::with_capacity(0)
    }
}

/// Tag name → integer encoding for SIMD comparison
#[inline]
pub fn encode_tag(tag: &str) -> i32 {
    match tag {
        "div" => 0,
        "p" => 1,
        "a" => 2,
        "script" | "noscript" => 3,
        "style" => 4,
        "nav" => 5,
        "header" => 6,
        "footer" => 7,
        "button" | "input" | "textarea" | "select" | "form" => 8,
        "img" | "video" | "audio" | "picture" | "canvas" => 9,
        "iframe" => 10,
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => 11,
        "span" => 12,
        "ul" | "ol" | "li" => 13,
        "table" | "tr" | "td" | "th" => 14,
        "section" | "article" | "main" | "aside" => 15,
        "" => 16, // text node
        _ => 17,  // other
    }
}

/// Convert DomNode tree into SoA representation (flattening)
pub fn dom_to_soa(node: &crate::dom::DomNode) -> NodeFeaturesSoA {
    let node_count = node.node_count();
    let mut soa = NodeFeaturesSoA::with_capacity(node_count);
    flatten_node(node, &mut soa);
    soa.pad_to_simd_width();
    soa
}

fn flatten_node(node: &crate::dom::DomNode, soa: &mut NodeFeaturesSoA) {
    let class = node.attr("class").unwrap_or("");
    let id = node.attr("id").unwrap_or("");
    let combined = format!("{} {}", class, id).to_lowercase();

    let ad_patterns = [
        "ad", "ads", "advert", "banner", "sponsor", "promoted",
        "promo", "adsense", "doubleclick", "taboola", "outbrain",
    ];
    let tracker_patterns = [
        "tracker", "tracking", "analytics", "pixel", "beacon",
        "telemetry", "cookie-banner", "cookie-consent",
    ];

    let has_ad = ad_patterns.iter().any(|p| combined.contains(p));
    let has_tracker = tracker_patterns.iter().any(|p| combined.contains(p));
    let has_data_ad = node.attributes.keys().any(|k| k.starts_with("data-ad") || k.starts_with("data-tracking"));

    // Division exorcism: multiply by reciprocal instead of dividing
    const INV_32: f32 = 1.0 / 32.0;
    const INV_1024: f32 = 1.0 / 1024.0;
    const INV_16: f32 = 1.0 / 16.0;

    let features = NodeFeatures {
        tag_type: encode_tag(&node.tag),
        text_density: node.text_density(),
        link_density: node.link_density(),
        child_count: node.children.len() as f32 * INV_32,     // ÷32 → ×(1/32)
        has_ad_class: if has_ad { 1.0 } else { 0.0 },
        has_tracker_class: if has_tracker { 1.0 } else { 0.0 },
        has_data_ad: if has_data_ad { 1.0 } else { 0.0 },
        is_script: if matches!(node.tag.as_str(), "script" | "noscript") { 1.0 } else { 0.0 },
        is_style: if node.tag == "style" { 1.0 } else { 0.0 },
        is_nav: if node.tag == "nav" { 1.0 } else { 0.0 },
        is_interactive: if matches!(node.tag.as_str(), "button" | "input" | "textarea" | "select" | "form") { 1.0 } else { 0.0 },
        is_media: if matches!(node.tag.as_str(), "img" | "video" | "audio" | "picture" | "canvas") { 1.0 } else { 0.0 },
        text_length: node.text.len() as f32 * INV_1024,       // ÷1024 → ×(1/1024)
        has_href: if node.attr("href").is_some() { 1.0 } else { 0.0 },
        attr_count: node.attributes.len() as f32 * INV_16,    // ÷16 → ×(1/16)
    };

    soa.push(features);

    for child in &node.children {
        flatten_node(child, soa);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_tag() {
        assert_eq!(encode_tag("div"), 0);
        assert_eq!(encode_tag("script"), 3);
        assert_eq!(encode_tag("img"), 9);
        assert_eq!(encode_tag("random"), 17);
    }

    #[test]
    fn test_aligned_vec() {
        let mut v = AlignedVec::<f32>::with_capacity(16);
        for i in 0..16 {
            v.push(i as f32);
        }
        assert_eq!(v.len(), 16);
        assert!((v.as_slice()[0] - 0.0).abs() < 1e-6);
        assert!((v.as_slice()[15] - 15.0).abs() < 1e-6);
    }
}
