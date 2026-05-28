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
cargo run --example river_audit          # river network diagnostics across 5 seeds
cargo test
```

## CLI flags

| Flag | Default | Notes |
|------|---------|-------|
| `--seed` | random | u64 |
| `--width` / `--height` | 384 | tiles; max 4096 |
| `--scale` | 1 | world multiplier; `--scale 2` → 768×768 tiles, same pixels/tile as default |
| `--sea-level` | 0.52 | 0.2–0.8 |
| `--world-size` | 0 | tiles per world unit; 0 = match min(width,height); auto-fixed by --scale |
| `--out-dir` | `output/` | directory for PNG + metadata JSON |
| `--temperature-bias` | 0.0 | shifts global temperature; useful for polar/tropical worlds |
| `--moisture-bias` | 0.0 | shifts global precipitation |
| `--rainfall-scale` | 1.0 | 0.25–4.0; scales precipitation before runoff |
| `--runoff-scale` | 1.0 | 0.25–4.0; multiplies per-tile runoff coefficient |
| `--channel-density` | 1.0 | 0.25–4.0; lowers/raises discharge threshold for river classification |

`--scale` is the main lever for larger worlds. `--scale 2` generates 768×768 tiles with the same ~4 px/tile density as the 384×384 default — a genuinely bigger world, not a zoomed-in view. It automatically sets `world_size=384` so each tile covers the same geographic area at any scale. For asymmetric maps use `--width`/`--height` directly and set `--world-size 384` manually.

## worldgen library — public API

```rust
generate_world(&WorldConfig) -> Result<World, String>
render_world(&World, RenderConfig) -> DynamicImage
build_metadata(&World, &WorldConfig) -> WorldMetadata
```

`WorldConfig` fields mirror the CLI flags. Use `..WorldConfig::default()` for any omitted fields.

## Generation pipeline (`generate/mod.rs`)

1. `terrain::populate_raw_elevation` — plate tectonics + 18-step erosion
2. `hydrology::classify_ocean` — flood-fill ocean from boundary
3. `climate::populate_base_climate` — first-pass temperature/moisture (no lakes/rivers yet)
4. `hydrology::simulate_hydrology` — flow routing, lake formation, channel carving (first pass)
5. `hydrology::apply_channel_carving` — lowers elevation along river cells
6. Re-run steps 2–4 on carved terrain
7. Second `hydrology::simulate_hydrology` — final river/lake classification
8. `hydrology::apply_hydrology_to_world` — writes hydrology fields to tiles
9. `climate::populate_climate` — final temperature/moisture using rivers and lakes
10. `biomes::assign_biomes`

## Key subsystems

### terrain.rs

**Coordinate space**: everything normalized to world units. `xf = x / world.effective_world_size()`. For a 384×384 map this is [0,1]×[0,1]; for a 1024×1024 map with `world_size=384` it's [0,2.67]×[0,2.67].

**Continental placement**: `build_continental_config(seed, world_units_x, world_units_y)` precomputes `PreparedLobe` / `PreparedCut` structs once. `sample_continental_fields(&cfg, xf, yf)` is per-tile distance evaluation only. Lobe positions span `[-0.2, 1.2] × world_units` so lobes always overlap the visible area. Formula: `continental.support * 0.36 + continent_noise * 0.40 + ...` — noise drives organic coastlines, lobes steer continent locations.

**Sea level floor**: after normalization, `sea_level` is lowered if needed to guarantee ≥25% land tiles.

**Erosion routing fields**: precomputed once before the 18-step loop (`routing_noise_field`, `flow_opportunity`, `trib_opportunity`, `meander_field`). Cell sizes scale by `width / effective_world_size()` so noise texture frequency stays geographically consistent.

**Plate count**: `base = (ws² / 18000).round()`, then scaled by world area. Clamped to 8–120.

### hydrology.rs

**River thresholds** are based on `ws² × 0.00075` (world-unit area), not pixel count, so river density is geographically consistent at any resolution. `secondary = stream × 6.5`, `trunk = stream × 18.0`.

**Channel order** (1–4) is assigned from discharge percentiles: order 4 ≥ p94, order 3 ≥ p82, order 2 ≥ p55, order 1 below. Order is propagated downstream so a trunk tile is never lower-order than its tributaries.

**Key per-tile hydrology fields** (on `Tile`):
- `runoff` — per-tile precipitation-based runoff coefficient × precip^1.35
- `discharge` — accumulated runoff from entire upstream catchment
- `stream_power` — `discharge^0.88 × (slope + 0.0035)^0.70`; drives river classification alongside discharge
- `contributing_area` — tile count of upstream drainage area
- `channel_order` — 0 (non-river) or 1–4
- `downstream` — index of the next tile in the drainage network
- `basin_id` — drainage basin identifier (shared by lake and ocean-mouth basins)
- `hydro_elevation` — depression-filled elevation used for routing

### climate.rs

`latitude_factor(y, height)` returns 0 at the equator (center row), 1 at poles.

**Wind**: `prevailing_wind_angle(seed)` returns a seed-specific ±45° offset from pure westerly. `wind_at_latitude(tilt, lat)` applies Hadley cells (tropical easterlies lat<0.24, mid-latitude westerlies 0.24–0.70, polar easterlies lat>0.70) combined with the world tilt. This means each seed has a distinct moisture asymmetry.

**Rain shadow** (`rain_shadow`): scans 16 tiles upwind for an ocean source; terrain barriers along the path reduce the moisture return. A secondary 8-tile downwind scan adds leeward moisture. Weight in moisture formula: `shadow × 0.20`.

**Temperature**: `equatorial_warmth = (1 - lat^1.08) × 0.90`; decreases with elevation (`× 0.34`). Maritime bonus from nearby water bodies contributes after the second hydrology pass.

### render.rs

**Rendering pipeline** (multi-pass):
1. Compute hillshade per tile (bilinear-interpolated at sub-pixel level).
2. `land_base_colors` — per-tile: `biome_color_climatic` (moisture/temperature variants) + elevation shade + per-tile hash noise + medium-scale `sample_noise` (cell=14) + permasnow overlay + riparian zone boost.
3. `soften_biome_edges` — one weighted-average pass at biome boundaries (own weight 10, each different-biome 8-neighbor weight 1).
4. Main loop: ocean depth gradient + land hillshade + terrain symbols + directional coastline.
5. Lake pass (depth gradient via `water_level`).
6. River pass (variable-width lines).

**Ocean**: three-stop depth gradient — shelf [58,132,182] → open ocean [38,84,148] → abyss [18,46,102]. Subtle per-tile hash texture.

**River rendering**: variable-width lines; three discharge tiers (headwater/secondary/trunk) map to three colors. Width scales with `channel_order` and discharge vs. trunk threshold.

**Hillshading**: bilinear-interpolated, adaptive `z_scale = 3.5 + height_above_sea × 12.0` (gentle on plains, dramatic on mountains). Light from NW at 45°. Shadow range `0.46–1.0`.

**Biome colors**: `biome_color_climatic` varies within-biome by moisture and temperature — drier steppe is more golden, hot desert more orange, cold boreal darker, wet tundra greener.

**Terrain symbols**: Alpine → peak glyph; Foothills → hill arcs; Desert/PolarDesert → dune diagonals; forest biomes → tree dot cluster. Coast draws a beach line on the ocean-facing edge(s) only.

**Permasnow**: `sea_level + 0.26 + temperature × 0.20`, capped at `sea_level + 0.46`. Only true glacier/alpine snowfields qualify; polar lowlands use biome color, not a snow overlay.

### world.rs

```rust
world.effective_world_size() -> f32
// Returns world_size if set, else min(width, height).
```

`Surface` enum: `Ocean`, `Coast`, `Land`, `Lake`, `River`.
`Coast` is a 1-tile-wide land ring adjacent to ocean, classified during `classify_surfaces`.

## Tests

Integration tests: `crates/worldgen/tests/generation.rs` (24 tests). Unit tests: `crates/worldgen/src/lib.rs` (4 tests). Run with `cargo test`.

Notable test seeds: 42, 97, 3000, 7073116918442829777, 12302556654306610728.

Tests use `..WorldConfig::default()` so new `WorldConfig` fields don't require test changes.

Key integration test invariants: rivers reach a sink (ocean or lake), drainage never routes uphill (hydro_elevation monotone), trunk rivers are less mountain-confined than headwaters, trunk straight-run ratio < 0.62, tributary spacing variance > 35.
