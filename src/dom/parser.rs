use crate::dom::{DomNode, DomTree};
use scraper::{ElementRef, Html, Node};
use std::collections::HashMap;

/// Tags whose children should be stripped (invisible/script content)
const SKIP_CHILDREN: &[&str] = &["script", "style", "noscript", "svg"];

/// Parse raw HTML string into an ALICE DomTree
pub fn parse_html(html: &str, url: &str) -> DomTree {
    let document = Html::parse_document(html);

    // Extract <title>
    let title = scraper::Selector::parse("title")
        .ok()
        .and_then(|sel| document.select(&sel).next())
        .map(|el| el.text().collect::<String>())
        .unwrap_or_default();

    let root = convert_element(document.root_element());

    DomTree {
        root,
        url: url.to_string(),
        title: title.trim().to_string(),
    }
}

fn convert_element(el: ElementRef<'_>) -> DomNode {
    let tag = el.value().name.local.as_ref().to_string();
    let attributes: HashMap<String, String> = el
        .value()
        .attrs()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    // Skip children of invisible elements
    if SKIP_CHILDREN.contains(&tag.as_str()) {
        return DomNode::element(tag, attributes, Vec::new());
    }

    let mut children = Vec::new();

    for child_ref in el.children() {
        match child_ref.value() {
            Node::Element(_) => {
                if let Some(child_el) = ElementRef::wrap(child_ref) {
                    children.push(convert_element(child_el));
                }
            }
            Node::Text(t) => {
                let s = t.text.to_string();
                if !s.trim().is_empty() {
                    children.push(DomNode::text(s));
                }
            }
            _ => {}
        }
    }

    DomNode::element(tag, attributes, children)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_html() {
        let html = r#"
        <html>
            <head><title>Test Page</title></head>
            <body>
                <h1>Hello, ALICE</h1>
                <p>Content paragraph</p>
            </body>
        </html>
        "#;

        let tree = parse_html(html, "https://example.com");
        assert_eq!(tree.title, "Test Page");
        assert!(tree.root.node_count() > 0);
    }

    #[test]
    fn strips_script_children() {
        let html = r#"
        <html><body>
            <p>Visible</p>
            <script>alert("hidden");</script>
        </body></html>
        "#;

        let tree = parse_html(html, "https://example.com");
        let text = tree.root.collect_text();
        assert!(text.contains("Visible"));
        assert!(!text.contains("alert"));
    }
}
