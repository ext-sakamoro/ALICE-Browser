//! ALICE-Cache bridge: DOM classification caching
//!
//! Caches DOM node classification results (content, ad, tracker, etc.)
//! using ALICE-Cache's predictive caching to avoid re-classification
//! of previously seen nodes.
//!
//! # Pipeline
//!
//! ```text
//! URL + DOM hash → AliceCache lookup → hit: return cached class
//!                                     → miss: classify → store → return
//! ```

use alice_cache::AliceCache;

/// Classification result for a DOM node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DomClass {
    /// Main content
    Content = 0,
    /// Navigation element
    Navigation = 1,
    /// Advertisement
    Ad = 2,
    /// Tracker / analytics script
    Tracker = 3,
    /// Widget / social embed
    Widget = 4,
    /// Unknown / unclassified
    Unknown = 255,
}

impl DomClass {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Content,
            1 => Self::Navigation,
            2 => Self::Ad,
            3 => Self::Tracker,
            4 => Self::Widget,
            _ => Self::Unknown,
        }
    }
}

/// Cache for DOM classification results.
///
/// Uses ALICE-Cache's shard-based, predictive architecture for
/// high-throughput concurrent lookups.
pub struct DomClassificationCache {
    cache: AliceCache<u64, u8>,
}

impl DomClassificationCache {
    /// Create a new classification cache with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: AliceCache::new(capacity),
        }
    }

    /// Look up a cached classification for a DOM node hash.
    pub fn get(&self, node_hash: u64) -> Option<DomClass> {
        self.cache.get(&node_hash).map(DomClass::from_u8)
    }

    /// Store a classification result.
    pub fn put(&self, node_hash: u64, class: DomClass) {
        self.cache.put(node_hash, class as u8);
    }

    /// Get the cache hit rate (0.0 - 1.0).
    pub fn hit_rate(&self) -> f64 {
        self.cache.hit_rate()
    }
}

/// Compute a hash for a DOM node based on tag, class, and URL.
///
/// Uses FNV-1a for speed.
pub fn dom_node_hash(tag: &str, class_attr: &str, url: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325; // FNV offset basis
    for b in tag
        .bytes()
        .chain(b":".iter().copied())
        .chain(class_attr.bytes())
        .chain(b"@".iter().copied())
        .chain(url.bytes())
    {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3); // FNV prime
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_put_get() {
        let cache = DomClassificationCache::new(100);
        let hash = dom_node_hash("div", "ad-banner", "https://example.com");

        cache.put(hash, DomClass::Ad);
        assert_eq!(cache.get(hash), Some(DomClass::Ad));
    }

    #[test]
    fn test_cache_miss() {
        let cache = DomClassificationCache::new(100);
        assert_eq!(cache.get(12345), None);
    }

    #[test]
    fn test_dom_node_hash_deterministic() {
        let h1 = dom_node_hash("div", "content", "https://example.com");
        let h2 = dom_node_hash("div", "content", "https://example.com");
        assert_eq!(h1, h2);

        let h3 = dom_node_hash("span", "content", "https://example.com");
        assert_ne!(h1, h3);
    }
}
