use image::Rgba;

use crate::generate::{hash01, smoothstep};
use crate::{Biome, World, permanent_snow_cover};

use super::biome_style::coastal_hinterland_biome;
use super::shading::{lerp_rgba, offset, sample_noise};

pub(super) fn land_base_colors(world: &World, scale: u32) -> Vec<Rgba<u8>> {
    // Keep visual noise blob size roughly constant in pixels across scales.
    let noise_cell = ((56 / scale.max(1)) as usize).clamp(10, 56);
    // Per-tile micro hash: scale down amplitude at scale=1 to avoid salt-and-pepper noise.
    let micro_amp = (scale.clamp(1, 4) as f32 / 4.0 * 12.0) as i16;

    (0..world.tiles.len())
        .map(|idx| {
            let tile = &world.tiles[idx];
            if is_water_layer(tile.biome) {
                return biome_color_climatic(tile.biome, tile.temperature, tile.moisture);
            }
            tile_land_color(world, idx, tile.biome, tile.moisture, noise_cell, micro_amp)
        })
        .collect()
}

fn tile_land_color(
    world: &World,
    idx: usize,
    biome: Biome,
    moisture: f32,
    noise_cell: usize,
    micro_amp: i16,
) -> Rgba<u8> {
    let tile = &world.tiles[idx];
    let (x, y) = world.coords(idx);
    let visual_biome = if biome == Biome::Coast {
        coastal_hinterland_biome(tile.temperature, moisture)
    } else {
        biome
    };
    let mut color = biome_color_climatic(visual_biome, tile.temperature, moisture);
    if biome != Biome::Ocean {
        let height_above_sea = (tile.raw_elevation - world.sea_level).max(0.0);
        if matches!(biome, Biome::Alpine) {
            let alpine_t = smoothstep(0.38, 0.72, height_above_sea);
            let rugged = smoothstep(0.020, 0.080, tile.relief + tile.slope * 0.75);
            color = lerp_rgba(
                lerp_rgba(
                    Rgba([104, 102, 96, 255]),
                    Rgba([136, 132, 122, 255]),
                    rugged,
                ),
                lerp_rgba(
                    Rgba([132, 132, 126, 255]),
                    Rgba([170, 170, 164, 255]),
                    rugged,
                ),
                alpine_t * 0.55 + rugged * 0.25,
            );
        } else {
            let tint_strength = smoothstep(0.04, 0.30, height_above_sea) * 0.28;
            if tint_strength > 0.0 {
                color = lerp_rgba(color, elevation_tint(height_above_sea), tint_strength);
            }
        }
        let variation = hash01(world.seed, x, y);
        let regional = sample_noise(world.seed.wrapping_add(0xCAFE_BABE), x, y, noise_cell);
        let elev_shade = (height_above_sea * 24.0) as i16;
        let micro = (variation * micro_amp as f32) as i16 - micro_amp / 2;
        let macro_v = ((regional - 0.5) * 10.0) as i16;
        color = offset(color, elev_shade + micro + macro_v);
    }
    color
}

pub(super) fn soften_biome_edges(world: &World, colors: &[Rgba<u8>]) -> Vec<Rgba<u8>> {
    let mut out = colors.to_vec();
    let edge_zone = biome_edge_zone(world, 3);
    for idx in 0..world.tiles.len() {
        let tile = &world.tiles[idx];
        let my_biome = tile.biome;
        if is_water_layer(my_biome) || !edge_zone[idx] {
            continue;
        }
        let (x, y) = world.coords(idx);
        let mut r = colors[idx][0] as f32 * 12.0;
        let mut g = colors[idx][1] as f32 * 12.0;
        let mut b = colors[idx][2] as f32 * 12.0;
        let mut w = 12.0_f32;

        for dy in -3_isize..=3 {
            for dx in -3_isize..=3 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = x as isize + dx;
                let ny = y as isize + dy;
                if !world.in_bounds(nx, ny) {
                    continue;
                }
                let nidx = world.idx(nx as usize, ny as usize);
                let nb = world.tiles[nidx].biome;
                if nb == my_biome || is_water_layer(nb) {
                    continue;
                }
                let dist2 = (dx * dx + dy * dy) as f32;
                let weight = 2.2 / (dist2 + 0.5);
                r += colors[nidx][0] as f32 * weight;
                g += colors[nidx][1] as f32 * weight;
                b += colors[nidx][2] as f32 * weight;
                w += weight;
            }
        }
        out[idx] = Rgba([(r / w) as u8, (g / w) as u8, (b / w) as u8, 255]);
    }
    out
}

fn biome_edge_zone(world: &World, radius: usize) -> Vec<bool> {
    let mut edge = vec![false; world.tiles.len()];
    for idx in 0..world.tiles.len() {
        let tile = &world.tiles[idx];
        if is_water_layer(tile.biome) {
            continue;
        }
        let (x, y) = world.coords(idx);
        edge[idx] = world.neighbors8(x, y).any(|(nx, ny)| {
            let neighbor = world.tiles[world.idx(nx, ny)].biome;
            neighbor != tile.biome && !is_water_layer(neighbor)
        });
    }

    let mut zone = edge;
    let mut frontier = zone.clone();
    let mut next_frontier = vec![false; world.tiles.len()];
    for _ in 0..radius {
        next_frontier.fill(false);
        for idx in 0..world.tiles.len() {
            if !frontier[idx] {
                continue;
            }
            let (x, y) = world.coords(idx);
            for (nx, ny) in world.neighbors8(x, y) {
                let nidx = world.idx(nx, ny);
                if zone[nidx] || is_water_layer(world.tiles[nidx].biome) {
                    continue;
                }
                zone[nidx] = true;
                next_frontier[nidx] = true;
            }
        }
        std::mem::swap(&mut frontier, &mut next_frontier);
    }

    zone
}

fn is_water_layer(biome: Biome) -> bool {
    matches!(biome, Biome::Ocean | Biome::Freshwater)
}

pub(super) fn apply_snow_overlay_in_place(world: &World, colors: &mut [Rgba<u8>]) {
    let snow_cover = smoothed_snow_cover(world);
    for (idx, color) in colors.iter_mut().enumerate() {
        let snow = snow_cover[idx];
        if snow > 0.0 {
            *color = lerp_rgba(*color, Rgba([240, 244, 248, 255]), snow);
        }
    }
}

fn smoothed_snow_cover(world: &World) -> Vec<f32> {
    let mut cover = (0..world.tiles.len())
        .map(|idx| permanent_snow_cover(world, idx))
        .collect::<Vec<_>>();
    let mut scratch = vec![0.0_f32; cover.len()];

    for _ in 0..2 {
        for idx in 0..world.tiles.len() {
            if !can_carry_snow_overlay(world.tiles[idx].biome) {
                scratch[idx] = 0.0;
                continue;
            }

            let (x, y) = world.coords(idx);
            let mut sum = cover[idx] * 6.0;
            let mut weight = 6.0_f32;
            for (nx, ny) in world.neighbors8(x, y) {
                let nidx = world.idx(nx, ny);
                if can_carry_snow_overlay(world.tiles[nidx].biome) {
                    sum += cover[nidx];
                    weight += 1.0;
                }
            }
            scratch[idx] = sum / weight;
        }

        std::mem::swap(&mut cover, &mut scratch);
    }

    cover
}

fn can_carry_snow_overlay(biome: Biome) -> bool {
    matches!(
        biome,
        Biome::Alpine | Biome::Foothills | Biome::Tundra | Biome::PolarDesert
    )
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
        Biome::Wetland => {
            let wet = moisture.clamp(0.45, 0.90);
            let dark = ((wet - 0.45) / 0.45 * 10.0) as i16;
            Rgba([
                (base[0] as i16 - dark / 2).clamp(0, 255) as u8,
                (base[1] as i16 + dark / 2).clamp(0, 255) as u8,
                (base[2] as i16 - dark).clamp(0, 255) as u8,
                255,
            ])
        }
        Biome::Freshwater => base,
        _ => base,
    }
}

fn biome_color(biome: Biome) -> Rgba<u8> {
    match biome {
        Biome::Ocean => Rgba([38, 84, 148, 255]),
        Biome::Coast => Rgba([204, 198, 148, 255]),
        Biome::PolarDesert => Rgba([212, 220, 218, 255]),
        Biome::Tundra => Rgba([148, 168, 126, 255]),
        Biome::BorealForest => Rgba([64, 112, 68, 255]),
        Biome::TemperateGrassland => Rgba([158, 184, 90, 255]),
        Biome::TemperateForest => Rgba([80, 138, 76, 255]),
        Biome::Woodland => Rgba([106, 150, 78, 255]),
        Biome::Wetland => Rgba([92, 128, 78, 255]),
        Biome::Freshwater => Rgba([58, 128, 148, 255]),
        Biome::Foothills => Rgba([160, 148, 110, 255]),
        Biome::Steppe => Rgba([176, 168, 96, 255]),
        Biome::Desert => Rgba([218, 196, 126, 255]),
        Biome::Savanna => Rgba([186, 180, 76, 255]),
        Biome::TropicalForest => Rgba([56, 148, 70, 255]),
        Biome::Rainforest => Rgba([34, 112, 52, 255]),
        Biome::Alpine => Rgba([144, 146, 142, 255]),
    }
}
