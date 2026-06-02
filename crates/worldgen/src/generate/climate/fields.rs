use std::collections::VecDeque;

use crate::World;

pub(super) struct ClimateFields {
    pub(super) ocean: Vec<bool>,
    pub(super) distance_to_ocean: Vec<u16>,
    pub(super) nearby_water: Vec<f32>,
    pub(super) regional_continentality: Vec<f32>,
}

impl ClimateFields {
    pub(super) fn sample(world: &World, ocean: &[bool]) -> Self {
        Self {
            ocean: ocean.to_vec(),
            distance_to_ocean: fill_ocean_distance(world, ocean),
            nearby_water: compute_nearby_water(world, ocean),
            regional_continentality: compute_regional_continentality(world, ocean),
        }
    }
}

fn compute_nearby_water(world: &World, ocean: &[bool]) -> Vec<f32> {
    let mut nearby = vec![0.0_f32; world.tile_count()];
    for idx in 0..world.tile_count() {
        if ocean[idx] {
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
    let mut field = vec![0.0_f32; world.tile_count()];
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
        let distance_bias = (((x.min(world.width - 1 - x) + y.min(world.height - 1 - y)) as f32)
            / max_extent)
            .clamp(0.0, 1.0);
        field[idx] =
            (distance_bias * 0.5 + mean_fetch * 0.35 + (1.0 - openness) * 0.25).clamp(0.0, 1.0);
    }
    field
}

fn fill_ocean_distance(world: &World, ocean: &[bool]) -> Vec<u16> {
    let mut out = vec![u16::MAX; world.tile_count()];
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
