//! Mobile UI Components — Bottom Bar & Fullscreen Mode
//!
//! Mobile-first UI following MOBILE_SPEC.md:
//!
//! ┌─────────────────────────┐
//! │ [ブロック数: 12] [🔒]    │  ← Status bar (minimal, auto-hide)
//! ├─────────────────────────┤
//! │                          │
//! │     Web page content     │  ← Content area (maximized)
//! │                          │
//! ├─────────────────────────┤
//! │ [←] [→] [URL...   ] [⋮] │  ← Bottom bar (thumb-friendly)
//! └─────────────────────────┘

use super::touch::{Gesture, GestureRecognizer, SwipeDirection};

/// Mobile UI state
pub struct MobileUI {
    /// Whether bottom bar is visible
    pub bottom_bar_visible: bool,
    /// Whether status bar is visible
    pub status_bar_visible: bool,
    /// Whether in fullscreen mode
    pub fullscreen: bool,
    /// Current zoom level
    pub zoom_level: f32,
    /// Scroll offset Y
    pub scroll_y: f32,
    /// URL being displayed
    pub current_url: String,
    /// Whether URL bar is focused (editing)
    pub url_editing: bool,
    /// Block statistics
    pub block_stats: MobileBlockStats,
    /// Gesture recognizer
    pub gestures: GestureRecognizer,
    /// Navigation history
    pub can_go_back: bool,
    pub can_go_forward: bool,
    /// HTTPS status
    pub is_secure: bool,
    /// Menu open
    pub menu_open: bool,
}

/// Block statistics for mobile display
#[derive(Debug, Clone, Default)]
pub struct MobileBlockStats {
    pub page_ads_blocked: usize,
    pub page_trackers_blocked: usize,
    pub total_ads_blocked: usize,
    pub total_trackers_blocked: usize,
    pub data_saved_kb: f32,
    pub time_saved_ms: f32,
}

impl MobileBlockStats {
    pub fn page_total(&self) -> usize {
        self.page_ads_blocked + self.page_trackers_blocked
    }

    pub fn lifetime_total(&self) -> usize {
        self.total_ads_blocked + self.total_trackers_blocked
    }
}

/// Actions that the mobile UI can trigger
#[derive(Debug, Clone)]
pub enum MobileAction {
    Navigate(String),
    GoBack,
    GoForward,
    Refresh,
    ToggleFullscreen,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ShowLinkPreview(f32, f32),
    ToggleMenu,
    ToggleReaderMode,
    ToggleDarkMode,
    ShowBlockStats,
    OpenSettings,
    None,
}

impl MobileUI {
    pub fn new(screen_width: f32, screen_height: f32) -> Self {
        Self {
            bottom_bar_visible: true,
            status_bar_visible: true,
            fullscreen: false,
            zoom_level: 1.0,
            scroll_y: 0.0,
            current_url: String::new(),
            url_editing: false,
            block_stats: MobileBlockStats::default(),
            gestures: GestureRecognizer::new(screen_width, screen_height),
            can_go_back: false,
            can_go_forward: false,
            is_secure: false,
            menu_open: false,
        }
    }

    /// Process a recognized gesture and return the corresponding action
    pub fn process_gesture(&mut self, gesture: &Gesture) -> MobileAction {
        match gesture {
            Gesture::Tap { x, y } => {
                if self.menu_open {
                    self.menu_open = false;
                    return MobileAction::None;
                }
                // Check if tap is in the URL bar area
                if self.bottom_bar_visible && self.is_in_url_bar(*x, *y) {
                    self.url_editing = true;
                    return MobileAction::None;
                }
                MobileAction::None
            }

            Gesture::DoubleTap { .. } => {
                // Toggle zoom between 1.0 and 2.0
                if (self.zoom_level - 1.0).abs() < 0.1 {
                    self.zoom_level = 2.0;
                    MobileAction::ZoomIn
                } else {
                    self.zoom_level = 1.0;
                    MobileAction::ZoomReset
                }
            }

            Gesture::LongPress { x, y } => {
                MobileAction::ShowLinkPreview(*x, *y)
            }

            Gesture::Swipe { direction, .. } => {
                match direction {
                    SwipeDirection::Right => {
                        if self.can_go_back {
                            MobileAction::GoBack
                        } else {
                            MobileAction::None
                        }
                    }
                    SwipeDirection::Left => {
                        if self.can_go_forward {
                            MobileAction::GoForward
                        } else {
                            MobileAction::None
                        }
                    }
                    SwipeDirection::Up => {
                        self.bottom_bar_visible = false;
                        self.status_bar_visible = false;
                        self.fullscreen = true;
                        MobileAction::ToggleFullscreen
                    }
                    SwipeDirection::Down => {
                        self.status_bar_visible = true;
                        self.bottom_bar_visible = true;
                        self.fullscreen = false;
                        MobileAction::ToggleFullscreen
                    }
                }
            }

            Gesture::Pinch { scale, .. } => {
                self.zoom_level = (self.zoom_level * scale).clamp(0.5, 4.0);
                if *scale > 1.0 {
                    MobileAction::ZoomIn
                } else {
                    MobileAction::ZoomOut
                }
            }

            Gesture::Scroll { dy, .. } => {
                self.scroll_y -= dy;
                self.scroll_y = self.scroll_y.max(0.0);
                MobileAction::None
            }

            Gesture::None => MobileAction::None,
        }
    }

    /// Render the mobile UI using egui.
    ///
    /// Layout:
    /// - Status bar at top (if visible)
    /// - Content area (maximized)
    /// - Bottom bar at bottom (if visible)
    pub fn render(&mut self, ui: &mut egui::Ui) {
        let available = ui.available_rect_before_wrap();

        // Status bar height
        let status_height = if self.status_bar_visible { 28.0 } else { 0.0 };
        // Bottom bar height
        let bottom_height = if self.bottom_bar_visible { 48.0 } else { 0.0 };

        // Status bar
        if self.status_bar_visible {
            let status_rect = egui::Rect::from_min_size(
                available.min,
                egui::vec2(available.width(), status_height),
            );
            let builder = egui::UiBuilder::new().max_rect(status_rect);
            ui.allocate_new_ui(builder, |ui| {
                self.render_status_bar(ui);
            });
        }

        // Content area (returned for external rendering)
        let content_top = available.min.y + status_height;
        let content_height = available.height() - status_height - bottom_height;
        let _content_rect = egui::Rect::from_min_size(
            egui::pos2(available.min.x, content_top),
            egui::vec2(available.width(), content_height),
        );

        // Bottom bar
        if self.bottom_bar_visible {
            let bottom_rect = egui::Rect::from_min_size(
                egui::pos2(available.min.x, available.max.y - bottom_height),
                egui::vec2(available.width(), bottom_height),
            );
            let builder = egui::UiBuilder::new().max_rect(bottom_rect);
            ui.allocate_new_ui(builder, |ui| {
                self.render_bottom_bar(ui);
            });
        }
    }

    /// Render status bar: [blocked: N] [lock icon]
    fn render_status_bar(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;

            // Block count
            let blocked = self.block_stats.page_total();
            ui.label(
                egui::RichText::new(format!("Blocked: {}", blocked))
                    .size(12.0)
                    .color(if blocked > 0 {
                        egui::Color32::from_rgb(76, 175, 80) // Green
                    } else {
                        egui::Color32::GRAY
                    }),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Security indicator
                let (icon, color) = if self.is_secure {
                    ("HTTPS", egui::Color32::from_rgb(76, 175, 80))
                } else {
                    ("HTTP", egui::Color32::from_rgb(255, 152, 0))
                };
                ui.label(egui::RichText::new(icon).size(11.0).color(color));
            });
        });
    }

    /// Render bottom bar: [back] [forward] [URL bar] [menu]
    fn render_bottom_bar(&mut self, ui: &mut egui::Ui) -> MobileAction {
        let mut action = MobileAction::None;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;

            // Back button
            let back_color = if self.can_go_back {
                egui::Color32::WHITE
            } else {
                egui::Color32::from_gray(80)
            };
            if ui.add(egui::Button::new(
                egui::RichText::new("<").size(20.0).color(back_color)
            ).min_size(egui::vec2(40.0, 40.0))).clicked() && self.can_go_back {
                action = MobileAction::GoBack;
            }

            // Forward button
            let fwd_color = if self.can_go_forward {
                egui::Color32::WHITE
            } else {
                egui::Color32::from_gray(80)
            };
            if ui.add(egui::Button::new(
                egui::RichText::new(">").size(20.0).color(fwd_color)
            ).min_size(egui::vec2(40.0, 40.0))).clicked() && self.can_go_forward {
                action = MobileAction::GoForward;
            }

            // URL bar (takes remaining space)
            let url_width = ui.available_width() - 48.0;
            let response = ui.add_sized(
                [url_width, 36.0],
                egui::TextEdit::singleline(&mut self.current_url)
                    .font(egui::TextStyle::Body)
                    .desired_width(url_width),
            );

            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let url = self.current_url.clone();
                self.url_editing = false;
                action = MobileAction::Navigate(url);
            }

            // Menu button
            if ui.add(egui::Button::new(
                egui::RichText::new("...").size(20.0)
            ).min_size(egui::vec2(40.0, 40.0))).clicked() {
                self.menu_open = !self.menu_open;
                action = MobileAction::ToggleMenu;
            }
        });

        action
    }

    /// Render popup menu
    pub fn render_menu(&mut self, ui: &mut egui::Ui) -> MobileAction {
        let mut action = MobileAction::None;

        if !self.menu_open {
            return action;
        }

        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_min_width(200.0);

            if ui.button("Refresh").clicked() {
                action = MobileAction::Refresh;
                self.menu_open = false;
            }
            if ui.button("Reader Mode").clicked() {
                action = MobileAction::ToggleReaderMode;
                self.menu_open = false;
            }
            if ui.button("Dark Mode").clicked() {
                action = MobileAction::ToggleDarkMode;
                self.menu_open = false;
            }

            ui.separator();

            // Block stats summary
            ui.label(egui::RichText::new("Block Statistics").strong());
            ui.label(format!("Page: {} ads, {} trackers",
                self.block_stats.page_ads_blocked,
                self.block_stats.page_trackers_blocked));
            ui.label(format!("Total: {} blocked",
                self.block_stats.lifetime_total()));
            if self.block_stats.data_saved_kb > 0.0 {
                ui.label(format!("Data saved: {:.1} KB", self.block_stats.data_saved_kb));
            }

            ui.separator();

            if ui.button("Settings").clicked() {
                action = MobileAction::OpenSettings;
                self.menu_open = false;
            }
        });

        action
    }

    /// Check if a tap position is within the URL bar area
    fn is_in_url_bar(&self, _x: f32, y: f32) -> bool {
        // Bottom bar is in the last 48 pixels
        y > self.gestures.screen_height - 48.0
    }

    /// Update block stats from engine
    pub fn update_block_stats(&mut self, page_ads: usize, page_trackers: usize, total_ads: usize, total_trackers: usize) {
        self.block_stats.page_ads_blocked = page_ads;
        self.block_stats.page_trackers_blocked = page_trackers;
        self.block_stats.total_ads_blocked = total_ads;
        self.block_stats.total_trackers_blocked = total_trackers;
        // Rough estimate: each blocked request ≈ 30KB saved, 100ms saved
        let total_blocked = (page_ads + page_trackers) as f32;
        self.block_stats.data_saved_kb = total_blocked * 30.0;
        self.block_stats.time_saved_ms = total_blocked * 100.0;
    }
}

/// Content area dimensions (for the rendering engine)
pub struct ContentArea {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl MobileUI {
    /// Calculate the content area rect based on UI visibility state
    pub fn content_area(&self) -> ContentArea {
        let status_h = if self.status_bar_visible { 28.0 } else { 0.0 };
        let bottom_h = if self.bottom_bar_visible { 48.0 } else { 0.0 };

        ContentArea {
            x: 0.0,
            y: status_h,
            width: self.gestures.screen_width,
            height: self.gestures.screen_height - status_h - bottom_h,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mobile_ui_creation() {
        let ui = MobileUI::new(400.0, 800.0);
        assert!(ui.bottom_bar_visible);
        assert!(ui.status_bar_visible);
        assert!(!ui.fullscreen);
        assert!((ui.zoom_level - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_content_area() {
        let ui = MobileUI::new(400.0, 800.0);
        let area = ui.content_area();
        assert!((area.y - 28.0).abs() < 1e-6); // status bar height
        assert!((area.height - (800.0 - 28.0 - 48.0)).abs() < 1e-6);
    }

    #[test]
    fn test_fullscreen_content_area() {
        let mut ui = MobileUI::new(400.0, 800.0);
        ui.status_bar_visible = false;
        ui.bottom_bar_visible = false;
        ui.fullscreen = true;
        let area = ui.content_area();
        assert!((area.y - 0.0).abs() < 1e-6);
        assert!((area.height - 800.0).abs() < 1e-6);
    }

    #[test]
    fn test_block_stats() {
        let mut ui = MobileUI::new(400.0, 800.0);
        ui.update_block_stats(5, 3, 100, 50);
        assert_eq!(ui.block_stats.page_total(), 8);
        assert_eq!(ui.block_stats.lifetime_total(), 150);
        assert!(ui.block_stats.data_saved_kb > 0.0);
    }

    #[test]
    fn test_gesture_swipe_up_fullscreen() {
        let mut ui = MobileUI::new(400.0, 800.0);
        let gesture = Gesture::Swipe {
            direction: SwipeDirection::Up,
            velocity: 500.0,
        };
        let action = ui.process_gesture(&gesture);
        assert!(ui.fullscreen);
        assert!(!ui.bottom_bar_visible);
        match action {
            MobileAction::ToggleFullscreen => {}
            _ => panic!("Expected ToggleFullscreen"),
        }
    }

    #[test]
    fn test_double_tap_zoom() {
        let mut ui = MobileUI::new(400.0, 800.0);

        let gesture = Gesture::DoubleTap { x: 200.0, y: 400.0 };
        let action = ui.process_gesture(&gesture);
        assert!((ui.zoom_level - 2.0).abs() < 1e-6);
        match action {
            MobileAction::ZoomIn => {}
            _ => panic!("Expected ZoomIn"),
        }

        // Double-tap again → reset
        let action = ui.process_gesture(&gesture);
        assert!((ui.zoom_level - 1.0).abs() < 1e-6);
        match action {
            MobileAction::ZoomReset => {}
            _ => panic!("Expected ZoomReset"),
        }
    }
}
