#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_lossless,
    clippy::similar_names,
    clippy::many_single_char_names,
    clippy::module_name_repetitions,
    clippy::inline_always,
    clippy::too_many_lines
)]

pub mod dom;
pub mod engine;
pub mod net;
pub mod render;

// Deep-Fried Rust: カリッカリ最適化モジュール
pub mod branchless;
pub mod fast_math;
pub mod simd;

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

/// LOL DSL テキストから `SdfNode` をパースする。
///
/// ブラウザ内でユーザーが LOL テキストを入力し、
/// SDF レンダリングに渡すためのヘルパー。
#[cfg(feature = "lol")]
pub fn parse_lol(text: &str) -> Result<alice_sdf::SdfNode, String> {
    alice_lol::runtime_parser::parse_lol(text).map_err(|e| format!("LOL parse error: {e}"))
}

#[cfg(feature = "sdf-web")]
pub mod sdf_bridge;

#[cfg(feature = "voice-web")]
pub mod voice_bridge;
