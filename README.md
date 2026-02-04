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
- **Modular**: Each ALICE integration is an optional feature flag

## Feature Flags

| Flag | Description | Dependencies |
|------|-------------|--------------|
| `sdf-render` (default) | GPU SDF rendering | ALICE-SDF, wgpu |
| `ml-filter` | ML content filtering | ALICE-ML |
| `smart-cache` | Predictive caching | ALICE-Cache |
| `search` | Local full-text search | ALICE-Search |
| `telemetry` | Privacy analytics | ALICE-Analytics |
| `mobile` | Mobile optimized | Cache + Search |
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

## License

Licensed under either of:

- MIT License ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.

## Author

Moroya Sakamoto
