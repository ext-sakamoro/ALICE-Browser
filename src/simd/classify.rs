//! SIMD Batch DOM Classification — 8 nodes at once, zero branches
//!
//! Instead of classifying nodes one-by-one with match/if chains,
//! we process 8 nodes simultaneously using SIMD comparisons and
//! branchless mask.blend() operations.
//!
//! Performance model:
//!   - Traditional: 1 node × N branches × pipeline flush risk = slow
//!   - SIMD batch:  8 nodes × 0 branches × full pipeline = 8x+ faster

use super::soa::NodeFeaturesSoA;
use super::{F32x8, I32x8, MaskF32x8};

/// Classification indices matching dom::Classification
const CLASS_CONTENT: i32 = 0;
const CLASS_NAVIGATION: i32 = 1;
const CLASS_ADVERTISEMENT: i32 = 2;
const CLASS_TRACKER: i32 = 3;
const CLASS_DECORATION: i32 = 4;
const CLASS_INTERACTIVE: i32 = 5;
const CLASS_MEDIA: i32 = 6;
const CLASS_STRUCTURAL: i32 = 7;
const CLASS_UNKNOWN: i32 = 8;

/// SIMD classification statistics (no atomic overhead in batch mode)
#[derive(Debug, Default)]
pub struct SimdFilterStats {
    pub total_nodes: usize,
    pub content_nodes: usize,
    pub ad_nodes: usize,
    pub tracker_nodes: usize,
    pub nav_nodes: usize,
    pub removed_nodes: usize,
}

/// Classify all nodes in the SoA using branchless SIMD operations.
///
/// This is the heart of the "カリッカリ" optimization:
/// - No if/else per node — only mask.blend()
/// - 8 nodes processed per iteration
/// - All feature comparisons happen in parallel across 8 lanes
///
/// The classification logic is:
///   1. script/noscript → Tracker
///   2. style → Decoration
///   3. nav → Navigation
///   4. interactive tags → Interactive
///   5. media tags → Media
///   6. has_ad_class || has_data_ad → Advertisement
///   7. has_tracker_class → Tracker
///   8. link_density > 0.6 && child_count > 3/32 → Navigation
///   9. text_density > 10.0 → Content
///  10. header/footer tags → Structural
///  11. otherwise → Unknown
pub fn classify_batch(soa: &mut NodeFeaturesSoA) -> SimdFilterStats {
    let mut stats = SimdFilterStats::default();
    stats.total_nodes = soa.count;

    let batches = soa.batch_count();
    let node_count = soa.count;

    // Threshold constants (splatted once, reused across all batches)
    let threshold_link_density = F32x8::splat(0.6);
    let threshold_child_count = F32x8::splat(3.0 / 32.0); // normalized
    let threshold_text_density = F32x8::splat(10.0);
    let half = F32x8::splat(0.5);

    // Classification constants
    let cls_content = I32x8::splat(CLASS_CONTENT);
    let cls_nav = I32x8::splat(CLASS_NAVIGATION);
    let cls_ad = I32x8::splat(CLASS_ADVERTISEMENT);
    let cls_tracker = I32x8::splat(CLASS_TRACKER);
    let cls_decoration = I32x8::splat(CLASS_DECORATION);
    let cls_interactive = I32x8::splat(CLASS_INTERACTIVE);
    let cls_media = I32x8::splat(CLASS_MEDIA);
    let cls_structural = I32x8::splat(CLASS_STRUCTURAL);
    let cls_unknown = I32x8::splat(CLASS_UNKNOWN);

    // Helper: load 8 f32 from slice at batch offset (safe, handles short slices)
    #[inline(always)]
    fn load_f32(slice: &[f32], batch: usize) -> F32x8 {
        let off = batch * 8;
        if off + 8 <= slice.len() {
            F32x8::load(&slice[off..])
        } else {
            // Partial batch: copy available, pad with zero
            let mut v = [0.0f32; 8];
            let avail = slice.len().saturating_sub(off);
            v[..avail].copy_from_slice(&slice[off..off + avail]);
            F32x8 { v }
        }
    }

    #[inline(always)]
    fn load_i32(slice: &[i32], batch: usize) -> I32x8 {
        let off = batch * 8;
        if off + 8 <= slice.len() {
            I32x8::load(&slice[off..])
        } else {
            let mut v = [0i32; 8];
            let avail = slice.len().saturating_sub(off);
            v[..avail].copy_from_slice(&slice[off..off + avail]);
            I32x8 { v }
        }
    }

    for batch in 0..batches {
        let offset = batch * 8;

        // Load all features for this batch of 8 nodes
        // Direct slice access avoids &mut/& borrow conflicts
        let is_script = load_f32(soa.is_script.as_slice(), batch);
        let is_style = load_f32(soa.is_style.as_slice(), batch);
        let is_nav = load_f32(soa.is_nav.as_slice(), batch);
        let is_interactive = load_f32(soa.is_interactive.as_slice(), batch);
        let is_media = load_f32(soa.is_media.as_slice(), batch);
        let has_ad = load_f32(soa.has_ad_class.as_slice(), batch);
        let has_tracker = load_f32(soa.has_tracker_class.as_slice(), batch);
        let has_data_ad = load_f32(soa.has_data_ad.as_slice(), batch);
        let link_density = load_f32(soa.link_densities.as_slice(), batch);
        let child_count = load_f32(soa.child_counts.as_slice(), batch);
        let text_density = load_f32(soa.text_densities.as_slice(), batch);

        // Load tag types to detect structural tags (header=6, footer=7)
        let tag_types = load_i32(soa.tag_types.as_slice(), batch);

        // ─── Branchless classification cascade ───
        //
        // Start with Unknown, then overwrite with higher-priority classes.
        // Each step: result = mask.blend(new_class, previous_result)
        // This means later (higher priority) checks override earlier ones.
        //
        // Priority (lowest → highest):
        //   Unknown → Content → Structural → Navigation → Media →
        //   Interactive → Decoration → Tracker → Advertisement
        //
        // Because blend overwrites, we go from LOW priority to HIGH:

        let mut result = cls_unknown;

        // 9. text_density > 10.0 → Content
        let mask_content = text_density.cmp_gt(threshold_text_density);
        result = blend_i32(mask_content, cls_content, result);

        // 10. header(6)/footer(7) → Structural
        let mask_header = tag_types.cmp_eq(I32x8::splat(6));
        let mask_footer = tag_types.cmp_eq(I32x8::splat(7));
        let mask_structural = mask_header.or(mask_footer);
        result = blend_i32(mask_structural, cls_structural, result);

        // 8. link_density > 0.6 && child_count > threshold → Navigation
        let mask_link = link_density.cmp_gt(threshold_link_density);
        let mask_children = child_count.cmp_gt(threshold_child_count);
        let mask_nav_heuristic = mask_link.and(mask_children);
        result = blend_i32(mask_nav_heuristic, cls_nav, result);

        // 5. media tags → Media
        let mask_media = is_media.cmp_gt(half);
        result = blend_i32(mask_media, cls_media, result);

        // 4. interactive tags → Interactive
        let mask_interactive = is_interactive.cmp_gt(half);
        result = blend_i32(mask_interactive, cls_interactive, result);

        // 3. nav tag → Navigation
        let mask_nav = is_nav.cmp_gt(half);
        result = blend_i32(mask_nav, cls_nav, result);

        // 2. style → Decoration
        let mask_style = is_style.cmp_gt(half);
        result = blend_i32(mask_style, cls_decoration, result);

        // 7. has_tracker_class → Tracker
        let mask_tracker = has_tracker.cmp_gt(half);
        result = blend_i32(mask_tracker, cls_tracker, result);

        // 1. script/noscript → Tracker (highest priority for tracker)
        let mask_script = is_script.cmp_gt(half);
        result = blend_i32(mask_script, cls_tracker, result);

        // 6. has_ad_class || has_data_ad → Advertisement (highest priority)
        let mask_ad = has_ad.cmp_gt(half).or(has_data_ad.cmp_gt(half));
        result = blend_i32(mask_ad, cls_ad, result);

        // Store results back (safe: handle partial last batch)
        let cls_slice = soa.classifications.as_mut_slice();
        if offset + 8 <= cls_slice.len() {
            result.store(&mut cls_slice[offset..]);
        } else {
            let avail = cls_slice.len().saturating_sub(offset);
            cls_slice[offset..offset + avail].copy_from_slice(&result.v[..avail]);
        }

        // Accumulate stats (for the valid nodes only)
        let valid_count = (node_count - offset).min(8);
        for i in 0..valid_count {
            match result.v[i] {
                x if x == CLASS_CONTENT => stats.content_nodes += 1,
                x if x == CLASS_ADVERTISEMENT => stats.ad_nodes += 1,
                x if x == CLASS_TRACKER => stats.tracker_nodes += 1,
                x if x == CLASS_NAVIGATION => stats.nav_nodes += 1,
                _ => {}
            }
        }
    }

    stats.removed_nodes = stats.ad_nodes + stats.tracker_nodes;
    stats
}

/// Branchless i32 blend using f32 mask bits.
///
/// Where mask is true (0xFFFFFFFF), select `a`; where false (0x00000000), select `b`.
/// This maps directly to vblendvps on AVX2.
#[inline(always)]
fn blend_i32(mask: MaskF32x8, a: I32x8, b: I32x8) -> I32x8 {
    let mut out = [0i32; 8];
    for i in 0..8 {
        // Branchless: use arithmetic instead of if/else
        // mask bit is either 0xFFFFFFFF (-1 as i32) or 0x00000000 (0)
        let m = mask.bits[i] as i32; // -1 or 0
        // m & (a - b) + b  ≡  if m then a else b
        // But since m is all-ones or all-zeros, we can use:
        out[i] = (a.v[i] & m) | (b.v[i] & !m);
    }
    I32x8 { v: out }
}

/// Convert SIMD classification index back to dom::Classification
#[inline]
pub fn index_to_classification(idx: i32) -> crate::dom::Classification {
    crate::dom::Classification::from_index(idx as usize)
}

/// Apply SIMD classification results back to the DOM tree.
///
/// Walks the DOM in the same order as dom_to_soa flattening,
/// applying the SIMD-computed classifications.
pub fn apply_classifications(
    node: &mut crate::dom::DomNode,
    classifications: &[i32],
    index: &mut usize,
) {
    if *index < classifications.len() {
        node.classification = index_to_classification(classifications[*index]);
        *index += 1;
    }

    for child in &mut node.children {
        apply_classifications(child, classifications, index);
    }
}

/// Prune ad/tracker subtrees (same as original but called after SIMD classify)
pub fn prune_ads(node: &mut crate::dom::DomNode) {
    node.children.retain(|c| {
        c.classification != crate::dom::Classification::Advertisement
            && c.classification != crate::dom::Classification::Tracker
    });
    for child in &mut node.children {
        prune_ads(child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simd::soa::{NodeFeatures, NodeFeaturesSoA};

    fn make_test_soa() -> NodeFeaturesSoA {
        let mut soa = NodeFeaturesSoA::with_capacity(8);

        // Node 0: script → should be Tracker
        soa.push(NodeFeatures {
            tag_type: 3, is_script: 1.0, is_style: 0.0, is_nav: 0.0,
            is_interactive: 0.0, is_media: 0.0, has_ad_class: 0.0,
            has_tracker_class: 0.0, has_data_ad: 0.0, text_density: 0.0,
            link_density: 0.0, child_count: 0.0, text_length: 0.0,
            has_href: 0.0, attr_count: 0.0,
        });

        // Node 1: ad class → should be Advertisement
        soa.push(NodeFeatures {
            tag_type: 0, is_script: 0.0, is_style: 0.0, is_nav: 0.0,
            is_interactive: 0.0, is_media: 0.0, has_ad_class: 1.0,
            has_tracker_class: 0.0, has_data_ad: 0.0, text_density: 5.0,
            link_density: 0.0, child_count: 0.0, text_length: 0.0,
            has_href: 0.0, attr_count: 0.0,
        });

        // Node 2: high text density → should be Content
        soa.push(NodeFeatures {
            tag_type: 1, is_script: 0.0, is_style: 0.0, is_nav: 0.0,
            is_interactive: 0.0, is_media: 0.0, has_ad_class: 0.0,
            has_tracker_class: 0.0, has_data_ad: 0.0, text_density: 15.0,
            link_density: 0.0, child_count: 0.0, text_length: 0.5,
            has_href: 0.0, attr_count: 0.0,
        });

        // Node 3: nav tag → should be Navigation
        soa.push(NodeFeatures {
            tag_type: 5, is_script: 0.0, is_style: 0.0, is_nav: 1.0,
            is_interactive: 0.0, is_media: 0.0, has_ad_class: 0.0,
            has_tracker_class: 0.0, has_data_ad: 0.0, text_density: 2.0,
            link_density: 0.0, child_count: 0.0, text_length: 0.0,
            has_href: 0.0, attr_count: 0.0,
        });

        // Nodes 4-7: padding (Unknown)
        for _ in 4..8 {
            soa.push(NodeFeatures {
                tag_type: 17, is_script: 0.0, is_style: 0.0, is_nav: 0.0,
                is_interactive: 0.0, is_media: 0.0, has_ad_class: 0.0,
                has_tracker_class: 0.0, has_data_ad: 0.0, text_density: 0.0,
                link_density: 0.0, child_count: 0.0, text_length: 0.0,
                has_href: 0.0, attr_count: 0.0,
            });
        }

        soa.pad_to_simd_width();
        soa
    }

    #[test]
    fn test_simd_classification() {
        let mut soa = make_test_soa();
        let stats = classify_batch(&mut soa);

        let classes = soa.classifications.as_slice();
        assert_eq!(classes[0], CLASS_TRACKER, "script → Tracker");
        assert_eq!(classes[1], CLASS_ADVERTISEMENT, "ad class → Ad");
        assert_eq!(classes[2], CLASS_CONTENT, "high text density → Content");
        assert_eq!(classes[3], CLASS_NAVIGATION, "nav tag → Navigation");

        assert!(stats.tracker_nodes >= 1);
        assert!(stats.ad_nodes >= 1);
        assert!(stats.content_nodes >= 1);
        assert!(stats.nav_nodes >= 1);
    }
}
