use std::collections::VecDeque;

use noise::OpenSimplex;

use crate::{Surface, World, WorldConfig};

use super::util::{latitude_factor, octave_noise, smoothstep};

pub(super) fn populate_climate(
    world: &mut World,
    config: &WorldConfig,
    ocean: &[bool],
    distance_to_ocean: &[u16],
    climate: &OpenSimplex,
) {
    let nearby_water = compute_nearby_water(world);
    let regional_continentality = compute_regional_continentality(world, ocean);

    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            let elevation = world.tiles[idx].raw_elevation;
            let lat = latitude_factor(y, world.height);
            let climate_noise =
                octave_noise(climate, x as f64 * 0.008, y as f64 * 0.008, 3, 0.5, 2.0);
            let seasonal_noise =
                octave_noise(climate, x as f64 * 0.004 - 19.0, y as f64 * 0.004 + 31.0, 2, 0.5, 2.0);
            let lowland = 1.0 - ((elevation - world.sea_level) / 0.24).clamp(0.0, 1.0);
            let equatorial_warmth = (1.0 - lat.powf(1.15)).clamp(0.0, 1.0);
            let subtropical_cooling =
                smoothstep(0.16, 0.34, lat) * (1.0_f32 - smoothstep(0.46, 0.68, lat));
            let maritime_temp = nearby_water[idx] * 0.05 + (1.0 - regional_continentality[idx]) * 0.05;
            let temperature = (equatorial_warmth * 0.88
                - subtropical_cooling * 0.04
                + climate_noise * 0.09
                + seasonal_noise * 0.06
                + maritime_temp
                - elevation * 0.38
                - regional_continentality[idx] * 0.08 * lowland
                + config.temperature_bias)
                .clamp(0.0, 1.0);
            let moisture = (moisture_value(
                world,
                ocean,
                distance_to_ocean,
                climate,
                &nearby_water,
                &regional_continentality,
                x,
                y,
            ) + config.moisture_bias)
                .clamp(0.0, 1.0);
            world.tiles[idx].temperature = temperature;
            world.tiles[idx].moisture = moisture;
        }
    }
}

fn compute_nearby_water(world: &World) -> Vec<f32> {
    let mut nearby = vec![0.0_f32; world.tiles.len()];
    for idx in 0..world.tiles.len() {
        if matches!(
            world.tiles[idx].surface,
            Surface::Ocean | Surface::Lake | Surface::River
        ) {
            nearby[idx] = 1.0;
            let (x, y) = world.coords(idx);
            for (nx, ny) in world.neighbors8(x, y) {
                let nidx = world.idx(nx, ny);
                nearby[nidx] = nearby[nidx].max(0.35);
            }
        }
    }
    nearby
}

fn compute_regional_continentality(world: &World, ocean: &[bool]) -> Vec<f32> {
    let mut field = vec![0.0_f32; world.tiles.len()];
    let max_extent = (world.width.max(world.height) as f32 * 0.36).max(1.0);
    for idx in 0..world.tiles.len() {
        if ocean[idx] {
            continue;
        }
        let (x, y) = world.coords(idx);
        let mut ocean_hits = 0.0_f32;
        let mut weighted_distance = 0.0_f32;
        for (dx, dy, weight) in [
            (-1_isize, 0_isize, 1.0_f32),
            (1, 0, 1.0),
            (0, -1, 0.85),
            (0, 1, 0.85),
        ] {
            for step in 1..=18 {
                let nx = x as isize + dx * step;
                let ny = y as isize + dy * step;
                if !world.in_bounds(nx, ny) {
                    break;
                }
                let nidx = world.idx(nx as usize, ny as usize);
                if ocean[nidx] {
                    ocean_hits += weight;
                    weighted_distance += (step as f32 / 18.0) * weight;
                    break;
                }
            }
        }
        let openness = (ocean_hits / 3.7).clamp(0.0, 1.0);
        let mean_fetch = if ocean_hits > f32::EPSILON {
            (weighted_distance / ocean_hits).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let distance_bias = (((x.min(world.width - 1 - x) + y.min(world.height - 1 - y)) as f32) / max_extent)
            .clamp(0.0, 1.0);
        field[idx] = (distance_bias * 0.5 + mean_fetch * 0.35 + (1.0 - openness) * 0.25).clamp(0.0, 1.0);
    }
    field
}

pub(super) fn fill_ocean_distance(world: &World, ocean: &[bool]) -> Vec<u16> {
    let mut out = vec![u16::MAX; world.tiles.len()];
    let mut queue = VecDeque::new();
    for (idx, is_ocean) in ocean.iter().enumerate() {
        if *is_ocean {
            out[idx] = 0;
            queue.push_back(idx);
        }
    }
    while let Some(idx) = queue.pop_front() {
        let (x, y) = world.coords(idx);
        let next_dist = out[idx].saturating_add(1);
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            if out[nidx] > next_dist {
                out[nidx] = next_dist;
                queue.push_back(nidx);
            }
        }
    }
    out
}

fn moisture_value(
    world: &World,
    ocean: &[bool],
    distance_to_ocean: &[u16],
    climate: &OpenSimplex,
    nearby_water: &[f32],
    regional_continentality: &[f32],
    x: usize,
    y: usize,
) -> f32 {
    let idx = world.idx(x, y);
    if ocean[idx] {
        return 1.0;
    }

    let ocean_influence =
        1.0 - (distance_to_ocean[idx] as f32 / (world.width.max(world.height) as f32 * 0.45))
            .clamp(0.0, 1.0);
    let rain_shadow = rain_shadow(world, ocean, x, y);
    let lat = latitude_factor(y, world.height);
    let subtropical_dryness =
        smoothstep(0.14, 0.28, lat) * (1.0_f32 - smoothstep(0.42, 0.62, lat));
    let equatorial_wetness = (1.0_f32 - smoothstep(0.0, 0.34, lat)).clamp(0.0, 1.0);
    let polar_dryness = smoothstep(0.68, 0.92, lat);
    let zonal = (0.22 + equatorial_wetness * 0.32 - subtropical_dryness * 0.22 - polar_dryness * 0.08)
        .clamp(0.0, 1.0);
    let noise = octave_noise(
        climate,
        x as f64 * 0.014 + 7.0,
        y as f64 * 0.014 - 9.0,
        4,
        0.55,
        2.0,
    );
    let monsoon = octave_noise(
        climate,
        x as f64 * 0.006 - 41.0,
        y as f64 * 0.006 + 17.0,
        3,
        0.55,
        2.0,
    );
    let continentality = regional_continentality[idx];
    let lowland = 1.0 - ((world.tiles[idx].raw_elevation - world.sea_level) / 0.24).clamp(0.0, 1.0);

    (ocean_influence * 0.36
        + zonal * 0.26
        + noise * 0.14
        + monsoon * 0.08 * equatorial_wetness
        + rain_shadow * 0.12
        + nearby_water[idx] * 0.16
        - continentality * 0.22 * (0.7 + subtropical_dryness * 0.45) * lowland)
        .clamp(0.0, 1.0)
}

fn rain_shadow(world: &World, ocean: &[bool], x: usize, y: usize) -> f32 {
    let mut moisture = 0.0_f32;
    let mut height_barrier = 0.0_f32;

    for step in 1..=12 {
        let nx = x.saturating_sub(step);
        let idx = world.idx(nx, y);
        if ocean[idx] {
            moisture += 0.08;
            break;
        }
        height_barrier += (world.tiles[idx].raw_elevation - world.sea_level).max(0.0) * 0.12;
    }

    for step in 1..=8 {
        let nx = (x + step).min(world.width - 1);
        let idx = world.idx(nx, y);
        if ocean[idx] {
            moisture += 0.04;
            break;
        }
    }

    (moisture - height_barrier).clamp(0.0, 1.0)
}
