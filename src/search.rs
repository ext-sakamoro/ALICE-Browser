//! ALICE-Search powered page search.
//!
//! Builds an FM-Index from page text for O(pattern_length) search,
//! independent of page size. Count, locate, and contains operations
//! are all sublinear.

use alice_search::AliceIndex;

/// FM-Index based page search.
///
/// Built once per page load, supports instant pattern matching
/// regardless of page size.
pub struct PageSearch {
    index: AliceIndex,
    text: String,
}

impl PageSearch {
    /// Build an FM-Index from the page's full text content.
    ///
    /// SA sampling step of 4 provides a good balance between
    /// index size and locate performance.
    pub fn build(text: &str) -> Self {
        let lower = text.to_lowercase();
        let index = AliceIndex::build(lower.as_bytes(), 4);
        Self {
            index,
            text: lower,
        }
    }

    /// Count occurrences of query in the page text. O(query_length).
    pub fn count(&self, query: &str) -> usize {
        if query.is_empty() {
            return 0;
        }
        self.index.count(query.to_lowercase().as_bytes())
    }

    /// Check if query exists in the page text. O(query_length).
    pub fn contains(&self, query: &str) -> bool {
        if query.is_empty() {
            return false;
        }
        self.index.contains(query.to_lowercase().as_bytes())
    }

    /// Total indexed text length in bytes.
    pub fn text_len(&self) -> usize {
        self.text.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_basic() {
        let search = PageSearch::build("Hello world, hello ALICE browser");
        assert_eq!(search.count("hello"), 2); // case-insensitive
        assert_eq!(search.count("alice"), 1);
        assert_eq!(search.count("xyz"), 0);
        assert!(search.contains("browser"));
        assert!(!search.contains("firefox"));
    }

    #[test]
    fn search_empty_query() {
        let search = PageSearch::build("Some text");
        assert_eq!(search.count(""), 0);
        assert!(!search.contains(""));
    }

    #[test]
    fn search_japanese() {
        let search = PageSearch::build("東京都渋谷区で開催されるイベント");
        assert_eq!(search.count("渋谷"), 1);
        assert!(search.contains("東京"));
    }
}
