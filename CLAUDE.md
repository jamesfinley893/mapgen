# mapgen

Rust workspace that generates seeded procedural world maps as PNG files.

## Structure

```
crates/
  worldgen/   — library: generation, hydrology, climate, biomes, rendering
  cli/        — binary: `mapgen generate` CLI
output/       — generated maps land here (gitignored)
```

## Build & run

```sh
cargo build
cargo run --bin mapgen -- generate --seed 42
cargo run --bin mapgen -- generate --seed 42 --width 1024 --height 1024 --world-size 384
cargo test
```

## CLI flags

| Flag | Default | Notes |
|------|---------|-------|
| `--seed` | random | u64 |
| `--width` / `--height` | 384 | tiles; max 4096 |
| `--scale` | auto | pixels/tile; auto targets ~1536px on the long axis |
| `--sea-level` | 0.52 | 0.2–0.8 |
| `--world-size` | 0 | tiles per world unit; 0 = match min(width,height) |
| `--out-dir` | `output/` | directory for PNG + metadata JSON |

`--world-size` is the key lever for geographic expanse. With `--width 1024 --height 1024 --world-size 384` the map covers ~2.67× more geographic area than the 384×384 default instead of just upscaling the same world.

## worldgen library — public API

```rust
generate_world(&WorldConfig) -> Result<World, String>
render_world(&World, RenderConfig) -> DynamicImage
build_metadata(&World, &WorldConfig) -> WorldMetadata
```

`WorldConfig` fields mirror the CLI flags. Use `..WorldConfig::default()` for any omitted fields.

## Generation pipeline (`generate/mod.rs`)

1. `terrain::populate_raw_elevation` — plate tectonics + 18-step erosion
2. `hydrology::classify_ocean` + `simulate_hydrology` — flow routing, lake formation, channel carving (run twice: carve first, then classify)
3. `climate::populate_climate` — temperature, moisture, ocean distance
4. `biomes::assign_biomes`

## Key subsystems

### terrain.rs

**Coordinate space**: everything normalized to world units. `xf = x / world.effective_world_size()`. For a 384×384 map this is [0,1]×[0,1]; for a 1024×1024 map with `world_size=384` it's [0,2.67]×[0,2.67].

**Continental placement**: `build_continental_config(seed, world_units_x, world_units_y)` precomputes `PreparedLobe` / `PreparedCut` structs once. `sample_continental_fields(&cfg, xf, yf)` is per-tile distance evaluation only. Lobe positions span `[-0.2, 1.2] × world_units` so lobes always overlap the visible area. Formula: `continental.support * 0.36 + continent_noise * 0.40 + ...` — noise drives organic coastlines, lobes steer continent locations.

**Sea level floor**: after normalization, `sea_level` is lowered if needed to guarantee ≥25% land tiles.

**Erosion routing fields**: precomputed once before the 18-step loop (`routing_noise_field`, `flow_opportunity`, `trib_opportunity`, `meander_field`). Cell sizes scale by `width / effective_world_size()` so noise texture frequency stays geographically consistent.

**Plate count**: `base = (ws² / 18000).round()`, then scaled by world area. Clamped to 8–120.

### hydrology.rs

River and lake thresholds are based on `ws² × 0.00075` (world-unit area), not pixel count, so river density is geographically consistent at any resolution.

### world.rs

```rust
world.effective_world_size() -> f32
// Returns world_size if set, else min(width, height).
```

## Tests

Integration tests live in `crates/worldgen/tests/generation.rs` (24 tests). Unit tests are in `crates/worldgen/src/lib.rs` (4 tests). Run with `cargo test`.

Notable test seeds: 42, 97, 3000, 7073116918442829777, 12302556654306610728.

Tests use `..WorldConfig::default()` so new `WorldConfig` fields don't require test changes.
