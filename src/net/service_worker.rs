//! Service Worker キャッシュ同期 — オフラインファーストブラウジング
//!
//! キャッシュ戦略に基づいたリクエスト管理と、
//! オフライン時のリクエストキューイングを提供する。

use std::collections::HashMap;

/// キャッシュ戦略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStrategy {
    /// ネットワーク優先、失敗時にキャッシュ。
    NetworkFirst,
    /// キャッシュ優先、キャッシュミス時にネットワーク。
    CacheFirst,
    /// キャッシュを返しつつバックグラウンドで更新。
    StaleWhileRevalidate,
    /// ネットワークのみ (キャッシュしない)。
    NetworkOnly,
    /// キャッシュのみ (ネットワークアクセスしない)。
    CacheOnly,
}

/// キャッシュエントリー。
#[derive(Debug, Clone)]
pub struct CacheEntry {
    /// URL。
    pub url: String,
    /// レスポンスボディ。
    pub body: Vec<u8>,
    /// Content-Type。
    pub content_type: String,
    /// キャッシュ時刻 (秒、モノトニック)。
    pub cached_at: f64,
    /// 有効期限 (秒)。TTL = 0 は無期限。
    pub ttl: f64,
}

impl CacheEntry {
    /// 期限切れか。
    #[must_use]
    pub fn is_expired(&self, now: f64) -> bool {
        self.ttl > 0.0 && (now - self.cached_at) > self.ttl
    }

    /// レスポンスサイズ (バイト)。
    #[must_use]
    pub const fn size(&self) -> usize {
        self.body.len()
    }
}

/// キューイングされたリクエスト。
#[derive(Debug, Clone)]
pub struct QueuedRequest {
    /// URL。
    pub url: String,
    /// HTTP メソッド。
    pub method: String,
    /// リクエストボディ (POST 等)。
    pub body: Option<Vec<u8>>,
    /// キューイング時刻。
    pub queued_at: f64,
    /// リトライ回数。
    pub retry_count: u32,
}

/// Service Worker キャッシュ。
#[derive(Debug)]
pub struct SwCache {
    /// URL → キャッシュエントリー。
    entries: HashMap<String, CacheEntry>,
    /// URL パターン → キャッシュ戦略。
    strategies: Vec<(String, CacheStrategy)>,
    /// デフォルト戦略。
    default_strategy: CacheStrategy,
    /// オフラインリクエストキュー。
    queue: Vec<QueuedRequest>,
    /// オンライン状態。
    online: bool,
    /// 最大キャッシュサイズ (バイト)。
    max_size: usize,
    /// 現在のキャッシュサイズ (バイト)。
    current_size: usize,
}

impl SwCache {
    /// 新しいキャッシュを作成。
    #[must_use]
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: HashMap::new(),
            strategies: Vec::new(),
            default_strategy: CacheStrategy::NetworkFirst,
            queue: Vec::new(),
            online: true,
            max_size,
            current_size: 0,
        }
    }

    /// URL パターンに対するキャッシュ戦略を登録。
    pub fn add_strategy(&mut self, pattern: &str, strategy: CacheStrategy) {
        self.strategies.push((pattern.to_string(), strategy));
    }

    /// デフォルト戦略を設定。
    pub const fn set_default_strategy(&mut self, strategy: CacheStrategy) {
        self.default_strategy = strategy;
    }

    /// オンライン状態を設定。
    pub const fn set_online(&mut self, online: bool) {
        self.online = online;
    }

    /// オンラインか。
    #[must_use]
    pub const fn is_online(&self) -> bool {
        self.online
    }

    /// URL に対する戦略を解決。
    #[must_use]
    pub fn resolve_strategy(&self, url: &str) -> CacheStrategy {
        for (pattern, strategy) in &self.strategies {
            if url.contains(pattern.as_str()) {
                return *strategy;
            }
        }
        self.default_strategy
    }

    /// キャッシュにエントリーを追加。
    pub fn put(&mut self, entry: CacheEntry) {
        let size = entry.size();
        // 既存エントリーのサイズを差し引く
        if let Some(old) = self.entries.get(&entry.url) {
            self.current_size = self.current_size.saturating_sub(old.size());
        }
        // サイズ超過時は古いエントリーを削除
        while self.current_size + size > self.max_size && !self.entries.is_empty() {
            if let Some(oldest_url) = self.oldest_entry_url() {
                self.remove(&oldest_url);
            } else {
                break;
            }
        }
        self.current_size += size;
        self.entries.insert(entry.url.clone(), entry);
    }

    /// キャッシュからエントリーを取得。
    #[must_use]
    pub fn get(&self, url: &str) -> Option<&CacheEntry> {
        self.entries.get(url)
    }

    /// 有効な (期限切れでない) エントリーを取得。
    #[must_use]
    pub fn get_valid(&self, url: &str, now: f64) -> Option<&CacheEntry> {
        let entry = self.entries.get(url)?;
        if entry.is_expired(now) {
            None
        } else {
            Some(entry)
        }
    }

    /// エントリーを削除。
    pub fn remove(&mut self, url: &str) {
        if let Some(entry) = self.entries.remove(url) {
            self.current_size = self.current_size.saturating_sub(entry.size());
        }
    }

    /// 最も古いエントリーの URL を取得。
    fn oldest_entry_url(&self) -> Option<String> {
        self.entries
            .values()
            .min_by(|a, b| {
                a.cached_at
                    .partial_cmp(&b.cached_at)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|e| e.url.clone())
    }

    /// 期限切れエントリーをパージ。
    pub fn purge_expired(&mut self, now: f64) {
        let expired: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, e)| e.is_expired(now))
            .map(|(url, _)| url.clone())
            .collect();
        for url in expired {
            self.remove(&url);
        }
    }

    /// キャッシュエントリー数。
    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// 現在のキャッシュサイズ (バイト)。
    #[must_use]
    pub const fn current_size(&self) -> usize {
        self.current_size
    }

    /// リクエストをオフラインキューに追加。
    pub fn enqueue_request(&mut self, url: &str, method: &str, body: Option<Vec<u8>>, now: f64) {
        self.queue.push(QueuedRequest {
            url: url.to_string(),
            method: method.to_string(),
            body,
            queued_at: now,
            retry_count: 0,
        });
    }

    /// キューからリクエストを取り出し (FIFO)。
    pub fn dequeue_request(&mut self) -> Option<QueuedRequest> {
        if self.queue.is_empty() {
            None
        } else {
            Some(self.queue.remove(0))
        }
    }

    /// キューサイズ。
    #[must_use]
    pub const fn queue_size(&self) -> usize {
        self.queue.len()
    }

    /// キューをクリア。
    pub fn clear_queue(&mut self) {
        self.queue.clear();
    }

    /// フェッチ判定: 戦略とオンライン状態に基づいてアクションを返す。
    #[must_use]
    pub fn fetch_action(&self, url: &str, now: f64) -> FetchAction {
        let strategy = self.resolve_strategy(url);
        let cached = self.get_valid(url, now);

        match strategy {
            CacheStrategy::CacheOnly => {
                if cached.is_some() {
                    FetchAction::ServeCache
                } else {
                    FetchAction::Error
                }
            }
            CacheStrategy::NetworkOnly => {
                if self.online {
                    FetchAction::FetchNetwork
                } else {
                    FetchAction::QueueOffline
                }
            }
            CacheStrategy::CacheFirst => {
                if cached.is_some() {
                    FetchAction::ServeCache
                } else if self.online {
                    FetchAction::FetchNetwork
                } else {
                    FetchAction::QueueOffline
                }
            }
            CacheStrategy::NetworkFirst => {
                if self.online {
                    FetchAction::FetchNetwork
                } else if cached.is_some() {
                    FetchAction::ServeCache
                } else {
                    FetchAction::QueueOffline
                }
            }
            CacheStrategy::StaleWhileRevalidate => {
                if cached.is_some() {
                    if self.online {
                        FetchAction::ServeCacheAndRevalidate
                    } else {
                        FetchAction::ServeCache
                    }
                } else if self.online {
                    FetchAction::FetchNetwork
                } else {
                    FetchAction::QueueOffline
                }
            }
        }
    }
}

/// フェッチアクション — キャッシュ戦略の判定結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchAction {
    /// キャッシュから配信。
    ServeCache,
    /// ネットワークからフェッチ。
    FetchNetwork,
    /// キャッシュから配信しつつバックグラウンド更新。
    ServeCacheAndRevalidate,
    /// オフラインキューに追加。
    QueueOffline,
    /// エラー (キャッシュなし + オフライン等)。
    Error,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(url: &str, size: usize, cached_at: f64, ttl: f64) -> CacheEntry {
        CacheEntry {
            url: url.to_string(),
            body: vec![0u8; size],
            content_type: "text/html".to_string(),
            cached_at,
            ttl,
        }
    }

    #[test]
    fn cache_entry_expiry() {
        let entry = make_entry("http://a.com", 10, 1.0, 5.0);
        assert!(!entry.is_expired(3.0));
        assert!(entry.is_expired(7.0));
    }

    #[test]
    fn cache_entry_no_ttl() {
        let entry = make_entry("http://a.com", 10, 1.0, 0.0);
        assert!(!entry.is_expired(999.0));
    }

    #[test]
    fn cache_put_get() {
        let mut cache = SwCache::new(1024);
        cache.put(make_entry("http://a.com", 100, 1.0, 60.0));
        assert_eq!(cache.entry_count(), 1);
        assert!(cache.get("http://a.com").is_some());
    }

    #[test]
    fn cache_get_valid_expired() {
        let mut cache = SwCache::new(1024);
        cache.put(make_entry("http://a.com", 100, 1.0, 5.0));
        assert!(cache.get_valid("http://a.com", 3.0).is_some());
        assert!(cache.get_valid("http://a.com", 7.0).is_none());
    }

    #[test]
    fn cache_eviction() {
        let mut cache = SwCache::new(200);
        cache.put(make_entry("http://a.com", 100, 1.0, 0.0));
        cache.put(make_entry("http://b.com", 100, 2.0, 0.0));
        assert_eq!(cache.entry_count(), 2);
        // 追加で 150 バイト → 古いものを削除
        cache.put(make_entry("http://c.com", 150, 3.0, 0.0));
        assert!(cache.get("http://a.com").is_none());
    }

    #[test]
    fn cache_remove() {
        let mut cache = SwCache::new(1024);
        cache.put(make_entry("http://a.com", 100, 1.0, 0.0));
        cache.remove("http://a.com");
        assert_eq!(cache.entry_count(), 0);
        assert_eq!(cache.current_size(), 0);
    }

    #[test]
    fn cache_purge_expired() {
        let mut cache = SwCache::new(1024);
        cache.put(make_entry("http://a.com", 10, 1.0, 2.0));
        cache.put(make_entry("http://b.com", 10, 1.0, 100.0));
        cache.purge_expired(5.0);
        assert_eq!(cache.entry_count(), 1);
        assert!(cache.get("http://b.com").is_some());
    }

    #[test]
    fn strategy_resolution() {
        let mut cache = SwCache::new(1024);
        cache.add_strategy("/api/", CacheStrategy::NetworkOnly);
        cache.add_strategy("/static/", CacheStrategy::CacheFirst);
        assert_eq!(
            cache.resolve_strategy("http://a.com/api/data"),
            CacheStrategy::NetworkOnly
        );
        assert_eq!(
            cache.resolve_strategy("http://a.com/static/img.png"),
            CacheStrategy::CacheFirst
        );
        assert_eq!(
            cache.resolve_strategy("http://a.com/page"),
            CacheStrategy::NetworkFirst
        );
    }

    #[test]
    fn queue_enqueue_dequeue() {
        let mut cache = SwCache::new(1024);
        cache.enqueue_request("http://a.com", "GET", None, 1.0);
        cache.enqueue_request("http://b.com", "POST", Some(vec![1, 2]), 2.0);
        assert_eq!(cache.queue_size(), 2);

        let req = cache.dequeue_request().unwrap();
        assert_eq!(req.url, "http://a.com");
        assert_eq!(cache.queue_size(), 1);
    }

    #[test]
    fn queue_clear() {
        let mut cache = SwCache::new(1024);
        cache.enqueue_request("http://a.com", "GET", None, 1.0);
        cache.clear_queue();
        assert_eq!(cache.queue_size(), 0);
    }

    #[test]
    fn fetch_action_network_first_online() {
        let cache = SwCache::new(1024);
        assert_eq!(
            cache.fetch_action("http://a.com", 1.0),
            FetchAction::FetchNetwork
        );
    }

    #[test]
    fn fetch_action_network_first_offline_cached() {
        let mut cache = SwCache::new(1024);
        cache.put(make_entry("http://a.com", 10, 1.0, 0.0));
        cache.set_online(false);
        assert_eq!(
            cache.fetch_action("http://a.com", 2.0),
            FetchAction::ServeCache
        );
    }

    #[test]
    fn fetch_action_network_first_offline_no_cache() {
        let mut cache = SwCache::new(1024);
        cache.set_online(false);
        assert_eq!(
            cache.fetch_action("http://a.com", 1.0),
            FetchAction::QueueOffline
        );
    }

    #[test]
    fn fetch_action_cache_only_miss() {
        let mut cache = SwCache::new(1024);
        cache.add_strategy("/", CacheStrategy::CacheOnly);
        assert_eq!(
            cache.fetch_action("http://a.com/page", 1.0),
            FetchAction::Error
        );
    }

    #[test]
    fn fetch_action_stale_while_revalidate() {
        let mut cache = SwCache::new(1024);
        cache.add_strategy("/", CacheStrategy::StaleWhileRevalidate);
        cache.put(make_entry("http://a.com/page", 10, 1.0, 0.0));
        assert_eq!(
            cache.fetch_action("http://a.com/page", 2.0),
            FetchAction::ServeCacheAndRevalidate
        );
    }

    #[test]
    fn online_offline_toggle() {
        let mut cache = SwCache::new(1024);
        assert!(cache.is_online());
        cache.set_online(false);
        assert!(!cache.is_online());
    }
}
