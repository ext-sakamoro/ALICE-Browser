# Changelog

All notable changes to ALICE-Browser will be documented in this file.

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
