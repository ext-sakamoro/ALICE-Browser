//! ALICE-Browser × ALICE-Analytics bridge
//!
//! Browser telemetry: page load metrics, resource timing, and user engagement.
//!
//! Author: Moroya Sakamoto

use alice_analytics::{DDSketch, HyperLogLog, CountMinSketch};

/// Browser telemetry collector
pub struct BrowserMetrics {
    load_times: DDSketch,
    unique_urls: HyperLogLog,
    resource_frequency: CountMinSketch,
    pub page_loads: u64,
    pub total_bytes: u64,
}

impl BrowserMetrics {
    pub fn new() -> Self {
        Self {
            load_times: DDSketch::new(128),
            unique_urls: HyperLogLog::new(12),
            resource_frequency: CountMinSketch::new(1024, 4),
            page_loads: 0,
            total_bytes: 0,
        }
    }

    /// Record a page load event
    pub fn record_page_load(&mut self, url_hash: u64, load_time_ms: f64, bytes: u64) {
        self.load_times.add(load_time_ms);
        self.unique_urls.insert(url_hash);
        self.resource_frequency.increment(url_hash);
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
        self.unique_urls.count()
    }

    /// Average page size in bytes
    pub fn avg_page_bytes(&self) -> f64 {
        if self.page_loads == 0 { return 0.0; }
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

    #[test]
    fn test_empty_metrics() {
        let m = BrowserMetrics::new();
        assert_eq!(m.page_loads, 0);
        assert_eq!(m.avg_page_bytes(), 0.0);
    }
}
