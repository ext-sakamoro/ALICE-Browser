//! OZ mode link-preview data types and background-fetch helpers.
//!
//! Everything in this module runs either on the main thread (data types,
//! state accessors) or in a spawned background thread (fetch helpers).
//! No egui types are imported here so the module stays renderer-agnostic.

use alice_browser::dom::DomNode;
use alice_browser::render::stream::TextMeta;

// ─── Data types ──────────────────────────────────────────────────────────────

/// Preview data fetched for a grabbed OZ-mode link.
#[derive(Clone)]
pub struct LinkPreview {
    pub url: String,
    pub title: String,
    pub description: String,
    pub texts: Vec<String>,
    pub status: LinkPreviewStatus,
}

#[derive(Clone, PartialEq)]
pub enum LinkPreviewStatus {
    Loading,
    Ready,
    Error(String),
}

// ─── URL helpers ─────────────────────────────────────────────────────────────

/// Resolve a potentially relative URL against a base URL.
pub fn resolve_url(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    if href.starts_with("//") {
        return format!("https:{}", href);
    }
    if let Ok(base_url) = url::Url::parse(base) {
        if let Ok(resolved) = base_url.join(href) {
            return resolved.to_string();
        }
    }
    href.to_string()
}

// ─── DOM href collection ─────────────────────────────────────────────────────

/// Collect unique hrefs from a DomNode tree, resolved to absolute URLs.
pub fn collect_hrefs_from_dom(node: &DomNode, base_url: &str, limit: usize) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut hrefs = Vec::new();
    collect_hrefs_recursive(node, base_url, limit, &mut seen, &mut hrefs);
    hrefs
}

fn collect_hrefs_recursive(
    node: &DomNode,
    base_url: &str,
    limit: usize,
    seen: &mut std::collections::HashSet<String>,
    out: &mut Vec<String>,
) {
    if out.len() >= limit {
        return;
    }
    if node.tag == "a" {
        if let Some(href) = node.attributes.get("href") {
            let abs = resolve_url(base_url, href);
            if abs.starts_with("http") && seen.insert(abs.clone()) {
                out.push(abs);
                if out.len() >= limit {
                    return;
                }
            }
        }
    }
    for child in &node.children {
        collect_hrefs_recursive(child, base_url, limit, seen, out);
        if out.len() >= limit {
            return;
        }
    }
}

// ─── Prefetch text extraction ─────────────────────────────────────────────────

/// Extract texts from a prefetched page as `TextMeta` for injection into the Rotunda.
pub fn extract_prefetch_texts(node: &DomNode, out: &mut Vec<TextMeta>, depth: usize) {
    use alice_browser::dom::Classification;

    if out.len() >= 60 {
        return;
    }
    if depth > 20 {
        return;
    }

    if matches!(
        node.classification,
        Classification::Advertisement | Classification::Tracker | Classification::Decoration
    ) {
        return;
    }

    let tag = node.tag.as_str();
    let (importance, is_leaf) = match tag {
        "h1" | "h2" => (0.9, true),
        "h3" | "h4" | "h5" | "h6" => (0.5, true),
        "a" => (0.4, true),
        "p" | "li" => (0.2, true),
        "span" | "em" | "strong" | "b" | "i" | "u" | "small" => (0.15, true),
        "td" | "th" | "dt" | "dd" | "figcaption" | "summary" | "time" => (0.15, true),
        _ => (0.1, false),
    };

    if is_leaf {
        let full = collect_dom_text(node);
        let trimmed = full.trim();
        if trimmed.len() > 1 && trimmed.chars().count() <= 80 {
            let display: String = trimmed.chars().take(40).collect();
            let href = node.attributes.get("href").cloned();
            out.push(TextMeta {
                display,
                full_text: trimmed.chars().take(300).collect(),
                tag: tag.to_string(),
                href,
                category_index: 0,
                importance,
            });
        }
        return;
    }

    for child in &node.children {
        extract_prefetch_texts(child, out, depth + 1);
    }
}

// ─── Link preview fetching ────────────────────────────────────────────────────

/// Fetch a URL and extract preview info (title + description + key texts).
/// Intended to run in a background thread.
pub fn fetch_link_preview(url: &str) -> LinkPreview {
    use alice_browser::net::fetch::fetch_url;
    use alice_browser::dom::parser::parse_html;

    match fetch_url(url) {
        Ok(result) => {
            let dom = parse_html(&result.html, &result.url);
            let title = if dom.title.is_empty() {
                url.to_string()
            } else {
                dom.title.clone()
            };

            let description = extract_meta_description(&dom.root);

            let mut headings = Vec::new();
            let mut paragraphs = Vec::new();
            let mut others = Vec::new();
            extract_preview_texts_ranked(&dom.root, &mut headings, &mut paragraphs, &mut others, 0);

            let mut texts = Vec::new();
            for t in &headings {
                if texts.len() < 50 {
                    texts.push(t.clone());
                }
            }
            for t in &paragraphs {
                if texts.len() < 50 {
                    texts.push(t.clone());
                }
            }
            for t in &others {
                if texts.len() < 50 {
                    texts.push(t.clone());
                }
            }

            LinkPreview {
                url: url.to_string(),
                title,
                description,
                texts,
                status: LinkPreviewStatus::Ready,
            }
        }
        Err(e) => LinkPreview {
            url: url.to_string(),
            title: String::new(),
            description: String::new(),
            texts: Vec::new(),
            status: LinkPreviewStatus::Error(e.to_string()),
        },
    }
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Collect all text content from a DOM node and its descendants.
pub fn collect_dom_text(node: &DomNode) -> String {
    let mut s = String::new();
    if !node.text.is_empty() {
        s.push_str(node.text.trim());
    }
    for child in &node.children {
        let ct = collect_dom_text(child);
        if !ct.is_empty() {
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str(&ct);
        }
    }
    s
}

/// Extract meta description from DOM (`<meta name="description">` or `og:description`).
fn extract_meta_description(node: &DomNode) -> String {
    if node.tag == "meta" {
        let name = node.attributes.get("name").map(|s| s.to_lowercase());
        let property = node.attributes.get("property").map(|s| s.to_lowercase());
        let is_desc = name.as_deref() == Some("description")
            || property.as_deref() == Some("og:description");
        if is_desc {
            if let Some(content) = node.attributes.get("content") {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
    }
    for child in &node.children {
        let desc = extract_meta_description(child);
        if !desc.is_empty() {
            return desc;
        }
    }
    String::new()
}

/// Extract texts ranked by importance: headings, paragraphs, then others.
fn extract_preview_texts_ranked(
    node: &DomNode,
    headings: &mut Vec<String>,
    paragraphs: &mut Vec<String>,
    others: &mut Vec<String>,
    depth: usize,
) {
    use alice_browser::dom::Classification;

    if matches!(
        node.classification,
        Classification::Advertisement | Classification::Tracker | Classification::Decoration
    ) {
        return;
    }
    if matches!(
        node.tag.as_str(),
        "nav" | "header" | "footer" | "script" | "style" | "noscript"
    ) {
        return;
    }

    let tag = node.tag.as_str();

    if matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6") {
        let text = collect_dom_text(node);
        let trimmed = text.trim().to_string();
        if trimmed.chars().count() > 2 && headings.len() < 10 {
            headings.push(trimmed);
        }
        return;
    }

    if tag == "p" {
        let text = collect_dom_text(node);
        let trimmed = text.trim().to_string();
        if trimmed.chars().count() > 8 && paragraphs.len() < 30 {
            paragraphs.push(trimmed);
        }
        return;
    }

    if matches!(
        tag,
        "li" | "td" | "th" | "dd" | "blockquote" | "figcaption" | "article"
    ) {
        let text = collect_dom_text(node);
        let trimmed = text.trim().to_string();
        if trimmed.chars().count() > 6 && others.len() < 20 {
            others.push(trimmed);
        }
        return;
    }

    if !node.text.is_empty() && node.tag.is_empty() {
        let t = node.text.trim();
        if t.chars().count() > 8 && others.len() < 20 {
            others.push(t.to_string());
        }
    }

    if depth < 10 {
        for child in &node.children {
            if headings.len() >= 10 && paragraphs.len() >= 30 && others.len() >= 20 {
                break;
            }
            extract_preview_texts_ranked(child, headings, paragraphs, others, depth + 1);
        }
    }
}
