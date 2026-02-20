//! ALICE-ML powered DOM node classifier.
//!
//! Uses a 2-layer ternary neural network (16→32→9) to classify DOM nodes
//! into semantic categories. Weights are {-1, 0, +1} only — no multiplication,
//! just add/sub via `ternary_matvec`.

use alice_ml::{TernaryWeight, ternary_matvec};
use crate::dom::{Classification, DomNode, NodeType};
use super::{AD_PATTERNS, TRACKER_PATTERNS};

const NUM_FEATURES: usize = 16;
const HIDDEN_SIZE: usize = 32;
const NUM_CLASSES: usize = 9;

/// Ternary neural network classifier for DOM nodes.
///
/// Architecture: 16 features → 32 hidden (ReLU) → 9 classes (argmax)
/// All weights are ternary {-1, 0, +1}, inference uses only add/sub.
pub struct MlClassifier {
    layer1: TernaryWeight, // NUM_FEATURES → HIDDEN_SIZE
    layer2: TernaryWeight, // HIDDEN_SIZE → NUM_CLASSES
}

impl MlClassifier {
    pub fn new() -> Self {
        Self {
            layer1: init_layer1(),
            layer2: init_layer2(),
        }
    }

    /// Classify a DOM node using ternary neural network inference.
    pub fn classify(&self, node: &DomNode) -> Classification {
        // Text nodes are always content (skip inference)
        if node.node_type == NodeType::Text {
            return Classification::Content;
        }

        let features = extract_features(node);

        // Layer 1: features → hidden (with ReLU)
        let mut hidden = [0.0f32; HIDDEN_SIZE];
        ternary_matvec(&features, &self.layer1, &mut hidden);
        for h in &mut hidden {
            *h = h.max(0.0);
        }

        // Layer 2: hidden → output
        let mut output = [0.0f32; NUM_CLASSES];
        ternary_matvec(&hidden, &self.layer2, &mut output);

        // argmax → Classification
        let best_idx = output
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
            .unwrap_or(8);

        Classification::from_index(best_idx)
    }
}

/// Extract 16-dimensional feature vector from a DOM node.
///
/// Features:
///  0: tag_type (normalized encoding of HTML tag)
///  1: text_density (text chars per node, normalized)
///  2: link_density (ratio of link text to total text)
///  3: child_count (normalized)
///  4: has_ad_class (binary: class/id matches ad patterns)
///  5: has_tracker_class (binary: class/id matches tracker patterns)
///  6: has_data_ad_attr (binary: data-ad* or data-tracking* attributes)
///  7: is_script (binary)
///  8: is_style (binary)
///  9: is_nav (binary)
/// 10: is_interactive (binary: button/input/form/etc)
/// 11: is_media (binary: img/video/audio/etc)
/// 12: is_text_node (binary)
/// 13: text_length (normalized)
/// 14: has_href (binary)
/// 15: attr_count (normalized)
fn extract_features(node: &DomNode) -> [f32; NUM_FEATURES] {
    let mut f = [0.0f32; NUM_FEATURES];

    // F0: tag type encoding (normalized to ~[0, 1])
    f[0] = match node.tag.as_str() {
        "div" | "span" | "section" | "article" => 1.0,
        "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => 2.0,
        "a" => 3.0,
        "script" | "noscript" => 4.0,
        "style" => 5.0,
        "nav" => 6.0,
        "button" | "input" | "form" | "textarea" | "select" => 7.0,
        "img" | "video" | "audio" | "canvas" | "picture" => 8.0,
        "iframe" => 9.0,
        "header" | "footer" => 10.0,
        "ul" | "ol" | "li" => 11.0,
        "table" | "tr" | "td" | "th" => 12.0,
        _ => 0.0,
    } / 12.0;

    // F1: text density
    f[1] = (node.text_density() / 50.0).min(1.0);

    // F2: link density
    f[2] = node.link_density();

    // F3: child count (normalized)
    f[3] = (node.children.len() as f32 / 20.0).min(1.0);

    // F4-F5: class/id pattern matching
    let class = node.attr("class").unwrap_or("");
    let id = node.attr("id").unwrap_or("");
    let combined = format!("{} {}", class, id).to_lowercase();

    f[4] = if AD_PATTERNS.iter().any(|p| combined.contains(p)) {
        1.0
    } else {
        0.0
    };
    f[5] = if TRACKER_PATTERNS.iter().any(|p| combined.contains(p)) {
        1.0
    } else {
        0.0
    };

    // F6: data-ad / data-tracking attributes
    f[6] = if node
        .attributes
        .keys()
        .any(|k| k.starts_with("data-ad") || k.starts_with("data-tracking"))
    {
        1.0
    } else {
        0.0
    };

    // F7-F12: binary tag-type features
    f[7] = if node.tag == "script" || node.tag == "noscript" {
        1.0
    } else {
        0.0
    };
    f[8] = if node.tag == "style" { 1.0 } else { 0.0 };
    f[9] = if node.tag == "nav" { 1.0 } else { 0.0 };
    f[10] = if matches!(
        node.tag.as_str(),
        "button" | "input" | "textarea" | "select" | "form"
    ) {
        1.0
    } else {
        0.0
    };
    f[11] = if matches!(
        node.tag.as_str(),
        "img" | "video" | "audio" | "picture" | "canvas"
    ) {
        1.0
    } else {
        0.0
    };
    f[12] = if node.node_type == NodeType::Text {
        1.0
    } else {
        0.0
    };

    // F13: text length (normalized)
    f[13] = (node.collect_text().len() as f32 / 500.0).min(1.0);

    // F14: has href
    f[14] = if node.attr("href").is_some() {
        1.0
    } else {
        0.0
    };

    // F15: attribute count (normalized)
    f[15] = (node.attributes.len() as f32 / 10.0).min(1.0);

    f
}

/// Initialize Layer 1 weights: NUM_FEATURES(16) → HIDDEN_SIZE(32)
///
/// Hidden neurons are organized in groups of 4, each detecting one class:
///   H0-H3:   Content detectors
///   H4-H7:   Navigation detectors
///   H8-H11:  Ad detectors
///   H12-H15: Tracker detectors
///   H16-H19: Decoration detectors
///   H20-H23: Interactive detectors
///   H24-H27: Media detectors
///   H28-H31: Structural detectors
fn init_layer1() -> TernaryWeight {
    let mut w = vec![0i8; HIDDEN_SIZE * NUM_FEATURES];

    let set = |w: &mut Vec<i8>, row: usize, col: usize, val: i8| {
        w[row * NUM_FEATURES + col] = val;
    };

    // === Content detectors (H0-H3) ===
    // H0: +text_density, +text_length, -has_ad, -has_tracker, -is_script
    set(&mut w, 0, 1, 1);
    set(&mut w, 0, 4, -1);
    set(&mut w, 0, 5, -1);
    set(&mut w, 0, 7, -1);
    set(&mut w, 0, 13, 1);
    // H1: +is_text, +text_length
    set(&mut w, 1, 12, 1);
    set(&mut w, 1, 13, 1);
    // H2: +text_density, -link_density
    set(&mut w, 2, 1, 1);
    set(&mut w, 2, 2, -1);
    // H3: +text_density, -has_ad, -has_tracker, +is_text, -is_script
    set(&mut w, 3, 1, 1);
    set(&mut w, 3, 4, -1);
    set(&mut w, 3, 5, -1);
    set(&mut w, 3, 7, -1);
    set(&mut w, 3, 12, 1);

    // === Navigation detectors (H4-H7) ===
    // H4: +link_density, +is_nav
    set(&mut w, 4, 2, 1);
    set(&mut w, 4, 9, 1);
    // H5: -text_density, +link_density, +child_count
    set(&mut w, 5, 1, -1);
    set(&mut w, 5, 2, 1);
    set(&mut w, 5, 3, 1);
    // H6: +is_nav
    set(&mut w, 6, 9, 1);
    // H7: +link_density, +has_href
    set(&mut w, 7, 2, 1);
    set(&mut w, 7, 14, 1);

    // === Ad detectors (H8-H11) ===
    // H8: +has_ad_class, +has_data_ad
    set(&mut w, 8, 4, 1);
    set(&mut w, 8, 6, 1);
    // H9: +has_ad_class
    set(&mut w, 9, 4, 1);
    // H10: +has_data_ad, -text_density
    set(&mut w, 10, 1, -1);
    set(&mut w, 10, 6, 1);
    // H11: +has_ad_class, +has_data_ad, -is_text
    set(&mut w, 11, 4, 1);
    set(&mut w, 11, 6, 1);
    set(&mut w, 11, 12, -1);

    // === Tracker detectors (H12-H15) ===
    // H12: +has_tracker_class, +is_script
    set(&mut w, 12, 5, 1);
    set(&mut w, 12, 7, 1);
    // H13: +is_script
    set(&mut w, 13, 7, 1);
    // H14: +has_tracker_class
    set(&mut w, 14, 5, 1);
    // H15: +has_tracker_class, +is_script, -text_density
    set(&mut w, 15, 1, -1);
    set(&mut w, 15, 5, 1);
    set(&mut w, 15, 7, 1);

    // === Decoration detectors (H16-H19) ===
    // H16-H19: +is_style
    set(&mut w, 16, 8, 1);
    set(&mut w, 17, 8, 1);
    set(&mut w, 17, 1, -1); // -text_density
    set(&mut w, 18, 8, 1);
    set(&mut w, 19, 8, 1);

    // === Interactive detectors (H20-H23) ===
    // H20-H23: +is_interactive
    set(&mut w, 20, 10, 1);
    set(&mut w, 21, 10, 1);
    set(&mut w, 22, 10, 1);
    set(&mut w, 22, 11, -1); // -is_media
    set(&mut w, 23, 10, 1);

    // === Media detectors (H24-H27) ===
    // H24-H27: +is_media
    set(&mut w, 24, 11, 1);
    set(&mut w, 25, 11, 1);
    set(&mut w, 25, 10, -1); // -is_interactive
    set(&mut w, 26, 11, 1);
    set(&mut w, 27, 11, 1);

    // === Structural detectors (H28-H31) ===
    // H28: +tag_type, -is_script, -is_nav
    set(&mut w, 28, 0, 1);
    set(&mut w, 28, 7, -1);
    set(&mut w, 28, 9, -1);
    // H29: +tag_type, -is_interactive, -is_media
    set(&mut w, 29, 0, 1);
    set(&mut w, 29, 10, -1);
    set(&mut w, 29, 11, -1);
    // H30: +tag_type, +child_count
    set(&mut w, 30, 0, 1);
    set(&mut w, 30, 3, 1);
    // H31: +tag_type, +attr_count
    set(&mut w, 31, 0, 1);
    set(&mut w, 31, 15, 1);

    TernaryWeight::from_ternary(&w, HIDDEN_SIZE, NUM_FEATURES)
}

/// Initialize Layer 2 weights: HIDDEN_SIZE(32) → NUM_CLASSES(9)
///
/// Each output class connects positively to its 4 dedicated hidden neurons.
fn init_layer2() -> TernaryWeight {
    let mut w = vec![0i8; NUM_CLASSES * HIDDEN_SIZE];

    let set = |w: &mut Vec<i8>, row: usize, col: usize, val: i8| {
        w[row * HIDDEN_SIZE + col] = val;
    };

    // Output 0 (Content) ← H0-H3
    set(&mut w, 0, 0, 1);
    set(&mut w, 0, 1, 1);
    set(&mut w, 0, 2, 1);
    set(&mut w, 0, 3, 1);

    // Output 1 (Navigation) ← H4-H7
    set(&mut w, 1, 4, 1);
    set(&mut w, 1, 5, 1);
    set(&mut w, 1, 6, 1);
    set(&mut w, 1, 7, 1);

    // Output 2 (Advertisement) ← H8-H11
    set(&mut w, 2, 8, 1);
    set(&mut w, 2, 9, 1);
    set(&mut w, 2, 10, 1);
    set(&mut w, 2, 11, 1);

    // Output 3 (Tracker) ← H12-H15
    set(&mut w, 3, 12, 1);
    set(&mut w, 3, 13, 1);
    set(&mut w, 3, 14, 1);
    set(&mut w, 3, 15, 1);

    // Output 4 (Decoration) ← H16-H19
    set(&mut w, 4, 16, 1);
    set(&mut w, 4, 17, 1);
    set(&mut w, 4, 18, 1);
    set(&mut w, 4, 19, 1);

    // Output 5 (Interactive) ← H20-H23
    set(&mut w, 5, 20, 1);
    set(&mut w, 5, 21, 1);
    set(&mut w, 5, 22, 1);
    set(&mut w, 5, 23, 1);

    // Output 6 (Media) ← H24-H27
    set(&mut w, 6, 24, 1);
    set(&mut w, 6, 25, 1);
    set(&mut w, 6, 26, 1);
    set(&mut w, 6, 27, 1);

    // Output 7 (Structural) ← H28-H31
    set(&mut w, 7, 28, 1);
    set(&mut w, 7, 29, 1);
    set(&mut w, 7, 30, 1);
    set(&mut w, 7, 31, 1);

    // Output 8 (Unknown) — no connections (default fallback)

    TernaryWeight::from_ternary(&w, NUM_CLASSES, HIDDEN_SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::parser::parse_html;

    #[test]
    fn feature_extraction_basic() {
        let html = r#"<html><body><p>Hello world</p></body></html>"#;
        let tree = parse_html(html, "https://example.com");

        // Find the <p> node
        fn find_tag<'a>(node: &'a DomNode, tag: &str) -> Option<&'a DomNode> {
            if node.tag == tag {
                return Some(node);
            }
            for child in &node.children {
                if let Some(found) = find_tag(child, tag) {
                    return Some(found);
                }
            }
            None
        }

        let p_node = find_tag(&tree.root, "p").expect("should find <p>");
        let features = extract_features(p_node);

        // tag_type for "p" should be 2.0/12.0
        assert!((features[0] - 2.0 / 12.0).abs() < 0.01);
        // is_script should be 0
        assert_eq!(features[7], 0.0);
        // is_text_node should be 0 (it's an element)
        assert_eq!(features[12], 0.0);
    }

    #[test]
    fn ml_classifies_script_as_tracker() {
        let html = r#"<html><body><script>var x = 1;</script></body></html>"#;
        let tree = parse_html(html, "https://example.com");

        fn find_tag<'a>(node: &'a DomNode, tag: &str) -> Option<&'a DomNode> {
            if node.tag == tag {
                return Some(node);
            }
            for child in &node.children {
                if let Some(found) = find_tag(child, tag) {
                    return Some(found);
                }
            }
            None
        }

        let script = find_tag(&tree.root, "script").expect("should find <script>");
        let classifier = MlClassifier::new();
        let result = classifier.classify(script);
        assert_eq!(result, Classification::Tracker);
    }

    #[test]
    fn ml_classifies_ad_div() {
        use std::collections::HashMap;

        let mut attrs = HashMap::new();
        attrs.insert("class".to_string(), "ad-banner sponsored".to_string());
        let node = DomNode::element("div", attrs, Vec::new());

        let classifier = MlClassifier::new();
        let result = classifier.classify(&node);
        assert_eq!(result, Classification::Advertisement);
    }

    #[test]
    fn ml_classifies_nav() {
        use std::collections::HashMap;

        let node = DomNode::element("nav", HashMap::new(), Vec::new());
        let classifier = MlClassifier::new();
        let result = classifier.classify(&node);
        assert_eq!(result, Classification::Navigation);
    }
}
