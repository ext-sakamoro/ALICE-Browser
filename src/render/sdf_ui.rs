/// SDF-based UI rendering.
///
/// Maps DOM elements to SDF primitives for procedural rendering.
/// Phase 1: Scene description generation.
/// Phase 2: ALICE-SDF integration for GPU rendering.
use crate::dom::Classification;
use crate::render::layout::LayoutNode;

/// SDF primitive types for UI elements
#[derive(Debug, Clone)]
pub enum SdfPrimitive {
    /// Rounded rectangle (buttons, cards, containers)
    RoundedBox {
        center: [f32; 3],
        size: [f32; 3],
        radius: f32,
        color: [f32; 4],
    },
    /// Flat plane (text backgrounds, images)
    Plane {
        center: [f32; 3],
        size: [f32; 2],
        color: [f32; 4],
    },
    /// Text label (rendered as texture on plane)
    TextLabel {
        position: [f32; 3],
        text: String,
        font_size: f32,
        color: [f32; 4],
    },
    /// Line separator
    Line {
        start: [f32; 3],
        end: [f32; 3],
        thickness: f32,
        color: [f32; 4],
    },
    /// Sphere (OZ Mode: planet/satellite nodes)
    Sphere {
        center: [f32; 3],
        radius: f32,
        color: [f32; 4],
    },
    /// Billboard text (always faces camera; OZ Mode: floating hologram labels)
    Billboard {
        position: [f32; 3],
        size: [f32; 2],
        text: String,
        color: [f32; 4],
        /// Opacity 0.0–1.0 (default 1.0). Used for holographic transparency.
        opacity: f32,
    },
    /// Torus ring (OZ Mode: orbital rings)
    Torus {
        center: [f32; 3],
        major_radius: f32,
        minor_radius: f32,
        /// Axis of rotation: [nx, ny, nz] (normalized)
        axis: [f32; 3],
        color: [f32; 4],
    },
}

/// Complete SDF scene for a web page
#[derive(Debug, Clone)]
pub struct SdfScene {
    pub primitives: Vec<SdfPrimitive>,
    pub background_color: [f32; 4],
}

/// Convert a layout tree to an SDF scene description
pub fn layout_to_sdf(root: &LayoutNode, scale: f32) -> SdfScene {
    let mut primitives = Vec::new();
    emit_sdf_primitives(root, &mut primitives, scale, 0);

    SdfScene {
        primitives,
        background_color: [0.98, 0.98, 0.98, 1.0],
    }
}

fn emit_sdf_primitives(
    node: &LayoutNode,
    primitives: &mut Vec<SdfPrimitive>,
    scale: f32,
    depth: u32,
) {
    let b = &node.bounds;
    let z = depth as f32 * -0.01 * scale;

    match node.tag.as_str() {
        // Headings
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let text = collect_child_text(node);
            if !text.is_empty() {
                primitives.push(SdfPrimitive::TextLabel {
                    position: [b.x * scale, -b.y * scale, z],
                    text,
                    font_size: node.font_size * scale,
                    color: [0.1, 0.1, 0.1, 1.0],
                });
            }
        }
        // Paragraphs and list items
        "p" | "span" | "li" => {
            let text = collect_child_text(node);
            if !text.is_empty() {
                primitives.push(SdfPrimitive::TextLabel {
                    position: [b.x * scale, -b.y * scale, z],
                    text,
                    font_size: node.font_size * scale,
                    color: [0.2, 0.2, 0.2, 1.0],
                });
            }
        }
        // Links: highlighted text
        "a" => {
            let text = collect_child_text(node);
            if !text.is_empty() {
                primitives.push(SdfPrimitive::TextLabel {
                    position: [b.x * scale, -b.y * scale, z],
                    text,
                    font_size: node.font_size * scale,
                    color: [0.0, 0.4, 0.8, 1.0],
                });
            }
        }
        // Buttons: rounded box
        "button" => {
            primitives.push(SdfPrimitive::RoundedBox {
                center: [
                    (b.x + b.width / 2.0) * scale,
                    -(b.y + b.height / 2.0) * scale,
                    z,
                ],
                size: [b.width * scale, b.height * scale, 0.02 * scale],
                radius: 4.0 * scale,
                color: [0.2, 0.5, 0.9, 1.0],
            });
        }
        // Images: placeholder plane
        "img" => {
            primitives.push(SdfPrimitive::Plane {
                center: [
                    (b.x + b.width / 2.0) * scale,
                    -(b.y + b.height / 2.0) * scale,
                    z,
                ],
                size: [b.width * scale, b.height * scale],
                color: [0.85, 0.85, 0.85, 1.0],
            });
        }
        // Horizontal rule
        "hr" => {
            primitives.push(SdfPrimitive::Line {
                start: [b.x * scale, -b.y * scale, z],
                end: [(b.x + b.width) * scale, -b.y * scale, z],
                thickness: 1.0 * scale,
                color: [0.7, 0.7, 0.7, 1.0],
            });
        }
        // Containers: subtle background if content-rich
        "div" | "section" | "article" | "main" => {
            if node.classification == Classification::Content && b.height > 0.0 {
                primitives.push(SdfPrimitive::RoundedBox {
                    center: [
                        (b.x + b.width / 2.0) * scale,
                        -(b.y + b.height / 2.0) * scale,
                        z - 0.001,
                    ],
                    size: [b.width * scale, b.height * scale, 0.001 * scale],
                    radius: 2.0 * scale,
                    color: [1.0, 1.0, 1.0, 0.5],
                });
            }
        }
        _ => {}
    }

    // Text-only nodes
    if node.tag.is_empty() && !node.text.is_empty() {
        primitives.push(SdfPrimitive::TextLabel {
            position: [b.x * scale, -b.y * scale, z],
            text: node.text.clone(),
            font_size: node.font_size * scale,
            color: [0.2, 0.2, 0.2, 1.0],
        });
    }

    for child in &node.children {
        emit_sdf_primitives(child, primitives, scale, depth + 1);
    }
}

fn collect_child_text(node: &LayoutNode) -> String {
    let mut text = String::new();
    if !node.text.is_empty() {
        text.push_str(node.text.trim());
    }
    for child in &node.children {
        let ct = collect_child_text(child);
        if !ct.is_empty() {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&ct);
        }
    }
    text
}

// ── Paint elements for egui Painter-based SDF rendering ──

/// Paint element kind for interactive SDF UI.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaintKind {
    /// Card container with shadow (section, article)
    Card,
    /// Regular body text
    Text,
    /// Heading text (h1-h6)
    Heading,
    /// Clickable link
    Link,
    /// Button
    Button,
    /// Horizontal separator
    Separator,
    /// Image placeholder
    ImagePlaceholder,
}

/// A UI element for egui Painter-based SDF rendering.
#[derive(Debug, Clone)]
pub struct PaintElement {
    pub id: usize,
    pub kind: PaintKind,
    /// Bounding rect in screen pixels: [x, y, width, height]
    pub rect: [f32; 4],
    pub color: [f32; 4],
    pub corner_radius: f32,
    pub shadow_depth: f32,
    pub text: Option<String>,
    pub font_size: f32,
    pub href: Option<String>,
    pub image_url: Option<String>,
}

/// Convert a layout tree into paint elements for egui SDF rendering.
pub fn layout_to_paint(root: &LayoutNode) -> Vec<PaintElement> {
    let mut elements = Vec::new();
    let mut id = 0;
    emit_paint_elements(root, &mut elements, &mut id);
    elements
}

fn emit_paint_elements(
    node: &LayoutNode,
    out: &mut Vec<PaintElement>,
    id: &mut usize,
) {
    let b = &node.bounds;
    if b.height <= 0.0 && node.text.is_empty() && node.children.is_empty() {
        return;
    }

    match node.tag.as_str() {
        // Container cards
        "section" | "article" | "main" | "aside" => {
            if node.classification == Classification::Content && b.height > 10.0 {
                *id += 1;
                out.push(PaintElement {
                    id: *id, kind: PaintKind::Card,
                    rect: [b.x, b.y, b.width, b.height],
                    color: [1.0, 1.0, 1.0, 1.0],
                    corner_radius: 8.0, shadow_depth: 3.0,
                    text: None, font_size: 0.0, href: None, image_url: None,
                });
            }
        }
        "nav" | "header" | "footer" => {
            if b.height > 5.0 {
                *id += 1;
                out.push(PaintElement {
                    id: *id, kind: PaintKind::Card,
                    rect: [b.x, b.y, b.width, b.height],
                    color: [0.96, 0.97, 0.98, 1.0],
                    corner_radius: 4.0, shadow_depth: 1.0,
                    text: None, font_size: 0.0, href: None, image_url: None,
                });
            }
        }
        // Headings
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let text = collect_child_text(node);
            if !text.is_empty() {
                *id += 1;
                out.push(PaintElement {
                    id: *id, kind: PaintKind::Heading,
                    rect: [b.x, b.y, b.width, b.height],
                    color: [0.1, 0.1, 0.15, 1.0],
                    corner_radius: 0.0, shadow_depth: 0.0,
                    text: Some(text), font_size: node.font_size, href: None, image_url: None,
                });
            }
            return; // text already collected
        }
        // Paragraphs / list items
        "p" | "span" | "li" => {
            let text = collect_child_text(node);
            if !text.is_empty() {
                *id += 1;
                let prefix = if node.tag == "li" { "\u{2022} " } else { "" };
                out.push(PaintElement {
                    id: *id, kind: PaintKind::Text,
                    rect: [b.x, b.y, b.width, b.height],
                    color: [0.15, 0.15, 0.18, 1.0],
                    corner_radius: 0.0, shadow_depth: 0.0,
                    text: Some(format!("{}{}", prefix, text)),
                    font_size: node.font_size, href: None, image_url: None,
                });
            }
            return;
        }
        // Links
        "a" => {
            let text = collect_child_text(node);
            if !text.is_empty() {
                *id += 1;
                out.push(PaintElement {
                    id: *id, kind: PaintKind::Link,
                    rect: [b.x, b.y, b.width, b.height],
                    color: [0.0, 0.4, 0.85, 1.0],
                    corner_radius: 3.0, shadow_depth: 0.0,
                    text: Some(text), font_size: node.font_size,
                    href: node.href.clone(), image_url: None,
                });
            }
            return;
        }
        // Buttons
        "button" => {
            let text = collect_child_text(node);
            *id += 1;
            out.push(PaintElement {
                id: *id, kind: PaintKind::Button,
                rect: [b.x, b.y, b.width, b.height.max(32.0)],
                color: [0.2, 0.5, 0.95, 1.0],
                corner_radius: 6.0, shadow_depth: 2.0,
                text: if text.is_empty() { None } else { Some(text) },
                font_size: node.font_size, href: None, image_url: None,
            });
            return;
        }
        // Images
        "img" => {
            *id += 1;
            let img_url = node.href.clone(); // layout stores src in href for img tags
            out.push(PaintElement {
                id: *id, kind: PaintKind::ImagePlaceholder,
                rect: [b.x, b.y, b.width.min(400.0), b.height.max(60.0).min(200.0)],
                color: [0.92, 0.92, 0.94, 1.0],
                corner_radius: 4.0, shadow_depth: 1.0,
                text: None, font_size: 0.0, href: None, image_url: img_url,
            });
            return;
        }
        // Horizontal rule
        "hr" => {
            *id += 1;
            out.push(PaintElement {
                id: *id, kind: PaintKind::Separator,
                rect: [b.x, b.y, b.width, 1.0],
                color: [0.8, 0.8, 0.82, 1.0],
                corner_radius: 0.0, shadow_depth: 0.0,
                text: None, font_size: 0.0, href: None, image_url: None,
            });
            return;
        }
        _ => {
            // Bare text nodes
            if node.tag.is_empty() && !node.text.is_empty() {
                *id += 1;
                out.push(PaintElement {
                    id: *id, kind: PaintKind::Text,
                    rect: [b.x, b.y, b.width, b.height],
                    color: [0.15, 0.15, 0.18, 1.0],
                    corner_radius: 0.0, shadow_depth: 0.0,
                    text: Some(node.text.trim().to_string()),
                    font_size: node.font_size, href: None, image_url: None,
                });
            }
        }
    }

    // Recurse for container elements
    for child in &node.children {
        emit_paint_elements(child, out, id);
    }
}
