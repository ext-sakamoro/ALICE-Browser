//! Readability-style content extraction.
//!
//! Scores DOM subtrees by text density, link density, tag semantics,
//! and class/id keyword hints.  The highest-scoring block is promoted
//! to `Classification::Content` so layout and paint emphasise it.

use crate::dom::{Classification, DomNode, NodeType};

/// Score a single element node for content-richness.
fn score_node(node: &DomNode) -> f32 {
    let text = node.collect_text();
    let text_len = text.len() as f32;

    if text_len < 25.0 {
        return -1.0;
    }

    let mut score: f32 = 0.0;

    // Text length (log-scale, capped)
    score += text_len.ln().min(8.0);

    // Text density bonus
    score += node.text_density().min(50.0) * 0.3;

    // Link density penalty
    score -= node.link_density() * 25.0;

    // Tag bonuses / penalties
    match node.tag.as_str() {
        "article" | "main" => score += 10.0,
        "section" => score += 5.0,
        "p" => score += 3.0,
        "blockquote" | "pre" => score += 3.0,
        "div" => score += 1.0,
        "nav" | "aside" => score -= 10.0,
        "footer" | "header" => score -= 5.0,
        "form" => score -= 5.0,
        _ => {}
    }

    // Classification bonuses
    match node.classification {
        Classification::Content => score += 5.0,
        Classification::Navigation => score -= 8.0,
        Classification::Structural => score -= 3.0,
        _ => {}
    }

    // ID / class keyword hints
    let id_class = format!(
        "{} {}",
        node.attr("id").unwrap_or(""),
        node.attr("class").unwrap_or("")
    )
    .to_lowercase();

    for kw in &["content", "article", "post", "entry", "main-text", "body-text"] {
        if id_class.contains(kw) {
            score += 8.0;
        }
    }
    for kw in &[
        "sidebar", "nav", "menu", "comment", "footer", "header", "ad", "social", "share", "widget",
    ] {
        if id_class.contains(kw) {
            score -= 8.0;
        }
    }

    // Paragraph count bonus
    let p_count = node.children.iter().filter(|c| c.tag == "p").count();
    score += p_count as f32 * 2.0;

    score
}

/// Walk the tree and find the path (child indices) to the best content node.
fn find_best_path(
    node: &DomNode,
    current: &mut Vec<usize>,
    best_path: &mut Vec<usize>,
    best_score: &mut f32,
) {
    if node.node_type == NodeType::Element {
        let s = score_node(node);
        if s > *best_score {
            *best_score = s;
            *best_path = current.clone();
        }
    }
    for (i, child) in node.children.iter().enumerate() {
        current.push(i);
        find_best_path(child, current, best_path, best_score);
        current.pop();
    }
}

fn walk_path_mut<'a>(root: &'a mut DomNode, path: &[usize]) -> Option<&'a mut DomNode> {
    let mut current = root;
    for &idx in path {
        if idx >= current.children.len() {
            return None;
        }
        current = &mut current.children[idx];
    }
    Some(current)
}

fn mark_content(node: &mut DomNode) {
    if node.classification == Classification::Unknown {
        node.classification = Classification::Content;
    }
    for child in &mut node.children {
        mark_content(child);
    }
}

/// Boost the most content-rich subtree to `Classification::Content`.
pub fn readability_boost(root: &mut DomNode) {
    let mut best_score = 5.0f32; // minimum threshold
    let mut best_path: Vec<usize> = Vec::new();

    find_best_path(root, &mut Vec::new(), &mut best_path, &mut best_score);

    if best_path.is_empty() {
        return;
    }

    if let Some(node) = walk_path_mut(root, &best_path) {
        mark_content(node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn elem(tag: &str, text: &str, children: Vec<DomNode>) -> DomNode {
        let mut node = DomNode::element(tag, HashMap::new(), children);
        if !text.is_empty() {
            node.children.push(DomNode::text(text));
        }
        node
    }

    #[test]
    fn article_scores_higher_than_nav() {
        let article = elem(
            "article",
            &"content text ".repeat(20),
            vec![
                elem("p", &"paragraph one ".repeat(10), vec![]),
                elem("p", &"paragraph two ".repeat(10), vec![]),
            ],
        );
        let nav = elem(
            "nav",
            "",
            vec![
                elem("a", "Link 1", vec![]),
                elem("a", "Link 2", vec![]),
            ],
        );
        assert!(score_node(&article) > score_node(&nav));
    }

    #[test]
    fn boost_marks_content() {
        let mut root = DomNode::element(
            "body",
            HashMap::new(),
            vec![
                elem("nav", "", vec![elem("a", "Home", vec![])]),
                elem(
                    "article",
                    "",
                    vec![
                        elem("p", &"Long article text. ".repeat(15), vec![]),
                        elem("p", &"More article text. ".repeat(15), vec![]),
                    ],
                ),
                elem("footer", "Copyright", vec![]),
            ],
        );

        readability_boost(&mut root);

        // Article children should now be Content
        assert_eq!(
            root.children[1].children[0].classification,
            Classification::Content
        );
    }
}
