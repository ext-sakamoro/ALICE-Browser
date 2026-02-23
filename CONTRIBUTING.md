# Contributing to ALICE-Browser

## Build

```bash
cargo build
```

## Test

```bash
cargo test
```

## Lint

```bash
cargo clippy -- -W clippy::all
cargo fmt -- --check
cargo doc --no-deps 2>&1 | grep warning
```

## Optional Features

```bash
# Full ALICE ecosystem integration
cargo build --features alice-full

# Individual bridges
cargo build --features "search,telemetry,text,cache,cdn"

# Mobile UI (includes smart-cache + search)
cargo build --features mobile
```

## Design Constraints

- **SoA + SIMD batch**: node features stored as Structure of Arrays for 8-wide SIMD classification.
- **Branchless classification**: `BitMask64` + `ComparisonMask` — zero `if/else` in hot loops.
- **Division exorcism**: all hot-path divisions replaced with reciprocal multiplication or `fast_rcp`.
- **Cache-line alignment**: SIMD data is `align(32)` (AVX2) / `align(16)` (NEON).
- **3D rotunda rendering**: web page content displayed as text particles in a cylindrical 3D scene.
- **Zero-copy bridges**: optional ALICE ecosystem crates connected via feature-gated bridge modules.
