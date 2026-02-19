//! Content-area rendering for `BrowserApp`.
//!
//! Contains four methods:
//!
//! - `draw_content`      — top-level dispatcher (spinner, error, flat/SDF/3-D)
//! - `draw_sdf_paint`    — 2-D SDF paint layer (always compiled)
//! - `draw_sdf_content`  — 3-D / OZ raymarched view (`sdf-render` feature)
//! - `draw_stats_panel`  — right-side statistics panel

use eframe::egui;
use alice_browser::render::RenderMode;

use crate::oz::{resolve_url, fetch_link_preview, LinkPreviewStatus};
use crate::ui::{render_layout_node, truncate_str};
use super::BrowserApp;

impl BrowserApp {
    // ── 2-D SDF paint ────────────────────────────────────────────────────────

    /// Lazily build and paint the 2-D SDF element list.  Returns the href of
    /// any element the user clicked on.
    pub fn draw_sdf_paint(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<String> {
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

    // ── 3-D / OZ raymarched view ─────────────────────────────────────────────

    #[cfg(feature = "sdf-render")]
    pub fn draw_sdf_content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use std::sync::mpsc;
        use alice_browser::render::sdf_renderer::{render_sdf_interactive, auto_camera};

        // Build spatial scene lazily
        if self.spatial_scene.is_none() {
            if let Some(ref page) = self.page {
                if self.render_mode == RenderMode::OzMode {
                    // OZ "The Stream" Mode: cylindrical immersion
                    let stream =
                        alice_browser::render::stream::StreamState::from_layout(&page.layout);
                    let scene = stream.to_sdf_scene();
                    self.cam_params = alice_browser::render::sdf_renderer::CameraParams {
                        azimuth: 0.0,
                        elevation: 0.0,
                        distance: 0.0,
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
                }
                self.cam_dirty = true;
                if let Some(ref mut gpu) = self.gpu_renderer {
                    gpu.invalidate();
                }
            }
        }

        // OZ mode: update particle flow every frame
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
                self.oz_hologram_alpha = (elapsed / 0.3).clamp(0.0, 1.0);
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
                self.cam_params.azimuth -= delta.x * 0.005;
                self.cam_params.elevation =
                    (self.cam_params.elevation + delta.y * 0.005).clamp(-0.8, 0.8);
            }

            // OZ: click to grab nearest text
            if response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    let rect = response.rect;
                    let fov: f32 = 110.0_f32.to_radians();
                    let fov_h = fov * 0.5;
                    let aspect = rect.width() / rect.height();
                    let fov_v = fov_h / aspect;

                    let ndc_x = (pos.x - rect.center().x) / (rect.width() * 0.5);
                    let ndc_y = (pos.y - rect.center().y) / (rect.height() * 0.5);

                    if let Some(ref mut stream) = self.stream_state {
                        stream.try_grab_screen(
                            ndc_x,
                            ndc_y,
                            self.cam_params.azimuth,
                            self.cam_params.elevation,
                            fov_h,
                            fov_v,
                            aspect,
                        );

                        if let Some(info) = stream.grabbed_info() {
                            self.oz_hologram_screen_pos = Some(pos);
                            self.oz_hologram_alpha = 0.0;
                            self.oz_hologram_start = Some(std::time::Instant::now());

                            let fetch_url_str = if let Some(ref href) = info.meta.href {
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

                            if !fetch_url_str.is_empty()
                                && self.oz_preview_for.as_deref() != Some(&fetch_url_str)
                            {
                                self.oz_preview_for = Some(fetch_url_str.clone());
                                self.oz_preview = Some(crate::oz::LinkPreview {
                                    url: fetch_url_str.clone(),
                                    title: String::new(),
                                    description: String::new(),
                                    texts: Vec::new(),
                                    status: LinkPreviewStatus::Loading,
                                });
                                let (tx, rx) = mpsc::channel();
                                self.oz_preview_rx = Some(rx);
                                let url_for_thread = fetch_url_str.clone();
                                std::thread::spawn(move || {
                                    let preview = fetch_link_preview(&url_for_thread);
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

            // OZ: double-click on grabbed link → schedule navigation
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
            // Spatial3D: drag to orbit camera around scene
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

        // Raymarch render (Spatial3D only — OZ uses egui overlay)
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
                        let image =
                            egui::ColorImage::from_rgba_unmultiplied([w, h], &pixels);
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
            ui.painter()
                .rect_filled(response.rect, 0.0, egui::Color32::WHITE);
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

                    // Skip particles behind camera
                    if rz < 1.0 {
                        continue;
                    }

                    // Perspective projection
                    let ndc_x = rx / (rz * tan_fov_h);
                    let ndc_y = -ry / (rz * tan_fov_h / aspect);

                    if ndc_x.abs() > 1.3 || ndc_y.abs() > 1.3 {
                        continue;
                    }

                    let sx = rect.center().x + ndc_x * rect.width() * 0.5;
                    let sy = rect.center().y + ndc_y * rect.height() * 0.5;

                    let cat_color = stream
                        .categories
                        .get(p.category_index)
                        .map(|c| c.color)
                        .unwrap_or([0.3, 0.3, 0.3, 1.0]);

                    let alpha = StreamState::particle_opacity(p);
                    if alpha < 0.01 {
                        continue;
                    }

                    // Font size: layer-based + importance + perspective
                    let layer_scale = StreamState::layer_font_scale(p.layer);
                    let depth_scale = (12.0 / rz).clamp(0.5, 2.0);
                    let base_font: f32 =
                        (13.0 + p.importance * 14.0) * layer_scale * depth_scale;
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
                        let text_w =
                            p.text.chars().count().min(30) as f32 * font_size * 0.55;
                        let pad = 4.0;
                        let bg_rect = egui::Rect::from_center_size(
                            egui::pos2(sx, sy),
                            egui::vec2(text_w + pad * 2.0, font_size + pad * 2.0),
                        );
                        painter.rect(
                            bg_rect,
                            4.0,
                            egui::Color32::from_rgba_unmultiplied(r, g, b, 20),
                            egui::Stroke::new(
                                1.5,
                                egui::Color32::from_rgba_unmultiplied(r, g, b, 160),
                            ),
                        );
                    }
                }

                // ── Hologram Overlay ──────────────────────────────────────────
                if let Some(info) = stream.grabbed_info() {
                    let holo_alpha = self.oz_hologram_alpha;
                    if holo_alpha > 0.01 {
                        let has_href = info.meta.href.is_some();
                        let has_preview = self
                            .oz_preview
                            .as_ref()
                            .map(|p| {
                                p.status != LinkPreviewStatus::Loading || !p.title.is_empty()
                            })
                            .unwrap_or(false);
                        let is_loading = self
                            .oz_preview
                            .as_ref()
                            .map(|p| p.status == LinkPreviewStatus::Loading)
                            .unwrap_or(false);

                        let has_desc = self
                            .oz_preview
                            .as_ref()
                            .map(|p| !p.description.is_empty())
                            .unwrap_or(false);
                        let preview_lines = if has_preview {
                            self.oz_preview
                                .as_ref()
                                .map(|p| p.texts.len().min(12))
                                .unwrap_or(0)
                        } else {
                            0
                        };
                        let panel_w = 500.0_f32.min(rect.width() - 40.0);
                        let base_h = 50.0_f32;
                        let link_h =
                            if has_href || is_loading || has_preview { 22.0_f32 } else { 0.0 };
                        let desc_h = if has_desc { 36.0_f32 } else { 0.0 };
                        let preview_h = if has_preview || is_loading {
                            24.0 + desc_h + preview_lines as f32 * 17.0
                        } else {
                            0.0
                        };
                        let panel_h =
                            (base_h + link_h + preview_h).min(rect.height() * 0.55);

                        // Smart placement: anchor near hologram_screen_pos
                        let anchor =
                            self.oz_hologram_screen_pos.unwrap_or(rect.center());
                        let panel_x = (anchor.x - panel_w * 0.5)
                            .clamp(rect.left() + 8.0, rect.right() - panel_w - 8.0);
                        let panel_y = if anchor.y < rect.center().y {
                            (anchor.y + 30.0).min(rect.bottom() - panel_h - 8.0)
                        } else {
                            (anchor.y - panel_h - 30.0).max(rect.top() + 8.0)
                        };

                        let panel_rect = egui::Rect::from_min_size(
                            egui::pos2(panel_x, panel_y),
                            egui::vec2(panel_w, panel_h),
                        );

                        let cat_color = stream
                            .categories
                            .get(info.particle.category_index)
                            .map(|c| c.color)
                            .unwrap_or([0.3, 0.3, 0.3, 1.0]);
                        let cr = (cat_color[0] * 255.0) as u8;
                        let cg = (cat_color[1] * 255.0) as u8;
                        let cb = (cat_color[2] * 255.0) as u8;
                        let accent = egui::Color32::from_rgba_unmultiplied(
                            cr,
                            cg,
                            cb,
                            (holo_alpha * 255.0) as u8,
                        );
                        let bg_alpha = (holo_alpha * 235.0) as u8;

                        // Cyber hologram background — glow shadow
                        painter.rect_filled(
                            panel_rect.expand(3.0),
                            6.0,
                            egui::Color32::from_rgba_unmultiplied(
                                cr,
                                cg,
                                cb,
                                (holo_alpha * 30.0) as u8,
                            ),
                        );

                        // Main background
                        painter.rect(
                            panel_rect,
                            4.0,
                            egui::Color32::from_rgba_unmultiplied(250, 250, 255, bg_alpha),
                            egui::Stroke::new(
                                1.5,
                                egui::Color32::from_rgba_unmultiplied(
                                    cr,
                                    cg,
                                    cb,
                                    (holo_alpha * 180.0) as u8,
                                ),
                            ),
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
                        painter.line_segment(
                            [
                                panel_rect.left_top(),
                                panel_rect.left_top() + egui::vec2(bk_len, 0.0),
                            ],
                            bk_stroke,
                        );
                        painter.line_segment(
                            [
                                panel_rect.left_top(),
                                panel_rect.left_top() + egui::vec2(0.0, bk_len),
                            ],
                            bk_stroke,
                        );
                        painter.line_segment(
                            [
                                panel_rect.right_top(),
                                panel_rect.right_top() + egui::vec2(-bk_len, 0.0),
                            ],
                            bk_stroke,
                        );
                        painter.line_segment(
                            [
                                panel_rect.right_top(),
                                panel_rect.right_top() + egui::vec2(0.0, bk_len),
                            ],
                            bk_stroke,
                        );
                        painter.line_segment(
                            [
                                panel_rect.left_bottom(),
                                panel_rect.left_bottom() + egui::vec2(bk_len, 0.0),
                            ],
                            bk_stroke,
                        );
                        painter.line_segment(
                            [
                                panel_rect.left_bottom(),
                                panel_rect.left_bottom() + egui::vec2(0.0, -bk_len),
                            ],
                            bk_stroke,
                        );
                        painter.line_segment(
                            [
                                panel_rect.right_bottom(),
                                panel_rect.right_bottom() + egui::vec2(-bk_len, 0.0),
                            ],
                            bk_stroke,
                        );
                        painter.line_segment(
                            [
                                panel_rect.right_bottom(),
                                panel_rect.right_bottom() + egui::vec2(0.0, -bk_len),
                            ],
                            bk_stroke,
                        );

                        let text_alpha = (holo_alpha * 255.0) as u8;
                        let left = panel_rect.left() + 16.0;
                        let mut y = panel_rect.top() + 12.0;

                        // Header: dot + category + tag badge
                        painter.circle_filled(
                            egui::pos2(left + 2.0, y + 6.0),
                            5.0,
                            accent,
                        );
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
                            "a" => "LINK",
                            "p" => "TEXT",
                            "li" => "LIST",
                            "span" => "SPAN",
                            "button" => "BUTTON",
                            "" => "TEXT",
                            other => other,
                        };
                        let tag_x = left
                            + 14.0
                            + info.category_name.chars().count().min(16) as f32 * 7.5;
                        let tag_bg = egui::Rect::from_min_size(
                            egui::pos2(tag_x, y - 1.0),
                            egui::vec2(tag_text.len() as f32 * 7.0 + 10.0, 16.0),
                        );
                        painter.rect_filled(
                            tag_bg,
                            8.0,
                            egui::Color32::from_rgba_unmultiplied(
                                cr,
                                cg,
                                cb,
                                (holo_alpha * 25.0) as u8,
                            ),
                        );
                        painter.text(
                            tag_bg.center(),
                            egui::Align2::CENTER_CENTER,
                            tag_text,
                            egui::FontId::proportional(10.0),
                            egui::Color32::from_rgba_unmultiplied(
                                cr,
                                cg,
                                cb,
                                (holo_alpha * 200.0) as u8,
                            ),
                        );

                        // Selected text
                        y += 20.0;
                        let max_chars = ((panel_w - 40.0) / 8.5) as usize;
                        let display_text = if info.meta.full_text.chars().count() > max_chars {
                            let t: String =
                                info.meta.full_text.chars().take(max_chars).collect();
                            format!("{}...", t)
                        } else {
                            info.meta.full_text.clone()
                        };
                        painter.text(
                            egui::pos2(left, y),
                            egui::Align2::LEFT_TOP,
                            &display_text,
                            egui::FontId::proportional(14.0),
                            egui::Color32::from_rgba_unmultiplied(25, 25, 25, text_alpha),
                        );
                        y += 22.0;

                        // Separator
                        painter.line_segment(
                            [
                                egui::pos2(left, y),
                                egui::pos2(panel_rect.right() - 16.0, y),
                            ],
                            egui::Stroke::new(
                                0.5,
                                egui::Color32::from_rgba_unmultiplied(
                                    cr,
                                    cg,
                                    cb,
                                    (holo_alpha * 60.0) as u8,
                                ),
                            ),
                        );
                        y += 6.0;

                        // URL (if link)
                        if let Some(ref href) = info.meta.href {
                            let link_display = truncate_str(href, 70);
                            painter.text(
                                egui::pos2(left, y),
                                egui::Align2::LEFT_TOP,
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
                                    egui::pos2(left, y),
                                    egui::Align2::LEFT_TOP,
                                    "Loading preview...",
                                    egui::FontId::proportional(12.0),
                                    egui::Color32::from_rgba_unmultiplied(
                                        80,
                                        80,
                                        80,
                                        (holo_alpha * 200.0) as u8,
                                    ),
                                );
                            } else if let LinkPreviewStatus::Error(ref e) = preview.status {
                                painter.text(
                                    egui::pos2(left, y),
                                    egui::Align2::LEFT_TOP,
                                    &format!("Error: {}", e),
                                    egui::FontId::proportional(11.0),
                                    egui::Color32::from_rgba_unmultiplied(
                                        200, 60, 60, text_alpha,
                                    ),
                                );
                            } else {
                                let max_y = panel_rect.bottom() - 20.0;
                                let text_max_chars =
                                    ((panel_w - 40.0) / 7.0) as usize;

                                if !preview.title.is_empty() && y < max_y {
                                    let title_display =
                                        truncate_str(&preview.title, text_max_chars);
                                    painter.text(
                                        egui::pos2(left, y),
                                        egui::Align2::LEFT_TOP,
                                        &title_display,
                                        egui::FontId::proportional(14.0),
                                        egui::Color32::from_rgba_unmultiplied(
                                            15, 15, 15, text_alpha,
                                        ),
                                    );
                                    y += 20.0;
                                }

                                if !preview.description.is_empty() && y < max_y {
                                    let desc_chars: Vec<char> =
                                        preview.description.chars().collect();
                                    let mut offset = 0;
                                    for _ in 0..3 {
                                        if offset >= desc_chars.len() || y >= max_y {
                                            break;
                                        }
                                        let end = (offset + text_max_chars)
                                            .min(desc_chars.len());
                                        let mut line: String =
                                            desc_chars[offset..end].iter().collect();
                                        if end < desc_chars.len() {
                                            line.push_str("...");
                                        }
                                        painter.text(
                                            egui::pos2(left, y),
                                            egui::Align2::LEFT_TOP,
                                            &line,
                                            egui::FontId::proportional(12.0),
                                            egui::Color32::from_rgba_unmultiplied(
                                                50, 50, 70, text_alpha,
                                            ),
                                        );
                                        y += 16.0;
                                        offset = end;
                                    }
                                    y += 4.0;
                                }

                                for (i, text) in preview.texts.iter().take(12).enumerate() {
                                    if y >= max_y {
                                        break;
                                    }
                                    let line = truncate_str(text, text_max_chars);
                                    let line_alpha = if i < 4 {
                                        230
                                    } else if i < 8 {
                                        180
                                    } else {
                                        140
                                    };
                                    let fa = ((line_alpha as f32 / 255.0)
                                        * holo_alpha
                                        * 255.0) as u8;
                                    let font_size = if i < 3 { 12.0 } else { 11.0 };
                                    painter.text(
                                        egui::pos2(left, y),
                                        egui::Align2::LEFT_TOP,
                                        &line,
                                        egui::FontId::proportional(font_size),
                                        egui::Color32::from_rgba_unmultiplied(40, 40, 40, fa),
                                    );
                                    y += 17.0;
                                }
                            }
                        }

                        // Hint: double-click to open
                        if info.meta.href.is_some() {
                            painter.text(
                                egui::pos2(
                                    panel_rect.right() - 16.0,
                                    panel_rect.bottom() - 16.0,
                                ),
                                egui::Align2::RIGHT_BOTTOM,
                                "Double-click to open",
                                egui::FontId::proportional(10.0),
                                egui::Color32::from_rgba_unmultiplied(
                                    120,
                                    120,
                                    120,
                                    (holo_alpha * 150.0) as u8,
                                ),
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
                format!("Drag: rotate | Scroll: zoom | d={:.1}", self.cam_params.distance),
                egui::FontId::proportional(12.0),
                egui::Color32::from_rgba_premultiplied(255, 255, 255, 180),
            );
        }
    }

    // ── Main content dispatcher ──────────────────────────────────────────────

    /// Render the central content panel.
    pub fn draw_content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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

        // SDF Paint mode (interactive 2-D)
        if self.render_mode == RenderMode::Sdf2D && self.page.is_some() {
            let clicked = self.draw_sdf_paint(ui, ctx);
            if let Some(href) = clicked {
                let base = self.page.as_ref().map(|p| p.dom.url.as_str()).unwrap_or("");
                self.url_input = resolve_url(base, &href);
                self.navigate(ctx);
            }
            return;
        }

        // Raymarched 3-D mode (Spatial3D or OzMode)
        #[cfg(feature = "sdf-render")]
        if (self.render_mode == RenderMode::Spatial3D || self.render_mode == RenderMode::OzMode)
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

    // ── Stats side panel ─────────────────────────────────────────────────────

    /// Render the right-side statistics panel.
    pub fn draw_stats_panel(&self, ui: &mut egui::Ui) {
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
