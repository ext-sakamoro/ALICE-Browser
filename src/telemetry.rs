//! ALICE-Analytics powered browser telemetry.
//!
//! Tracks browsing performance using probabilistic data structures:
//! - **DDSketch**: Page load latency quantiles (P50, P99)
//! - **HyperLogLog**: Unique domains visited
//! - **Counters**: Pages loaded, ads/trackers blocked
//!
//! All structures have fixed memory footprint with mathematical error guarantees.

use alice_analytics::sketch::{DDSketch, FnvHasher, HyperLogLog};

/// Relative accuracy of the load-time quantiles (5 %)
const LOAD_TIME_ALPHA: f64 = 0.05;

/// Browser performance metrics snapshot.
pub struct MetricsSnapshot {
    pub page_loads: u64,
    pub p50_load_ms: f64,
    pub p99_load_ms: f64,
    pub unique_domains: f64,
    pub total_blocked: u64,
    pub total_dom_nodes: u64,
}

/// Probabilistic browser telemetry using ALICE-Analytics.
///
/// One field per metric (no lookup by name, so two metrics can never share storage): a
/// `DDSketch` for load times, a `HyperLogLog` for domains, and exact counters.
pub struct BrowserMetrics {
    page_loads: u64,
    load_times: DDSketch,
    domains: HyperLogLog,
    total_blocked: u64,
    dom_stats_recorded: u64,
}

impl Default for BrowserMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl BrowserMetrics {
    #[must_use]
    pub fn new() -> Self {
        Self {
            page_loads: 0,
            load_times: DDSketch::new(LOAD_TIME_ALPHA),
            domains: HyperLogLog::new(),
            total_blocked: 0,
            dom_stats_recorded: 0,
        }
    }

    /// Record a completed page load.
    pub fn record_page_load(&mut self, load_time_ms: f64, url: &str) {
        self.page_loads += 1;
        self.load_times.insert(load_time_ms);
        if let Ok(parsed) = url::Url::parse(url) {
            if let Some(domain) = parsed.domain() {
                self.domains
                    .insert_hash(FnvHasher::hash_bytes(domain.as_bytes()));
            }
        }
    }

    /// Record DOM filter statistics.
    pub fn record_dom_stats(&mut self, _total_nodes: usize, blocked_nodes: usize) {
        self.dom_stats_recorded += 1;
        self.total_blocked += blocked_nodes as u64;
    }

    /// Get a snapshot of current metrics for display.
    #[must_use]
    pub fn snapshot(&self) -> MetricsSnapshot {
        let (p50, p99) = if self.load_times.count() == 0 {
            (0.0, 0.0)
        } else {
            (
                self.load_times.quantile(0.50),
                self.load_times.quantile(0.99),
            )
        };
        MetricsSnapshot {
            page_loads: self.page_loads,
            p50_load_ms: p50,
            p99_load_ms: p99,
            unique_domains: self.domains.cardinality(),
            total_blocked: self.total_blocked,
            total_dom_nodes: self.dom_stats_recorded,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_snapshot() {
        let mut metrics = BrowserMetrics::new();

        metrics.record_page_load(150.0, "https://example.com/page1");
        metrics.record_page_load(200.0, "https://example.com/page2");
        metrics.record_page_load(50.0, "https://other.org/test");
        metrics.record_dom_stats(500, 30);
        metrics.record_dom_stats(300, 10);

        let snap = metrics.snapshot();
        assert_eq!(snap.page_loads, 3);
        assert!(snap.p50_load_ms > 0.0);
        // example.com and other.org: 2 domains, HyperLogLog is exact this far below its register count
        assert_eq!(snap.unique_domains.round(), 2.0);
        // 50, 150, 200 ms: the median is 150 ms within the 5 % relative accuracy
        assert!(
            (snap.p50_load_ms - 150.0).abs() <= 150.0 * 0.05,
            "{}",
            snap.p50_load_ms
        );
        assert_eq!(snap.total_blocked, 40);
        assert_eq!(snap.total_dom_nodes, 2); // 2 dom_stats recorded
    }
}
