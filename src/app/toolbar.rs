//! Toolbar rendering for `BrowserApp`.
//!
//! Draws the address bar, back/forward buttons, render-mode selector,
//! dark-mode toggle, and the optional in-page search field.

use eframe::egui;
use alice_browser::render::RenderMode;

use super::BrowserApp;

impl BrowserApp {
    /// Render the top toolbar strip.
    pub fn draw_toolbar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.add_space(4.0);

            // Back / Forward
            let can_back = self.history_idx > 0;
            let can_fwd = self.history_idx + 1 < self.history.len();
            if ui
                .add_enabled(
                    can_back,
                    egui::Button::new("\u{25C0}").min_size(egui::vec2(28.0, 24.0)),
                )
                .clicked()
            {
                self.go_back(ctx);
            }
            if ui
                .add_enabled(
                    can_fwd,
                    egui::Button::new("\u{25B6}").min_size(egui::vec2(28.0, 24.0)),
                )
                .clicked()
            {
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

            // Page search (feature-gated)
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

            // Suppress unused-variable warning when no feature flags are active
            let _ = prev_mode;
        });
    }
}
