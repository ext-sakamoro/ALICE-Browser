use crate::dom::{Classification, DomNode, NodeType};

/// Bounding box for a laid-out DOM node
#[derive(Debug, Clone, Copy)]
pub struct LayoutBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A DOM node with computed layout
#[derive(Debug, Clone)]
pub struct LayoutNode {
    pub tag: String,
    pub text: String,
    pub classification: Classification,
    pub bounds: LayoutBox,
    pub children: Vec<LayoutNode>,
    pub is_block: bool,
    pub font_size: f32,
    pub href: Option<String>,
}

const BLOCK_TAGS: &[&str] = &[
    "html",
    "body",
    "div",
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "ul",
    "ol",
    "li",
    "table",
    "tr",
    "td",
    "th",
    "form",
    "section",
    "article",
    "aside",
    "main",
    "header",
    "footer",
    "nav",
    "blockquote",
    "pre",
    "figure",
    "figcaption",
    "details",
    "summary",
];

/// Per-tag vertical margins (top, bottom) in pixels.
fn tag_margins(tag: &str) -> (f32, f32) {
    match tag {
        "h1" => (24.0, 16.0),
        "h2" => (20.0, 12.0),
        "h3" | "h4" => (16.0, 10.0),
        "h5" | "h6" => (12.0, 8.0),
        "p" => (4.0, 10.0),
        "ul" | "ol" => (8.0, 8.0),
        "li" => (2.0, 2.0),
        "section" | "article" | "main" => (16.0, 16.0),
        "nav" | "header" | "footer" => (12.0, 12.0),
        "blockquote" => (12.0, 12.0),
        "pre" => (8.0, 8.0),
        "hr" => (8.0, 8.0),
        _ => (0.0, 0.0),
    }
}

/// Per-tag padding in pixels.
fn tag_padding(tag: &str, is_block: bool) -> f32 {
    match tag {
        "section" | "article" | "main" | "aside" => 16.0,
        "nav" | "header" | "footer" => 12.0,
        "blockquote" => 20.0,
        _ if is_block => 4.0,
        _ => 0.0,
    }
}

/// Compute layout for a DOM tree (simple top-to-bottom block model).
pub fn compute_layout(root: &DomNode, viewport_width: f32) -> LayoutNode {
    let mut cursor_y = 0.0;
    layout_node(root, 0.0, &mut cursor_y, viewport_width, 16.0)
}

fn layout_node(
    node: &DomNode,
    x: f32,
    cursor_y: &mut f32,
    available_width: f32,
    parent_font_size: f32,
) -> LayoutNode {
    // Skip invisible nodes
    if !node.is_visible() {
        return LayoutNode {
            tag: node.tag.clone(),
            text: String::new(),
            classification: node.classification,
            bounds: LayoutBox {
                x,
                y: *cursor_y,
                width: 0.0,
                height: 0.0,
            },
            children: Vec::new(),
            is_block: false,
            font_size: parent_font_size,
            href: None,
        };
    }

    let is_block =
        node.node_type == NodeType::Element && BLOCK_TAGS.contains(&node.tag.as_str());

    let font_size = match node.tag.as_str() {
        "h1" => 32.0,
        "h2" => 24.0,
        "h3" => 20.0,
        "h4" => 18.0,
        "h5" | "h6" => 16.0,
        "small" => 12.0,
        _ => parent_font_size,
    };

    let (margin_top, margin_bottom) = tag_margins(&node.tag);
    let padding = tag_padding(&node.tag, is_block);

    if is_block {
        *cursor_y += margin_top;
    }

    let start_y = *cursor_y;

    if padding > 0.0 {
        *cursor_y += padding;
    }

    // Layout children
    let child_x = x + padding;
    let child_width = (available_width - padding * 2.0).max(0.0);
    let mut children = Vec::new();

    for child in &node.children {
        if !child.is_visible() {
            continue;
        }
        let laid_out = layout_node(child, child_x, cursor_y, child_width, font_size);
        children.push(laid_out);
    }

    // Text content contributes to height
    let text = node.text.clone();
    if !text.is_empty() {
        let line_height = font_size * 1.4;
        let chars_per_line = (available_width / (font_size * 0.6)).max(1.0) as usize;
        let lines = (text.len() as f32 / chars_per_line as f32).ceil().max(1.0);
        *cursor_y += lines * line_height;
    }

    if padding > 0.0 {
        *cursor_y += padding;
    }

    let height = *cursor_y - start_y;

    if is_block {
        *cursor_y += margin_bottom;
    }

    // Extract href from <a> tags, or src from <img> tags
    let href = match node.tag.as_str() {
        "a" => node.attr("href").map(|s| s.to_string()),
        "img" => node.attr("src").map(|s| s.to_string()),
        _ => None,
    };

    LayoutNode {
        tag: node.tag.clone(),
        text,
        classification: node.classification,
        bounds: LayoutBox {
            x,
            y: start_y,
            width: available_width,
            height,
        },
        children,
        is_block,
        font_size,
        href,
    }
}
