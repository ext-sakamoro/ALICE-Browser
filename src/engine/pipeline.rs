use std::sync::Arc;

use crate::dom::parser::parse_html;
use crate::dom::filter::{FilterStats, SemanticFilter};
use crate::dom::readability::readability_boost;
use crate::dom::DomTree;
use crate::net::adblock::AdBlockEngine;
use crate::net::fetch::fetch_url;
use crate::render::layout::{compute_layout, LayoutNode};
use crate::render::sdf_ui::{layout_to_sdf, SdfScene};

// Deep-Fried Rust: SIMD pipeline imports
use crate::simd::soa::dom_to_soa;
use crate::simd::classify::{classify_batch, apply_classifications, prune_ads, SimdFilterStats};
use crate::simd::layout::{flatten_dom, compute_layout_simd, FlatNode, ComputedBox};

/// Result of loading and processing a web page
pub struct PageResult {
    pub dom: DomTree,
    pub filter_stats: FilterStats,
    pub layout: LayoutNode,
    pub sdf_scene: SdfScene,
    pub fetch_status: u16,
}

/// Result from the SIMD-accelerated pipeline
pub struct SimdPageResult {
    pub dom: DomTree,
    pub simd_stats: SimdFilterStats,
    pub flat_nodes: Vec<FlatNode>,
    pub layout_boxes: Vec<ComputedBox>,
    pub fetch_status: u16,
}

/// Error during page loading
pub struct PageError {
    pub message: String,
    pub phase: &'static str,
}

impl std::fmt::Display for PageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.phase, self.message)
    }
}

/// The browser engine pipeline: Fetch → AdBlock → Parse → Filter → Layout → SDF
pub struct BrowserEngine {
    filter: SemanticFilter,
    viewport_width: f32,
    adblock: Option<Arc<AdBlockEngine>>,
    /// Use SIMD-accelerated pipeline (default: true)
    use_simd: bool,
}

impl BrowserEngine {
    pub fn new(viewport_width: f32) -> Self {
        Self {
            filter: SemanticFilter::new(),
            viewport_width,
            adblock: None,
            use_simd: true,
        }
    }

    /// Set the ad blocker engine (shared reference).
    pub fn with_adblock(mut self, adblock: Arc<AdBlockEngine>) -> Self {
        self.adblock = Some(adblock);
        self
    }

    /// Enable/disable SIMD pipeline
    pub fn with_simd(mut self, enabled: bool) -> Self {
        self.use_simd = enabled;
        self
    }

    /// Load a URL through the full pipeline
    pub fn load_page(&self, url: &str) -> Result<PageResult, PageError> {
        // Ad block check on the main page URL
        if let Some(ref ab) = self.adblock {
            if let Some(reason) = ab.should_block(url) {
                return Err(PageError {
                    message: format!("Blocked ({:?}): {}", reason, url),
                    phase: "adblock",
                });
            }
        }

        let fetch_result = fetch_url(url).map_err(|e| PageError {
            message: e.message,
            phase: "fetch",
        })?;

        self.process_html(&fetch_result.html, &fetch_result.url, fetch_result.status)
    }

    /// Load a URL through the pipeline using ALICE-Cache for caching
    #[cfg(feature = "smart-cache")]
    pub fn load_page_cached(
        &self,
        url: &str,
        cache: &crate::net::cache::CachedFetcher,
    ) -> Result<PageResult, PageError> {
        // Ad block check on the main page URL
        if let Some(ref ab) = self.adblock {
            if let Some(reason) = ab.should_block(url) {
                return Err(PageError {
                    message: format!("Blocked ({:?}): {}", reason, url),
                    phase: "adblock",
                });
            }
        }

        let fetch_result = cache.fetch(url).map_err(|e| PageError {
            message: e.message,
            phase: "fetch",
        })?;

        self.process_html(&fetch_result.html, &fetch_result.url, fetch_result.status)
    }

    /// Process raw HTML through the pipeline (for testing)
    pub fn process_html(
        &self,
        html: &str,
        url: &str,
        status: u16,
    ) -> Result<PageResult, PageError> {
        // Phase 2: Parse
        let mut dom = parse_html(html, url);

        // Phase 3: Semantic Filter
        // Use SIMD-accelerated classification if enabled
        let filter_stats = if self.use_simd {
            self.filter_simd(&mut dom)
        } else {
            self.filter.filter(&mut dom)
        };

        // Phase 3.5: Readability boost — promote main content
        readability_boost(&mut dom.root);

        // Phase 4: Layout
        let layout = compute_layout(&dom.root, self.viewport_width);

        // Phase 5: SDF Scene Generation
        let sdf_scene = layout_to_sdf(&layout, 1.0);

        Ok(PageResult {
            dom,
            filter_stats,
            layout,
            sdf_scene,
            fetch_status: status,
        })
    }

    /// SIMD-accelerated page processing pipeline.
    ///
    /// Fetch → Parse → SoA Transform → SIMD Classify → Prune → SIMD Layout
    ///
    /// This is the "カリッカリ" (Deep Fried) pipeline:
    /// - SoA: data laid out for sequential SIMD access
    /// - SIMD: 8 nodes classified per instruction
    /// - Branchless: zero conditional branches in classification
    /// - Division Exorcism: all divisions replaced with reciprocal multiplication
    pub fn load_page_simd(&self, url: &str) -> Result<SimdPageResult, PageError> {
        // Phase 1: Ad block check
        if let Some(ref ab) = self.adblock {
            if let Some(reason) = ab.should_block(url) {
                return Err(PageError {
                    message: format!("Blocked ({:?}): {}", reason, url),
                    phase: "adblock",
                });
            }
        }

        // Phase 2: Fetch
        let fetch_result = fetch_url(url).map_err(|e| PageError {
            message: e.message,
            phase: "fetch",
        })?;

        self.process_html_simd(&fetch_result.html, &fetch_result.url, fetch_result.status)
    }

    /// Process HTML through the SIMD pipeline
    pub fn process_html_simd(
        &self,
        html: &str,
        url: &str,
        status: u16,
    ) -> Result<SimdPageResult, PageError> {
        // Phase 2: Parse HTML → DOM tree
        let mut dom = parse_html(html, url);

        // Phase 3: SoA Transform + SIMD Classify
        //
        // Traditional: iterate DOM tree, classify each node (N branches per node)
        // SIMD: flatten to SoA, classify 8 nodes per SIMD instruction (0 branches)
        let mut soa = dom_to_soa(&dom.root);
        let simd_stats = classify_batch(&mut soa);

        // Phase 3.5: Apply classifications back to DOM tree
        let mut idx = 0;
        apply_classifications(&mut dom.root, soa.classifications.as_slice(), &mut idx);

        // Phase 3.6: Prune ad/tracker subtrees
        prune_ads(&mut dom.root);

        // Phase 3.7: Readability boost
        readability_boost(&mut dom.root);

        // Phase 4: SIMD Layout
        //
        // Traditional: recursive layout_node() with cursor_y accumulation
        // SIMD: flatten visible nodes, batch-compute margins/padding/heights
        let mut flat_nodes = Vec::new();
        flatten_dom(&dom.root, 0, &mut flat_nodes);
        let layout_boxes = compute_layout_simd(&flat_nodes, self.viewport_width);

        Ok(SimdPageResult {
            dom,
            simd_stats,
            flat_nodes,
            layout_boxes,
            fetch_status: status,
        })
    }

    /// SIMD-accelerated filter pass (used by process_html when use_simd=true)
    fn filter_simd(&self, dom: &mut DomTree) -> FilterStats {
        let mut soa = dom_to_soa(&dom.root);
        let simd_stats = classify_batch(&mut soa);

        let mut idx = 0;
        apply_classifications(&mut dom.root, soa.classifications.as_slice(), &mut idx);
        prune_ads(&mut dom.root);

        FilterStats {
            total_nodes: simd_stats.total_nodes,
            content_nodes: simd_stats.content_nodes,
            ad_nodes: simd_stats.ad_nodes,
            tracker_nodes: simd_stats.tracker_nodes,
            nav_nodes: simd_stats.nav_nodes,
            removed_nodes: simd_stats.removed_nodes,
        }
    }

    pub fn set_viewport_width(&mut self, width: f32) {
        self.viewport_width = width;
    }
}
