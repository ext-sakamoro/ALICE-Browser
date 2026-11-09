# Changelog

All notable changes to ALICE-Browser will be documented in this file.

## [Unreleased]

### Fixed

- `telemetry` feature: ALICE-Analytics の main で compile できなかった (`alice_analytics::prelude` と crate root の `DDSketch` / `HyperLogLog` / `CountMinSketch` が無い) `alice_analytics::sketch` から引くように変更
- `telemetry::BrowserMetrics`: 名前の hash で枠を引く `MetricPipeline<128, 256>` (約 715 KB) をやめ、指標ごとの field (`DDSketch` / `HyperLogLog` / 計数) に変更 構築時に 2 MB の thread stack が溢れていた 枠を減らすと 2 つの指標が同じ枠に入って値が 0 になった (8 枠・32 枠で再現)
- `analytics_bridge::BrowserMetrics`: 現在の sketch の API (`DDSketch::new(alpha)`、`insert_hash`、`cardinality`) に合わせた 試験: 1..=100 ms の中央値 50.5 / P95 95.05 と、異なる 10,000 件の個数を許容誤差の中で比べる
- `simd`: x86_64 の AVX2 経路の `transmute` に型を明示した (Linux の clippy が `missing_transmute_annotations` で失敗していた)
- CI: `telemetry` feature を実物の ALICE-Analytics (main) で clippy と test する step を追加した (これまでは空の代用品で、どの job も build していなかった)

### Changed

- Licence: `MIT OR Apache-2.0` → `LicenseRef-Proprietary`

## [0.2.0] - 2026-02-23

### Added
- `dom` — HTML parsing, DOM tree construction, semantic node classification
- `engine` — Layout engine, content extraction, page scoring
- `net` — HTTP fetching with `reqwest` blocking client
- `render` — 3D rotunda text-particle stream, grabbed-info overlay, egui integration
- `branchless` — `BitMask64`, `ComparisonMask`, blend/select primitives (zero branch)
- `fast_math` — `fast_rcp`, `fast_inv_sqrt`, `fast_sqrt`, FMA, batch operations, division exorcism
- `simd` — SoA node features, SIMD batch classification, aligned vectors
- `mobile` — Touch gesture handling, mobile viewport, swipe navigation
- `search` — (feature `search`) ALICE-Search full-text integration
- `telemetry` — (feature `telemetry`) ALICE-Analytics bridge
- `text_bridge` — (feature `text`) ALICE-Text compression bridge
- `cache_bridge` — (feature `cache`) ALICE-Cache smart caching bridge
- `cdn_bridge` — (feature `cdn`) ALICE-CDN Vivaldi coordinate routing bridge
- `view_bridge` — (feature `view-sdf`) SDF-based resolution-independent UI bridge
- `sdf_bridge` — (feature `sdf-web`) Web SDF scene evaluation bridge
- `voice_bridge` — (feature `voice-web`) Browser voice activity detection bridge
- 134 unit tests
