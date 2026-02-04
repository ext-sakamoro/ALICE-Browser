/// ALICE Ad Blocker — EasyList-compatible filter engine.
///
/// Blocks ads and trackers at the URL level before requests are made.
/// Supports a subset of EasyList/AdBlock Plus filter syntax.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Block statistics, shared across threads.
#[derive(Debug, Clone)]
pub struct BlockStats {
    /// Ads blocked (page-level, reset per navigation)
    pub page_ads: Arc<AtomicUsize>,
    /// Trackers blocked (page-level)
    pub page_trackers: Arc<AtomicUsize>,
    /// Total ads blocked (lifetime)
    pub total_ads: Arc<AtomicUsize>,
    /// Total trackers blocked (lifetime)
    pub total_trackers: Arc<AtomicUsize>,
    /// Total requests checked
    pub total_checked: Arc<AtomicUsize>,
}

impl BlockStats {
    pub fn new() -> Self {
        Self {
            page_ads: Arc::new(AtomicUsize::new(0)),
            page_trackers: Arc::new(AtomicUsize::new(0)),
            total_ads: Arc::new(AtomicUsize::new(0)),
            total_trackers: Arc::new(AtomicUsize::new(0)),
            total_checked: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn reset_page(&self) {
        self.page_ads.store(0, Ordering::Relaxed);
        self.page_trackers.store(0, Ordering::Relaxed);
    }

    pub fn record_ad(&self) {
        self.page_ads.fetch_add(1, Ordering::Relaxed);
        self.total_ads.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_tracker(&self) {
        self.page_trackers.fetch_add(1, Ordering::Relaxed);
        self.total_trackers.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_check(&self) {
        self.total_checked.fetch_add(1, Ordering::Relaxed);
    }

    pub fn page_blocked(&self) -> usize {
        self.page_ads.load(Ordering::Relaxed) + self.page_trackers.load(Ordering::Relaxed)
    }

    pub fn total_blocked(&self) -> usize {
        self.total_ads.load(Ordering::Relaxed) + self.total_trackers.load(Ordering::Relaxed)
    }
}

/// Classification of why a URL was blocked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BlockReason {
    Ad,
    Tracker,
}

/// A single filter rule parsed from EasyList format.
#[derive(Debug, Clone)]
enum FilterRule {
    /// Domain-level block: ||example.com^
    DomainBlock(String),
    /// URL substring block: some-ad-path
    SubstringBlock(String),
    /// Exception (whitelist): @@||example.com^
    Exception(String),
}

/// The ad blocker engine.
pub struct AdBlockEngine {
    domain_blocks: Vec<String>,
    substring_blocks: Vec<String>,
    exceptions: Vec<String>,
    pub stats: BlockStats,
}

impl AdBlockEngine {
    /// Create a new engine with built-in rules.
    pub fn new() -> Self {
        let mut engine = Self {
            domain_blocks: Vec::new(),
            substring_blocks: Vec::new(),
            exceptions: Vec::new(),
            stats: BlockStats::new(),
        };
        engine.load_builtin_rules();
        engine
    }

    /// Load EasyList-format rules from a string.
    pub fn load_rules(&mut self, rules_text: &str) {
        for line in rules_text.lines() {
            let line = line.trim();
            // Skip comments and empty lines
            if line.is_empty() || line.starts_with('!') || line.starts_with('[') {
                continue;
            }
            if let Some(rule) = Self::parse_rule(line) {
                match rule {
                    FilterRule::DomainBlock(d) => self.domain_blocks.push(d),
                    FilterRule::SubstringBlock(s) => self.substring_blocks.push(s),
                    FilterRule::Exception(e) => self.exceptions.push(e),
                }
            }
        }
    }

    fn parse_rule(line: &str) -> Option<FilterRule> {
        // Exception rules: @@||domain^
        if line.starts_with("@@") {
            let rest = line.trim_start_matches("@@").trim_start_matches("||");
            let domain = rest.trim_end_matches('^').trim_end_matches('*');
            if !domain.is_empty() {
                return Some(FilterRule::Exception(domain.to_lowercase()));
            }
            return None;
        }

        // Domain rules: ||domain.com^
        if line.starts_with("||") {
            let rest = &line[2..];
            let domain = rest.split('^').next().unwrap_or(rest);
            let domain = domain.split('$').next().unwrap_or(domain);
            if !domain.is_empty() {
                return Some(FilterRule::DomainBlock(domain.to_lowercase()));
            }
            return None;
        }

        // Cosmetic filters (##, #@#, #?#) — skip, we handle these at DOM level
        if line.contains("##") || line.contains("#@#") || line.contains("#?#") {
            return None;
        }

        // URL substring rules
        let cleaned = line.split('$').next().unwrap_or(line);
        let cleaned = cleaned.trim_matches('*').trim_matches('|');
        if cleaned.len() >= 4 {
            return Some(FilterRule::SubstringBlock(cleaned.to_lowercase()));
        }

        None
    }

    /// Check if a URL should be blocked.
    pub fn should_block(&self, url: &str) -> Option<BlockReason> {
        self.stats.record_check();

        let url_lower = url.to_lowercase();

        // Check exceptions first
        for exc in &self.exceptions {
            if url_lower.contains(exc) {
                return None;
            }
        }

        // Extract domain from URL
        let domain = extract_domain(&url_lower);

        // Check domain blocks
        for blocked_domain in &self.domain_blocks {
            if domain == *blocked_domain || domain.ends_with(&format!(".{}", blocked_domain)) {
                let reason = classify_block_reason(blocked_domain);
                match reason {
                    BlockReason::Ad => self.stats.record_ad(),
                    BlockReason::Tracker => self.stats.record_tracker(),
                }
                return Some(reason);
            }
        }

        // Check substring blocks
        for pattern in &self.substring_blocks {
            if url_lower.contains(pattern) {
                let reason = classify_block_reason(pattern);
                match reason {
                    BlockReason::Ad => self.stats.record_ad(),
                    BlockReason::Tracker => self.stats.record_tracker(),
                }
                return Some(reason);
            }
        }

        None
    }

    /// Load built-in ad/tracker domain rules (most common).
    fn load_builtin_rules(&mut self) {
        // ── Major ad networks ──
        let ad_domains = [
            "doubleclick.net",
            "googlesyndication.com",
            "googleadservices.com",
            "google-analytics.com",
            "googletagmanager.com",
            "googletagservices.com",
            "pagead2.googlesyndication.com",
            "adservice.google.com",
            "ads.google.com",
            "adsense.google.com",
            "adnxs.com",
            "adsrvr.org",
            "advertising.com",
            "adform.net",
            "adroll.com",
            "outbrain.com",
            "taboola.com",
            "criteo.com",
            "criteo.net",
            "moatads.com",
            "media.net",
            "amazon-adsystem.com",
            "serving-sys.com",
            "bidswitch.net",
            "casalemedia.com",
            "demdex.net",
            "openx.net",
            "pubmatic.com",
            "rubiconproject.com",
            "smartadserver.com",
            "turn.com",
            "yieldmanager.com",
            "zedo.com",
            "ad.doubleclick.net",
            "a]d-delivery.net",
            "adcolony.com",
            "admob.com",
            "mopub.com",
            "unity3d.com/ads",
            "vungle.com",
            "applovin.com",
            "chartboost.com",
            "inmobi.com",
            "smaato.net",
            "tapjoy.com",
        ];

        // ── Trackers ──
        let tracker_domains = [
            "facebook.net",
            "facebook.com/tr",
            "connect.facebook.net",
            "pixel.facebook.com",
            "analytics.twitter.com",
            "t.co/i",
            "bat.bing.com",
            "hotjar.com",
            "mixpanel.com",
            "segment.io",
            "segment.com",
            "amplitude.com",
            "fullstory.com",
            "mouseflow.com",
            "luckyorange.com",
            "crazyegg.com",
            "optimizely.com",
            "newrelic.com",
            "nr-data.net",
            "sentry.io",
            "bugsnag.com",
            "rollbar.com",
            "quantserve.com",
            "scorecardresearch.com",
            "bluekai.com",
            "krxd.net",
            "exelator.com",
            "eyeota.net",
            "rlcdn.com",
            "tapad.com",
            "sharethrough.com",
            "mathtag.com",
            "adsymptotic.com",
            "doubleverify.com",
            "moat.com",
            "omtrdc.net",
            "everesttech.net",
            "agkn.com",
            "bounceexchange.com",
        ];

        // ── URL patterns (substring blocks) ──
        let ad_patterns = [
            "/ads/",
            "/ad/",
            "/adserver/",
            "/adx/",
            "/adsense/",
            "/admanager/",
            "/advert",
            "/banner/",
            "/banners/",
            "/popup/",
            "/popunder/",
            "/interstitial",
            "ads.js",
            "ad.js",
            "tracking.js",
            "tracker.js",
            "analytics.js",
            "pixel.gif",
            "pixel.png",
            "spacer.gif",
            "/beacon?",
            "/collect?",
            "/pageview?",
            "/__utm.gif",
            "/piwik.",
            "/matomo.",
        ];

        for d in &ad_domains {
            self.domain_blocks.push(d.to_string());
        }
        for d in &tracker_domains {
            self.domain_blocks.push(d.to_string());
        }
        for p in &ad_patterns {
            self.substring_blocks.push(p.to_string());
        }
    }

    /// Number of loaded rules.
    pub fn rule_count(&self) -> usize {
        self.domain_blocks.len() + self.substring_blocks.len() + self.exceptions.len()
    }
}

/// Extract domain from a URL string.
fn extract_domain(url: &str) -> String {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let domain = without_scheme.split('/').next().unwrap_or(without_scheme);
    let domain = domain.split(':').next().unwrap_or(domain);
    domain.to_string()
}

/// Classify whether a matched pattern is ad or tracker.
fn classify_block_reason(pattern: &str) -> BlockReason {
    let tracker_keywords = [
        "analytics", "tracker", "tracking", "pixel", "beacon",
        "collect", "pageview", "telemetry", "metrics", "sentry",
        "bugsnag", "rollbar", "hotjar", "mixpanel", "segment",
        "amplitude", "fullstory", "mouseflow", "crazyegg",
        "optimizely", "newrelic", "quantserve", "scorecard",
        "bluekai", "exelator", "facebook.net", "facebook.com/tr",
        "matomo", "piwik",
    ];

    for kw in &tracker_keywords {
        if pattern.contains(kw) {
            return BlockReason::Tracker;
        }
    }

    BlockReason::Ad
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_blocks() {
        let engine = AdBlockEngine::new();
        assert!(engine.should_block("https://doubleclick.net/ad.js").is_some());
        assert!(engine.should_block("https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js").is_some());
        assert!(engine.should_block("https://example.com/page").is_none());
    }

    #[test]
    fn test_tracker_detection() {
        let engine = AdBlockEngine::new();
        let reason = engine.should_block("https://google-analytics.com/collect?v=1");
        assert_eq!(reason, Some(BlockReason::Tracker));
    }

    #[test]
    fn test_ad_pattern() {
        let engine = AdBlockEngine::new();
        let reason = engine.should_block("https://example.com/ads/banner.js");
        assert_eq!(reason, Some(BlockReason::Ad));
    }

    #[test]
    fn test_stats() {
        let engine = AdBlockEngine::new();
        engine.should_block("https://doubleclick.net/ad.js");
        engine.should_block("https://google-analytics.com/collect");
        engine.should_block("https://example.com/page");
        assert!(engine.stats.total_blocked() >= 2);
        assert_eq!(engine.stats.total_checked.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_easylist_parse() {
        let mut engine = AdBlockEngine::new();
        let rules = r#"
! EasyList comment
[Adblock Plus]
||evil-ads.com^
||sneaky-tracker.net^$third-party
@@||allowed-ads.com^
/some-ad-path/
example.com##.ad-banner
"#;
        engine.load_rules(rules);
        assert!(engine.should_block("https://evil-ads.com/banner.js").is_some());
        assert!(engine.should_block("https://sub.evil-ads.com/x").is_some());
        assert!(engine.should_block("https://allowed-ads.com/ad.js").is_none());
    }

    #[test]
    fn test_extract_domain() {
        assert_eq!(extract_domain("https://www.example.com/path"), "www.example.com");
        assert_eq!(extract_domain("http://test.org:8080/x"), "test.org");
    }
}
