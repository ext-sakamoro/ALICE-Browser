//! ALICE-Analytics powered browser telemetry.
//!
//! Tracks browsing performance using probabilistic data structures:
//! - **DDSketch**: Page load latency quantiles (P50, P99)
//! - **HyperLogLog**: Unique domains visited
//! - **Counters**: Pages loaded, ads/trackers blocked
//!
//! All structures have fixed memory footprint with mathematical error guarantees.

use alice_analytics::prelude::*;

fn h(name: &str) -> u64 {
    FnvHasher::hash_bytes(name.as_bytes())
}

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
/// Fixed memory: ~32KB total (pipeline slots + sketches).
/// Uses 128 slots to minimize hash collisions across 5 metric names.
pub struct BrowserMetrics {
    pipeline: MetricPipeline<128, 256>,
}

impl BrowserMetrics {
    pub fn new() -> Self {
        Self {
            pipeline: MetricPipeline::new(0.05),
        }
    }

    /// Record a completed page load.
    pub fn record_page_load(&mut self, load_time_ms: f64, url: &str) {
        // Page load counter
        self.pipeline
            .submit(MetricEvent::counter(h("page_loads"), 1.0));
        // Load time histogram
        self.pipeline
            .submit(MetricEvent::histogram(h("load_time"), load_time_ms));
        // Unique domain tracking
        if let Ok(parsed) = url::Url::parse(url) {
            if let Some(domain) = parsed.domain() {
                let domain_hash = FnvHasher::hash_bytes(domain.as_bytes());
                self.pipeline
                    .submit(MetricEvent::unique(h("unique_domains"), domain_hash));
            }
        }
        self.pipeline.flush();
    }

    /// Record DOM filter statistics.
    pub fn record_dom_stats(&mut self, total_nodes: usize, blocked_nodes: usize) {
        self.pipeline
            .submit(MetricEvent::histogram(h("dom_nodes"), total_nodes as f64));
        self.pipeline.submit(MetricEvent::counter(
            h("blocked_total"),
            blocked_nodes as f64,
        ));
        self.pipeline.flush();
    }

    /// Get a snapshot of current metrics for display.
    pub fn snapshot(&self) -> MetricsSnapshot {
        let page_loads = self
            .pipeline
            .get_slot(h("page_loads"))
            .map(|s| s.counter as u64)
            .unwrap_or(0);

        let (p50, p99) = self
            .pipeline
            .get_slot(h("load_time"))
            .map(|s| (s.ddsketch.quantile(0.50), s.ddsketch.quantile(0.99)))
            .unwrap_or((0.0, 0.0));

        let unique_domains = self
            .pipeline
            .get_slot(h("unique_domains"))
            .map(|s| s.hll.cardinality())
            .unwrap_or(0.0);

        let total_blocked = self
            .pipeline
            .get_slot(h("blocked_total"))
            .map(|s| s.counter as u64)
            .unwrap_or(0);

        let total_dom_nodes = self
            .pipeline
            .get_slot(h("dom_nodes"))
            .map(|s| s.ddsketch.count() as u64)
            .unwrap_or(0);

        MetricsSnapshot {
            page_loads,
            p50_load_ms: p50,
            p99_load_ms: p99,
            unique_domains,
            total_blocked,
            total_dom_nodes,
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
        assert!(snap.unique_domains >= 1.0); // at least 1 domain
        assert_eq!(snap.total_blocked, 40);
        assert_eq!(snap.total_dom_nodes, 2); // 2 dom_stats recorded
    }
}
