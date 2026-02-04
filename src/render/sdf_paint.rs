//! egui Painter-based SDF-styled UI rendering.
//!
//! Draws PaintElements using egui's Painter API with smooth hover
//! animations, drop shadows, and rounded corners inspired by SDF rendering.

use std::collections::HashMap;
use egui::{Color32, FontId, Pos2, Rect, Rounding, Stroke, TextureHandle, Vec2};

use crate::render::sdf_ui::{PaintElement, PaintKind};

/// Theme colors for SDF paint rendering.
struct Theme {
    page_bg: Color32,
    card_bg: Color32,
    heading_color: Color32,
    heading_accent: Color32,
    text_color: Color32,
    link_color: Color32,
    link_hover: Color32,
    separator_color: Color32,
    img_bg: Color32,
    img_border: Color32,
    img_text: Color32,
}

impl Theme {
    fn light() -> Self {
        Self {
            page_bg: Color32::from_rgb(250, 250, 252),
            card_bg: Color32::WHITE,
            heading_color: Color32::from_rgb(25, 25, 38),
            heading_accent: Color32::from_rgb(0, 80, 180),
            text_color: Color32::from_rgb(38, 38, 46),
            link_color: Color32::from_rgb(0, 102, 217),
            link_hover: Color32::from_rgb(0, 60, 160),
            separator_color: Color32::from_rgb(204, 204, 209),
            img_bg: Color32::from_rgb(235, 235, 240),
            img_border: Color32::from_rgb(200, 200, 205),
            img_text: Color32::from_rgb(160, 160, 165),
        }
    }

    fn dark() -> Self {
        Self {
            page_bg: Color32::from_rgb(24, 24, 30),
            card_bg: Color32::from_rgb(36, 36, 44),
            heading_color: Color32::from_rgb(230, 230, 240),
            heading_accent: Color32::from_rgb(80, 160, 255),
            text_color: Color32::from_rgb(200, 200, 210),
            link_color: Color32::from_rgb(80, 160, 255),
            link_hover: Color32::from_rgb(120, 185, 255),
            separator_color: Color32::from_rgb(60, 60, 70),
            img_bg: Color32::from_rgb(40, 40, 50),
            img_border: Color32::from_rgb(60, 60, 70),
            img_text: Color32::from_rgb(100, 100, 110),
        }
    }
}

/// Persistent state for SDF paint rendering.
pub struct SdfPaintState {
    hovered_id: Option<usize>,
}

impl SdfPaintState {
    pub fn new() -> Self {
        Self { hovered_id: None }
    }

    /// Draw all paint elements and return any clicked link href.
    pub fn paint(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        elements: &[PaintElement],
        dark_mode: bool,
        textures: &HashMap<String, TextureHandle>,
    ) -> Option<String> {
        if elements.is_empty() {
            ui.colored_label(Color32::GRAY, "No renderable content");
            return None;
        }

        let available_width = ui.available_width();
        let total_height = elements
            .iter()
            .map(|e| e.rect[1] + e.rect[3])
            .fold(0.0f32, f32::max)
            + 32.0;

        let mut clicked_href: Option<String> = None;

        egui::ScrollArea::vertical().show(ui, |ui: &mut egui::Ui| {
            let (full_rect, response) = ui.allocate_exact_size(
                Vec2::new(available_width, total_height),
                egui::Sense::click().union(egui::Sense::hover()),
            );

            let painter = ui.painter_at(full_rect);
            let origin = full_rect.min;
            let theme = if dark_mode { Theme::dark() } else { Theme::light() };

            // Page background
            painter.rect_filled(full_rect, Rounding::ZERO, theme.page_bg);

            let mouse_pos = response.hover_pos();

            // Determine hovered element (foreground first, then cards)
            self.hovered_id = None;
            if let Some(pos) = mouse_pos {
                for elem in elements.iter().rev() {
                    if elem.kind == PaintKind::Card {
                        continue;
                    }
                    let r = elem_rect(elem, origin);
                    if r.contains(pos) {
                        self.hovered_id = Some(elem.id);
                        break;
                    }
                }
                if self.hovered_id.is_none() {
                    for elem in elements.iter().rev() {
                        if elem.kind != PaintKind::Card {
                            continue;
                        }
                        let r = elem_rect(elem, origin);
                        if r.contains(pos) {
                            self.hovered_id = Some(elem.id);
                            break;
                        }
                    }
                }
            }

            let mut animating = false;

            // Draw each element
            for elem in elements {
                let rect = elem_rect(elem, origin);

                // Cull offscreen
                if rect.max.y < full_rect.min.y || rect.min.y > full_rect.max.y {
                    continue;
                }

                let is_hovered = self.hovered_id == Some(elem.id);
                let hover_t = ctx.animate_value_with_time(
                    egui::Id::new(("sdf_h", elem.id)),
                    if is_hovered { 1.0 } else { 0.0 },
                    0.15,
                );
                if hover_t > 0.001 && hover_t < 0.999 {
                    animating = true;
                }

                match elem.kind {
                    PaintKind::Card => draw_card(&painter, rect, elem, hover_t, &theme),
                    PaintKind::Heading => draw_heading(&painter, ctx, rect, elem, hover_t, &theme),
                    PaintKind::Text => draw_text(&painter, ctx, rect, elem, &theme),
                    PaintKind::Link => {
                        draw_link(&painter, ctx, rect, elem, hover_t, &theme);
                        if is_hovered {
                            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                    }
                    PaintKind::Button => {
                        draw_button(&painter, ctx, rect, elem, hover_t);
                        if is_hovered {
                            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                    }
                    PaintKind::Separator => draw_separator(&painter, rect, &theme),
                    PaintKind::ImagePlaceholder => {
                        draw_image_placeholder(&painter, rect, elem, hover_t, &theme, textures);
                    }
                }
            }

            // Handle click
            if response.clicked() {
                if let Some(pos) = mouse_pos {
                    for elem in elements.iter().rev() {
                        if elem.href.is_some() {
                            let r = elem_rect(elem, origin);
                            if r.contains(pos) {
                                clicked_href = elem.href.clone();
                                break;
                            }
                        }
                    }
                }
            }

            if animating {
                ctx.request_repaint();
            }
        });

        clicked_href
    }
}

fn elem_rect(elem: &PaintElement, origin: Pos2) -> Rect {
    Rect::from_min_size(
        Pos2::new(origin.x + elem.rect[0], origin.y + elem.rect[1]),
        Vec2::new(elem.rect[2].max(1.0), elem.rect[3].max(1.0)),
    )
}

fn color4(c: [f32; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (c[0] * 255.0) as u8,
        (c[1] * 255.0) as u8,
        (c[2] * 255.0) as u8,
        (c[3] * 255.0) as u8,
    )
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |a: u8, b: u8| ((a as f32) * (1.0 - t) + (b as f32) * t) as u8;
    Color32::from_rgba_unmultiplied(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()), m(a.a(), b.a()))
}

fn paint_text_wrapped(
    painter: &egui::Painter,
    ctx: &egui::Context,
    pos: Pos2,
    text: &str,
    font_size: f32,
    color: Color32,
    max_width: f32,
) -> Rect {
    if text.is_empty() {
        return Rect::from_min_size(pos, Vec2::ZERO);
    }
    let job = egui::text::LayoutJob::simple(
        text.to_string(),
        FontId::proportional(font_size),
        color,
        max_width,
    );
    let galley = ctx.fonts(|f: &egui::epaint::Fonts| f.layout_job(job));
    let size = galley.rect.size();
    painter.galley(pos, galley, color);
    Rect::from_min_size(pos, size)
}

// ── Drawing functions ──

fn draw_card(painter: &egui::Painter, rect: Rect, elem: &PaintElement, hover_t: f32, theme: &Theme) {
    let shadow_depth = elem.shadow_depth * (1.0 + hover_t * 1.5);
    let radius = elem.corner_radius + hover_t * 2.0;
    let rounding = Rounding::same(radius);

    // Multi-layer soft shadow
    for i in 0..3 {
        let offset = shadow_depth * (i as f32 + 1.0) / 3.0;
        let alpha = (18.0 - i as f32 * 5.0).max(2.0) as u8;
        let shadow_rect = rect.translate(Vec2::new(0.0, offset));
        painter.rect_filled(
            shadow_rect.expand(offset * 0.5),
            Rounding::same(radius + offset),
            Color32::from_rgba_premultiplied(0, 0, 0, alpha),
        );
    }

    // Card background
    painter.rect_filled(rect, rounding, theme.card_bg);

    // Hover accent border
    if hover_t > 0.01 {
        let alpha = (hover_t * 80.0) as u8;
        painter.rect_stroke(
            rect,
            rounding,
            Stroke::new(1.0, Color32::from_rgba_premultiplied(0, 100, 220, alpha)),
        );
    }
}

fn draw_heading(
    painter: &egui::Painter,
    ctx: &egui::Context,
    rect: Rect,
    elem: &PaintElement,
    hover_t: f32,
    theme: &Theme,
) {
    if let Some(ref text) = elem.text {
        // Accent bar on hover
        if hover_t > 0.01 {
            let bar_w = 3.0 * hover_t;
            let bar = Rect::from_min_size(
                Pos2::new(rect.min.x - 8.0, rect.min.y + 2.0),
                Vec2::new(bar_w, elem.font_size),
            );
            painter.rect_filled(
                bar,
                Rounding::same(1.5),
                Color32::from_rgba_premultiplied(
                    theme.heading_accent.r(), theme.heading_accent.g(), theme.heading_accent.b(),
                    (hover_t * 200.0) as u8,
                ),
            );
        }

        let color = lerp_color(theme.heading_color, theme.heading_accent, hover_t * 0.3);

        paint_text_wrapped(painter, ctx, rect.min, text, elem.font_size, color, rect.width());
    }
}

fn draw_text(
    painter: &egui::Painter,
    ctx: &egui::Context,
    rect: Rect,
    elem: &PaintElement,
    theme: &Theme,
) {
    if let Some(ref text) = elem.text {
        paint_text_wrapped(painter, ctx, rect.min, text, elem.font_size, theme.text_color, rect.width());
    }
}

fn draw_link(
    painter: &egui::Painter,
    ctx: &egui::Context,
    rect: Rect,
    elem: &PaintElement,
    hover_t: f32,
    theme: &Theme,
) {
    if let Some(ref text) = elem.text {
        let color = lerp_color(theme.link_color, theme.link_hover, hover_t);

        // Hover background highlight
        if hover_t > 0.01 {
            let bg_alpha = (hover_t * 25.0) as u8;
            let bg_rect = Rect::from_min_size(
                rect.min - Vec2::new(3.0, 1.0),
                Vec2::new(rect.width().min(elem.font_size * text.len() as f32 * 0.55 + 6.0), elem.font_size + 4.0),
            );
            painter.rect_filled(
                bg_rect,
                Rounding::same(3.0),
                Color32::from_rgba_premultiplied(
                    theme.link_color.r(), theme.link_color.g(), theme.link_color.b(), bg_alpha,
                ),
            );
        }

        // Text
        let text_rect = paint_text_wrapped(painter, ctx, rect.min, text, elem.font_size, color, rect.width());

        // Underline
        let alpha = ((0.4 + hover_t * 0.6) * 255.0) as u8;
        let y = text_rect.max.y;
        painter.line_segment(
            [Pos2::new(text_rect.min.x, y), Pos2::new(text_rect.max.x, y)],
            Stroke::new(1.0, Color32::from_rgba_premultiplied(
                theme.link_color.r(), theme.link_color.g(), theme.link_color.b(), alpha,
            )),
        );
    }
}

fn draw_button(
    painter: &egui::Painter,
    ctx: &egui::Context,
    rect: Rect,
    elem: &PaintElement,
    hover_t: f32,
) {
    let radius = elem.corner_radius + hover_t * 2.0;
    let rounding = Rounding::same(radius);

    // Shadow
    let shadow_d = elem.shadow_depth * (1.0 + hover_t);
    if shadow_d > 0.0 {
        let sr = rect.translate(Vec2::new(0.0, shadow_d));
        painter.rect_filled(sr, Rounding::same(radius + 1.0), Color32::from_rgba_premultiplied(0, 0, 0, 20));
    }

    // Background
    let base = color4(elem.color);
    let bright = Color32::from_rgb(50, 140, 255);
    painter.rect_filled(rect, rounding, lerp_color(base, bright, hover_t * 0.3));

    // Label
    if let Some(ref text) = elem.text {
        paint_text_wrapped(painter, ctx, rect.center() - Vec2::new(0.0, elem.font_size * 0.5), text, elem.font_size, Color32::WHITE, rect.width());
    }
}

fn draw_separator(painter: &egui::Painter, rect: Rect, theme: &Theme) {
    let y = rect.center().y;
    painter.line_segment(
        [Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)],
        Stroke::new(1.0, theme.separator_color),
    );
}

fn draw_image_placeholder(
    painter: &egui::Painter,
    rect: Rect,
    elem: &PaintElement,
    hover_t: f32,
    theme: &Theme,
    textures: &HashMap<String, TextureHandle>,
) {
    let r = Rounding::same(elem.corner_radius + hover_t);

    // If we have a loaded texture for this image, draw it
    if let Some(ref url) = elem.image_url {
        if let Some(tex) = textures.get(url) {
            painter.rect_filled(rect, r, theme.img_bg);
            let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
            painter.image(tex.id(), rect, uv, Color32::WHITE);
            // Border on hover
            if hover_t > 0.01 {
                painter.rect_stroke(rect, r, Stroke::new(1.0, theme.img_border));
            }
            return;
        }
    }

    // Fallback placeholder
    painter.rect_filled(rect, r, theme.img_bg);
    painter.rect_stroke(rect, r, Stroke::new(1.0, theme.img_border));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "[Image]",
        FontId::proportional(14.0),
        theme.img_text,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elem_rect_offset() {
        let elem = PaintElement {
            id: 1, kind: PaintKind::Text,
            rect: [10.0, 20.0, 100.0, 30.0],
            color: [0.0; 4], corner_radius: 0.0, shadow_depth: 0.0,
            text: Some("hi".into()), font_size: 16.0, href: None, image_url: None,
        };
        let r = elem_rect(&elem, Pos2::new(50.0, 100.0));
        assert!((r.min.x - 60.0).abs() < 0.01);
        assert!((r.min.y - 120.0).abs() < 0.01);
    }

    #[test]
    fn color_conversion() {
        let c = color4([1.0, 0.0, 0.5, 1.0]);
        assert_eq!(c.r(), 255);
        assert_eq!(c.g(), 0);
        assert_eq!(c.b(), 127);
    }

    #[test]
    fn lerp_colors() {
        let a = Color32::from_rgb(0, 0, 0);
        let b = Color32::from_rgb(100, 200, 50);
        let mid = lerp_color(a, b, 0.5);
        assert_eq!(mid.r(), 50);
        assert_eq!(mid.g(), 100);
        assert_eq!(mid.b(), 25);
    }
}
