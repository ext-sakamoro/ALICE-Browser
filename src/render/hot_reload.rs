//! シェーダーホットリロード — TDR 回避のためのプログレッシブコンパイル
//!
//! シェーダーをバージョン管理し、段階的にコンパイルすることで
//! GPU ドライバーの TDR (Timeout Detection & Recovery) を回避する。

use std::collections::HashMap;

/// シェーダーバージョン情報。
#[derive(Debug, Clone)]
pub struct ShaderVersion {
    /// バージョン番号 (インクリメンタル)。
    pub version: u64,
    /// シェーダーソースコード。
    pub source: String,
    /// コンパイル済みか。
    pub compiled: bool,
    /// コンパイル所要時間 (ms)。
    pub compile_time_ms: f64,
}

/// コンパイルステージ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileStage {
    /// 軽量プロキシシェーダー (起動用)。
    Proxy,
    /// 中間品質シェーダー。
    Medium,
    /// フル品質シェーダー。
    Full,
}

impl CompileStage {
    /// 次のステージを返す。`Full` の場合は `None`。
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self {
            Self::Proxy => Some(Self::Medium),
            Self::Medium => Some(Self::Full),
            Self::Full => None,
        }
    }

    /// ステージに対応する推奨 march 回数。
    #[must_use]
    pub const fn march_steps(self) -> u32 {
        match self {
            Self::Proxy => 20,
            Self::Medium => 50,
            Self::Full => 80,
        }
    }

    /// ステージに対応する推奨解像度スケール (0.0–1.0)。
    #[must_use]
    pub const fn resolution_scale(self) -> f32 {
        match self {
            Self::Proxy => 0.25,
            Self::Medium => 0.5,
            Self::Full => 1.0,
        }
    }
}

/// シェーダーキャッシュ — バージョン管理付きコンパイル済みシェーダー保持。
#[derive(Debug, Default)]
pub struct ShaderCache {
    /// シェーダー名 → バージョン履歴。
    shaders: HashMap<String, Vec<ShaderVersion>>,
    /// コンパイル待ちキュー (名前のリスト)。
    compile_queue: Vec<String>,
}

impl ShaderCache {
    /// 新しいキャッシュを作成。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// シェーダーソースを登録 (未コンパイル)。
    pub fn register(&mut self, name: &str, source: &str) -> u64 {
        let versions = self.shaders.entry(name.to_string()).or_default();
        let version = versions.len() as u64 + 1;
        versions.push(ShaderVersion {
            version,
            source: source.to_string(),
            compiled: false,
            compile_time_ms: 0.0,
        });
        self.compile_queue.push(name.to_string());
        version
    }

    /// シェーダーをコンパイル済みとしてマーク。
    pub fn mark_compiled(&mut self, name: &str, compile_time_ms: f64) {
        if let Some(versions) = self.shaders.get_mut(name) {
            if let Some(latest) = versions.last_mut() {
                latest.compiled = true;
                latest.compile_time_ms = compile_time_ms;
            }
        }
        self.compile_queue.retain(|n| n != name);
    }

    /// 最新バージョンを取得。
    #[must_use]
    pub fn latest(&self, name: &str) -> Option<&ShaderVersion> {
        self.shaders.get(name)?.last()
    }

    /// 最新コンパイル済みバージョンを取得。
    #[must_use]
    pub fn latest_compiled(&self, name: &str) -> Option<&ShaderVersion> {
        self.shaders.get(name)?.iter().rev().find(|v| v.compiled)
    }

    /// 変更検出: 前回コンパイル済みバージョンと最新バージョンが異なるか。
    #[must_use]
    pub fn needs_recompile(&self, name: &str) -> bool {
        let Some(versions) = self.shaders.get(name) else {
            return false;
        };
        let latest = versions.last();
        let latest_compiled = versions.iter().rev().find(|v| v.compiled);
        match (latest, latest_compiled) {
            (Some(l), Some(c)) => l.version != c.version,
            (Some(_), None) => true,
            _ => false,
        }
    }

    /// コンパイル待ちキューの先頭を取得。
    #[must_use]
    pub fn next_to_compile(&self) -> Option<&str> {
        self.compile_queue.first().map(String::as_str)
    }

    /// コンパイル待ちキューのサイズ。
    #[must_use]
    pub const fn queue_size(&self) -> usize {
        self.compile_queue.len()
    }

    /// 登録済みシェーダー数。
    #[must_use]
    pub fn shader_count(&self) -> usize {
        self.shaders.len()
    }
}

/// プログレッシブコンパイラー — 段階的シェーダーコンパイル管理。
#[derive(Debug)]
pub struct ProgressiveCompiler {
    /// 現在のステージ。
    stage: CompileStage,
    /// ステージ遷移閾値 (フレーム数)。
    frames_per_stage: u32,
    /// 現在のフレームカウンター。
    frame_count: u32,
    /// 各ステージのシェーダーソース。
    stages: [Option<String>; 3],
}

impl ProgressiveCompiler {
    /// 新しいプログレッシブコンパイラーを作成。
    #[must_use]
    pub fn new(proxy: &str, medium: &str, full: &str, frames_per_stage: u32) -> Self {
        Self {
            stage: CompileStage::Proxy,
            frames_per_stage: frames_per_stage.max(1),
            frame_count: 0,
            stages: [
                Some(proxy.to_string()),
                Some(medium.to_string()),
                Some(full.to_string()),
            ],
        }
    }

    /// 現在のステージ。
    #[must_use]
    pub const fn current_stage(&self) -> CompileStage {
        self.stage
    }

    /// フレーム進行: 必要ならステージを昇格し、新シェーダーソースを返す。
    pub fn tick(&mut self) -> Option<&str> {
        self.frame_count += 1;
        if self.frame_count >= self.frames_per_stage {
            self.frame_count = 0;
            if let Some(next) = self.stage.next() {
                self.stage = next;
                let idx = match next {
                    CompileStage::Proxy => 0,
                    CompileStage::Medium => 1,
                    CompileStage::Full => 2,
                };
                return self.stages[idx].as_deref();
            }
        }
        None
    }

    /// 完了したか (Full ステージに到達)。
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.stage == CompileStage::Full
    }

    /// 現在のステージに対応するシェーダーソースを取得。
    #[must_use]
    pub fn current_source(&self) -> Option<&str> {
        let idx = match self.stage {
            CompileStage::Proxy => 0,
            CompileStage::Medium => 1,
            CompileStage::Full => 2,
        };
        self.stages[idx].as_deref()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_stage_next() {
        assert_eq!(CompileStage::Proxy.next(), Some(CompileStage::Medium));
        assert_eq!(CompileStage::Medium.next(), Some(CompileStage::Full));
        assert_eq!(CompileStage::Full.next(), None);
    }

    #[test]
    fn compile_stage_march_steps() {
        assert_eq!(CompileStage::Proxy.march_steps(), 20);
        assert_eq!(CompileStage::Full.march_steps(), 80);
    }

    #[test]
    fn compile_stage_resolution() {
        assert!((CompileStage::Proxy.resolution_scale() - 0.25).abs() < 1e-6);
        assert!((CompileStage::Full.resolution_scale() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cache_register_and_get() {
        let mut cache = ShaderCache::new();
        let v = cache.register("main", "fn main() {}");
        assert_eq!(v, 1);
        assert_eq!(cache.shader_count(), 1);
        let latest = cache.latest("main").unwrap();
        assert_eq!(latest.version, 1);
        assert!(!latest.compiled);
    }

    #[test]
    fn cache_mark_compiled() {
        let mut cache = ShaderCache::new();
        cache.register("main", "fn main() {}");
        cache.mark_compiled("main", 5.0);
        let latest = cache.latest("main").unwrap();
        assert!(latest.compiled);
        assert!((latest.compile_time_ms - 5.0).abs() < 1e-10);
    }

    #[test]
    fn cache_latest_compiled() {
        let mut cache = ShaderCache::new();
        cache.register("main", "v1");
        cache.mark_compiled("main", 1.0);
        cache.register("main", "v2");
        // v2 は未コンパイル
        let compiled = cache.latest_compiled("main").unwrap();
        assert_eq!(compiled.version, 1);
    }

    #[test]
    fn cache_needs_recompile() {
        let mut cache = ShaderCache::new();
        cache.register("main", "v1");
        assert!(cache.needs_recompile("main"));
        cache.mark_compiled("main", 1.0);
        assert!(!cache.needs_recompile("main"));
        cache.register("main", "v2");
        assert!(cache.needs_recompile("main"));
    }

    #[test]
    fn cache_compile_queue() {
        let mut cache = ShaderCache::new();
        cache.register("a", "src_a");
        cache.register("b", "src_b");
        assert_eq!(cache.queue_size(), 2);
        assert_eq!(cache.next_to_compile(), Some("a"));
        cache.mark_compiled("a", 1.0);
        assert_eq!(cache.queue_size(), 1);
        assert_eq!(cache.next_to_compile(), Some("b"));
    }

    #[test]
    fn cache_nonexistent() {
        let cache = ShaderCache::new();
        assert!(cache.latest("nope").is_none());
        assert!(cache.latest_compiled("nope").is_none());
        assert!(!cache.needs_recompile("nope"));
    }

    #[test]
    fn progressive_initial_stage() {
        let pc = ProgressiveCompiler::new("proxy", "medium", "full", 10);
        assert_eq!(pc.current_stage(), CompileStage::Proxy);
        assert!(!pc.is_complete());
    }

    #[test]
    fn progressive_stage_advancement() {
        let mut pc = ProgressiveCompiler::new("proxy", "medium", "full", 2);
        assert!(pc.tick().is_none()); // frame 1
        let src = pc.tick(); // frame 2 → advance to Medium
        assert!(src.is_some());
        assert_eq!(src.unwrap(), "medium");
        assert_eq!(pc.current_stage(), CompileStage::Medium);
    }

    #[test]
    fn progressive_reaches_full() {
        let mut pc = ProgressiveCompiler::new("proxy", "medium", "full", 1);
        pc.tick(); // → Medium
        pc.tick(); // → Full
        assert!(pc.is_complete());
        assert_eq!(pc.current_stage(), CompileStage::Full);
    }

    #[test]
    fn progressive_stays_at_full() {
        let mut pc = ProgressiveCompiler::new("proxy", "medium", "full", 1);
        pc.tick(); // → Medium
        pc.tick(); // → Full
        let src = pc.tick(); // stays Full
        assert!(src.is_none());
        assert!(pc.is_complete());
    }

    #[test]
    fn progressive_current_source() {
        let pc = ProgressiveCompiler::new("proxy", "medium", "full", 10);
        assert_eq!(pc.current_source(), Some("proxy"));
    }
}
