pub mod dom;
pub mod net;
pub mod render;
pub mod engine;

// Deep-Fried Rust: カリッカリ最適化モジュール
pub mod simd;
pub mod branchless;
pub mod fast_math;

// Mobile UI (always compiled, feature-gated internally where needed)
pub mod mobile;

#[cfg(feature = "search")]
pub mod search;

#[cfg(feature = "telemetry")]
pub mod telemetry;

#[cfg(feature = "text")]
pub mod text_bridge;

#[cfg(feature = "cache")]
pub mod cache_bridge;

#[cfg(feature = "search")]
pub mod search_bridge;

#[cfg(feature = "telemetry")]
pub mod analytics_bridge;

#[cfg(feature = "cdn")]
pub mod cdn_bridge;

#[cfg(feature = "view-sdf")]
pub mod view_bridge;

#[cfg(feature = "sdf-web")]
pub mod sdf_bridge;

#[cfg(feature = "voice-web")]
pub mod voice_bridge;
