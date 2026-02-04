use crate::dom::{Classification, DomNode, DomTree};

#[cfg(not(feature = "ml-filter"))]
use crate::dom::NodeType;

#[cfg(feature = "ml-filter")]
#[path = "ml_classifier.rs"]
mod ml_classifier;

/// Statistics from the semantic filtering pass
pub struct FilterStats {
    pub total_nodes: usize,
    pub content_nodes: usize,
    pub ad_nodes: usize,
    pub tracker_nodes: usize,
    pub nav_nodes: usize,
    pub removed_nodes: usize,
}

/// Known advertising patterns in class names and IDs
const AD_PATTERNS: &[&str] = &[
    "ad",
    "ads",
    "advert",
    "advertisement",
    "banner",
    "sponsor",
    "promoted",
    "promo",
    "commercial",
    "marketing",
    "adsense",
    "doubleclick",
    "taboola",
    "outbrain",
    "prebid",
    "ad-slot",
    "ad-container",
    "ad-wrapper",
    "ad-unit",
    "google-ad",
    "dfp-ad",
    "gpt-ad",
];

const TRACKER_PATTERNS: &[&str] = &[
    "tracker",
    "tracking",
    "analytics",
    "pixel",
    "beacon",
    "telemetry",
    "fingerprint",
    "cookie-banner",
    "cookie-consent",
    "gdpr",
    "ccpa",
    "privacy-notice",
    "newsletter-popup",
    "subscribe-modal",
    "popup-overlay",
];

#[cfg(not(feature = "ml-filter"))]
const AD_DOMAINS: &[&str] = &[
    "doubleclick.net",
    "googlesyndication.com",
    "googleadservices.com",
    "moatads.com",
    "amazon-adsystem.com",
    "facebook.com/tr",
    "adnxs.com",
    "criteo.com",
    "taboola.com",
    "outbrain.com",
];

/// Semantic filter: classifies DOM nodes and removes ads/trackers.
///
/// Phase 1: Rule-based heuristics (class/id patterns, tag types, content density).
/// Phase 2: ALICE-ML ternary inference for learned classification.
pub struct SemanticFilter {
    #[cfg(feature = "ml-filter")]
    ml: ml_classifier::MlClassifier,
}

impl SemanticFilter {
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "ml-filter")]
            ml: ml_classifier::MlClassifier::new(),
        }
    }

    /// Classify and filter a DOM tree in-place. Returns filter statistics.
    pub fn filter(&self, tree: &mut DomTree) -> FilterStats {
        let mut stats = FilterStats {
            total_nodes: 0,
            content_nodes: 0,
            ad_nodes: 0,
            tracker_nodes: 0,
            nav_nodes: 0,
            removed_nodes: 0,
        };

        #[cfg(feature = "ml-filter")]
        classify_recursive_ml(&self.ml, &mut tree.root, &mut stats);

        #[cfg(not(feature = "ml-filter"))]
        classify_recursive(&mut tree.root, &mut stats);

        prune_recursive(&mut tree.root);
        stats.removed_nodes = stats.ad_nodes + stats.tracker_nodes;
        stats
    }
}

impl Default for SemanticFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Recursively classify every node in the tree (rule-based fallback)
#[cfg(not(feature = "ml-filter"))]
fn classify_recursive(node: &mut DomNode, stats: &mut FilterStats) {
    stats.total_nodes += 1;

    node.classification = classify_node(node);

    match node.classification {
        Classification::Content => stats.content_nodes += 1,
        Classification::Advertisement => stats.ad_nodes += 1,
        Classification::Tracker => stats.tracker_nodes += 1,
        Classification::Navigation => stats.nav_nodes += 1,
        _ => {}
    }

    for child in &mut node.children {
        classify_recursive(child, stats);
    }
}

/// Recursively classify using ALICE-ML ternary inference
#[cfg(feature = "ml-filter")]
fn classify_recursive_ml(
    ml: &ml_classifier::MlClassifier,
    node: &mut DomNode,
    stats: &mut FilterStats,
) {
    stats.total_nodes += 1;

    node.classification = ml.classify(node);

    match node.classification {
        Classification::Content => stats.content_nodes += 1,
        Classification::Advertisement => stats.ad_nodes += 1,
        Classification::Tracker => stats.tracker_nodes += 1,
        Classification::Navigation => stats.nav_nodes += 1,
        _ => {}
    }

    for child in &mut node.children {
        classify_recursive_ml(ml, child, stats);
    }
}

/// Remove ad and tracker subtrees
fn prune_recursive(node: &mut DomNode) {
    node.children.retain(|c| {
        c.classification != Classification::Advertisement
            && c.classification != Classification::Tracker
    });

    for child in &mut node.children {
        prune_recursive(child);
    }
}

/// Classify a single DOM node using heuristics (rule-based fallback)
#[cfg(not(feature = "ml-filter"))]
fn classify_node(node: &DomNode) -> Classification {
    // Text nodes are always content
    if node.node_type == NodeType::Text {
        return Classification::Content;
    }

    // --- Tag-based classification ---
    match node.tag.as_str() {
        "script" | "noscript" => return Classification::Tracker,
        "style" => return Classification::Decoration,
        "nav" => return Classification::Navigation,
        "header" | "footer" => return Classification::Structural,
        "button" | "input" | "textarea" | "select" | "form" => {
            return Classification::Interactive;
        }
        "img" | "video" | "audio" | "picture" | "canvas" => {
            return Classification::Media;
        }
        "iframe" => {
            if let Some(src) = node.attr("src") {
                if is_ad_url(src) {
                    return Classification::Advertisement;
                }
            }
            return Classification::Media;
        }
        _ => {}
    }

    // --- Class/ID pattern matching ---
    let class = node.attr("class").unwrap_or("");
    let id = node.attr("id").unwrap_or("");
    let combined = format!("{} {}", class, id).to_lowercase();

    for pattern in AD_PATTERNS {
        if combined.contains(pattern) {
            return Classification::Advertisement;
        }
    }

    for pattern in TRACKER_PATTERNS {
        if combined.contains(pattern) {
            return Classification::Tracker;
        }
    }

    // --- Data attributes that indicate ads ---
    if node
        .attributes
        .keys()
        .any(|k| k.starts_with("data-ad") || k.starts_with("data-tracking"))
    {
        return Classification::Advertisement;
    }

    // --- Content density heuristics ---
    let link_density = node.link_density();
    if link_density > 0.6 && node.children.len() > 3 {
        return Classification::Navigation;
    }

    let text_density = node.text_density();
    if text_density > 10.0 {
        return Classification::Content;
    }

    Classification::Unknown
}

#[cfg(not(feature = "ml-filter"))]
fn is_ad_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    AD_DOMAINS.iter().any(|d| lower.contains(d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::parser::parse_html;

    #[test]
    fn filters_ad_divs() {
        let html = r#"
        <html><body>
            <div class="ad-banner">Buy stuff!</div>
            <div class="content">Real content here</div>
        </body></html>
        "#;

        let mut tree = parse_html(html, "https://example.com");
        let filter = SemanticFilter::new();
        let stats = filter.filter(&mut tree);

        assert!(stats.ad_nodes > 0);
        let text = tree.root.collect_text();
        assert!(text.contains("Real content"));
        assert!(!text.contains("Buy stuff"));
    }

    #[test]
    #[cfg(feature = "ml-filter")]
    fn ml_classifier_detects_ads() {
        let html = r#"
        <html><body>
            <div class="ad-banner">Buy stuff!</div>
            <p>Real content here with enough text to be classified</p>
        </body></html>
        "#;

        let mut tree = parse_html(html, "https://example.com");
        let filter = SemanticFilter::new();
        let stats = filter.filter(&mut tree);

        assert!(stats.ad_nodes > 0, "ML classifier should detect ad nodes");
        let text = tree.root.collect_text();
        assert!(!text.contains("Buy stuff"), "Ad content should be pruned");
    }

    #[test]
    fn filters_tracker_scripts() {
        let html = r#"
        <html><body>
            <p>Content</p>
            <script src="https://tracker.example.com/track.js"></script>
            <div class="tracking-pixel"></div>
        </body></html>
        "#;

        let mut tree = parse_html(html, "https://example.com");
        let filter = SemanticFilter::new();
        let stats = filter.filter(&mut tree);

        assert!(stats.tracker_nodes > 0);
    }
}
