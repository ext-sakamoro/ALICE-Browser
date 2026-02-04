use std::sync::Arc;

use crate::dom::parser::parse_html;
use crate::dom::filter::{FilterStats, SemanticFilter};
use crate::dom::readability::readability_boost;
use crate::dom::DomTree;
use crate::net::adblock::AdBlockEngine;
use crate::net::fetch::fetch_url;
use crate::render::layout::{compute_layout, LayoutNode};
use crate::render::sdf_ui::{layout_to_sdf, SdfScene};

/// Result of loading and processing a web page
pub struct PageResult {
    pub dom: DomTree,
    pub filter_stats: FilterStats,
    pub layout: LayoutNode,
    pub sdf_scene: SdfScene,
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
}

impl BrowserEngine {
    pub fn new(viewport_width: f32) -> Self {
        Self {
            filter: SemanticFilter::new(),
            viewport_width,
            adblock: None,
        }
    }

    /// Set the ad blocker engine (shared reference).
    pub fn with_adblock(mut self, adblock: Arc<AdBlockEngine>) -> Self {
        self.adblock = Some(adblock);
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

        // Phase 3: Semantic Filter (ALICE-AdBlock)
        let filter_stats = self.filter.filter(&mut dom);

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

    pub fn set_viewport_width(&mut self, width: f32) {
        self.viewport_width = width;
    }
}
