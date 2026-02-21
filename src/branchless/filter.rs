//! Branchless DOM Filtering
//!
//! Traditional DOM filtering:
//!   for each node:
//!     if class.contains("ad") → mark as ad
//!     if class.contains("tracker") → mark as tracker
//!     if tag == "script" → mark as tracker
//!     ...
//!
//! This is a branch-fest: O(nodes × patterns × chars) with N branches per node.
//!
//! Branchless approach:
//! 1. Pre-compute feature masks for all nodes in batch
//! 2. Combine masks using bitwise AND/OR (zero branches)
//! 3. Apply final classification via mask.blend()

use super::mask::{BitMask64, ComparisonMask};

/// Branchless filter result for a batch of up to 64 nodes.
#[derive(Debug)]
pub struct BatchFilterResult {
    /// Mask of nodes classified as ads
    pub ad_mask: BitMask64,
    /// Mask of nodes classified as trackers
    pub tracker_mask: BitMask64,
    /// Mask of nodes classified as content
    pub content_mask: BitMask64,
    /// Mask of nodes classified as navigation
    pub nav_mask: BitMask64,
    /// Mask of nodes to prune (ad | tracker)
    pub prune_mask: BitMask64,
    /// Total nodes in this batch
    pub count: usize,
}

impl BatchFilterResult {
    /// Count nodes that will be pruned
    #[inline]
    pub fn pruned_count(&self) -> u32 {
        self.prune_mask.count_ones()
    }

    /// Count content nodes
    #[inline]
    pub fn content_count(&self) -> u32 {
        self.content_mask.count_ones()
    }
}

/// Batch-classify up to 64 nodes using branchless mask operations.
///
/// Input: SoA feature arrays (each array has one value per node)
/// Output: BatchFilterResult with classification masks
///
/// The entire classification is done with zero conditional branches:
/// only bitwise AND, OR, NOT operations on masks.
pub fn classify_batch_branchless(
    is_script: &[f32],
    is_style: &[f32],
    is_nav: &[f32],
    is_interactive: &[f32],
    is_media: &[f32],
    has_ad_class: &[f32],
    has_tracker_class: &[f32],
    has_data_ad: &[f32],
    text_densities: &[f32],
    link_densities: &[f32],
    child_counts: &[f32],
    count: usize,
) -> BatchFilterResult {
    // Step 1: Create comparison masks (one pass per feature)
    let mask_script = ComparisonMask::nonzero(is_script);
    let _mask_style = ComparisonMask::nonzero(is_style);
    let mask_nav_tag = ComparisonMask::nonzero(is_nav);
    let _mask_interactive = ComparisonMask::nonzero(is_interactive);
    let _mask_media = ComparisonMask::nonzero(is_media);
    let mask_ad_class = ComparisonMask::nonzero(has_ad_class);
    let mask_tracker_class = ComparisonMask::nonzero(has_tracker_class);
    let mask_data_ad = ComparisonMask::nonzero(has_data_ad);
    let mask_high_text = ComparisonMask::gt(text_densities, 10.0);
    let mask_high_link = ComparisonMask::gt(link_densities, 0.6);
    let mask_many_children = ComparisonMask::gt(child_counts, 3.0 / 32.0);

    // Step 2: Combine masks using pure bitwise operations (ZERO branches!)

    // Tracker = script | tracker_class
    let tracker_mask = mask_script.or(mask_tracker_class);

    // Ad = ad_class | data_ad
    let ad_mask = mask_ad_class.or(mask_data_ad);

    // Navigation = nav_tag | (high_link & many_children)
    let heuristic_nav = mask_high_link.and(mask_many_children);
    let nav_mask = mask_nav_tag.or(heuristic_nav);

    // Content = high_text & !ad & !tracker & !nav
    let content_mask = mask_high_text
        .and(ad_mask.not())
        .and(tracker_mask.not())
        .and(nav_mask.not());

    // Prune = ad | tracker
    let prune_mask = ad_mask.or(tracker_mask);

    BatchFilterResult {
        ad_mask,
        tracker_mask,
        content_mask,
        nav_mask,
        prune_mask,
        count,
    }
}

/// Apply batch filter result back to classification array.
///
/// Writes classification indices into the output array.
/// Priority (highest wins): Ad > Tracker > Navigation > Content > Unknown
pub fn apply_batch_result(
    result: &BatchFilterResult,
    classifications: &mut [i32],
) {
    let count = result.count.min(classifications.len()).min(64);

    // Start with Unknown (8)
    for c in classifications[..count].iter_mut() {
        *c = 8; // Unknown
    }

    // Apply in priority order (lowest first, highest overwrites)
    for pos in result.content_mask.iter_set_bits() {
        if pos < count { classifications[pos] = 0; } // Content
    }
    for pos in result.nav_mask.iter_set_bits() {
        if pos < count { classifications[pos] = 1; } // Navigation
    }
    for pos in result.tracker_mask.iter_set_bits() {
        if pos < count { classifications[pos] = 3; } // Tracker
    }
    for pos in result.ad_mask.iter_set_bits() {
        if pos < count { classifications[pos] = 2; } // Advertisement
    }
}

/// Count filter statistics from a batch result
pub fn batch_stats(result: &BatchFilterResult) -> FilterStatsAccum {
    FilterStatsAccum {
        total: result.count,
        content: result.content_mask.count_ones() as usize,
        ads: result.ad_mask.count_ones() as usize,
        trackers: result.tracker_mask.count_ones() as usize,
        nav: result.nav_mask.count_ones() as usize,
        removed: result.prune_mask.count_ones() as usize,
    }
}

/// Accumulator for filter statistics across multiple batches
#[derive(Debug, Default)]
pub struct FilterStatsAccum {
    pub total: usize,
    pub content: usize,
    pub ads: usize,
    pub trackers: usize,
    pub nav: usize,
    pub removed: usize,
}

impl FilterStatsAccum {
    pub fn merge(&mut self, other: &FilterStatsAccum) {
        self.total += other.total;
        self.content += other.content;
        self.ads += other.ads;
        self.trackers += other.trackers;
        self.nav += other.nav;
        self.removed += other.removed;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_branchless_classify() {
        // 4 nodes: script, ad-class, high-text-density, nav-tag
        let is_script = [1.0, 0.0, 0.0, 0.0];
        let is_style = [0.0; 4];
        let is_nav = [0.0, 0.0, 0.0, 1.0];
        let is_interactive = [0.0; 4];
        let is_media = [0.0; 4];
        let has_ad = [0.0, 1.0, 0.0, 0.0];
        let has_tracker = [0.0; 4];
        let has_data_ad = [0.0; 4];
        let text_dens = [0.0, 0.0, 15.0, 0.0];
        let link_dens = [0.0; 4];
        let child_counts = [0.0; 4];

        let result = classify_batch_branchless(
            &is_script, &is_style, &is_nav, &is_interactive, &is_media,
            &has_ad, &has_tracker, &has_data_ad,
            &text_dens, &link_dens, &child_counts, 4,
        );

        assert!(result.tracker_mask.test(0), "script → tracker");
        assert!(result.ad_mask.test(1), "ad class → ad");
        assert!(result.content_mask.test(2), "high text → content");
        assert!(result.nav_mask.test(3), "nav tag → nav");
        assert_eq!(result.pruned_count(), 2); // script + ad
    }

    #[test]
    fn test_apply_batch_result() {
        let is_script = [1.0, 0.0, 0.0, 0.0];
        let has_ad = [0.0, 1.0, 0.0, 0.0];
        let text_dens = [0.0, 0.0, 15.0, 0.0];
        let zeros = [0.0f32; 4];

        let result = classify_batch_branchless(
            &is_script, &zeros, &zeros, &zeros, &zeros,
            &has_ad, &zeros, &zeros,
            &text_dens, &zeros, &zeros, 4,
        );

        let mut classifications = [0i32; 4];
        apply_batch_result(&result, &mut classifications);

        assert_eq!(classifications[0], 3); // Tracker
        assert_eq!(classifications[1], 2); // Advertisement
        assert_eq!(classifications[2], 0); // Content
        assert_eq!(classifications[3], 8); // Unknown
    }
}
