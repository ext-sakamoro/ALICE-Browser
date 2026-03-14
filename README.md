# ALICE-Browser

**The Web Recompiled — Semantic browser powered by ALICE ecosystem**

> "Don't render pixels. Render meaning."

## Overview

ALICE-Browser is a next-generation web browser that replaces the traditional HTML/CSS pixel pipeline with ALICE ecosystem components. Instead of painting bitmaps, it renders web content through SDF-based graphics, ternary ML inference, and predictive caching.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      ALICE-Browser                          │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─── Rendering ──────┐  ┌─── Intelligence ──────────────┐ │
│  │ ALICE-SDF (GPU)    │  │ ALICE-ML (content filtering)  │ │
│  │ wgpu pipeline      │  │ ALICE-Search (full-text)      │ │
│  │ egui integration   │  │ ALICE-Analytics (telemetry)   │ │
│  └────────────────────┘  └───────────────────────────────┘ │
│                                                             │
│  ┌─── Networking ─────┐  ┌─── Core ─────────────────────┐ │
│  │ reqwest (HTTP)     │  │ DOM parser (scraper)          │ │
│  │ ALICE-Cache        │  │ Layout engine                 │ │
│  │ Predictive fetch   │  │ Tab management                │ │
│  └────────────────────┘  └───────────────────────────────┘ │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

## Features

- **SDF Rendering**: GPU-accelerated page rendering via ALICE-SDF
- **Ternary ML**: Content filtering with 1.58-bit inference via ALICE-ML
- **Smart Cache**: Predictive caching via ALICE-Cache
- **Full-Text Search**: FM-Index local search via ALICE-Search
- **Telemetry**: Privacy-preserving analytics via ALICE-Analytics
- **LOL DSL**: Law-Oriented Language parsing via ALICE-LOL (`parse_lol` helper, 120 constructs)
- **Modular**: Each ALICE integration is an optional feature flag

## Feature Flags

| Flag | Description | Dependencies |
|------|-------------|--------------|
| `sdf-render` (default) | GPU SDF rendering | ALICE-SDF, wgpu |
| `ml-filter` | ML content filtering | ALICE-ML |
| `smart-cache` | Predictive caching | ALICE-Cache |
| `search` | Local full-text search | ALICE-Search |
| `telemetry` | Privacy analytics | ALICE-Analytics |
| `cdn` | ALICE-CDN Vivaldi coordinate routing | ALICE-CDN |
| `view-sdf` | SDF-based resolution-independent UI | ALICE-View |
| `sdf-web` | Web SDF scene evaluation | ALICE-SDF |
| `voice-web` | Browser voice activity detection | ALICE-Voice |
| `mobile` | Mobile optimized | Cache + Search |
| `lol` | LOL DSL parsing (`parse_lol` helper) | ALICE-LOL |
| `alice-full` | All ALICE features | All above |

## Quick Start

```bash
# Clone
git clone https://github.com/ext-sakamoro/ALICE-Browser.git
cd ALICE-Browser

# Run with default features (SDF rendering)
cargo run

# Run with all ALICE features
cargo run --features alice-full

# Run minimal (no ALICE deps)
cargo run --no-default-features
```

## Cross-Crate Bridges

ALICE-Browser connects to other ALICE ecosystem crates via feature-gated bridge modules:

| Bridge | Feature | Target Crate | Description |
|--------|---------|--------------|-------------|
| `text_bridge` | `text` | [ALICE-Text](../ALICE-Text) | Advanced text shaping and rendering |
| `cache_bridge` | `cache` | [ALICE-Cache](../ALICE-Cache) | DOM classification caching with FNV-1a content hashing |
| `lol_bridge` | `lol` | [ALICE-LOL](https://github.com/ext-sakamoro/ALICE-LOL) | Law-Oriented Language DSL parsing (120 constructs) |

### Cache Bridge (feature: `cache`)

Caches DOM node classification results (Content, Navigation, Ad, Tracker, Widget) using ALICE-Cache with FNV-1a content hashing. Avoids redundant ML inference for previously classified DOM patterns.

```toml
[dependencies]
alice-browser = { path = "../ALICE-Browser", features = ["cache"] }
```

```rust
use alice_browser::cache_bridge::{DomClassificationCache, DomClass, dom_node_hash};

// Create cache (capacity = max entries)
let cache = DomClassificationCache::new(10_000);

// Compute content hash for a DOM node
let hash = dom_node_hash("div", "sidebar ad-container", "https://example.com");

// Store classification
cache.put(hash, DomClass::Ad);

// Lookup (O(1) via ALICE-Cache)
if let Some(class) = cache.get(hash) {
    // Skip ML inference, use cached result
}
```

### ALICE-Search Bridge (feature: `search`)

In-page full-text search using FM-Index.

- `DomSearchIndex` — FM-Index wrapper for DOM text content
- In-page search with O(m) backward search complexity

Enable: `alice-browser = { features = ["search"] }`

### ALICE-Analytics Bridge (feature: `telemetry`)

Browser telemetry with streaming aggregation.

- `BrowserMetrics` — DDSketch (page load latency), HyperLogLog (unique domains), CountMinSketch (hot URLs)
- `record_navigation()` / `record_resource()` — Record browser events

Enable: `alice-browser = { features = ["telemetry"] }`

### ALICE-CDN Bridge (feature: `cdn`)

Vivaldi coordinate-based content routing.

- `BrowserCdnRouter` — Nearest-node selection via Vivaldi coordinates
- `route_request()` — Route content requests to optimal edge node

Enable: `alice-browser = { features = ["cdn"] }`

### ALICE-View (SDF UI) Bridge (feature: `view-sdf`)

Resolution-independent UI primitives via SDF rendering.

- `SdfUiBatch` / `SdfUiCommand` — Batched SDF UI draw commands
- `sdf_rounded_rect()` — Infinite-resolution rounded rectangles

Enable: `alice-browser = { features = ["view-sdf"] }`

### ALICE-SDF Bridge (feature: `sdf-web`)

Web SDF scene evaluation and sphere tracing.

- `WebSdfScene` / `WebSdfPrimitive` — Sphere, Box, Cylinder primitives
- `eval_scene()` / `sphere_trace()` — SDF evaluation and ray marching

Enable: `alice-browser = { features = ["sdf-web"] }`

### ALICE-Voice Bridge (feature: `voice-web`)

Browser voice activity detection and audio processing.

- `BrowserVoiceSession` — Voice activity detection + downsampling
- `detect_voice_activity()` — Energy-based VAD
- `downsample_to_16k()` — Resample to 16kHz for codec input

Enable: `alice-browser = { features = ["voice-web"] }`

### ALICE-LOL Bridge (feature: `lol`)

Law-Oriented Language DSL parsing via the `parse_lol` helper. Exposes all 120 LOL constructs for semantic rule evaluation inside browser content pipelines (content policy enforcement, ad-blocker rules, navigation guards).

```toml
[dependencies]
alice-browser = { path = "../ALICE-Browser", features = ["lol"] }
```

```rust
use alice_browser::lol_bridge::parse_lol;

// Parse a LOL DSL expression into a structured rule tree
let rule = parse_lol("DENY url CONTAINS \"tracker.js\" AND method IS GET")?;

// Evaluate against a request context
if rule.eval(&request_ctx) {
    // Block the request
}
```

Enable: `alice-browser = { features = ["lol"] }`

## Related Projects

- [ALICE-LOL](https://github.com/ext-sakamoro/ALICE-LOL) — Law-Oriented Language DSL (120 constructs)

## License

Licensed under either of:

- MIT License ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.

## Author

Moroya Sakamoto
