//! ALICE-Cache powered page caching with Markov oracle prefetch prediction.
//!
//! Wraps `AliceCache` to cache fetched web pages. The Markov oracle learns
//! navigation patterns and predicts which pages to prefetch next.

use alice_cache::AliceCache;

use super::fetch::{fetch_url, FetchError, FetchResult};

/// Page cache with predictive prefetching.
///
/// Uses ALICE-Cache's sharded architecture for O(1) lookups and
/// Markov oracle for navigation pattern prediction.
pub struct CachedFetcher {
    cache: AliceCache<String, FetchResult>,
}

impl CachedFetcher {
    /// Create a new page cache with the given capacity (number of pages).
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: AliceCache::new(capacity),
        }
    }

    /// Fetch a URL, returning cached result on hit or fetching from network on miss.
    pub fn fetch(&self, url: &str) -> Result<FetchResult, FetchError> {
        let key = url.to_string();

        // Cache hit
        if let Some(cached) = self.cache.get(&key) {
            log::debug!("Cache HIT: {}", url);
            return Ok(cached);
        }

        // Cache miss — fetch from network
        log::debug!("Cache MISS: {}", url);
        let result = fetch_url(url)?;
        self.cache.put(key, result.clone());
        Ok(result)
    }

    /// Check if the oracle predicts navigation from current to candidate URL.
    pub fn should_prefetch(&self, current_url: &str, candidate_url: &str) -> bool {
        self.cache
            .should_prefetch(&current_url.to_string(), &candidate_url.to_string())
    }

    /// Number of cached pages.
    pub fn cached_pages(&self) -> usize {
        self.cache.len()
    }

    /// Cache hit rate (0.0 to 1.0).
    pub fn hit_rate(&self) -> f64 {
        self.cache.hit_rate()
    }
}
