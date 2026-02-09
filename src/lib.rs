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
