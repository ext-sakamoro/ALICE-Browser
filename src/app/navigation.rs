//! Navigation methods for `BrowserApp`.
//!
//! Covers history management (`go_back`, `go_forward`, `navigate`) and the
//! asynchronous page-fetch lifecycle (`navigate_no_history`, `check_fetch`).

use std::sync::mpsc;
use eframe::egui;

use alice_browser::engine::pipeline::BrowserEngine;

use super::BrowserApp;

impl BrowserApp {
    /// Navigate one step back in history.
    pub fn go_back(&mut self, ctx: &egui::Context) {
        if self.history_idx > 0 {
            self.history_idx -= 1;
            self.url_input = self.history[self.history_idx].clone();
            self.navigate_no_history(ctx);
        }
    }

    /// Navigate one step forward in history.
    pub fn go_forward(&mut self, ctx: &egui::Context) {
        if self.history_idx + 1 < self.history.len() {
            self.history_idx += 1;
            self.url_input = self.history[self.history_idx].clone();
            self.navigate_no_history(ctx);
        }
    }

    /// Push the current URL to history and start loading.
    pub fn navigate(&mut self, ctx: &egui::Context) {
        let url = self.url_input.clone();
        if self.history.is_empty() || self.history[self.history_idx] != url {
            // Truncate forward history before pushing
            self.history.truncate(self.history_idx + 1);
            self.history.push(url);
            self.history_idx = self.history.len() - 1;
        }
        self.navigate_no_history(ctx);
    }

    /// Start an async page fetch without touching history.
    pub fn navigate_no_history(&mut self, ctx: &egui::Context) {
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

    /// Poll the async fetch channel and update app state when a result arrives.
    pub fn check_fetch(&mut self) {
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
                            use crate::oz::{collect_hrefs_from_dom, extract_prefetch_texts};

                            self.oz_prefetch_started = true;
                            self.oz_prefetch_buffer.clear();
                            let base_url = self.url_input.clone();
                            let hrefs =
                                collect_hrefs_from_dom(&page.dom.root, &base_url, 10);
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
                                            let dom =
                                                parse_html(&result.html, &result.url);
                                            extract_prefetch_texts(
                                                &dom.root, &mut batch, 0,
                                            );
                                        }
                                        if !batch.is_empty() {
                                            if tx.send(batch).is_err() {
                                                break;
                                            }
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
}
