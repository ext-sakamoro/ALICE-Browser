use eframe::egui;

mod app;
mod oz;
mod ui;

use app::BrowserApp;
use oz::resolve_url;

fn main() {
    env_logger::init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
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
