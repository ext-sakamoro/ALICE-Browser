//! Generic egui UI helper functions shared across render modes.
//!
//! This module contains stateless functions that translate `LayoutNode` trees
//! into egui widgets, plus small text-manipulation utilities used throughout
//! the browser UI.

use eframe::egui;
use alice_browser::render::layout::LayoutNode;

// ─── Layout rendering ─────────────────────────────────────────────────────────

/// Recursively render a `LayoutNode` tree using egui widgets.
pub fn render_layout_node(
    ui: &mut egui::Ui,
    node: &LayoutNode,
    depth: usize,
    clicked_link: &mut Option<String>,
    highlight: Option<&str>,
) {
    // Skip invisible / empty nodes
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
                    let link = ui.add(egui::Label::new(rt).sense(egui::Sense::click()));
                    if link.clicked() {
                        *clicked_link = Some(href.clone());
                    }
                    link.on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(href);
                } else {
                    let rt = maybe_highlight(
                        egui::RichText::new(&text).color(egui::Color32::from_rgb(0, 100, 200)),
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

    // Render children for non-container leaf elements
    for child in &node.children {
        render_layout_node(ui, child, depth + 1, clicked_link, highlight);
    }
}

// ─── Text utilities ───────────────────────────────────────────────────────────

/// Truncate `s` to at most `max_chars` Unicode scalar values, appending `"..."` if truncated.
pub fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let t: String = s.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{}...", t)
    }
}

/// Check if `text` contains the highlight query (case-insensitive).
pub fn text_matches(text: &str, highlight: Option<&str>) -> bool {
    match highlight {
        Some(q) if !q.is_empty() => text.to_lowercase().contains(&q.to_lowercase()),
        _ => false,
    }
}

/// Apply a yellow highlight background to `rt` if it matches the search query.
pub fn maybe_highlight(rt: egui::RichText, text: &str, highlight: Option<&str>) -> egui::RichText {
    if text_matches(text, highlight) {
        rt.background_color(egui::Color32::from_rgb(255, 255, 100))
    } else {
        rt
    }
}

/// Collect the display text of a `LayoutNode` and all its descendants.
pub fn collect_display_text(node: &LayoutNode) -> String {
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
