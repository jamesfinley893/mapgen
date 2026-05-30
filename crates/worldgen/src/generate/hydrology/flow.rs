use std::collections::{HashMap, VecDeque};

use crate::{World, WorldConfig};

use super::routing::local_relief;
use super::{ConditioningState, ValleyErosion};
use crate::generate::util::{neighbor_distance, smoothstep};

// Topological sort of the routing DAG using Kahn's algorithm.
//
// Sorting by elevation alone is incorrect when flat-region routing chooses a
// neighbor that is within HYDRO_EPSILON but technically slightly higher: the
// downstream tile would be processed before its upstream contributor, producing
// non-monotone discharge. A true topological sort on the downstream pointers
// guarantees each tile is processed after all its upstream contributors.
pub(super) fn flow_accumulation_order(
    conditioning: &ConditioningState,
    downstream: &[Option<usize>],
    ocean: &[bool],
) -> Vec<usize> {
    let n = conditioning.hydro_elevation.len();

    // In-degree: how many non-ocean upstream tiles route into each tile.
    let mut in_degree = vec![0_u32; n];
    for (idx, &next_opt) in downstream.iter().enumerate() {
        if ocean[idx] {
            continue;
        }
        if let Some(next) = next_opt {
            if !ocean[next] {
                in_degree[next] += 1;
            }
        }
    }

    // Seed queue with all source tiles (no non-ocean tile routes into them).
    // Sort deterministically: highest elevation / highest rank (most upstream) first.
    let mut sources: Vec<usize> = (0..n).filter(|&i| !ocean[i] && in_degree[i] == 0).collect();
    sources.sort_by(|a, b| {
        conditioning.hydro_elevation[*b]
            .total_cmp(&conditioning.hydro_elevation[*a])
            .then_with(|| conditioning.rank[*b].cmp(&conditioning.rank[*a]))
    });

    let mut order = Vec::with_capacity(n);
    let mut queue: VecDeque<usize> = sources.into_iter().collect();

    while let Some(idx) = queue.pop_front() {
        order.push(idx);
        if let Some(next) = downstream[idx] {
            if !ocean[next] {
                in_degree[next] -= 1;
                if in_degree[next] == 0 {
                    queue.push_back(next);
                }
            }
        }
    }

    // Safety: include any tiles left over from residual cycles (shouldn't
    // occur after break_downstream_cycles, but prevents silent data loss).
    if order.len() < n {
        let mut visited = vec![false; n];
        for &i in &order {
            visited[i] = true;
        }
        for i in 0..n {
            if !ocean[i] && !visited[i] {
                order.push(i);
            }
        }
    }

    order
}

pub(super) fn accumulate_contributing_area(
    accumulation_order: &[usize],
    downstream: &[Option<usize>],
    ocean: &[bool],
) -> Vec<f32> {
    let mut contributing_area = vec![0.0; downstream.len()];

    for &idx in accumulation_order {
        if ocean[idx] {
            continue;
        }
        contributing_area[idx] += 1.0;
        if let Some(next) = downstream[idx] {
            contributing_area[next] += contributing_area[idx];
        }
    }
    contributing_area
}

pub(super) fn compute_runoff(
    world: &World,
    config: &WorldConfig,
    ocean: &[bool],
    conditioning: &ConditioningState,
    downstream: &[Option<usize>],
    valleys: &ValleyErosion,
) -> Vec<f32> {
    let mut runoff = vec![0.0_f32; world.tiles.len()];
    for idx in 0..world.tiles.len() {
        if ocean[idx] {
            continue;
        }
        let tile = &world.tiles[idx];
        let height_above_sea = (tile.raw_elevation - world.sea_level).max(0.0);
        let slope = downstream[idx]
            .map(|next| {
                let (x, y) = world.coords(idx);
                let (nx, ny) = world.coords(next);
                (conditioning.hydro_elevation[idx] - conditioning.hydro_elevation[next]).max(0.0)
                    / neighbor_distance(x, y, nx, ny)
            })
            .unwrap_or(0.0);
        let relief = local_relief(world, idx, 2);
        let precipitation = tile.precipitation.clamp(0.0, 1.0);
        let aridity = 1.0 - precipitation;
        let lowland_storage = (1.0 - smoothstep(0.0, 0.18, height_above_sea)) * 0.28;
        let slope_runoff = smoothstep(0.004, 0.055, slope) * 0.30;
        let relief_runoff = smoothstep(0.012, 0.08, relief) * 0.20;
        let convergence = topographic_convergence(world, idx);
        let valley_memory = valleys.activity(idx);
        let valley_depth = valleys.valley_depth(idx);
        let valley_width = (valleys.valley_width(idx) / 18.0).clamp(0.0, 1.0);
        let trunk_memory = valleys.trunk_strength(idx);
        let tributary_memory = valleys.tributary_strength(idx);
        let discharge_memory = smoothstep(
            8.0,
            (world.effective_world_size() * world.effective_world_size() * 0.025).max(64.0),
            valleys.long_term_discharge(idx),
        );
        let valley_capture = convergence
            * smoothstep(
                world.sea_level + 0.08,
                world.sea_level + 0.62,
                tile.raw_elevation,
            )
            * (0.55 + relief * 2.2).clamp(0.55, 1.45)
            + valley_memory * (0.34 + relief * 1.3).clamp(0.34, 0.92)
            + valley_depth * 2.6
            + valley_width * 0.20
            + trunk_memory * 0.22
            + tributary_memory * 0.08
            + discharge_memory * valley_memory * 0.16;
        let cold_rock = smoothstep(
            world.sea_level + 0.24,
            world.sea_level + 0.52,
            tile.raw_elevation,
        ) * (1.0 - tile.temperature)
            * 0.18;
        let infiltration = (0.18 + lowland_storage + aridity * 0.35
            - valley_capture * 0.20
            - valley_memory * 0.12
            - valley_depth * 0.55
            - trunk_memory * 0.08)
            .clamp(0.0, 0.72);
        let runoff_coeff = (0.18
            + precipitation * 0.46
            + slope_runoff
            + relief_runoff
            + cold_rock
            + valley_capture * 0.55
            + valley_memory * 0.18
            + valley_depth * 0.65
            + valley_width * 0.08
            + trunk_memory * 0.12
            - infiltration * 0.38)
            .clamp(0.04, 1.85);
        runoff[idx] = (precipitation.powf(1.30) * runoff_coeff * config.runoff_scale).max(0.0);
    }
    runoff
}

fn topographic_convergence(world: &World, idx: usize) -> f32 {
    let (x, y) = world.coords(idx);
    let current = world.tiles[idx].raw_elevation;
    let mut weighted_rise = 0.0_f32;
    let mut weighted_drop = 0.0_f32;
    let mut weight_sum = 0.0_f32;

    for (nx, ny) in world.neighbors8(x, y) {
        let nidx = world.idx(nx, ny);
        let distance = neighbor_distance(x, y, nx, ny);
        let weight = 1.0 / distance;
        let delta = world.tiles[nidx].raw_elevation - current;
        if delta > 0.0 {
            weighted_rise += delta * weight;
        } else {
            weighted_drop += -delta * weight;
        }
        weight_sum += weight;
    }

    if weight_sum <= f32::EPSILON {
        return 0.0;
    }

    let avg_rise = weighted_rise / weight_sum;
    let avg_drop = weighted_drop / weight_sum;
    let concavity = smoothstep(0.006, 0.055, avg_rise - avg_drop * 0.45);
    let enclosed = smoothstep(
        0.18,
        0.62,
        weighted_rise / (weighted_rise + weighted_drop + 0.0001),
    );
    (concavity * enclosed).clamp(0.0, 1.0)
}

pub(super) fn accumulate_discharge(
    accumulation_order: &[usize],
    downstream: &[Option<usize>],
    ocean: &[bool],
    runoff: &[f32],
) -> Vec<f32> {
    let mut discharge = vec![0.0; downstream.len()];

    for &idx in accumulation_order {
        if ocean[idx] {
            continue;
        }
        discharge[idx] += runoff[idx];
        if let Some(next) = downstream[idx] {
            discharge[next] += discharge[idx];
        }
    }
    discharge
}

pub(super) fn compute_stream_power(
    world: &World,
    conditioning: &ConditioningState,
    downstream: &[Option<usize>],
    discharge: &[f32],
) -> Vec<f32> {
    let mut power = vec![0.0_f32; world.tiles.len()];
    for idx in 0..world.tiles.len() {
        let Some(next) = downstream[idx] else {
            continue;
        };
        let (x, y) = world.coords(idx);
        let (nx, ny) = world.coords(next);
        let slope = (conditioning.hydro_elevation[idx] - conditioning.hydro_elevation[next])
            .max(0.0)
            / neighbor_distance(x, y, nx, ny);
        let effective_slope = (slope + 0.0035).powf(0.70);
        power[idx] = discharge[idx].max(0.0).powf(0.88) * effective_slope;
    }
    power
}

pub(super) fn assign_basin_ids(
    world: &World,
    ocean: &[bool],
    downstream: &[Option<usize>],
    lake_id: &[Option<u32>],
    basin_offset: u32,
) -> Vec<Option<u32>> {
    let mut basin_id = vec![None; world.tiles.len()];
    let mut mouth_to_basin = HashMap::<usize, u32>::new();
    let mut next_basin = 0_u32;

    for start in 0..world.tiles.len() {
        if ocean[start] || basin_id[start].is_some() {
            continue;
        }

        // Walk downstream, collecting path tiles. When we hit a cached result or a
        // terminal (lake, ocean mouth, or sink), backfill the whole path at once.
        // Each tile is backfilled at most once, so the total cost is O(n) amortised.
        let mut path: Vec<usize> = Vec::new();
        let mut current = start;
        let mut resolved: Option<u32> = None;

        loop {
            if path.len() >= world.tiles.len() || ocean[current] {
                break;
            }
            if let Some(b) = basin_id[current] {
                resolved = Some(b);
                break;
            }
            path.push(current);
            if let Some(lake) = lake_id[current] {
                resolved = Some(lake);
                break;
            }
            match downstream[current] {
                Some(next) if ocean[next] => {
                    let basin = *mouth_to_basin.entry(current).or_insert_with(|| {
                        let id = basin_offset + next_basin;
                        next_basin += 1;
                        id
                    });
                    resolved = Some(basin);
                    break;
                }
                Some(next) => current = next,
                None => break,
            }
        }

        if let Some(b) = resolved {
            for &p in &path {
                basin_id[p] = Some(b);
            }
        }
    }

    basin_id
}
