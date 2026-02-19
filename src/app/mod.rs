//! `BrowserApp` — the top-level egui application state.
//!
//! This module declares the `BrowserApp` struct and its `Default` impl.
//! All methods are split across the sibling sub-modules:
//!
//! - `navigation` — page loading, history, async fetch
//! - `toolbar`    — address bar and controls
//! - `content`    — main viewport rendering (2-D, SDF, OZ)

pub mod navigation;
pub mod toolbar;
pub mod content;

use std::sync::{mpsc, Arc};
use eframe::egui;

use alice_browser::engine::pipeline::{PageResult, PageError};
use alice_browser::net::adblock::{AdBlockEngine, BlockStats};
use alice_browser::render::RenderMode;

use crate::oz::LinkPreview;

// ─── Application state ───────────────────────────────────────────────────────

pub struct BrowserApp {
    pub url_input: String,
    pub page: Option<PageResult>,
    pub error: Option<String>,
    pub loading: bool,
    pub fetch_rx: Option<mpsc::Receiver<Result<PageResult, PageError>>>,
    pub render_mode: RenderMode,
    pub show_stats: bool,
    pub dark_mode: bool,
    // History (back / forward)
    pub history: Vec<String>,
    pub history_idx: usize,
    // Image loading
    pub image_loader: alice_browser::net::image::ImageLoader,
    pub image_textures: std::collections::HashMap<String, egui::TextureHandle>,
    #[cfg(feature = "smart-cache")]
    pub page_cache: std::sync::Arc<alice_browser::net::cache::CachedFetcher>,
    #[cfg(feature = "search")]
    pub search_query: String,
    #[cfg(feature = "search")]
    pub search_index: Option<alice_browser::search::PageSearch>,
    #[cfg(feature = "telemetry")]
    pub metrics: alice_browser::telemetry::BrowserMetrics,
    #[cfg(feature = "telemetry")]
    pub navigate_start: Option<std::time::Instant>,
    pub sdf_paint_state: alice_browser::render::sdf_paint::SdfPaintState,
    pub paint_elements: Option<Vec<alice_browser::render::sdf_ui::PaintElement>>,
    #[cfg(feature = "sdf-render")]
    pub sdf_texture: Option<egui::TextureHandle>,
    #[cfg(feature = "sdf-render")]
    pub sdf_mode_rendered: Option<RenderMode>,
    // 3-D camera state
    #[cfg(feature = "sdf-render")]
    pub cam_params: alice_browser::render::sdf_renderer::CameraParams,
    #[cfg(feature = "sdf-render")]
    pub cam_dirty: bool,
    #[cfg(feature = "sdf-render")]
    pub cam_dragging: bool,
    #[cfg(feature = "sdf-render")]
    pub spatial_scene: Option<alice_browser::render::sdf_ui::SdfScene>,
    #[cfg(feature = "sdf-render")]
    pub gpu_renderer: Option<alice_browser::render::gpu_renderer::GpuRenderer>,
    // OZ Stream state
    #[cfg(feature = "sdf-render")]
    pub stream_state: Option<alice_browser::render::stream::StreamState>,
    /// Pending URL from OZ mode double-click on a link
    #[cfg(feature = "sdf-render")]
    pub oz_pending_url: Option<String>,
    /// Link preview for grabbed text
    #[cfg(feature = "sdf-render")]
    pub oz_preview: Option<LinkPreview>,
    #[cfg(feature = "sdf-render")]
    pub oz_preview_rx: Option<mpsc::Receiver<LinkPreview>>,
    /// URL currently being previewed (to avoid re-fetching)
    #[cfg(feature = "sdf-render")]
    pub oz_preview_for: Option<String>,
    /// Screen position for hologram overlay (near grabbed particle)
    #[cfg(feature = "sdf-render")]
    pub oz_hologram_screen_pos: Option<egui::Pos2>,
    /// Hologram fade-in alpha (0.0 -> 1.0)
    #[cfg(feature = "sdf-render")]
    pub oz_hologram_alpha: f32,
    /// Hologram animation start time
    #[cfg(feature = "sdf-render")]
    pub oz_hologram_start: Option<std::time::Instant>,
    /// Background link prefetch receiver
    #[cfg(feature = "sdf-render")]
    pub oz_prefetch_rx: Option<mpsc::Receiver<Vec<alice_browser::render::stream::TextMeta>>>,
    /// Whether prefetch has been started for the current page
    #[cfg(feature = "sdf-render")]
    pub oz_prefetch_started: bool,
    /// Buffer for prefetched texts (accumulated before OZ mode is active)
    #[cfg(feature = "sdf-render")]
    pub oz_prefetch_buffer: Vec<alice_browser::render::stream::TextMeta>,
    pub app_start: std::time::Instant,
    #[cfg(feature = "sdf-render")]
    pub last_frame_time: std::time::Instant,
    // Ad blocker
    pub adblock: Arc<AdBlockEngine>,
    pub block_stats: BlockStats,
}

impl Default for BrowserApp {
    fn default() -> Self {
        Self {
            url_input: String::from("https://example.com"),
            page: None,
            error: None,
            loading: false,
            fetch_rx: None,
            render_mode: RenderMode::Flat,
            show_stats: true,
            dark_mode: false,
            history: Vec::new(),
            history_idx: 0,
            image_loader: alice_browser::net::image::ImageLoader::new(),
            image_textures: std::collections::HashMap::new(),
            #[cfg(feature = "smart-cache")]
            page_cache: std::sync::Arc::new(
                alice_browser::net::cache::CachedFetcher::new(256),
            ),
            #[cfg(feature = "search")]
            search_query: String::new(),
            #[cfg(feature = "search")]
            search_index: None,
            #[cfg(feature = "telemetry")]
            metrics: alice_browser::telemetry::BrowserMetrics::new(),
            #[cfg(feature = "telemetry")]
            navigate_start: None,
            sdf_paint_state: alice_browser::render::sdf_paint::SdfPaintState::new(),
            paint_elements: None,
            #[cfg(feature = "sdf-render")]
            sdf_texture: None,
            #[cfg(feature = "sdf-render")]
            sdf_mode_rendered: None,
            #[cfg(feature = "sdf-render")]
            cam_params: alice_browser::render::sdf_renderer::CameraParams::default(),
            #[cfg(feature = "sdf-render")]
            cam_dirty: true,
            #[cfg(feature = "sdf-render")]
            cam_dragging: false,
            #[cfg(feature = "sdf-render")]
            spatial_scene: None,
            #[cfg(feature = "sdf-render")]
            gpu_renderer: alice_browser::render::gpu_renderer::GpuRenderer::new(),
            #[cfg(feature = "sdf-render")]
            stream_state: None,
            #[cfg(feature = "sdf-render")]
            oz_pending_url: None,
            #[cfg(feature = "sdf-render")]
            oz_preview: None,
            #[cfg(feature = "sdf-render")]
            oz_preview_rx: None,
            #[cfg(feature = "sdf-render")]
            oz_preview_for: None,
            #[cfg(feature = "sdf-render")]
            oz_hologram_screen_pos: None,
            #[cfg(feature = "sdf-render")]
            oz_hologram_alpha: 0.0,
            #[cfg(feature = "sdf-render")]
            oz_hologram_start: None,
            #[cfg(feature = "sdf-render")]
            oz_prefetch_rx: None,
            #[cfg(feature = "sdf-render")]
            oz_prefetch_started: false,
            #[cfg(feature = "sdf-render")]
            oz_prefetch_buffer: Vec::new(),
            app_start: std::time::Instant::now(),
            #[cfg(feature = "sdf-render")]
            last_frame_time: std::time::Instant::now(),
            adblock: Arc::new(AdBlockEngine::new()),
            block_stats: BlockStats::new(),
        }
    }
}
