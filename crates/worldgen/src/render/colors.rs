use image::Rgba;

use crate::generate::{hash01, smoothstep};
use crate::{Biome, Surface, World, permanent_snow_cover};

use super::shading::{lerp_rgba, offset, sample_noise};

pub(super) fn land_base_colors(world: &World, scale: u32) -> Vec<Rgba<u8>> {
    // Minimum channel_order for riparian influence — matches river_radius_px draw thresholds
    // so the green corridor only appears where a river line is actually rendered.
    let min_river_order: u8 = if scale <= 1 { 2 } else { 1 };
    // Keep visual noise blob size roughly constant in pixels across scales.
    let noise_cell = ((56 / scale.max(1)) as usize).clamp(10, 56);
    // Per-tile micro hash: scale down amplitude at scale=1 to avoid salt-and-pepper noise.
    let micro_amp = (scale.clamp(1, 4) as f32 / 4.0 * 12.0) as i16;

    let mut colors: Vec<Rgba<u8>> = (0..world.tiles.len())
        .map(|idx| {
            tile_land_color(
                world,
                idx,
                world.tiles[idx].biome,
                world.tiles[idx].moisture,
                min_river_order,
                noise_cell,
                micro_amp,
            )
        })
        .collect();

    // When rivers are culled at small scale, both the river tile and its immediate
    // land neighbors carry climate moisture that was inflated by river proximity —
    // the river tile got +0.16, adjacent land tiles +0.056 (from compute_nearby_water).
    // That inflated moisture pushed them into greener biomes, leaving a visible
    // corridor even though no blue line is drawn.
    //
    // Fix: subtract the river's moisture contribution, re-derive the dry biome via
    // biome_for_tile, and recompute the full color pipeline. The tile renders as
    // if the river never existed — the culling negates all of its effects.
    if min_river_order > 1 {
        // Moisture contributions from climate::compute_nearby_water:
        //   river/lake/ocean tiles:  nearby_water = 1.0 → 1.0 × 0.16 = 0.16
        //   adjacent land tiles:     nearby_water = 0.35 → 0.35 × 0.16 = 0.056
        const RIVER_SELF: f32 = 0.160;
        const RIVER_FRINGE: f32 = 0.056;

        let is_culled: Vec<bool> = world
            .tiles
            .iter()
            .map(|t| t.surface == Surface::River && t.channel_order < min_river_order)
            .collect();

        // Mark land tiles that neighbor any culled river (the riparian fringe).
        let mut is_fringe = vec![false; world.tiles.len()];
        for (idx, culled) in is_culled.iter().copied().enumerate() {
            if !culled {
                continue;
            }
            let (x, y) = world.coords(idx);
            for (nx, ny) in world.neighbors8(x, y) {
                let nidx = world.idx(nx, ny);
                if matches!(world.tiles[nidx].surface, Surface::Land | Surface::Coast) {
                    is_fringe[nidx] = true;
                }
            }
        }

        for idx in 0..world.tiles.len() {
            let tile = &world.tiles[idx];
            let contribution;

            if is_culled[idx] {
                contribution = RIVER_SELF;
            } else if is_fringe[idx] {
                // A fringe tile adjacent to any still-visible water (drawn river or lake)
                // should keep its riparian coloring — only strip the effect when the
                // river causing it is entirely absent from the display.
                let (x, y) = world.coords(idx);
                let near_visible = world.neighbors8(x, y).any(|(nx, ny)| {
                    let t = &world.tiles[world.idx(nx, ny)];
                    matches!(t.surface, Surface::Lake)
                        || (t.surface == Surface::River && t.channel_order >= min_river_order)
                });
                if near_visible {
                    continue;
                }
                contribution = RIVER_FRINGE;
            } else {
                continue;
            }

            let dry_moisture = (tile.moisture - contribution).max(0.0);
            // tile.biome is already correct: assign_biomes uses surrounding_land_moisture
            // for river tiles, so it reflects the actual terrain context, not the
            // inflated river moisture. Only the colour-modulation moisture needs adjusting.
            colors[idx] = tile_land_color(
                world,
                idx,
                tile.biome,
                dry_moisture,
                min_river_order,
                noise_cell,
                micro_amp,
            );
        }
    }

    colors
}

fn tile_land_color(
    world: &World,
    idx: usize,
    biome: Biome,
    moisture: f32,
    min_river_order: u8,
    noise_cell: usize,
    micro_amp: i16,
) -> Rgba<u8> {
    let tile = &world.tiles[idx];
    let (x, y) = world.coords(idx);
    let mut color = biome_color_climatic(biome, tile.temperature, moisture);
    if !matches!(biome, Biome::Ocean | Biome::Lake) {
        let height_above_sea = (tile.raw_elevation - world.sea_level).max(0.0);
        if matches!(biome, Biome::Alpine) {
            let alpine_t = ((height_above_sea - 0.36) / 0.08).clamp(0.0, 1.0);
            color = lerp_rgba(
                Rgba([132, 124, 108, 255]),
                Rgba([152, 150, 144, 255]),
                alpine_t,
            );
        } else {
            let tint_strength = smoothstep(0.06, 0.32, height_above_sea) * 0.24;
            if tint_strength > 0.0 {
                color = lerp_rgba(color, elevation_tint(height_above_sea), tint_strength);
            }
        }
        let variation = hash01(world.seed, x, y);
        let regional = sample_noise(world.seed.wrapping_add(0xCAFE_BABE), x, y, noise_cell);
        let elev_shade = (height_above_sea * 18.0) as i16;
        let micro = (variation * micro_amp as f32) as i16 - micro_amp / 2;
        let macro_v = ((regional - 0.5) * 10.0) as i16;
        color = offset(color, elev_shade + micro + macro_v);
        // Riparian zone: dry biomes adjacent to drawn rivers or lakes get a slight
        // green push representing water-side vegetation.
        if matches!(
            biome,
            Biome::Steppe
                | Biome::TemperateGrassland
                | Biome::Savanna
                | Biome::Desert
                | Biome::PolarDesert
                | Biome::Tundra
                | Biome::Foothills
        ) {
            let near_water = world.neighbors8(x, y).any(|(nx, ny)| {
                let t = &world.tiles[world.idx(nx, ny)];
                matches!(t.surface, Surface::Lake)
                    || (t.surface == Surface::River && t.channel_order >= min_river_order)
            });
            if near_water {
                color = Rgba([
                    (color[0] as i16 - 5).clamp(0, 255) as u8,
                    (color[1] as i16 + 8).clamp(0, 255) as u8,
                    (color[2] as i16 - 4).clamp(0, 255) as u8,
                    255,
                ]);
            }
        }
    }
    color
}

// One pass of weighted neighbour blending at biome boundaries.
// Tiles deep in a biome are unchanged; tiles at edges blend ≈10–30% with neighbours.
pub(super) fn soften_biome_edges(world: &World, colors: &[Rgba<u8>]) -> Vec<Rgba<u8>> {
    let mut out = colors.to_vec();
    for idx in 0..world.tiles.len() {
        let my_biome = world.tiles[idx].biome;
        if matches!(my_biome, Biome::Ocean | Biome::Lake) {
            continue;
        }
        let (x, y) = world.coords(idx);
        let mut r = colors[idx][0] as f32 * 10.0;
        let mut g = colors[idx][1] as f32 * 10.0;
        let mut b = colors[idx][2] as f32 * 10.0;
        let mut w = 10.0_f32;
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            let nb = world.tiles[nidx].biome;
            if nb == my_biome || matches!(nb, Biome::Ocean | Biome::Lake) {
                continue;
            }
            r += colors[nidx][0] as f32;
            g += colors[nidx][1] as f32;
            b += colors[nidx][2] as f32;
            w += 1.0;
        }
        out[idx] = Rgba([(r / w) as u8, (g / w) as u8, (b / w) as u8, 255]);
    }
    out
}

pub(super) fn apply_snow_overlay(world: &World, colors: &[Rgba<u8>]) -> Vec<Rgba<u8>> {
    colors
        .iter()
        .enumerate()
        .map(|(idx, &color)| {
            let snow = permanent_snow_cover(world, idx);
            if snow > 0.0 {
                lerp_rgba(color, Rgba([240, 244, 248, 255]), snow)
            } else {
                color
            }
        })
        .collect()
}

fn elevation_tint(height_above_sea: f32) -> Rgba<u8> {
    // Three-stop ramp: upland olive → highland ochre → sub-alpine stone
    if height_above_sea < 0.20 {
        let s = (height_above_sea - 0.06).max(0.0) / 0.14;
        lerp_rgba(Rgba([144, 138, 90, 255]), Rgba([148, 122, 82, 255]), s)
    } else if height_above_sea < 0.34 {
        let s = (height_above_sea - 0.20) / 0.14;
        lerp_rgba(Rgba([148, 122, 82, 255]), Rgba([132, 116, 98, 255]), s)
    } else {
        Rgba([132, 116, 98, 255])
    }
}

fn biome_color_climatic(biome: Biome, temperature: f32, moisture: f32) -> Rgba<u8> {
    let base = biome_color(biome);
    match biome {
        Biome::Steppe | Biome::TemperateGrassland => {
            // Drier steppe = warmer/golden; moister = cooler green
            let dryness = (1.0 - (moisture.clamp(0.15, 0.45) - 0.15) / 0.30).max(0.0);
            let dr = (dryness * 12.0) as i16;
            let dg = (dryness * 2.0) as i16;
            let db = (dryness * 10.0) as i16;
            Rgba([
                (base[0] as i16 + dr).clamp(0, 255) as u8,
                (base[1] as i16 + dg).clamp(0, 255) as u8,
                (base[2] as i16 - db).clamp(0, 255) as u8,
                255,
            ])
        }
        Biome::Desert => {
            // Hot deserts more orange, cooler deserts more grey-brown
            let heat = (temperature - 0.4).clamp(0.0, 0.45) / 0.45;
            let hr = (heat * 10.0) as i16;
            Rgba([
                (base[0] as i16 + hr).clamp(0, 255) as u8,
                base[1],
                (base[2] as i16 - hr).clamp(0, 255) as u8,
                255,
            ])
        }
        Biome::Savanna => {
            // Dry savanna more golden; moist savanna slightly greener
            let dry = (1.0 - (moisture.clamp(0.2, 0.4) - 0.2) / 0.2).max(0.0);
            let dr = (dry * 8.0) as i16;
            Rgba([
                (base[0] as i16 + dr).clamp(0, 255) as u8,
                (base[1] as i16 + dr / 2).clamp(0, 255) as u8,
                (base[2] as i16 - dr).clamp(0, 255) as u8,
                255,
            ])
        }
        Biome::BorealForest => {
            // Cold boreal = darker/denser; warmer margins = slightly lighter
            let cold = (1.0 - (temperature.clamp(0.12, 0.32) - 0.12) / 0.20).max(0.0);
            let d = (cold * 7.0) as i16;
            Rgba([
                (base[0] as i16 - d).clamp(0, 255) as u8,
                (base[1] as i16 - d).clamp(0, 255) as u8,
                (base[2] as i16 - d).clamp(0, 255) as u8,
                255,
            ])
        }
        Biome::Tundra => {
            // Wetter tundra = slightly greener; drier = more grey
            let wet = (moisture.clamp(0.3, 0.7) - 0.3) / 0.4;
            let g = (wet * 8.0) as i16;
            Rgba([
                (base[0] as i16 - g / 2).clamp(0, 255) as u8,
                (base[1] as i16 + g).clamp(0, 255) as u8,
                base[2],
                255,
            ])
        }
        _ => base,
    }
}

fn biome_color(biome: Biome) -> Rgba<u8> {
    match biome {
        Biome::Ocean => Rgba([38, 84, 148, 255]),
        Biome::Coast => Rgba([204, 198, 148, 255]),
        Biome::Lake => Rgba([62, 148, 204, 255]),
        Biome::PolarDesert => Rgba([212, 220, 218, 255]),
        Biome::Tundra => Rgba([148, 168, 126, 255]),
        Biome::BorealForest => Rgba([64, 112, 68, 255]),
        Biome::TemperateGrassland => Rgba([158, 184, 90, 255]),
        Biome::TemperateForest => Rgba([80, 138, 76, 255]),
        Biome::Woodland => Rgba([106, 150, 78, 255]),
        Biome::Foothills => Rgba([160, 148, 110, 255]),
        Biome::Steppe => Rgba([176, 168, 96, 255]),
        Biome::Desert => Rgba([218, 196, 126, 255]),
        Biome::Savanna => Rgba([186, 180, 76, 255]),
        Biome::TropicalForest => Rgba([56, 148, 70, 255]),
        Biome::Rainforest => Rgba([34, 112, 52, 255]),
        Biome::Alpine => Rgba([144, 146, 142, 255]),
    }
}
