pub mod parser;
pub mod filter;
pub mod readability;
pub mod css;

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
    pub fn node_count(&self) -> usize {
        1 + self.children.iter().map(|c| c.node_count()).sum::<usize>()
    }

    /// Collect all text content recursively
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

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(|s| s.as_str())
    }

    /// Whether this node should be rendered (not an ad/tracker)
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
