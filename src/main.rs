use eframe::egui;
use std::sync::mpsc;
use std::sync::Arc;

use alice_browser::engine::pipeline::{BrowserEngine, PageError, PageResult};
use alice_browser::net::adblock::{AdBlockEngine, BlockStats};
use alice_browser::render::layout::LayoutNode;
use alice_browser::render::RenderMode;

/// Preview data fetched for a grabbed link
#[derive(Clone)]
struct LinkPreview {
    url: String,
    title: String,
    description: String,
    texts: Vec<String>,
    status: LinkPreviewStatus,
}

#[derive(Clone, PartialEq)]
enum LinkPreviewStatus {
    Loading,
    Ready,
    Error(String),
}

fn main() {
    env_logger::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };

    eframe::run_native(
        "ALICE Browser — The Web Recompiled",
        options,
        Box::new(|cc| {
            // Load Japanese font (Hiragino Sans on macOS)
            let mut fonts = egui::FontDefinitions::default();
            let font_paths = [
                "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
                "/System/Library/Fonts/HiraginoSans-W3.otf",
                "/System/Library/Fonts/ヒラギノ角ゴシック W4.ttc",
            ];
            for path in &font_paths {
                if let Ok(data) = std::fs::read(path) {
                    fonts.font_data.insert(
                        "japanese".to_owned(),
                        egui::FontData::from_owned(data),
                    );
                    fonts
                        .families
                        .get_mut(&egui::FontFamily::Proportional)
                        .unwrap()
                        .push("japanese".to_owned());
                    fonts
                        .families
                        .get_mut(&egui::FontFamily::Monospace)
                        .unwrap()
                        .push("japanese".to_owned());
                    break;
                }
            }
            cc.egui_ctx.set_fonts(fonts);

            Ok(Box::new(BrowserApp::default()))
        }),
    )
    .expect("Failed to start ALICE Browser");
}

struct BrowserApp {
    url_input: String,
    page: Option<PageResult>,
    error: Option<String>,
    loading: bool,
    fetch_rx: Option<mpsc::Receiver<Result<PageResult, PageError>>>,
    render_mode: RenderMode,
    show_stats: bool,
    dark_mode: bool,
    // History (back / forward)
    history: Vec<String>,
    history_idx: usize,
    // Image loading
    image_loader: alice_browser::net::image::ImageLoader,
    image_textures: std::collections::HashMap<String, egui::TextureHandle>,
    #[cfg(feature = "smart-cache")]
    page_cache: std::sync::Arc<alice_browser::net::cache::CachedFetcher>,
    #[cfg(feature = "search")]
    search_query: String,
    #[cfg(feature = "search")]
    search_index: Option<alice_browser::search::PageSearch>,
    #[cfg(feature = "telemetry")]
    metrics: alice_browser::telemetry::BrowserMetrics,
    #[cfg(feature = "telemetry")]
    navigate_start: Option<std::time::Instant>,
    sdf_paint_state: alice_browser::render::sdf_paint::SdfPaintState,
    paint_elements: Option<Vec<alice_browser::render::sdf_ui::PaintElement>>,
    #[cfg(feature = "sdf-render")]
    sdf_texture: Option<egui::TextureHandle>,
    #[cfg(feature = "sdf-render")]
    sdf_mode_rendered: Option<RenderMode>,
    // 3D camera state
    #[cfg(feature = "sdf-render")]
    cam_params: alice_browser::render::sdf_renderer::CameraParams,
    #[cfg(feature = "sdf-render")]
    cam_dirty: bool,
    #[cfg(feature = "sdf-render")]
    cam_dragging: bool,
    #[cfg(feature = "sdf-render")]
    spatial_scene: Option<alice_browser::render::sdf_ui::SdfScene>,
    #[cfg(feature = "sdf-render")]
    gpu_renderer: Option<alice_browser::render::gpu_renderer::GpuRenderer>,
    // OZ Stream state
    #[cfg(feature = "sdf-render")]
    stream_state: Option<alice_browser::render::stream::StreamState>,
    /// Pending URL from OZ mode double-click on a link
    #[cfg(feature = "sdf-render")]
    oz_pending_url: Option<String>,
    /// Link preview for grabbed text
    #[cfg(feature = "sdf-render")]
    oz_preview: Option<LinkPreview>,
    #[cfg(feature = "sdf-render")]
    oz_preview_rx: Option<mpsc::Receiver<LinkPreview>>,
    /// URL currently being previewed (to avoid re-fetching)
    #[cfg(feature = "sdf-render")]
    oz_preview_for: Option<String>,
    /// Screen position for hologram overlay (near grabbed particle)
    #[cfg(feature = "sdf-render")]
    oz_hologram_screen_pos: Option<egui::Pos2>,
    /// Hologram fade-in alpha (0.0 -> 1.0)
    #[cfg(feature = "sdf-render")]
    oz_hologram_alpha: f32,
    /// Hologram animation start time
    #[cfg(feature = "sdf-render")]
    oz_hologram_start: Option<std::time::Instant>,
    /// Background link prefetch receiver
    #[cfg(feature = "sdf-render")]
    oz_prefetch_rx: Option<mpsc::Receiver<Vec<alice_browser::render::stream::TextMeta>>>,
    /// Whether prefetch has been started for the current page
    #[cfg(feature = "sdf-render")]
    oz_prefetch_started: bool,
    /// Buffer for prefetched texts (accumulated before OZ mode is active)
    #[cfg(feature = "sdf-render")]
    oz_prefetch_buffer: Vec<alice_browser::render::stream::TextMeta>,
    app_start: std::time::Instant,
    #[cfg(feature = "sdf-render")]
    last_frame_time: std::time::Instant,
    // Ad blocker
    adblock: Arc<AdBlockEngine>,
    block_stats: BlockStats,
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

impl BrowserApp {
    fn go_back(&mut self, ctx: &egui::Context) {
        if self.history_idx > 0 {
            self.history_idx -= 1;
            self.url_input = self.history[self.history_idx].clone();
            self.navigate_no_history(ctx);
        }
    }

    fn go_forward(&mut self, ctx: &egui::Context) {
        if self.history_idx + 1 < self.history.len() {
            self.history_idx += 1;
            self.url_input = self.history[self.history_idx].clone();
            self.navigate_no_history(ctx);
        }
    }

    fn navigate(&mut self, ctx: &egui::Context) {
        // Push to history
        let url = self.url_input.clone();
        if self.history.is_empty() || self.history[self.history_idx] != url {
            // Truncate forward history
            self.history.truncate(self.history_idx + 1);
            self.history.push(url);
            self.history_idx = self.history.len() - 1;
        }
        self.navigate_no_history(ctx);
    }

    fn navigate_no_history(&mut self, ctx: &egui::Context) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.error = None;
        self.image_textures.clear();
        self.block_stats.reset_page();

        #[cfg(feature = "telemetry")]
        {
            self.navigate_start = Some(std::time::Instant::now());
        }

        let (tx, rx) = mpsc::channel();
        self.fetch_rx = Some(rx);

        let url = self.url_input.clone();
        let ctx = ctx.clone();

        #[cfg(feature = "smart-cache")]
        let cache = std::sync::Arc::clone(&self.page_cache);

        std::thread::spawn(move || {
            let engine = BrowserEngine::new(800.0);

            #[cfg(feature = "smart-cache")]
            let result = engine.load_page_cached(&url, &cache);

            #[cfg(not(feature = "smart-cache"))]
            let result = engine.load_page(&url);

            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    fn check_fetch(&mut self) {
        if let Some(rx) = &self.fetch_rx {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(page) => {
                        // Record telemetry
                        #[cfg(feature = "telemetry")]
                        {
                            let load_ms = self
                                .navigate_start
                                .map(|t| t.elapsed().as_secs_f64() * 1000.0)
                                .unwrap_or(0.0);
                            self.metrics.record_page_load(load_ms, &page.dom.url);
                            self.metrics.record_dom_stats(
                                page.filter_stats.total_nodes,
                                page.filter_stats.removed_nodes,
                            );
                            self.navigate_start = None;
                        }

                        // Build search index from page text
                        #[cfg(feature = "search")]
                        {
                            let full_text = page.dom.root.collect_text();
                            self.search_index =
                                Some(alice_browser::search::PageSearch::build(&full_text));
                            self.search_query.clear();
                        }

                        // Invalidate paint elements and SDF texture
                        self.paint_elements = None;
                        #[cfg(feature = "sdf-render")]
                        {
                            self.sdf_texture = None;
                            self.sdf_mode_rendered = None;
                            self.spatial_scene = None;
                            self.cam_dirty = true;
                        }

                        // Start background link prefetch immediately on page load
                        #[cfg(feature = "sdf-render")]
                        {
                            self.oz_prefetch_started = true;
                            self.oz_prefetch_buffer.clear();
                            let base_url = self.url_input.clone();
                            let hrefs = collect_hrefs_from_dom(&page.dom.root, &base_url, 10);
                            if !hrefs.is_empty() {
                                let (tx, rx) = mpsc::channel();
                                self.oz_prefetch_rx = Some(rx);
                                std::thread::spawn(move || {
                                    use alice_browser::net::fetch::fetch_url;
                                    use alice_browser::dom::parser::parse_html;
                                    use alice_browser::render::stream::TextMeta;

                                    for href in hrefs {
                                        let mut batch: Vec<TextMeta> = Vec::new();
                                        if let Ok(result) = fetch_url(&href) {
                                            let dom = parse_html(&result.html, &result.url);
                                            extract_prefetch_texts(
                                                &dom.root, &mut batch, 0,
                                            );
                                        }
                                        if !batch.is_empty() {
                                            if tx.send(batch).is_err() { break; }
                                        }
                                    }
                                });
                            }
                        }

                        self.page = Some(page);
                        self.error = None;
                    }
                    Err(e) => {
                        self.error = Some(e.to_string());
                        self.page = None;

                        #[cfg(feature = "search")]
                        {
                            self.search_index = None;
                        }
                    }
                }
                self.loading = false;
                self.fetch_rx = None;
            }
        }
    }

    fn draw_toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.add_space(4.0);

            // Back / Forward
            let can_back = self.history_idx > 0;
            let can_fwd = self.history_idx + 1 < self.history.len();
            if ui.add_enabled(can_back, egui::Button::new("\u{25C0}").min_size(egui::vec2(28.0, 24.0))).clicked() {
                self.go_back(ctx);
            }
            if ui.add_enabled(can_fwd, egui::Button::new("\u{25B6}").min_size(egui::vec2(28.0, 24.0))).clicked() {
                self.go_forward(ctx);
            }

            // URL bar
            let response = ui.add_sized(
                [ui.available_width() - 240.0, 24.0],
                egui::TextEdit::singleline(&mut self.url_input)
                    .hint_text("Enter URL...")
                    .font(egui::TextStyle::Monospace),
            );

            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.navigate(ctx);
            }

            if ui.button("Go").clicked() {
                self.navigate(ctx);
            }

            // Render mode selector
            let prev_mode = self.render_mode;
            egui::ComboBox::from_id_salt("render_mode")
                .selected_text(match self.render_mode {
                    RenderMode::Flat => "2D",
                    RenderMode::Sdf2D => "SDF",
                    RenderMode::Spatial3D => "3D",
                    RenderMode::OzMode => "OZ",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.render_mode, RenderMode::Flat, "2D Flat");
                    ui.selectable_value(&mut self.render_mode, RenderMode::Sdf2D, "SDF 2D");
                    ui.selectable_value(
                        &mut self.render_mode,
                        RenderMode::Spatial3D,
                        "3D Spatial",
                    );
                    ui.selectable_value(
                        &mut self.render_mode,
                        RenderMode::OzMode,
                        "OZ Orbital",
                    );
                });
            // Invalidate spatial scene when switching render modes
            #[cfg(feature = "sdf-render")]
            if self.render_mode != prev_mode {
                self.spatial_scene = None;
                self.stream_state = None;
                self.cam_dirty = true;
                self.oz_prefetch_started = false;
                self.oz_prefetch_rx = None;
                self.oz_prefetch_buffer.clear();
            }

            ui.toggle_value(&mut self.show_stats, "Stats");

            // Dark mode toggle
            let dark_label = if self.dark_mode { "\u{263E}" } else { "\u{2600}" };
            if ui.button(dark_label).clicked() {
                self.dark_mode = !self.dark_mode;
            }

            // Page search
            #[cfg(feature = "search")]
            if self.search_index.is_some() {
                ui.separator();
                ui.add_sized(
                    [120.0, 24.0],
                    egui::TextEdit::singleline(&mut self.search_query)
                        .hint_text("Find...")
                        .font(egui::TextStyle::Monospace),
                );
                if !self.search_query.is_empty() {
                    if let Some(ref idx) = self.search_index {
                        let count = idx.count(&self.search_query);
                        ui.colored_label(
                            if count > 0 {
                                egui::Color32::from_rgb(0, 180, 0)
                            } else {
                                egui::Color32::from_rgb(255, 80, 80)
                            },
                            format!("{}", count),
                        );
                    }
                }
            }
        });
    }

    fn draw_sdf_paint(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<String> {
        // Lazily generate paint elements
        if self.paint_elements.is_none() {
            if let Some(ref page) = self.page {
                self.paint_elements =
                    Some(alice_browser::render::sdf_ui::layout_to_paint(&page.layout));
            }
        }

        // Request images for any image placeholders
        if let Some(ref elems) = self.paint_elements {
            for elem in elems {
                if let Some(ref url) = elem.image_url {
                    self.image_loader.request(url);
                }
            }
        }

        let dark_mode = self.dark_mode;
        let paint_state = &mut self.sdf_paint_state;
        let elements = &self.paint_elements;
        let textures = &self.image_textures;

        if let Some(ref elems) = elements {
            paint_state.paint(ui, ctx, elems, dark_mode, textures)
        } else {
            None
        }
    }

    #[cfg(feature = "sdf-render")]
    fn draw_sdf_content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use alice_browser::render::sdf_renderer::{render_sdf_interactive, auto_camera};

        // Build spatial scene lazily
        if self.spatial_scene.is_none() {
            if let Some(ref page) = self.page {
                if self.render_mode == RenderMode::OzMode {
                    // OZ "The Stream" Mode: cylindrical immersion
                    let stream = alice_browser::render::stream::StreamState::from_layout(&page.layout);
                    let scene = stream.to_sdf_scene();
                    // Camera: user at center of cylinder, looking outward
                    self.cam_params = alice_browser::render::sdf_renderer::CameraParams {
                        azimuth: 0.0,
                        elevation: 0.0,
                        distance: 0.0, // unused for OZ
                        target: [0.0, 0.0, 0.0],
                    };
                    self.spatial_scene = Some(scene);
                    self.stream_state = Some(stream);
                    self.last_frame_time = std::time::Instant::now();

                    // Inject any prefetched texts that arrived while in another mode
                    if !self.oz_prefetch_buffer.is_empty() {
                        if let Some(ref mut ss) = self.stream_state {
                            ss.append_texts(self.oz_prefetch_buffer.drain(..).collect());
                        }
                    }
                } else {
                    // Spatial3D: Deep Web corridor layout
                    let scene = alice_browser::render::spatial::layout_to_spatial(
                        &page.layout,
                        &alice_browser::render::spatial::SpatialConfig::default(),
                    );
                    self.cam_params = auto_camera(&scene);
                    self.spatial_scene = Some(scene);
                    self.stream_state = None;
                };
                self.cam_dirty = true;
                if let Some(ref mut gpu) = self.gpu_renderer {
                    gpu.invalidate();
                }
            }
        }

        // OZ "The Stream": update particles every frame (tunnel flow)
        if self.render_mode == RenderMode::OzMode {
            if let Some(ref mut stream) = self.stream_state {
                let now = std::time::Instant::now();
                let dt = (now - self.last_frame_time).as_secs_f32().min(0.1);
                self.last_frame_time = now;
                stream.update_flow(dt);
                ctx.request_repaint();
            }

            // Animate hologram fade-in
            if let Some(start) = self.oz_hologram_start {
                let elapsed = start.elapsed().as_secs_f32();
                self.oz_hologram_alpha = (elapsed / 0.3).clamp(0.0, 1.0); // 0.3s fade-in
            }
        }

        // Handle mouse interaction
        let response = ui.allocate_response(
            ui.available_size(),
            egui::Sense::click_and_drag().union(egui::Sense::hover()),
        );

        if self.render_mode == RenderMode::OzMode {
            // OZ: drag to look around inside the cylinder
            if response.dragged() {
                let delta = response.drag_delta();
                self.cam_params.azimuth -= delta.x * 0.005; // horizontal look
                self.cam_params.elevation = (self.cam_params.elevation + delta.y * 0.005)
                    .clamp(-0.8, 0.8); // vertical look (limited)
            }

            // OZ: click to grab nearest text
            if response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let rect = response.rect;
                    let fov: f32 = 110.0_f32.to_radians();
                    let fov_h = fov * 0.5;
                    let aspect = rect.width() / rect.height();
                    let fov_v = fov_h / aspect;

                    // Click position in NDC [-1, 1]
                    let ndc_x = (pos.x - rect.center().x) / (rect.width() * 0.5);
                    let ndc_y = (pos.y - rect.center().y) / (rect.height() * 0.5);

                    if let Some(ref mut stream) = self.stream_state {
                        stream.try_grab_screen(
                            ndc_x, ndc_y,
                            self.cam_params.azimuth,
                            self.cam_params.elevation,
                            fov_h, fov_v,
                            aspect,
                        );

                        // Start preview fetch & set hologram position
                        if let Some(info) = stream.grabbed_info() {
                            // Compute screen position of grabbed particle for hologram anchor
                            self.oz_hologram_screen_pos = Some(pos);
                            self.oz_hologram_alpha = 0.0;
                            self.oz_hologram_start = Some(std::time::Instant::now());

                            let fetch_url = if let Some(ref href) = info.meta.href {
                                resolve_url(&self.url_input, href)
                            } else {
                                let query = info.meta.display.trim().to_string();
                                if query.len() > 1 {
                                    format!(
                                        "https://www.google.com/search?q={}",
                                        query.replace(' ', "+")
                                    )
                                } else {
                                    String::new()
                                }
                            };

                            if !fetch_url.is_empty()
                                && self.oz_preview_for.as_deref() != Some(&fetch_url)
                            {
                                self.oz_preview_for = Some(fetch_url.clone());
                                self.oz_preview = Some(LinkPreview {
                                    url: fetch_url.clone(),
                                    title: String::new(),
                                    description: String::new(),
                                    texts: Vec::new(),
                                    status: LinkPreviewStatus::Loading,
                                });
                                let (tx, rx) = mpsc::channel();
                                self.oz_preview_rx = Some(rx);
                                std::thread::spawn(move || {
                                    let preview = fetch_link_preview(&fetch_url);
                                    let _ = tx.send(preview);
                                });
                            }
                        } else {
                            // Grab failed: clear hologram state
                            self.oz_hologram_screen_pos = None;
                            self.oz_hologram_alpha = 0.0;
                            self.oz_hologram_start = None;
                            self.oz_preview = None;
                            self.oz_preview_for = None;
                            self.oz_preview_rx = None;
                        }
                    }
                }
            }

            // OZ: double-click on grabbed link → navigate
            if response.double_clicked() {
                if let Some(ref stream) = self.stream_state {
                    if let Some(info) = stream.grabbed_info() {
                        if let Some(ref href) = info.meta.href {
                            self.oz_pending_url = Some(href.clone());
                        }
                    }
                }
            }
        } else {
            // Spatial3D: drag to rotate camera around scene
            if response.dragged() {
                let delta = response.drag_delta();
                self.cam_params.azimuth += delta.x * 0.008;
                self.cam_params.elevation = (self.cam_params.elevation - delta.y * 0.008)
                    .clamp(0.05, std::f32::consts::FRAC_PI_2 - 0.05);
                self.cam_dirty = true;
                self.cam_dragging = true;
            } else {
                self.cam_dragging = false;
            }

            // Scroll to dolly in/out (zoom)
            if response.hovered() {
                let scroll = ui.input(|i| i.raw_scroll_delta.y);
                if scroll.abs() > 0.1 {
                    self.cam_params.distance *= 1.0 - scroll * 0.003;
                    self.cam_params.distance = self.cam_params.distance.clamp(0.2, 100.0);
                    self.cam_dirty = true;
                }
            }
        }

        // Render (Spatial3D only — OZ uses egui overlay, no raymarching)
        if self.render_mode != RenderMode::OzMode {
            if self.cam_dirty || self.sdf_texture.is_none() {
                if let Some(ref scene) = self.spatial_scene {
                    let has_gpu = self.gpu_renderer.is_some();
                    let (w, h) = if self.cam_dragging {
                        if has_gpu { (640, 480) } else { (240, 180) }
                    } else {
                        if has_gpu { (1280, 960) } else { (640, 480) }
                    };

                    let pixels = self
                        .gpu_renderer
                        .as_mut()
                        .and_then(|gpu| gpu.render(scene, w, h, &self.cam_params))
                        .or_else(|| render_sdf_interactive(scene, w, h, &self.cam_params));

                    if let Some(pixels) = pixels {
                        let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &pixels);
                        self.sdf_texture = Some(ctx.load_texture(
                            "sdf_view",
                            image,
                            egui::TextureOptions::LINEAR,
                        ));
                        self.sdf_mode_rendered = Some(self.render_mode);
                    }
                    self.cam_dirty = false;
                    if self.cam_dragging {
                        ctx.request_repaint();
                    }
                }
            }
        }

        // Draw background
        if self.render_mode == RenderMode::OzMode {
            ui.painter().rect_filled(response.rect, 0.0, egui::Color32::WHITE);
        } else if let Some(ref tex) = self.sdf_texture {
            ui.painter().image(
                tex.id(),
                response.rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        } else {
            ui.colored_label(egui::Color32::GRAY, "SDF scene is empty");
        }

        // OZ Rotunda: perspective-project cylinder wall text onto screen
        if self.render_mode == RenderMode::OzMode {
            if let Some(ref stream) = self.stream_state {
                use alice_browser::render::stream::StreamState;

                let rect = response.rect;
                let painter = ui.painter_at(rect);
                let fov: f32 = 110.0_f32.to_radians();
                let fov_h = fov * 0.5;
                let tan_fov_h = fov_h.tan();
                let aspect = rect.width() / rect.height();
                let cam_az = self.cam_params.azimuth;
                let cam_el = self.cam_params.elevation;
                let time = stream.time;

                let sin_az = cam_az.sin();
                let cos_az = cam_az.cos();
                let sin_el = cam_el.sin();
                let cos_el = cam_el.cos();

                for p in &stream.particles {
                    let world = StreamState::particle_world_pos(p, time);

                    let wx = world[0];
                    let wy = world[1];
                    let wz = world[2];

                    // Camera rotation: azimuth (Y-axis) then elevation (X-axis)
                    let rx1 = wx * cos_az + wz * sin_az;
                    let ry1 = wy;
                    let rz1 = -wx * sin_az + wz * cos_az;

                    let rx = rx1;
                    let ry = ry1 * cos_el - rz1 * sin_el;
                    let rz = ry1 * sin_el + rz1 * cos_el;

                    // Skip particles behind camera (cylinder wall is at R=12)
                    if rz < 1.0 { continue; }

                    // Perspective projection
                    let ndc_x = rx / (rz * tan_fov_h);
                    let ndc_y = -ry / (rz * tan_fov_h / aspect);

                    // Cull off-screen
                    if ndc_x.abs() > 1.3 || ndc_y.abs() > 1.3 { continue; }

                    let sx = rect.center().x + ndc_x * rect.width() * 0.5;
                    let sy = rect.center().y + ndc_y * rect.height() * 0.5;

                    let cat_color = stream.categories.get(p.category_index)
                        .map(|c| c.color)
                        .unwrap_or([0.3, 0.3, 0.3, 1.0]);

                    let alpha = StreamState::particle_opacity(p);
                    if alpha < 0.01 { continue; }

                    // Font size: layer-based + importance + perspective
                    let layer_scale = StreamState::layer_font_scale(p.layer);
                    let depth_scale = (12.0 / rz).clamp(0.5, 2.0);
                    let base_font: f32 = (13.0 + p.importance * 14.0) * layer_scale * depth_scale;
                    let grabbed_scale: f32 = if p.grabbed { 1.4 } else { 1.0 };
                    let font_size = (base_font * grabbed_scale).clamp(8.0_f32, 48.0);

                    let r = (cat_color[0] * 255.0) as u8;
                    let g = (cat_color[1] * 255.0) as u8;
                    let b = (cat_color[2] * 255.0) as u8;
                    let a = (alpha * 255.0) as u8;
                    let color = egui::Color32::from_rgba_unmultiplied(r, g, b, a);

                    painter.text(
                        egui::pos2(sx, sy),
                        egui::Align2::CENTER_CENTER,
                        &p.text,
                        egui::FontId::proportional(font_size),
                        color,
                    );

                    // Grabbed: highlight background
                    if p.grabbed {
                        let text_w = p.text.chars().count().min(30) as f32 * font_size * 0.55;
                        let pad = 4.0;
                        let bg_rect = egui::Rect::from_center_size(
                            egui::pos2(sx, sy),
                            egui::vec2(text_w + pad * 2.0, font_size + pad * 2.0),
                        );
                        painter.rect(
                            bg_rect,
                            4.0,
                            egui::Color32::from_rgba_unmultiplied(r, g, b, 20),
                            egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(r, g, b, 160)),
                        );
                    }
                }

                // ── Hologram Overlay (floating near grabbed particle) ──
                if let Some(info) = stream.grabbed_info() {
                    let holo_alpha = self.oz_hologram_alpha;
                    if holo_alpha > 0.01 {
                        let has_href = info.meta.href.is_some();
                        let has_preview = self.oz_preview.as_ref()
                            .map(|p| p.status != LinkPreviewStatus::Loading || !p.title.is_empty())
                            .unwrap_or(false);
                        let is_loading = self.oz_preview.as_ref()
                            .map(|p| p.status == LinkPreviewStatus::Loading)
                            .unwrap_or(false);

                        // Dynamic panel sizing
                        let has_desc = self.oz_preview.as_ref()
                            .map(|p| !p.description.is_empty())
                            .unwrap_or(false);
                        let preview_lines = if has_preview {
                            self.oz_preview.as_ref().map(|p| p.texts.len().min(12)).unwrap_or(0)
                        } else {
                            0
                        };
                        let panel_w = 500.0_f32.min(rect.width() - 40.0);
                        let base_h = 50.0_f32;
                        let link_h = if has_href || is_loading || has_preview { 22.0_f32 } else { 0.0 };
                        let desc_h = if has_desc { 36.0_f32 } else { 0.0 };
                        let preview_h = if has_preview || is_loading {
                            24.0 + desc_h + preview_lines as f32 * 17.0
                        } else {
                            0.0
                        };
                        let panel_h = (base_h + link_h + preview_h).min(rect.height() * 0.55);

                        // Smart placement: anchor near hologram_screen_pos
                        let anchor = self.oz_hologram_screen_pos.unwrap_or(rect.center());
                        let panel_x = (anchor.x - panel_w * 0.5)
                            .clamp(rect.left() + 8.0, rect.right() - panel_w - 8.0);
                        let panel_y = if anchor.y < rect.center().y {
                            // Particle in upper half -> show panel below
                            (anchor.y + 30.0).min(rect.bottom() - panel_h - 8.0)
                        } else {
                            // Particle in lower half -> show panel above
                            (anchor.y - panel_h - 30.0).max(rect.top() + 8.0)
                        };

                        let panel_rect = egui::Rect::from_min_size(
                            egui::pos2(panel_x, panel_y),
                            egui::vec2(panel_w, panel_h),
                        );

                        let cat_color = stream.categories.get(info.particle.category_index)
                            .map(|c| c.color)
                            .unwrap_or([0.3, 0.3, 0.3, 1.0]);
                        let cr = (cat_color[0] * 255.0) as u8;
                        let cg = (cat_color[1] * 255.0) as u8;
                        let cb = (cat_color[2] * 255.0) as u8;
                        let accent = egui::Color32::from_rgba_unmultiplied(
                            cr, cg, cb, (holo_alpha * 255.0) as u8);
                        let bg_alpha = (holo_alpha * 235.0) as u8;

                        // ── Cyber hologram background ──
                        // Glow shadow
                        painter.rect_filled(
                            panel_rect.expand(3.0),
                            6.0,
                            egui::Color32::from_rgba_unmultiplied(cr, cg, cb, (holo_alpha * 30.0) as u8),
                        );

                        // Main background
                        painter.rect(
                            panel_rect,
                            4.0,
                            egui::Color32::from_rgba_unmultiplied(250, 250, 255, bg_alpha),
                            egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(
                                cr, cg, cb, (holo_alpha * 180.0) as u8)),
                        );

                        // Top scanline accent
                        painter.rect_filled(
                            egui::Rect::from_min_size(
                                panel_rect.left_top(),
                                egui::vec2(panel_w, 2.0),
                            ),
                            0.0,
                            accent,
                        );

                        // Corner brackets (cyber decoration)
                        let bk_len = 12.0;
                        let bk_stroke = egui::Stroke::new(1.5, accent);
                        // Top-left
                        painter.line_segment([panel_rect.left_top(), panel_rect.left_top() + egui::vec2(bk_len, 0.0)], bk_stroke);
                        painter.line_segment([panel_rect.left_top(), panel_rect.left_top() + egui::vec2(0.0, bk_len)], bk_stroke);
                        // Top-right
                        painter.line_segment([panel_rect.right_top(), panel_rect.right_top() + egui::vec2(-bk_len, 0.0)], bk_stroke);
                        painter.line_segment([panel_rect.right_top(), panel_rect.right_top() + egui::vec2(0.0, bk_len)], bk_stroke);
                        // Bottom-left
                        painter.line_segment([panel_rect.left_bottom(), panel_rect.left_bottom() + egui::vec2(bk_len, 0.0)], bk_stroke);
                        painter.line_segment([panel_rect.left_bottom(), panel_rect.left_bottom() + egui::vec2(0.0, -bk_len)], bk_stroke);
                        // Bottom-right
                        painter.line_segment([panel_rect.right_bottom(), panel_rect.right_bottom() + egui::vec2(-bk_len, 0.0)], bk_stroke);
                        painter.line_segment([panel_rect.right_bottom(), panel_rect.right_bottom() + egui::vec2(0.0, -bk_len)], bk_stroke);

                        let text_alpha = (holo_alpha * 255.0) as u8;
                        let left = panel_rect.left() + 16.0;
                        let mut y = panel_rect.top() + 12.0;

                        // ── Header: dot + category + tag badge ──
                        painter.circle_filled(egui::pos2(left + 2.0, y + 6.0), 5.0, accent);
                        painter.text(
                            egui::pos2(left + 12.0, y),
                            egui::Align2::LEFT_TOP,
                            info.category_name,
                            egui::FontId::proportional(12.0),
                            accent,
                        );
                        let tag_text = match info.meta.tag.as_str() {
                            "h1" | "h2" => "HEADING",
                            "h3" | "h4" | "h5" | "h6" => "SUBHEAD",
                            "a" => "LINK", "p" => "TEXT", "li" => "LIST",
                            "span" => "SPAN", "button" => "BUTTON",
                            "" => "TEXT", other => other,
                        };
                        let tag_x = left + 14.0
                            + info.category_name.chars().count().min(16) as f32 * 7.5;
                        let tag_bg = egui::Rect::from_min_size(
                            egui::pos2(tag_x, y - 1.0),
                            egui::vec2(tag_text.len() as f32 * 7.0 + 10.0, 16.0),
                        );
                        painter.rect_filled(tag_bg, 8.0,
                            egui::Color32::from_rgba_unmultiplied(cr, cg, cb, (holo_alpha * 25.0) as u8));
                        painter.text(tag_bg.center(), egui::Align2::CENTER_CENTER,
                            tag_text, egui::FontId::proportional(10.0),
                            egui::Color32::from_rgba_unmultiplied(cr, cg, cb, (holo_alpha * 200.0) as u8));

                        // Selected text
                        y += 20.0;
                        let max_chars = ((panel_w - 40.0) / 8.5) as usize;
                        let display_text = if info.meta.full_text.chars().count() > max_chars {
                            let t: String = info.meta.full_text.chars().take(max_chars).collect();
                            format!("{}...", t)
                        } else {
                            info.meta.full_text.clone()
                        };
                        painter.text(
                            egui::pos2(left, y), egui::Align2::LEFT_TOP,
                            &display_text,
                            egui::FontId::proportional(14.0),
                            egui::Color32::from_rgba_unmultiplied(25, 25, 25, text_alpha),
                        );
                        y += 22.0;

                        // Separator
                        painter.line_segment(
                            [egui::pos2(left, y), egui::pos2(panel_rect.right() - 16.0, y)],
                            egui::Stroke::new(0.5, egui::Color32::from_rgba_unmultiplied(
                                cr, cg, cb, (holo_alpha * 60.0) as u8)),
                        );
                        y += 6.0;

                        // URL (if link)
                        if let Some(ref href) = info.meta.href {
                            let link_display = truncate_str(href, 70);
                            painter.text(
                                egui::pos2(left, y), egui::Align2::LEFT_TOP,
                                &format!("\u{2197} {}", link_display),
                                egui::FontId::proportional(11.0),
                                egui::Color32::from_rgba_unmultiplied(0, 100, 200, text_alpha),
                            );
                            y += 16.0;
                        }

                        // Preview content
                        if let Some(ref preview) = self.oz_preview {
                            if preview.status == LinkPreviewStatus::Loading {
                                painter.text(
                                    egui::pos2(left, y), egui::Align2::LEFT_TOP,
                                    "Loading preview...",
                                    egui::FontId::proportional(12.0),
                                    egui::Color32::from_rgba_unmultiplied(80, 80, 80, (holo_alpha * 200.0) as u8),
                                );
                            } else if let LinkPreviewStatus::Error(ref e) = preview.status {
                                painter.text(
                                    egui::pos2(left, y), egui::Align2::LEFT_TOP,
                                    &format!("Error: {}", e),
                                    egui::FontId::proportional(11.0),
                                    egui::Color32::from_rgba_unmultiplied(200, 60, 60, text_alpha),
                                );
                            } else {
                                let max_y = panel_rect.bottom() - 20.0;
                                let text_max_chars = ((panel_w - 40.0) / 7.0) as usize;

                                if !preview.title.is_empty() && y < max_y {
                                    let title_display = truncate_str(&preview.title, text_max_chars);
                                    painter.text(
                                        egui::pos2(left, y), egui::Align2::LEFT_TOP,
                                        &title_display,
                                        egui::FontId::proportional(14.0),
                                        egui::Color32::from_rgba_unmultiplied(15, 15, 15, text_alpha),
                                    );
                                    y += 20.0;
                                }

                                if !preview.description.is_empty() && y < max_y {
                                    let desc_chars: Vec<char> = preview.description.chars().collect();
                                    let mut offset = 0;
                                    for _ in 0..3 {
                                        if offset >= desc_chars.len() || y >= max_y { break; }
                                        let end = (offset + text_max_chars).min(desc_chars.len());
                                        let mut line: String = desc_chars[offset..end].iter().collect();
                                        if end < desc_chars.len() {
                                            line.push_str("...");
                                        }
                                        painter.text(
                                            egui::pos2(left, y), egui::Align2::LEFT_TOP,
                                            &line,
                                            egui::FontId::proportional(12.0),
                                            egui::Color32::from_rgba_unmultiplied(50, 50, 70, text_alpha),
                                        );
                                        y += 16.0;
                                        offset = end;
                                    }
                                    y += 4.0;
                                }

                                for (i, text) in preview.texts.iter().take(12).enumerate() {
                                    if y >= max_y { break; }
                                    let line = truncate_str(text, text_max_chars);
                                    let line_alpha = if i < 4 { 230 } else if i < 8 { 180 } else { 140 };
                                    let fa = ((line_alpha as f32 / 255.0) * holo_alpha * 255.0) as u8;
                                    let font_size = if i < 3 { 12.0 } else { 11.0 };
                                    painter.text(
                                        egui::pos2(left, y), egui::Align2::LEFT_TOP,
                                        &line,
                                        egui::FontId::proportional(font_size),
                                        egui::Color32::from_rgba_unmultiplied(40, 40, 40, fa),
                                    );
                                    y += 17.0;
                                }
                            }
                        }

                        // Hint
                        if info.meta.href.is_some() {
                            painter.text(
                                egui::pos2(panel_rect.right() - 16.0, panel_rect.bottom() - 16.0),
                                egui::Align2::RIGHT_BOTTOM,
                                "Double-click to open",
                                egui::FontId::proportional(10.0),
                                egui::Color32::from_rgba_unmultiplied(120, 120, 120, (holo_alpha * 150.0) as u8),
                            );
                        }
                    }
                }
            }
        }

        // Camera info overlay
        if self.render_mode == RenderMode::OzMode {
            ui.painter().text(
                response.rect.left_bottom() + egui::vec2(8.0, -8.0),
                egui::Align2::LEFT_BOTTOM,
                "Drag: look around | Click: select | Double-click link: open",
                egui::FontId::proportional(12.0),
                egui::Color32::from_rgba_unmultiplied(120, 120, 130, 180),
            );
        } else {
            ui.painter().text(
                response.rect.left_bottom() + egui::vec2(8.0, -8.0),
                egui::Align2::LEFT_BOTTOM,
                format!(
                    "Drag: rotate | Scroll: zoom | d={:.1}",
                    self.cam_params.distance
                ),
                egui::FontId::proportional(12.0),
                egui::Color32::from_rgba_premultiplied(255, 255, 255, 180),
            );
        }
    }

    fn draw_content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.loading {
            ui.centered_and_justified(|ui| {
                ui.spinner();
            });
            return;
        }

        if let Some(ref error) = self.error {
            ui.colored_label(egui::Color32::RED, error);
            return;
        }

        // SDF Paint mode (interactive)
        if self.render_mode == RenderMode::Sdf2D && self.page.is_some() {
            let clicked = self.draw_sdf_paint(ui, ctx);
            if let Some(href) = clicked {
                let base = self.page.as_ref().map(|p| p.dom.url.as_str()).unwrap_or("");
                self.url_input = resolve_url(base, &href);
                self.navigate(ctx);
            }
            return;
        }

        // Raymarched 3D mode (Spatial3D or OzMode)
        #[cfg(feature = "sdf-render")]
        if (self.render_mode == RenderMode::Spatial3D
            || self.render_mode == RenderMode::OzMode)
            && self.page.is_some()
        {
            self.draw_sdf_content(ui, ctx);
            return;
        }

        if let Some(ref page) = self.page {
            // Page title
            if !page.dom.title.is_empty() {
                ui.heading(&page.dom.title);
                ui.separator();
            }

            let mut clicked_link: Option<String> = None;
            let base_url = page.dom.url.clone();

            #[cfg(feature = "search")]
            let highlight = if self.search_query.is_empty() {
                None
            } else {
                Some(self.search_query.as_str())
            };
            #[cfg(not(feature = "search"))]
            let highlight: Option<&str> = None;

            egui::ScrollArea::vertical().show(ui, |ui| {
                render_layout_node(ui, &page.layout, 0, &mut clicked_link, highlight);
            });

            // Navigate to clicked link
            if let Some(href) = clicked_link {
                let resolved = resolve_url(&base_url, &href);
                self.url_input = resolved;
                self.navigate(ctx);
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);
                    ui.heading("ALICE Browser");
                    ui.label("The Web Recompiled");
                    ui.add_space(20.0);
                    ui.label("Enter a URL and press Enter");
                });
            });
        }
    }

    fn draw_stats_panel(&self, ui: &mut egui::Ui) {
        if let Some(ref page) = self.page {
            let stats = &page.filter_stats;

            ui.heading("ALICE-AdBlock");
            ui.separator();

            ui.label(format!("Total nodes: {}", stats.total_nodes));

            ui.colored_label(
                egui::Color32::from_rgb(0, 180, 0),
                format!("Content: {}", stats.content_nodes),
            );
            ui.colored_label(
                egui::Color32::from_rgb(255, 80, 80),
                format!("Ads blocked: {}", stats.ad_nodes),
            );
            ui.colored_label(
                egui::Color32::from_rgb(255, 160, 0),
                format!("Trackers blocked: {}", stats.tracker_nodes),
            );
            ui.colored_label(
                egui::Color32::from_rgb(100, 150, 255),
                format!("Navigation: {}", stats.nav_nodes),
            );

            ui.separator();
            ui.label(format!("Removed: {} nodes", stats.removed_nodes));

            if stats.total_nodes > 0 {
                let pct = (stats.removed_nodes as f32 / stats.total_nodes as f32) * 100.0;
                ui.label(format!("Reduction: {:.1}%", pct));
            }

            ui.separator();
            ui.heading("Page Info");
            ui.label(format!("Title: {}", page.dom.title));
            ui.label(format!("URL: {}", page.dom.url));
            ui.label(format!("HTTP: {}", page.fetch_status));

            ui.separator();
            ui.heading("SDF Scene");
            ui.label(format!("Primitives: {}", page.sdf_scene.primitives.len()));

            #[cfg(feature = "sdf-render")]
            {
                ui.label(format!(
                    "Render: {}",
                    match self.render_mode {
                        RenderMode::Flat => "Off (2D Flat)",
                        RenderMode::Sdf2D => "ALICE-SDF 2D",
                        RenderMode::Spatial3D => "ALICE-SDF 3D",
                        RenderMode::OzMode => "OZ Orbital",
                    }
                ));
                if self.render_mode == RenderMode::Spatial3D
                    || self.render_mode == RenderMode::OzMode
                {
                    if let Some(ref scene) = self.spatial_scene {
                        ui.label(format!("3D Primitives: {}", scene.primitives.len()));
                    }
                    let res = if self.cam_dragging { "240x180" } else { "640x480" };
                    if self.sdf_texture.is_some() {
                        ui.colored_label(
                            egui::Color32::from_rgb(0, 180, 0),
                            format!("Raymarched: {}", res),
                        );
                    }
                    ui.label(format!("Cam dist: {:.2}", self.cam_params.distance));
                } else if self.sdf_texture.is_some() {
                    ui.colored_label(
                        egui::Color32::from_rgb(0, 180, 0),
                        "Raymarched: 640x480",
                    );
                }
            }
        }

        #[cfg(feature = "search")]
        if let Some(ref idx) = self.search_index {
            ui.separator();
            ui.heading("ALICE-Search");
            ui.label(format!("Indexed: {} bytes", idx.text_len()));
            if !self.search_query.is_empty() {
                ui.label(format!("Query: \"{}\"", self.search_query));
                ui.label(format!("Matches: {}", idx.count(&self.search_query)));
            }
        }

        #[cfg(feature = "smart-cache")]
        {
            ui.separator();
            ui.heading("ALICE-Cache");
            ui.label(format!("Cached: {} pages", self.page_cache.cached_pages()));
            ui.label(format!(
                "Hit rate: {:.1}%",
                self.page_cache.hit_rate() * 100.0
            ));
        }

        #[cfg(feature = "telemetry")]
        {
            let snap = self.metrics.snapshot();
            ui.separator();
            ui.heading("ALICE-Analytics");
            ui.label(format!("Pages loaded: {}", snap.page_loads));
            if snap.page_loads > 0 {
                ui.label(format!("P50 load: {:.0} ms", snap.p50_load_ms));
                ui.label(format!("P99 load: {:.0} ms", snap.p99_load_ms));
            }
            ui.label(format!("Domains: ~{:.0}", snap.unique_domains));
            ui.label(format!("Total blocked: {}", snap.total_blocked));
        }
    }
}

/// Collect unique hrefs from a DomNode tree, resolved to absolute URLs.
fn collect_hrefs_from_dom(
    node: &alice_browser::dom::DomNode,
    base_url: &str,
    limit: usize,
) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut hrefs = Vec::new();
    collect_hrefs_recursive(node, base_url, limit, &mut seen, &mut hrefs);
    hrefs
}

fn collect_hrefs_recursive(
    node: &alice_browser::dom::DomNode,
    base_url: &str,
    limit: usize,
    seen: &mut std::collections::HashSet<String>,
    out: &mut Vec<String>,
) {
    if out.len() >= limit { return; }
    if node.tag == "a" {
        if let Some(href) = node.attributes.get("href") {
            let abs = resolve_url(base_url, href);
            if abs.starts_with("http") && seen.insert(abs.clone()) {
                out.push(abs);
                if out.len() >= limit { return; }
            }
        }
    }
    for child in &node.children {
        collect_hrefs_recursive(child, base_url, limit, seen, out);
        if out.len() >= limit { return; }
    }
}

/// Extract texts from a prefetched page as TextMeta for injection into the Rotunda.
fn extract_prefetch_texts(
    node: &alice_browser::dom::DomNode,
    out: &mut Vec<alice_browser::render::stream::TextMeta>,
    depth: usize,
) {
    use alice_browser::dom::Classification;
    use alice_browser::render::stream::TextMeta;

    if out.len() >= 60 { return; }
    if depth > 20 { return; }

    // Skip non-content
    if matches!(
        node.classification,
        Classification::Advertisement | Classification::Tracker | Classification::Decoration
    ) {
        return;
    }

    let tag = node.tag.as_str();
    let (importance, is_leaf) = match tag {
        "h1" | "h2" => (0.9, true),
        "h3" | "h4" | "h5" | "h6" => (0.5, true),
        "a" => (0.4, true),
        "p" | "li" => (0.2, true),
        "span" | "em" | "strong" | "b" | "i" | "u" | "small" => (0.15, true),
        "td" | "th" | "dt" | "dd" | "figcaption" | "summary" | "time" => (0.15, true),
        _ => (0.1, false),
    };

    if is_leaf {
        let full = collect_dom_text(node);
        let trimmed = full.trim();
        if trimmed.len() > 1 && trimmed.chars().count() <= 80 {
            let display: String = trimmed.chars().take(40).collect();
            let href = node.attributes.get("href").cloned();
            out.push(TextMeta {
                display,
                full_text: trimmed.chars().take(300).collect(),
                tag: tag.to_string(),
                href,
                category_index: 0,
                importance,
            });
        }
        return;
    }

    // Recurse
    for child in &node.children {
        extract_prefetch_texts(child, out, depth + 1);
    }
}

/// Fetch a URL and extract preview info (title + description + key texts).
/// Runs in a background thread.
fn fetch_link_preview(url: &str) -> LinkPreview {
    use alice_browser::net::fetch::fetch_url;
    use alice_browser::dom::parser::parse_html;

    match fetch_url(url) {
        Ok(result) => {
            let dom = parse_html(&result.html, &result.url);
            let title = if dom.title.is_empty() {
                url.to_string()
            } else {
                dom.title.clone()
            };

            // Extract meta description from <meta name="description"> or og:description
            let description = extract_meta_description(&dom.root);

            // Extract text content: prioritize headings and paragraphs
            let mut headings = Vec::new();
            let mut paragraphs = Vec::new();
            let mut others = Vec::new();
            extract_preview_texts_ranked(&dom.root, &mut headings, &mut paragraphs, &mut others, 0);

            // Compose final text list: headings first, then paragraphs, then others
            let mut texts = Vec::new();
            for t in &headings { if texts.len() < 50 { texts.push(t.clone()); } }
            for t in &paragraphs { if texts.len() < 50 { texts.push(t.clone()); } }
            for t in &others { if texts.len() < 50 { texts.push(t.clone()); } }

            LinkPreview {
                url: url.to_string(),
                title,
                description,
                texts,
                status: LinkPreviewStatus::Ready,
            }
        }
        Err(e) => LinkPreview {
            url: url.to_string(),
            title: String::new(),
            description: String::new(),
            texts: Vec::new(),
            status: LinkPreviewStatus::Error(e.to_string()),
        },
    }
}

/// Extract meta description from DOM (checks <meta name="description"> and og:description).
fn extract_meta_description(node: &alice_browser::dom::DomNode) -> String {
    if node.tag == "meta" {
        let name = node.attributes.get("name").map(|s| s.to_lowercase());
        let property = node.attributes.get("property").map(|s| s.to_lowercase());
        let is_desc = name.as_deref() == Some("description")
            || property.as_deref() == Some("og:description");
        if is_desc {
            if let Some(content) = node.attributes.get("content") {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
    }
    for child in &node.children {
        let desc = extract_meta_description(child);
        if !desc.is_empty() {
            return desc;
        }
    }
    String::new()
}

/// Extract texts ranked by importance: headings, paragraphs, others.
fn extract_preview_texts_ranked(
    node: &alice_browser::dom::DomNode,
    headings: &mut Vec<String>,
    paragraphs: &mut Vec<String>,
    others: &mut Vec<String>,
    depth: usize,
) {
    use alice_browser::dom::Classification;

    // Skip non-content nodes
    if matches!(
        node.classification,
        Classification::Advertisement | Classification::Tracker | Classification::Decoration
    ) {
        return;
    }
    // Skip nav/header/footer for cleaner content
    if matches!(node.tag.as_str(), "nav" | "header" | "footer" | "script" | "style" | "noscript") {
        return;
    }

    let tag = node.tag.as_str();

    // Headings
    if matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
        let text = collect_dom_text(node);
        let trimmed = text.trim().to_string();
        if trimmed.chars().count() > 2 && headings.len() < 10 {
            headings.push(trimmed);
        }
        return;
    }

    // Paragraphs — the most valuable content
    if tag == "p" {
        let text = collect_dom_text(node);
        let trimmed = text.trim().to_string();
        // Only keep substantive paragraphs (more than just a few chars)
        if trimmed.chars().count() > 8 && paragraphs.len() < 30 {
            paragraphs.push(trimmed);
        }
        return;
    }

    // Other content tags
    if matches!(tag, "li" | "td" | "th" | "dd" | "blockquote" | "figcaption" | "article") {
        let text = collect_dom_text(node);
        let trimmed = text.trim().to_string();
        // Filter out short nav-like items (ホーム, 出品, etc.)
        if trimmed.chars().count() > 6 && others.len() < 20 {
            others.push(trimmed);
        }
        return;
    }

    // Bare text with some substance
    if !node.text.is_empty() && node.tag.is_empty() {
        let t = node.text.trim();
        if t.chars().count() > 8 && others.len() < 20 {
            others.push(t.to_string());
        }
    }

    if depth < 10 {
        for child in &node.children {
            if headings.len() >= 10 && paragraphs.len() >= 30 && others.len() >= 20 {
                break;
            }
            extract_preview_texts_ranked(child, headings, paragraphs, others, depth + 1);
        }
    }
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let t: String = s.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{}...", t)
    }
}

fn collect_dom_text(node: &alice_browser::dom::DomNode) -> String {
    let mut s = String::new();
    if !node.text.is_empty() {
        s.push_str(node.text.trim());
    }
    for child in &node.children {
        let ct = collect_dom_text(child);
        if !ct.is_empty() {
            if !s.is_empty() { s.push(' '); }
            s.push_str(&ct);
        }
    }
    s
}

impl eframe::App for BrowserApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.check_fetch();

        // OZ: handle pending URL navigation from double-click
        #[cfg(feature = "sdf-render")]
        if let Some(url) = self.oz_pending_url.take() {
            let full_url = resolve_url(&self.url_input, &url);
            self.url_input = full_url;
            self.navigate(ctx);
        }

        // OZ: poll link preview results
        #[cfg(feature = "sdf-render")]
        if let Some(ref rx) = self.oz_preview_rx {
            if let Ok(preview) = rx.try_recv() {
                self.oz_preview = Some(preview);
                self.oz_preview_rx = None;
            }
        }

        // Poll background prefetch results (runs in any mode)
        #[cfg(feature = "sdf-render")]
        if let Some(ref rx) = self.oz_prefetch_rx {
            while let Ok(batch) = rx.try_recv() {
                if let Some(ref mut stream) = self.stream_state {
                    // OZ mode active: inject directly
                    stream.append_texts(batch);
                } else {
                    // Not in OZ mode yet: buffer for later
                    self.oz_prefetch_buffer.extend(batch);
                }
            }
        }

        // Apply dark/light visuals
        if self.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        // Poll image loader and convert completed images to textures
        self.image_loader.poll();
        {
            let urls: Vec<String> = self.image_loader.loaded_urls();
            for url in urls {
                if self.image_textures.contains_key(&url) {
                    continue;
                }
                if let Some(data) = self.image_loader.get(&url) {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [data.width as usize, data.height as usize],
                        &data.rgba,
                    );
                    let tex = ctx.load_texture(
                        format!("img_{}", url),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.image_textures.insert(url, tex);
                }
            }
        }

        // Top toolbar
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            self.draw_toolbar(ui, ctx);
        });

        // Stats side panel
        if self.show_stats {
            egui::SidePanel::right("stats")
                .default_width(220.0)
                .show(ctx, |ui| {
                    self.draw_stats_panel(ui);
                });
        }

        // Main content area
        let ctx_clone = ctx.clone();
        egui::CentralPanel::default().show(ctx, |ui| {
            self.draw_content(ui, &ctx_clone);
        });
    }
}

/// Recursively render a layout node using egui widgets
fn render_layout_node(
    ui: &mut egui::Ui,
    node: &LayoutNode,
    depth: usize,
    clicked_link: &mut Option<String>,
    highlight: Option<&str>,
) {
    // Skip empty nodes
    if node.bounds.height <= 0.0 && node.text.is_empty() && node.children.is_empty() {
        return;
    }

    match node.tag.as_str() {
        "h1" => {
            let text = collect_display_text(node);
            if !text.is_empty() {
                let rt = maybe_highlight(
                    egui::RichText::new(&text).size(28.0).strong(),
                    &text,
                    highlight,
                );
                ui.heading(rt);
                ui.add_space(8.0);
            }
        }
        "h2" => {
            let text = collect_display_text(node);
            if !text.is_empty() {
                let rt = maybe_highlight(
                    egui::RichText::new(&text).size(22.0).strong(),
                    &text,
                    highlight,
                );
                ui.heading(rt);
                ui.add_space(6.0);
            }
        }
        "h3" | "h4" | "h5" | "h6" => {
            let text = collect_display_text(node);
            if !text.is_empty() {
                let rt = maybe_highlight(
                    egui::RichText::new(&text).size(18.0),
                    &text,
                    highlight,
                );
                ui.heading(rt);
                ui.add_space(4.0);
            }
        }
        "p" => {
            let text = collect_display_text(node);
            if !text.is_empty() {
                let rt = maybe_highlight(egui::RichText::new(&text), &text, highlight);
                ui.label(rt);
                ui.add_space(8.0);
            }
        }
        "a" => {
            let text = collect_display_text(node);
            if !text.is_empty() {
                if let Some(ref href) = node.href {
                    let mut rt = egui::RichText::new(&text)
                        .color(egui::Color32::from_rgb(0, 100, 200))
                        .underline();
                    if text_matches(&text, highlight) {
                        rt = rt.background_color(egui::Color32::from_rgb(255, 255, 100));
                    }
                    let link = ui.add(
                        egui::Label::new(rt).sense(egui::Sense::click()),
                    );
                    if link.clicked() {
                        *clicked_link = Some(href.clone());
                    }
                    link.on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(href);
                } else {
                    let rt = maybe_highlight(
                        egui::RichText::new(&text)
                            .color(egui::Color32::from_rgb(0, 100, 200)),
                        &text,
                        highlight,
                    );
                    ui.label(rt);
                }
            }
        }
        "li" => {
            let text = collect_display_text(node);
            if !text.is_empty() {
                ui.horizontal(|ui| {
                    ui.label("  \u{2022}");
                    let rt = maybe_highlight(egui::RichText::new(&text), &text, highlight);
                    ui.label(rt);
                });
            }
        }
        "hr" => {
            ui.separator();
        }
        "img" => {
            ui.colored_label(egui::Color32::GRAY, "[Image]");
        }
        "br" => {
            ui.add_space(4.0);
        }
        _ => {
            // Text-only nodes
            if node.tag.is_empty() && !node.text.is_empty() {
                let text = node.text.trim();
                let rt = maybe_highlight(egui::RichText::new(text), text, highlight);
                ui.label(rt);
            }
            // Recurse into children for container elements
            for child in &node.children {
                render_layout_node(ui, child, depth + 1, clicked_link, highlight);
            }
            return;
        }
    }

    // Render children for non-container elements
    for child in &node.children {
        render_layout_node(ui, child, depth + 1, clicked_link, highlight);
    }
}

/// Resolve a potentially relative URL against a base URL
fn resolve_url(base: &str, href: &str) -> String {
    // Already absolute
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    // Protocol-relative
    if href.starts_with("//") {
        return format!("https:{}", href);
    }
    // Resolve relative against base
    if let Ok(base_url) = url::Url::parse(base) {
        if let Ok(resolved) = base_url.join(href) {
            return resolved.to_string();
        }
    }
    href.to_string()
}

/// Check if text contains the highlight query (case-insensitive)
fn text_matches(text: &str, highlight: Option<&str>) -> bool {
    match highlight {
        Some(q) if !q.is_empty() => text.to_lowercase().contains(&q.to_lowercase()),
        _ => false,
    }
}

/// Apply highlight background to RichText if it matches the search query
fn maybe_highlight(rt: egui::RichText, text: &str, highlight: Option<&str>) -> egui::RichText {
    if text_matches(text, highlight) {
        rt.background_color(egui::Color32::from_rgb(255, 255, 100))
    } else {
        rt
    }
}

/// Collect text from a node and all descendants
fn collect_display_text(node: &LayoutNode) -> String {
    let mut text = String::new();
    if !node.text.is_empty() {
        text.push_str(node.text.trim());
    }
    for child in &node.children {
        let ct = collect_display_text(child);
        if !ct.is_empty() {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&ct);
        }
    }
    text
}
