use std::collections::VecDeque;

use noise::OpenSimplex;

use crate::{Surface, World, WorldConfig};

use super::util::{latitude_factor, octave_noise};

pub(super) fn populate_climate(
    world: &mut World,
    config: &WorldConfig,
    ocean: &[bool],
    distance_to_ocean: &[u16],
    climate: &OpenSimplex,
) {
    let nearby_water = compute_nearby_water(world);

    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            let elevation = world.tiles[idx].raw_elevation;
            let lat = latitude_factor(y, world.height);
            let climate_noise =
                octave_noise(climate, x as f64 * 0.008, y as f64 * 0.008, 3, 0.5, 2.0);
            let temperature =
                (1.0 - lat * 1.15 + climate_noise * 0.12 - elevation * 0.35 + config.temperature_bias)
                    .clamp(0.0, 1.0);
            let moisture = (moisture_value(
                world,
                ocean,
                distance_to_ocean,
                climate,
                &nearby_water,
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
    let zonal = (1.0 - (lat - 0.35).abs() * 1.4).clamp(0.0, 1.0);
    let noise = octave_noise(
        climate,
        x as f64 * 0.014 + 7.0,
        y as f64 * 0.014 - 9.0,
        4,
        0.55,
        2.0,
    );

    (ocean_influence * 0.44
        + zonal * 0.20
        + noise * 0.17
        + rain_shadow * 0.11
        + nearby_water[idx] * 0.18)
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
