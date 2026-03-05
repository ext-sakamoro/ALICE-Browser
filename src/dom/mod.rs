pub mod css;
pub mod filter;
pub mod parser;
pub mod readability;

use std::collections::HashMap;

/// DOM node classification for semantic filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Classification {
    /// Main article/page content
    Content,
    /// Navigation links, menus
    Navigation,
    /// Advertisements
    Advertisement,
    /// Tracking scripts, analytics
    Tracker,
    /// Decorative elements (borders, spacers)
    Decoration,
    /// Interactive elements (buttons, forms)
    Interactive,
    /// Images, video, audio
    Media,
    /// Headers, footers
    Structural,
    /// Not yet classified
    Unknown,
}

impl Classification {
    /// Convert from output neuron index to Classification
    #[must_use] 
    pub fn from_index(idx: usize) -> Self {
        match idx {
            0 => Classification::Content,
            1 => Classification::Navigation,
            2 => Classification::Advertisement,
            3 => Classification::Tracker,
            4 => Classification::Decoration,
            5 => Classification::Interactive,
            6 => Classification::Media,
            7 => Classification::Structural,
            _ => Classification::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Document,
    Element,
    Text,
}

/// Internal DOM node representation.
/// Unlike browser DOMs, each node carries a semantic classification.
#[derive(Debug, Clone)]
pub struct DomNode {
    pub tag: String,
    pub attributes: HashMap<String, String>,
    pub text: String,
    pub children: Vec<DomNode>,
    pub node_type: NodeType,
    pub classification: Classification,
}

impl DomNode {
    #[must_use] 
    pub fn document(children: Vec<DomNode>) -> Self {
        Self {
            tag: "#document".into(),
            attributes: HashMap::new(),
            text: String::new(),
            children,
            node_type: NodeType::Document,
            classification: Classification::Unknown,
        }
    }

    pub fn element(
        tag: impl Into<String>,
        attrs: HashMap<String, String>,
        children: Vec<DomNode>,
    ) -> Self {
        Self {
            tag: tag.into(),
            attributes: attrs,
            text: String::new(),
            children,
            node_type: NodeType::Element,
            classification: Classification::Unknown,
        }
    }

    pub fn text(content: impl Into<String>) -> Self {
        Self {
            tag: String::new(),
            attributes: HashMap::new(),
            text: content.into(),
            children: Vec::new(),
            node_type: NodeType::Text,
            classification: Classification::Content,
        }
    }

    /// Recursively count all nodes in this subtree
    #[must_use] 
    pub fn node_count(&self) -> usize {
        1 + self.children.iter().map(DomNode::node_count).sum::<usize>()
    }

    /// Collect all text content recursively
    #[must_use] 
    pub fn collect_text(&self) -> String {
        let mut buf = String::new();
        self.collect_text_inner(&mut buf);
        buf
    }

    fn collect_text_inner(&self, buf: &mut String) {
        if !self.text.is_empty() {
            if !buf.is_empty() {
                buf.push(' ');
            }
            buf.push_str(self.text.trim());
        }
        for child in &self.children {
            child.collect_text_inner(buf);
        }
    }

    /// Text-to-markup density (higher = more content-rich)
    #[must_use] 
    pub fn text_density(&self) -> f32 {
        let text_len = self.collect_text().len() as f32;
        let total_nodes = self.node_count() as f32;
        if total_nodes == 0.0 {
            0.0
        } else {
            text_len / total_nodes
        }
    }

    /// Link density (ratio of link text to total text)
    #[must_use] 
    pub fn link_density(&self) -> f32 {
        let total_text = self.collect_text().len() as f32;
        if total_text == 0.0 {
            return 0.0;
        }
        let link_text: usize = self
            .children
            .iter()
            .filter(|c| c.tag == "a")
            .map(|c| c.collect_text().len())
            .sum();
        link_text as f32 / total_text
    }

    #[must_use] 
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(std::string::String::as_str)
    }

    /// Whether this node should be rendered (not an ad/tracker)
    #[must_use] 
    pub fn is_visible(&self) -> bool {
        self.classification != Classification::Advertisement
            && self.classification != Classification::Tracker
    }
}

/// Parsed DOM tree with metadata
#[derive(Debug, Clone)]
pub struct DomTree {
    pub root: DomNode,
    pub url: String,
    pub title: String,
}

impl DomTree {
    /// Count nodes by classification
    #[must_use] 
    pub fn classification_stats(&self) -> HashMap<Classification, usize> {
        let mut stats = HashMap::new();
        count_classifications(&self.root, &mut stats);
        stats
    }
}

fn count_classifications(node: &DomNode, stats: &mut HashMap<Classification, usize>) {
    *stats.entry(node.classification).or_insert(0) += 1;
    for child in &node.children {
        count_classifications(child, stats);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classification_from_index() {
        assert_eq!(Classification::from_index(0), Classification::Content);
        assert_eq!(Classification::from_index(1), Classification::Navigation);
        assert_eq!(Classification::from_index(2), Classification::Advertisement);
        assert_eq!(Classification::from_index(3), Classification::Tracker);
        assert_eq!(Classification::from_index(4), Classification::Decoration);
        assert_eq!(Classification::from_index(5), Classification::Interactive);
        assert_eq!(Classification::from_index(6), Classification::Media);
        assert_eq!(Classification::from_index(7), Classification::Structural);
        assert_eq!(Classification::from_index(8), Classification::Unknown);
        assert_eq!(Classification::from_index(99), Classification::Unknown);
    }

    #[test]
    fn test_dom_node_text() {
        let node = DomNode::text("Hello world");
        assert_eq!(node.text, "Hello world");
        assert_eq!(node.node_type, NodeType::Text);
        assert_eq!(node.classification, Classification::Content);
        assert_eq!(node.node_count(), 1);
    }

    #[test]
    fn test_dom_node_element() {
        let child = DomNode::text("child text");
        let parent = DomNode::element("div", HashMap::new(), vec![child]);
        assert_eq!(parent.tag, "div");
        assert_eq!(parent.node_type, NodeType::Element);
        assert_eq!(parent.node_count(), 2);
    }

    #[test]
    fn test_dom_node_document() {
        let text = DomNode::text("content");
        let div = DomNode::element("div", HashMap::new(), vec![text]);
        let doc = DomNode::document(vec![div]);
        assert_eq!(doc.tag, "#document");
        assert_eq!(doc.node_type, NodeType::Document);
        assert_eq!(doc.node_count(), 3);
    }

    #[test]
    fn test_collect_text() {
        let t1 = DomNode::text("Hello");
        let t2 = DomNode::text("World");
        let div = DomNode::element("div", HashMap::new(), vec![t1, t2]);
        assert_eq!(div.collect_text(), "Hello World");
    }

    #[test]
    fn test_text_density() {
        let t = DomNode::text("Some text content here");
        let div = DomNode::element("div", HashMap::new(), vec![t]);
        let density = div.text_density();
        // text_len / node_count = 22 / 2 = 11.0
        assert!(density > 0.0);
    }

    #[test]
    fn test_link_density() {
        let link_text = DomNode::text("click here");
        let mut attrs = HashMap::new();
        attrs.insert("href".to_string(), "https://example.com".to_string());
        let link = DomNode::element("a", attrs, vec![link_text]);
        let other = DomNode::text("some normal text");
        let div = DomNode::element("div", HashMap::new(), vec![link, other]);
        let ld = div.link_density();
        assert!(ld > 0.0);
        assert!(ld <= 1.0);
    }

    #[test]
    fn test_link_density_no_text() {
        let div = DomNode::element("div", HashMap::new(), vec![]);
        assert_eq!(div.link_density(), 0.0);
    }

    #[test]
    fn test_attr() {
        let mut attrs = HashMap::new();
        attrs.insert("class".to_string(), "main-content".to_string());
        attrs.insert("id".to_string(), "article".to_string());
        let node = DomNode::element("div", attrs, vec![]);
        assert_eq!(node.attr("class"), Some("main-content"));
        assert_eq!(node.attr("id"), Some("article"));
        assert_eq!(node.attr("nonexistent"), None);
    }

    #[test]
    fn test_is_visible() {
        let mut content_node = DomNode::text("visible");
        content_node.classification = Classification::Content;
        assert!(content_node.is_visible());

        let mut ad_node = DomNode::text("ad");
        ad_node.classification = Classification::Advertisement;
        assert!(!ad_node.is_visible());

        let mut tracker_node = DomNode::text("tracker");
        tracker_node.classification = Classification::Tracker;
        assert!(!tracker_node.is_visible());

        let mut nav_node = DomNode::text("nav");
        nav_node.classification = Classification::Navigation;
        assert!(nav_node.is_visible());
    }

    #[test]
    fn test_classification_stats() {
        let mut c1 = DomNode::text("content");
        c1.classification = Classification::Content;
        let mut c2 = DomNode::text("more content");
        c2.classification = Classification::Content;
        let mut ad = DomNode::text("ad");
        ad.classification = Classification::Advertisement;
        let root = DomNode::document(vec![c1, c2, ad]);

        let tree = DomTree {
            root,
            url: "https://example.com".into(),
            title: "Test".into(),
        };
        let stats = tree.classification_stats();
        assert_eq!(*stats.get(&Classification::Content).unwrap_or(&0), 2);
        assert_eq!(*stats.get(&Classification::Advertisement).unwrap_or(&0), 1);
        // The document node itself is Unknown
        assert_eq!(*stats.get(&Classification::Unknown).unwrap_or(&0), 1);
    }
}
