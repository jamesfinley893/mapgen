use std::collections::VecDeque;

use crate::World;
use crate::generate::util::smoothstep;

const MARITIME_EXTENT_WORLD_FRACTION: f32 = 0.45;
const CONTINENTAL_FETCH_WORLD_FRACTION: f32 = 0.34;
const CONTINENTAL_FETCH_MAP_FRACTION: f32 = 0.45;
const MAX_CONTINENTAL_FETCH_STEPS: usize = 192;

pub(super) struct ClimateFields {
    pub(super) ocean: Vec<bool>,
    pub(super) distance_to_ocean: Vec<u16>,
    pub(super) maritime_influence: Vec<f32>,
    pub(super) continentality: Vec<f32>,
}

impl ClimateFields {
    pub(super) fn sample(world: &World, ocean: &[bool]) -> Self {
        let distance_to_ocean = fill_ocean_distance(world, ocean);
        let maritime_influence = compute_maritime_influence(world, &distance_to_ocean);
        let continentality = compute_continentality(world, ocean, &maritime_influence);

        Self {
            ocean: ocean.to_vec(),
            distance_to_ocean,
            maritime_influence,
            continentality,
        }
    }
}

fn compute_maritime_influence(world: &World, distance_to_ocean: &[u16]) -> Vec<f32> {
    let maritime_extent = maritime_extent(world);
    distance_to_ocean
        .iter()
        .map(|distance| {
            if *distance == u16::MAX {
                0.0
            } else {
                (1.0 - smoothstep(0.0, maritime_extent, *distance as f32)).clamp(0.0, 1.0)
            }
        })
        .collect()
}

fn compute_continentality(world: &World, ocean: &[bool], maritime_influence: &[f32]) -> Vec<f32> {
    let mut field = vec![0.0_f32; world.tile_count()];
    let fetch_steps = continental_fetch_steps(world);

    for idx in 0..world.tiles.len() {
        if ocean[idx] {
            continue;
        }

        let (x, y) = world.coords(idx);
        let ocean_openness = directional_ocean_openness(world, ocean, x, y, fetch_steps);
        let distance_continentality = 1.0 - maritime_influence[idx];
        let fetch_continentality = 1.0 - ocean_openness;
        field[idx] = (distance_continentality * 0.68 + fetch_continentality * 0.32).clamp(0.0, 1.0);
    }

    field
}

fn directional_ocean_openness(
    world: &World,
    ocean: &[bool],
    x: usize,
    y: usize,
    fetch_steps: usize,
) -> f32 {
    let mut openness = 0.0_f32;
    let mut total_weight = 0.0_f32;

    for (dx, dy, weight) in [
        (-1_isize, 0_isize, 1.0_f32),
        (1, 0, 1.0),
        (0, -1, 0.9),
        (0, 1, 0.9),
        (-1, -1, 0.7),
        (1, -1, 0.7),
        (-1, 1, 0.7),
        (1, 1, 0.7),
    ] {
        total_weight += weight;
        for step in 1..=fetch_steps {
            let nx = x as isize + dx * step as isize;
            let ny = y as isize + dy * step as isize;
            if !world.in_bounds(nx, ny) {
                break;
            }
            let nidx = world.idx(nx as usize, ny as usize);
            if ocean[nidx] {
                let proximity = 1.0 - smoothstep(0.0, 1.0, step as f32 / fetch_steps as f32);
                openness += proximity * weight;
                break;
            }
        }
    }

    if total_weight <= f32::EPSILON {
        0.0
    } else {
        (openness / total_weight).clamp(0.0, 1.0)
    }
}

fn maritime_extent(world: &World) -> f32 {
    let world_extent = world.effective_world_size() * MARITIME_EXTENT_WORLD_FRACTION;
    let map_extent = world.width.max(world.height) as f32 * 0.55;
    world_extent.min(map_extent).max(1.0)
}

fn continental_fetch_steps(world: &World) -> usize {
    scaled_scan_steps(
        world,
        CONTINENTAL_FETCH_WORLD_FRACTION,
        CONTINENTAL_FETCH_MAP_FRACTION,
        8,
        MAX_CONTINENTAL_FETCH_STEPS,
    )
}

fn scaled_scan_steps(
    world: &World,
    world_fraction: f32,
    map_fraction: f32,
    min_steps: usize,
    max_steps: usize,
) -> usize {
    let max_available = world.width.max(world.height).saturating_sub(1).max(1);
    let min_steps = min_steps.min(max_available);
    let max_steps = max_steps.min(max_available).max(min_steps);
    let world_steps = (world.effective_world_size() * world_fraction).round() as usize;
    let map_steps = (world.width.max(world.height) as f32 * map_fraction).round() as usize;
    world_steps
        .min(map_steps)
        .max(1)
        .clamp(min_steps, max_steps)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn coastal_mask(world: &World) -> Vec<bool> {
        let mut ocean = vec![false; world.tile_count()];
        for y in 0..world.height {
            ocean[world.idx(0, y)] = true;
        }
        ocean
    }

    #[test]
    fn ocean_distance_and_maritime_fields_are_bounded() {
        let world = World::new(1, 32, 32, 0.5, 0);
        let ocean = coastal_mask(&world);
        let fields = ClimateFields::sample(&world, &ocean);

        for idx in 0..world.tile_count() {
            assert!(fields.maritime_influence[idx].is_finite());
            assert!((0.0..=1.0).contains(&fields.maritime_influence[idx]));
            assert!(fields.continentality[idx].is_finite());
            assert!((0.0..=1.0).contains(&fields.continentality[idx]));
            if ocean[idx] {
                assert_eq!(fields.distance_to_ocean[idx], 0);
            } else {
                assert!(fields.distance_to_ocean[idx] > 0);
            }
        }

        let coastal_land = world.idx(1, world.height / 2);
        let inland = world.idx(world.width - 1, world.height / 2);
        assert!(fields.maritime_influence[coastal_land] > fields.maritime_influence[inland]);
        assert!(fields.continentality[inland] > fields.continentality[coastal_land]);
    }

    #[test]
    fn continental_fetch_scales_with_effective_world_size() {
        let near_world = World::new(1, 64, 32, 0.5, 32);
        let far_world = World::new(1, 64, 32, 0.5, 128);
        let near_fields = ClimateFields::sample(&near_world, &coastal_mask(&near_world));
        let far_fields = ClimateFields::sample(&far_world, &coastal_mask(&far_world));
        let sample = near_world.idx(20, near_world.height / 2);

        assert!(far_fields.maritime_influence[sample] > near_fields.maritime_influence[sample]);
        assert!(near_fields.continentality[sample] > far_fields.continentality[sample]);
    }
}
