//! ALICE-Browser × ALICE-Analytics bridge
//!
//! Browser telemetry: page load metrics, resource timing, and user engagement.
//!
//! Author: Moroya Sakamoto

use alice_analytics::sketch::{CountMinSketch, DDSketch, HyperLogLog};

/// Relative accuracy of the load-time quantiles (1 %)
const LOAD_TIME_ALPHA: f64 = 0.01;

/// Browser telemetry collector
pub struct BrowserMetrics {
    load_times: DDSketch,
    unique_urls: HyperLogLog,
    resource_frequency: CountMinSketch,
    pub page_loads: u64,
    pub total_bytes: u64,
}

impl Default for BrowserMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserMetrics {
    pub fn new() -> Self {
        Self {
            load_times: DDSketch::new(LOAD_TIME_ALPHA),
            unique_urls: HyperLogLog::new(),
            resource_frequency: CountMinSketch::new(),
            page_loads: 0,
            total_bytes: 0,
        }
    }

    /// Record a page load event
    pub fn record_page_load(&mut self, url_hash: u64, load_time_ms: f64, bytes: u64) {
        self.load_times.insert(load_time_ms);
        self.unique_urls.insert_hash(url_hash);
        self.resource_frequency.insert_hash(url_hash, 1);
        self.page_loads += 1;
        self.total_bytes += bytes;
    }

    /// Median page load time
    pub fn median_load_ms(&self) -> f64 {
        self.load_times.quantile(0.5)
    }

    /// P95 page load time
    pub fn p95_load_ms(&self) -> f64 {
        self.load_times.quantile(0.95)
    }

    /// Estimated unique URLs visited
    pub fn unique_url_count(&self) -> u64 {
        // the estimate is a non-negative double well inside u64
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let n = self.unique_urls.cardinality().round() as u64;
        n
    }

    /// Average page size in bytes
    pub fn avg_page_bytes(&self) -> f64 {
        if self.page_loads == 0 {
            return 0.0;
        }
        self.total_bytes as f64 / self.page_loads as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_page_load() {
        let mut m = BrowserMetrics::new();
        m.record_page_load(0xABCD, 150.0, 50_000);
        m.record_page_load(0xEF01, 200.0, 80_000);
        assert_eq!(m.page_loads, 2);
        assert_eq!(m.total_bytes, 130_000);
    }

    #[test]
    fn test_avg_page_bytes() {
        let mut m = BrowserMetrics::new();
        m.record_page_load(1, 100.0, 40_000);
        m.record_page_load(2, 200.0, 60_000);
        assert!((m.avg_page_bytes() - 50_000.0).abs() < 1.0);
    }

    // oracle: the median and P95 of the load times 1..=100 ms are 50.5 and 95.05 ms by
    // definition (linear interpolation), the sketch answers within its 1 % relative accuracy
    #[test]
    fn quantiles_of_known_load_times() {
        let mut m = BrowserMetrics::new();
        for t in 1..=100u64 {
            m.record_page_load(t, t as f64, 1);
        }
        assert!(
            (m.median_load_ms() - 50.5).abs() <= 50.5 * 0.03,
            "{}",
            m.median_load_ms()
        );
        assert!(
            (m.p95_load_ms() - 95.05).abs() <= 95.05 * 0.03,
            "{}",
            m.p95_load_ms()
        );
    }

    // oracle: 10 000 distinct hashes, each recorded twice; HyperLogLog with 2^14 registers has a
    // standard error of 1.04 / 128 = 0.81 %, so 4 standard errors is 3.3 %
    #[test]
    fn unique_urls_of_known_count() {
        let mut m = BrowserMetrics::new();
        for i in 0..10_000u64 {
            let h = alice_analytics::sketch::FnvHasher::hash_bytes(&i.to_le_bytes());
            m.record_page_load(h, 10.0, 1);
            m.record_page_load(h, 10.0, 1);
        }
        let n = m.unique_url_count() as f64;
        assert!((n - 10_000.0).abs() <= 330.0, "{n}");
    }

    #[test]
    fn test_empty_metrics() {
        let m = BrowserMetrics::new();
        assert_eq!(m.page_loads, 0);
        assert_eq!(m.avg_page_bytes(), 0.0);
    }
}
