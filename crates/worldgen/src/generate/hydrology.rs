use crate::{Surface, World};

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use super::climate::ClimateFields;
use super::terrain::TerrainFields;
use super::util::{hash01, smoothstep};

pub(crate) struct HydrologyFields {
    pub(crate) runoff: Vec<f32>,
    pub(crate) flow_accumulation: Vec<f32>,
    pub(crate) river: Vec<f32>,
    pub(crate) flow_direction: Vec<i8>,
}

#[derive(Clone, Copy)]
enum FlowTarget {
    Land { idx: usize, direction: i8 },
    Ocean { direction: i8 },
    Sink,
}

#[derive(Clone, Copy)]
struct QueueCell {
    elevation: f32,
    idx: usize,
}

pub(super) fn generate_hydrology_fields(
    world: &World,
    terrain: &TerrainFields,
    climate: &ClimateFields,
    ocean: &[bool],
    surfaces: &[Surface],
) -> HydrologyFields {
    let runoff = compute_runoff(world, terrain, climate, ocean);
    let filled_elevation = compute_filled_elevation(world, terrain, ocean);
    let flow_targets = compute_flow_targets(world, ocean, &filled_elevation);
    let elevation_order = elevation_order(world, &filled_elevation);
    let flow_accumulation = accumulate_flow(ocean, &elevation_order, &runoff, &flow_targets);
    let drains_to_ocean = compute_ocean_drainage(world, &elevation_order, &flow_targets);
    let river = classify_rivers(world, surfaces, &flow_accumulation, &drains_to_ocean);
    let flow_direction = compute_flow_directions(&flow_targets);

    HydrologyFields {
        runoff,
        flow_accumulation,
        river,
        flow_direction,
    }
}

fn compute_runoff(
    world: &World,
    terrain: &TerrainFields,
    climate: &ClimateFields,
    ocean: &[bool],
) -> Vec<f32> {
    let mut runoff = vec![0.0_f32; world.tile_count()];

    for idx in 0..world.tile_count() {
        if ocean[idx] {
            continue;
        }

        let height_above_sea = (terrain.elevation[idx] - world.sea_level).max(0.0);
        let slope_wash = smoothstep(0.012, 0.075, terrain.slope[idx]);
        let relief_wash = smoothstep(0.018, 0.095, terrain.relief[idx]);
        let saturation =
            (climate.moisture[idx] * 0.52 + climate.precipitation[idx] * 0.48).clamp(0.0, 1.0);
        let cold_storage = if climate.temperature[idx] < 0.26 && height_above_sea > 0.18 {
            smoothstep(0.18, 0.62, height_above_sea) * (0.26 - climate.temperature[idx]) * 0.55
        } else {
            0.0
        };
        let runoff_efficiency =
            0.14 + saturation * 0.52 + slope_wash * 0.18 + relief_wash * 0.11 + cold_storage;

        runoff[idx] = (climate.precipitation[idx] * runoff_efficiency).clamp(0.0, 1.0);
    }

    runoff
}

fn compute_filled_elevation(world: &World, terrain: &TerrainFields, ocean: &[bool]) -> Vec<f32> {
    let mut filled = terrain.elevation.clone();
    let mut visited = vec![false; world.tile_count()];
    let mut queue = BinaryHeap::new();

    for (idx, is_ocean) in ocean.iter().copied().enumerate() {
        if !is_ocean {
            continue;
        }
        visited[idx] = true;
        filled[idx] = world.sea_level.min(terrain.elevation[idx]);
        queue.push(QueueCell {
            elevation: filled[idx],
            idx,
        });
    }

    if queue.is_empty() {
        return filled;
    }

    while let Some(cell) = queue.pop() {
        let (x, y) = world.coords(cell.idx);
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            if visited[nidx] {
                continue;
            }
            visited[nidx] = true;

            let spill = cell.elevation + 0.0001;
            filled[nidx] = if ocean[nidx] {
                world.sea_level.min(terrain.elevation[nidx])
            } else {
                terrain.elevation[nidx].max(spill)
            };
            queue.push(QueueCell {
                elevation: filled[nidx],
                idx: nidx,
            });
        }
    }

    filled
}

fn compute_flow_targets(
    world: &World,
    ocean: &[bool],
    filled_elevation: &[f32],
) -> Vec<FlowTarget> {
    let mut targets = vec![FlowTarget::Sink; world.tile_count()];

    for idx in 0..world.tile_count() {
        if ocean[idx] {
            continue;
        }

        let current = filled_elevation[idx];
        let (x, y) = world.coords(idx);
        let mut best = FlowTarget::Sink;
        let mut best_score = 0.0_f32;

        for (dx, dy, direction) in FLOW_NEIGHBORS {
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if !world.in_bounds(nx, ny) {
                continue;
            }

            let nidx = world.idx(nx as usize, ny as usize);
            let distance = if dx != 0 && dy != 0 {
                std::f32::consts::SQRT_2
            } else {
                1.0
            };

            if ocean[nidx] {
                let drop = (current - world.sea_level).max(0.0) + 0.006;
                let score = drop / distance;
                if score > best_score {
                    best_score = score;
                    best = FlowTarget::Ocean { direction };
                }
                continue;
            }

            let neighbor = filled_elevation[nidx];
            let drop = current - neighbor;
            if drop <= 0.00001 {
                continue;
            }

            let gradient = drop / distance;
            let low_gradient = 1.0 - smoothstep(0.00018, 0.006, gradient);
            let route_bias = hash01(world.seed ^ 0xA5E1_F10D_7723_4B91, idx, nidx) * 0.00014;
            let score = gradient + route_bias * low_gradient;
            if score > best_score {
                best_score = score;
                best = FlowTarget::Land {
                    idx: nidx,
                    direction,
                };
            }
        }

        targets[idx] = best;
    }

    targets
}

fn accumulate_flow(
    ocean: &[bool],
    elevation_order: &[usize],
    runoff: &[f32],
    targets: &[FlowTarget],
) -> Vec<f32> {
    let mut flow = runoff.to_vec();
    for idx in elevation_order.iter().rev().copied() {
        if ocean[idx] {
            continue;
        }
        if let FlowTarget::Land { idx: target, .. } = targets[idx] {
            flow[target] += flow[idx];
        }
    }

    for (idx, is_ocean) in ocean.iter().copied().enumerate() {
        if is_ocean {
            flow[idx] = 0.0;
        }
    }

    flow
}

fn compute_ocean_drainage(
    world: &World,
    elevation_order: &[usize],
    targets: &[FlowTarget],
) -> Vec<bool> {
    let mut drains_to_ocean = vec![false; world.tile_count()];

    for idx in elevation_order.iter().copied() {
        drains_to_ocean[idx] = match targets[idx] {
            FlowTarget::Ocean { .. } => true,
            FlowTarget::Land { idx: target, .. } => drains_to_ocean[target],
            FlowTarget::Sink => false,
        };
    }

    drains_to_ocean
}

fn classify_rivers(
    world: &World,
    surfaces: &[Surface],
    flow_accumulation: &[f32],
    drains_to_ocean: &[bool],
) -> Vec<f32> {
    let mut river = vec![0.0_f32; world.tile_count()];
    let river_start = (world.tile_count() as f32).sqrt() * 0.24;
    let bankfull = river_start * 8.0;

    for idx in 0..world.tile_count() {
        if surfaces[idx] == Surface::Ocean || !drains_to_ocean[idx] {
            continue;
        }

        river[idx] = smoothstep(river_start, bankfull, flow_accumulation[idx]);
    }

    river
}

fn compute_flow_directions(targets: &[FlowTarget]) -> Vec<i8> {
    targets
        .iter()
        .map(|target| match *target {
            FlowTarget::Land { direction, .. } | FlowTarget::Ocean { direction, .. } => direction,
            FlowTarget::Sink => -1,
        })
        .collect()
}

fn elevation_order(world: &World, elevation: &[f32]) -> Vec<usize> {
    let mut order = (0..world.tile_count()).collect::<Vec<_>>();
    order.sort_unstable_by(|a, b| elevation[*a].total_cmp(&elevation[*b]));
    order
}

const FLOW_NEIGHBORS: [(isize, isize, i8); 8] = [
    (-1, -1, 7),
    (0, -1, 0),
    (1, -1, 1),
    (-1, 0, 6),
    (1, 0, 2),
    (-1, 1, 5),
    (0, 1, 4),
    (1, 1, 3),
];

impl Eq for QueueCell {}

impl PartialEq for QueueCell {
    fn eq(&self, other: &Self) -> bool {
        self.idx == other.idx && self.elevation.to_bits() == other.elevation.to_bits()
    }
}

impl Ord for QueueCell {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .elevation
            .total_cmp(&self.elevation)
            .then_with(|| other.idx.cmp(&self.idx))
    }
}

impl PartialOrd for QueueCell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::World;

    #[test]
    fn flow_targets_store_direction_for_land_and_ocean_outlets() {
        let world = World::new(11, 3, 3, 0.50, 0);
        let center = world.idx(1, 1);
        let east = world.idx(2, 1);
        let west = world.idx(0, 1);
        let mut elevation = vec![0.90_f32; world.tile_count()];
        elevation[center] = 0.82;
        elevation[east] = 0.30;
        elevation[west] = 0.70;

        let land_ocean = vec![false; world.tile_count()];
        let land_targets = compute_flow_targets(&world, &land_ocean, &elevation);
        let land_directions = compute_flow_directions(&land_targets);
        assert!(matches!(
            land_targets[center],
            FlowTarget::Land {
                idx,
                direction: 2
            } if idx == east
        ));
        assert_eq!(land_directions[center], 2);

        let mut ocean = vec![false; world.tile_count()];
        ocean[west] = true;
        let mut outlet_elevation = elevation;
        outlet_elevation[east] = 0.78;
        let ocean_targets = compute_flow_targets(&world, &ocean, &outlet_elevation);
        let ocean_directions = compute_flow_directions(&ocean_targets);
        assert!(matches!(
            ocean_targets[center],
            FlowTarget::Ocean { direction: 6 }
        ));
        assert_eq!(ocean_directions[center], 6);
    }
}
