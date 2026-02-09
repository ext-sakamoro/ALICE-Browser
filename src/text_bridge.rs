//! ALICE-Text bridge: Compress extracted DOM text
//!
//! Provides content-aware text extraction from the DOM tree,
//! filtering out advertisements, trackers, and navigation,
//! then compresses the result using ALICE-Text's pattern-based codec.
//!
//! # Example
//!
//! ```ignore
//! use alice_browser::text_bridge::*;
//! use alice_browser::dom::parser::parse_html;
//!
//! let tree = parse_html(html, url);
//! let result = compress_page_text(&tree)?;
//! println!("Compressed {} bytes → {} bytes (ratio: {:.1}x)",
//!     result.original_text_len,
//!     result.compressed.len(),
//!     result.stats.compression_ratio());
//! ```

use alice_text::{ALICEText, CompressionStats, EncodingMode};

use crate::dom::{Classification, DomNode, DomTree};

/// Result of compressing a page's text content
#[derive(Debug)]
pub struct CompressedPageText {
    /// ALICE-Text compressed bytes
    pub compressed: Vec<u8>,
    /// Compression statistics
    pub stats: CompressionStats,
    /// Length of content-only text (excluding ads/trackers/nav)
    pub content_text_len: usize,
    /// Length of all extracted text
    pub original_text_len: usize,
}

/// Extract content-classified text from the DOM tree and compress it.
///
/// Only text from nodes classified as `Content` is included.
/// Ads, trackers, navigation, and decorative elements are excluded.
pub fn compress_page_text(tree: &DomTree) -> Result<CompressedPageText, String> {
    let content_text = extract_content_text(&tree.root);
    let all_text = extract_all_text(&tree.root);
    let content_text_len = content_text.len();
    let original_text_len = all_text.len();

    if content_text.is_empty() {
        return Err("No content text extracted from DOM".into());
    }

    let mut encoder = ALICEText::new(EncodingMode::Pattern);
    let compressed = encoder
        .compress(&content_text)
        .map_err(|e| format!("Compression failed: {}", e))?;

    let stats = encoder
        .last_stats()
        .cloned()
        .ok_or_else(|| "No compression stats available".to_string())?;

    Ok(CompressedPageText {
        compressed,
        stats,
        content_text_len,
        original_text_len,
    })
}

/// Compress all visible text from the DOM tree.
///
/// Includes all text from visible nodes (content, navigation, etc.)
/// but still excludes scripts, styles, and hidden elements.
pub fn compress_all_text(tree: &DomTree) -> Result<CompressedPageText, String> {
    let all_text = extract_all_text(&tree.root);
    let content_text = extract_content_text(&tree.root);
    let original_text_len = all_text.len();
    let content_text_len = content_text.len();

    if all_text.is_empty() {
        return Err("No visible text in DOM".into());
    }

    let mut encoder = ALICEText::new(EncodingMode::Pattern);
    let compressed = encoder
        .compress(&all_text)
        .map_err(|e| format!("Compression failed: {}", e))?;

    let stats = encoder
        .last_stats()
        .cloned()
        .ok_or_else(|| "No compression stats available".to_string())?;

    Ok(CompressedPageText {
        compressed,
        stats,
        content_text_len,
        original_text_len,
    })
}

/// Extract text only from content-classified nodes.
///
/// Recursively walks the DOM, collecting text from nodes where
/// `classification == Content`.
pub fn extract_content_text(node: &DomNode) -> String {
    let mut buf = String::new();
    collect_text_by_class(node, &mut buf, &[Classification::Content]);
    buf
}

/// Extract all visible text from the DOM.
///
/// Excludes `Advertisement`, `Tracker`, script, and style nodes.
pub fn extract_all_text(node: &DomNode) -> String {
    let mut buf = String::new();
    collect_visible_text(node, &mut buf);
    buf
}

fn collect_text_by_class(node: &DomNode, buf: &mut String, classes: &[Classification]) {
    if classes.contains(&node.classification) && !node.text.is_empty() {
        if !buf.is_empty() {
            buf.push(' ');
        }
        buf.push_str(&node.text);
    }
    for child in &node.children {
        collect_text_by_class(child, buf, classes);
    }
}

fn collect_visible_text(node: &DomNode, buf: &mut String) {
    match node.classification {
        Classification::Advertisement | Classification::Tracker => return,
        _ => {}
    }
    // Skip script/style tags
    if node.tag == "script" || node.tag == "style" {
        return;
    }
    if !node.text.is_empty() {
        if !buf.is_empty() {
            buf.push(' ');
        }
        buf.push_str(&node.text);
    }
    for child in &node.children {
        collect_visible_text(child, buf);
    }
}

/// Decompress previously compressed page text.
pub fn decompress_page_text(data: &[u8]) -> Result<String, String> {
    let decoder = ALICEText::new(EncodingMode::Pattern);
    decoder
        .decompress(data)
        .map_err(|e| format!("Decompression failed: {}", e))
}
