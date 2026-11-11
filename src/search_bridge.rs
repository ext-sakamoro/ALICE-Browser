//! ALICE-Browser × ALICE-Search bridge
//!
//! FM-Index over DOM text content for instant in-page search.
//!
//! Author: Moroya Sakamoto

use alice_search::AliceIndex;

/// Suffix-array sampling step of the index: every 4th position is kept (lower = faster locate, more memory)
const SAMPLE_STEP: usize = 4;

/// DOM content search index
pub struct DomSearchIndex {
    index: AliceIndex,
    text_len: usize,
    pub queries_count: u64,
}

impl DomSearchIndex {
    /// Build search index from extracted DOM text
    pub fn build(text: &str) -> Self {
        let text_len = text.len();
        let index = AliceIndex::build(text.as_bytes(), SAMPLE_STEP);
        Self {
            index,
            text_len,
            queries_count: 0,
        }
    }

    /// Count pattern occurrences (O(|pattern|))
    pub fn count(&mut self, pattern: &str) -> usize {
        self.queries_count += 1;
        self.index.count(pattern.as_bytes())
    }

    /// Locate all pattern occurrences with byte offsets
    /// Byte offsets of every occurrence, in increasing order
    pub fn locate(&mut self, pattern: &str) -> Vec<usize> {
        self.queries_count += 1;
        let mut offsets = self.index.locate_all(pattern.as_bytes());
        offsets.sort_unstable();
        offsets
    }

    /// Find with surrounding context window
    pub fn search_with_context(
        &mut self,
        pattern: &str,
        context_bytes: usize,
    ) -> Vec<(usize, usize, usize)> {
        self.queries_count += 1;
        let mut offsets = self.index.locate_all(pattern.as_bytes());
        offsets.sort_unstable();
        offsets
            .into_iter()
            .map(|off| {
                let start = off.saturating_sub(context_bytes);
                let end = (off + pattern.len() + context_bytes).min(self.text_len);
                (off, start, end)
            })
            .collect()
    }

    pub fn text_len(&self) -> usize {
        self.text_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_and_count() {
        let mut idx = DomSearchIndex::build("the quick brown fox jumps over the lazy dog");
        assert_eq!(idx.count("the"), 2);
        assert_eq!(idx.count("fox"), 1);
        assert_eq!(idx.count("cat"), 0);
    }

    #[test]
    fn test_locate() {
        let mut idx = DomSearchIndex::build("abcabc");
        let positions = idx.locate("abc");
        assert_eq!(positions.len(), 2);
    }

    #[test]
    fn test_context_search() {
        let mut idx = DomSearchIndex::build("hello world test hello again");
        let hits = idx.search_with_context("hello", 3);
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn test_empty_text() {
        let mut idx = DomSearchIndex::build("");
        assert_eq!(idx.count("anything"), 0);
        assert_eq!(idx.text_len(), 0);
    }

    #[test]
    fn test_query_counter() {
        let mut idx = DomSearchIndex::build("test");
        idx.count("t");
        idx.locate("t");
        assert_eq!(idx.queries_count, 2);
    }
}
