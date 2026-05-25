use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, VecDeque};

use crate::{Surface, World};

use super::util::{
    direction_vector, hash01, local_aspect, local_aspect_on_values, neighbor_distance, normalize,
    sample_seed_field, smoothstep,
};
use super::HYDRO_EPSILON;

pub(super) struct HydrologyState {
    pub(super) hydro_elevation: Vec<f32>,
    pub(super) downstream: Vec<Option<usize>>,
    pub(super) contributing_area: Vec<f32>,
    pub(super) surfaces: Vec<Surface>,
    pub(super) lake_id: Vec<Option<u32>>,
    pub(super) water_level: Vec<Option<f32>>,
    pub(super) basin_id: Vec<Option<u32>>,
}

struct RiverThresholds {
    stream: f32,
    secondary: f32,
    trunk: f32,
}

#[derive(Clone, Copy, Debug)]
struct QueueCell {
    level: f32,
    idx: usize,
}

struct ConditioningState {
    hydro_elevation: Vec<f32>,
    fill_depth: Vec<f32>,
    parent: Vec<Option<usize>>,
    rank: Vec<usize>,
}

struct LakeData {
    lake_id: Vec<Option<u32>>,
    water_level: Vec<Option<f32>>,
    lake_count: u32,
}

#[derive(Clone, Copy)]
struct RoutingCandidate {
    next: usize,
    score: f32,
    slope: f32,
    direction: (isize, isize),
    mountain_front: f32,
}

impl PartialEq for QueueCell {
    fn eq(&self, other: &Self) -> bool {
        self.idx == other.idx && self.level.to_bits() == other.level.to_bits()
    }
}

impl Eq for QueueCell {}

impl Ord for QueueCell {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .level
            .total_cmp(&self.level)
            .then_with(|| other.idx.cmp(&self.idx))
    }
}

impl PartialOrd for QueueCell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub(super) fn classify_ocean(world: &World) -> Vec<bool> {
    let mut ocean = vec![false; world.tiles.len()];
    let mut queue = VecDeque::new();

    for x in 0..world.width {
        seed_ocean_boundary(world, &mut ocean, &mut queue, x, 0);
        seed_ocean_boundary(world, &mut ocean, &mut queue, x, world.height - 1);
    }
    for y in 0..world.height {
        seed_ocean_boundary(world, &mut ocean, &mut queue, 0, y);
        seed_ocean_boundary(world, &mut ocean, &mut queue, world.width - 1, y);
    }

    while let Some(idx) = queue.pop_front() {
        let (x, y) = world.coords(idx);
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            if ocean[nidx] || world.tiles[nidx].raw_elevation > world.sea_level {
                continue;
            }
            ocean[nidx] = true;
            queue.push_back(nidx);
        }
    }

    ocean
}

fn seed_ocean_boundary(
    world: &World,
    ocean: &mut [bool],
    queue: &mut VecDeque<usize>,
    x: usize,
    y: usize,
) {
    let idx = world.idx(x, y);
    if !ocean[idx] && world.tiles[idx].raw_elevation <= world.sea_level {
        ocean[idx] = true;
        queue.push_back(idx);
    }
}

pub(super) fn simulate_hydrology(world: &World, ocean: &[bool]) -> HydrologyState {
    let conditioning = condition_terrain(world, ocean);
    let provisional = identify_lakes(
        world,
        ocean,
        &conditioning.hydro_elevation,
        &conditioning.fill_depth,
        &conditioning.parent,
    );
    let mut downstream = build_downstream(world, ocean, &conditioning, &provisional.lake_id);
    break_downstream_cycles(&mut downstream, &conditioning.parent, ocean);
    let contributing_area = accumulate_contributing_area(&conditioning, &downstream, ocean);
    let basin_id = assign_basin_ids(
        world,
        ocean,
        &downstream,
        &provisional.lake_id,
        provisional.lake_count,
    );
    let surfaces = classify_surfaces(world, ocean, &contributing_area, &provisional.lake_id);

    HydrologyState {
        hydro_elevation: conditioning.hydro_elevation,
        downstream,
        contributing_area,
        surfaces,
        lake_id: provisional.lake_id,
        water_level: provisional.water_level,
        basin_id,
    }
}

fn condition_terrain(world: &World, ocean: &[bool]) -> ConditioningState {
    let count = world.tiles.len();
    let mut hydro = vec![0.0; count];
    let mut fill_depth = vec![0.0; count];
    let mut parent = vec![None; count];
    let mut rank = vec![usize::MAX; count];
    let mut visited = vec![false; count];
    let mut heap = BinaryHeap::new();
    let mut next_rank = 0_usize;

    for idx in 0..count {
        if ocean[idx] {
            visited[idx] = true;
            hydro[idx] = world.tiles[idx].raw_elevation;
            rank[idx] = next_rank;
            next_rank += 1;
            heap.push(QueueCell {
                level: hydro[idx],
                idx,
            });
        }
    }

    while let Some(cell) = heap.pop() {
        let (x, y) = world.coords(cell.idx);
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            if visited[nidx] {
                continue;
            }
            visited[nidx] = true;
            let raw = world.tiles[nidx].raw_elevation;
            let raised = raw.max(cell.level);
            hydro[nidx] = raised;
            fill_depth[nidx] = (raised - raw).max(0.0);
            parent[nidx] = Some(cell.idx);
            rank[nidx] = next_rank;
            next_rank += 1;
            heap.push(QueueCell {
                level: raised + HYDRO_EPSILON,
                idx: nidx,
            });
        }
    }

    ConditioningState {
        hydro_elevation: hydro,
        fill_depth,
        parent,
        rank,
    }
}

fn identify_lakes(
    world: &World,
    ocean: &[bool],
    hydro: &[f32],
    fill_depth: &[f32],
    parent: &[Option<usize>],
) -> LakeData {
    let mut lake_id = vec![None; world.tiles.len()];
    let mut water_level = vec![None; world.tiles.len()];
    let mut visited = vec![false; world.tiles.len()];
    let mut next_lake_id = 0_u32;
    let area_threshold = ((world.width * world.height) as f32 * 0.00075).ceil() as usize;
    let area_threshold = area_threshold.max(6);
    let volume_threshold = ((world.width * world.height) as f32 * 0.00011).max(0.06);
    let depth_threshold = 0.018;

    for idx in 0..world.tiles.len() {
        if visited[idx] || ocean[idx] || fill_depth[idx] <= depth_threshold {
            continue;
        }
        let mut region = Vec::new();
        let mut queue = VecDeque::from([idx]);
        visited[idx] = true;

        while let Some(current) = queue.pop_front() {
            region.push(current);
            let (x, y) = world.coords(current);
            for (nx, ny) in world.neighbors8(x, y) {
                let nidx = world.idx(nx, ny);
                if visited[nidx] || ocean[nidx] || fill_depth[nidx] <= depth_threshold {
                    continue;
                }
                if (hydro[nidx] - hydro[current]).abs() > 0.02 {
                    continue;
                }
                visited[nidx] = true;
                queue.push_back(nidx);
            }
        }

        let volume: f32 = region.iter().map(|&cell| fill_depth[cell]).sum();
        let avg_depth = volume / region.len() as f32;
        let max_depth = region
            .iter()
            .map(|&cell| fill_depth[cell])
            .fold(0.0_f32, f32::max);
        if region.len() < area_threshold && volume < volume_threshold {
            continue;
        }
        if avg_depth < 0.024 && max_depth < 0.05 {
            continue;
        }

        let mut in_region = vec![false; world.tiles.len()];
        for &cell in &region {
            in_region[cell] = true;
        }
        let mut outlet = None;
        let mut outlet_level = f32::MAX;
        for &cell in &region {
            if let Some(next) = parent[cell] {
                if !in_region[next] && hydro[cell] < outlet_level {
                    outlet_level = hydro[cell];
                    outlet = Some(next);
                }
            }
        }

        if outlet.is_none() {
            continue;
        }

        let refined_region = refine_lake_region(
            world,
            hydro,
            fill_depth,
            &region,
            outlet_level,
            max_depth,
        );
        if refined_region.len() < 4 {
            continue;
        }

        for &cell in &refined_region {
            lake_id[cell] = Some(next_lake_id);
            water_level[cell] = Some(hydro[cell]);
        }
        next_lake_id += 1;
    }

    LakeData {
        lake_id,
        water_level,
        lake_count: next_lake_id,
    }
}

fn refine_lake_region(
    world: &World,
    hydro: &[f32],
    fill_depth: &[f32],
    region: &[usize],
    outlet_level: f32,
    max_depth: f32,
) -> Vec<usize> {
    let mut mask = vec![false; world.tiles.len()];
    for &cell in region {
        mask[cell] = true;
    }

    for _ in 0..2 {
        let mut remove = Vec::new();
        for &cell in region {
            if !mask[cell] {
                continue;
            }
            let (x, y) = world.coords(cell);
            let neighbors = world
                .neighbors8(x, y)
                .filter(|(nx, ny)| mask[world.idx(*nx, *ny)])
                .count();
            let shallow = fill_depth[cell] < (0.012_f32).max(max_depth * 0.22);
            if neighbors <= 1 || (neighbors <= 2 && shallow) {
                remove.push(cell);
            }
        }
        if remove.is_empty() {
            break;
        }
        for cell in remove {
            mask[cell] = false;
        }
    }

    let mut add = Vec::new();
    for &cell in region {
        if !mask[cell] {
            continue;
        }
        let (x, y) = world.coords(cell);
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            if mask[nidx] {
                continue;
            }
            let ring_neighbors = world
                .neighbors8(nx, ny)
                .filter(|(rx, ry)| mask[world.idx(*rx, *ry)])
                .count();
            if ring_neighbors >= 4
                && fill_depth[nidx] > 0.006
                && hydro[nidx] <= outlet_level + 0.012
                && world.tiles[nidx].raw_elevation <= outlet_level + 0.01
            {
                add.push(nidx);
            }
        }
    }
    for cell in add {
        mask[cell] = true;
    }

    region.iter().copied().filter(|idx| mask[*idx]).collect()
}

fn build_downstream(
    world: &World,
    ocean: &[bool],
    conditioning: &ConditioningState,
    lake_id: &[Option<u32>],
) -> Vec<Option<usize>> {
    let mut downstream = vec![None; world.tiles.len()];
    let mut order: Vec<_> = (0..world.tiles.len()).collect();
    order.sort_by(|a, b| {
        conditioning.hydro_elevation[*b]
            .total_cmp(&conditioning.hydro_elevation[*a])
            .then_with(|| world.tiles[*b].raw_elevation.total_cmp(&world.tiles[*a].raw_elevation))
            .then_with(|| conditioning.rank[*b].cmp(&conditioning.rank[*a]))
    });

    for idx in order {
        if ocean[idx] {
            continue;
        }
        downstream[idx] = routing_candidates(world, idx, conditioning, lake_id)
            .first()
            .map(|candidate| candidate.next);
    }

    reduce_directional_bias(world, ocean, conditioning, lake_id, &mut downstream);
    perturb_mountain_exits(world, ocean, conditioning, lake_id, &mut downstream);

    downstream
}

fn routing_candidates(
    world: &World,
    idx: usize,
    conditioning: &ConditioningState,
    lake_id: &[Option<u32>],
) -> Vec<RoutingCandidate> {
    let (x, y) = world.coords(idx);
    let current_hydro = conditioning.hydro_elevation[idx];
    let current_raw = world.tiles[idx].raw_elevation;
    let raw_aspect = local_aspect(world, x, y);
    let hydro_aspect = local_aspect_on_values(
        &conditioning.hydro_elevation,
        world.width,
        world.height,
        x,
        y,
    );
    let perturb_angle = (sample_seed_field(world.seed, x, y, 18, 0xD1F1_0101) - 0.5) * 0.9;
    let rotated = rotate_vector(hydro_aspect, perturb_angle);
    let preferred = normalize((
        hydro_aspect.0 * 0.72 + raw_aspect.0 * 0.20 + rotated.0 * 0.08,
        hydro_aspect.1 * 0.72 + raw_aspect.1 * 0.20 + rotated.1 * 0.08,
    ));
    let persistence = conditioning.parent[idx].and_then(|parent| {
        let (px, py) = world.coords(parent);
        direction_vector((x, y), (px, py))
    });
    let tributary_opportunity = sample_seed_field(world.seed, x, y, 22, 0xD1F1_0202);
    let mountain_front = mountain_front_factor(world, idx);
    let lowland_opening = lowland_opening_factor(world, idx);
    let mut candidates = Vec::with_capacity(8);

    for (nx, ny) in world.neighbors8(x, y) {
        let next = world.idx(nx, ny);
        if lake_id[idx].is_some() && lake_id[idx] == lake_id[next] {
            continue;
        }
        let neighbor_hydro = conditioning.hydro_elevation[next];
        if neighbor_hydro > current_hydro + HYDRO_EPSILON {
            continue;
        }
        let is_flat_or_equal = (neighbor_hydro - current_hydro).abs() <= HYDRO_EPSILON;
        if is_flat_or_equal && conditioning.rank[next] >= conditioning.rank[idx] {
            continue;
        }

        let distance = neighbor_distance(x, y, nx, ny);
        let dir = direction_vector((x, y), (nx, ny)).unwrap_or((0.0, 0.0));
        let hydro_drop = (current_hydro - conditioning.hydro_elevation[next]).max(0.0);
        let raw_drop = (current_raw - world.tiles[next].raw_elevation).max(-0.08);
        let slope = hydro_drop / distance;
        let raw_slope = raw_drop / distance;
        let alignment = dir.0 * preferred.0 + dir.1 * preferred.1;
        let aspect_cross = dir.0 * -preferred.1 + dir.1 * preferred.0;
        let persistence_bonus = persistence
            .map(|prev| (dir.0 * prev.0 + dir.1 * prev.1).max(-0.5))
            .unwrap_or(0.0);
        let flat_bonus = if hydro_drop <= HYDRO_EPSILON {
            raw_slope.max(0.0) * 2.1 + alignment * 0.15
        } else {
            0.0
        };
        let meander_bonus = if slope < 0.03 {
            let signed_bias = hash01(world.seed.wrapping_add(211), idx, next) * 2.0 - 1.0;
            aspect_cross * signed_bias * (0.16 * (1.0 - (slope / 0.03).clamp(0.0, 1.0)))
        } else {
            0.0
        };
        let anisotropy_penalty = if slope < 0.022 {
            let cardinal = if dir.0.abs() > 0.92 || dir.1.abs() > 0.92 {
                1.0
            } else {
                0.0
            };
            let diagonal = if dir.0.abs() > 0.65 && dir.1.abs() > 0.65 {
                1.0
            } else {
                0.0
            };
            cardinal * (0.06 + lowland_opening * 0.03) + diagonal * 0.035
        } else {
            0.0
        };
        let spacing_bonus = (tributary_opportunity - 0.5) * (0.30 + lowland_opening * 0.14);
        let mountain_exit_bonus = if mountain_front > 0.0 {
            let exit_noise = sample_seed_field(world.seed, x, y, 14, 0xD1F1_0303) * 2.0 - 1.0;
            let lateral_preference = aspect_cross * exit_noise;
            lateral_preference * 0.28 * mountain_front
        } else {
            0.0
        };
        let parent_bonus = if Some(next) == conditioning.parent[idx] {
            0.02
        } else {
            0.0
        };
        let score = slope * 9.4
            + raw_slope.max(0.0) * 2.8
            + alignment * 1.05
            + persistence_bonus * 0.12
            + flat_bonus
            + meander_bonus
            + spacing_bonus
            + mountain_exit_bonus
            + parent_bonus
            - anisotropy_penalty;
        candidates.push(RoutingCandidate {
            next,
            score,
            slope,
            direction: (
                (nx as isize - x as isize).signum(),
                (ny as isize - y as isize).signum(),
            ),
            mountain_front,
        });
    }

    candidates.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.next.cmp(&b.next)));
    candidates
}

fn reduce_directional_bias(
    world: &World,
    ocean: &[bool],
    conditioning: &ConditioningState,
    lake_id: &[Option<u32>],
    downstream: &mut [Option<usize>],
) {
    let mut order: Vec<_> = (0..world.tiles.len()).collect();
    order.sort_by(|a, b| {
        conditioning.hydro_elevation[*b]
            .total_cmp(&conditioning.hydro_elevation[*a])
            .then_with(|| conditioning.rank[*b].cmp(&conditioning.rank[*a]))
    });

    for _ in 0..3 {
        for idx in order.iter().copied() {
            if ocean[idx] || downstream[idx].is_none() || !is_run_start(world, downstream, idx) {
                continue;
            }
            let segment = same_direction_segment(world, downstream, idx);
            let slope = local_downstream_slope(world, conditioning, downstream, idx);
            let limit = if slope < 0.012 {
                4
            } else if slope < 0.03 {
                5
            } else {
                6
            };
            if segment.len() < limit {
                continue;
            }
            let target = segment[segment.len() / 2];
            let target_slope = local_downstream_slope(world, conditioning, downstream, target);
            let current = downstream[target].unwrap_or(usize::MAX);
            let candidates = routing_candidates(world, target, conditioning, lake_id);
            let current_score = candidates
                .iter()
                .find(|candidate| candidate.next == current)
                .map(|candidate| candidate.score)
                .unwrap_or(f32::MIN);
            if let Some(alternative) = candidates.into_iter().find(|candidate| {
                candidate.next != current
                    && candidate.direction != direction_for(world, target, current)
                    && candidate.score
                        >= current_score - (0.24 + (0.03 - target_slope.min(0.03)) * 4.5)
            }) {
                downstream[target] = Some(alternative.next);
            }
        }
    }
}

fn perturb_mountain_exits(
    world: &World,
    ocean: &[bool],
    conditioning: &ConditioningState,
    lake_id: &[Option<u32>],
    downstream: &mut [Option<usize>],
) {
    for idx in 0..world.tiles.len() {
        if ocean[idx] || downstream[idx].is_none() {
            continue;
        }
        let candidates = routing_candidates(world, idx, conditioning, lake_id);
        let Some(current) = downstream[idx] else {
            continue;
        };
        let Some(best) = candidates.iter().find(|candidate| candidate.next == current) else {
            continue;
        };
        let best_mountain_front = best.mountain_front;
        let best_slope = best.slope;
        let best_score = best.score;
        if best_mountain_front < 0.34 || best_slope > 0.06 {
            continue;
        }
        let current_dir = direction_for(world, idx, current);
        if let Some(alternative) = candidates.into_iter().find(|candidate| {
            candidate.next != current
                && candidate.direction != current_dir
                && candidate.score >= best_score - (0.24 + best_mountain_front * 0.08)
        }) {
            downstream[idx] = Some(alternative.next);
        }
    }
}

fn direction_for(world: &World, idx: usize, next: usize) -> (isize, isize) {
    let (x, y) = world.coords(idx);
    let (nx, ny) = world.coords(next);
    (
        (nx as isize - x as isize).signum(),
        (ny as isize - y as isize).signum(),
    )
}

fn same_direction_segment(world: &World, downstream: &[Option<usize>], start: usize) -> Vec<usize> {
    let mut current = start;
    let mut direction = None;
    let mut segment = Vec::new();
    let mut guard = 0;

    while guard < world.tiles.len() {
        let Some(next) = downstream[current] else {
            break;
        };
        let dir = direction_for(world, current, next);
        if Some(dir) == direction {
            segment.push(current);
        } else if direction.is_none() {
            direction = Some(dir);
            segment.push(current);
        } else {
            break;
        }
        current = next;
        guard += 1;
    }

    segment
}

fn is_run_start(world: &World, downstream: &[Option<usize>], idx: usize) -> bool {
    let Some(next) = downstream[idx] else {
        return false;
    };
    let (x, y) = world.coords(idx);
    let current_dir = direction_for(world, idx, next);
    for (nx, ny) in world.neighbors8(x, y) {
        let nidx = world.idx(nx, ny);
        if downstream[nidx] == Some(idx) && direction_for(world, nidx, idx) == current_dir {
            return false;
        }
    }
    true
}

fn local_downstream_slope(
    world: &World,
    conditioning: &ConditioningState,
    downstream: &[Option<usize>],
    idx: usize,
) -> f32 {
    let Some(next) = downstream[idx] else {
        return 0.0;
    };
    let (x, y) = world.coords(idx);
    let (nx, ny) = world.coords(next);
    (conditioning.hydro_elevation[idx] - conditioning.hydro_elevation[next]).max(0.0)
        / neighbor_distance(x, y, nx, ny)
}

fn rotate_vector(v: (f32, f32), angle: f32) -> (f32, f32) {
    let (s, c) = angle.sin_cos();
    (v.0 * c - v.1 * s, v.0 * s + v.1 * c)
}

fn local_relief(world: &World, idx: usize, radius: isize) -> f32 {
    let (x, y) = world.coords(idx);
    let current = world.tiles[idx].raw_elevation;
    let mut rise = 0.0_f32;
    let mut count = 0.0_f32;

    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if !world.in_bounds(nx, ny) {
                continue;
            }
            let nidx = world.idx(nx as usize, ny as usize);
            rise += (world.tiles[nidx].raw_elevation - current).abs();
            count += 1.0;
        }
    }

    if count <= f32::EPSILON {
        0.0
    } else {
        rise / count
    }
}

fn mountain_front_factor(world: &World, idx: usize) -> f32 {
    let current = world.tiles[idx].raw_elevation;
    let relief = local_relief(world, idx, 2);
    let highland = smoothstep(world.sea_level + 0.12, world.sea_level + 0.34, current);
    let relief_factor = smoothstep(0.02, 0.11, relief);
    let mut downstream_opening = 0.0_f32;
    let (x, y) = world.coords(idx);
    for (nx, ny) in world.neighbors8(x, y) {
        let nidx = world.idx(nx, ny);
        let drop = (current - world.tiles[nidx].raw_elevation).max(0.0);
        let relief_gap = (relief - local_relief(world, nidx, 2)).max(0.0);
        downstream_opening = downstream_opening.max(
            smoothstep(0.02, 0.12, drop) * smoothstep(0.0, 0.05, relief_gap),
        );
    }
    (highland * relief_factor * downstream_opening).clamp(0.0, 1.0)
}

fn lowland_opening_factor(world: &World, idx: usize) -> f32 {
    let current = world.tiles[idx].raw_elevation;
    let relief = local_relief(world, idx, 2);
    let lowland = 1.0 - smoothstep(world.sea_level + 0.10, world.sea_level + 0.26, current);
    let relief_soft = 1.0 - smoothstep(0.025, 0.09, relief);
    (lowland * relief_soft).clamp(0.0, 1.0)
}

fn break_downstream_cycles(downstream: &mut [Option<usize>], parent: &[Option<usize>], ocean: &[bool]) {
    for _ in 0..4 {
        let mut changed = false;
        for start in 0..downstream.len() {
            if ocean[start] {
                continue;
            }
            let mut path = Vec::new();
            let mut current = start;
            loop {
                if ocean[current] {
                    break;
                }
                if let Some(pos) = path.iter().position(|&idx| idx == current) {
                    for &cycle_idx in &path[pos..] {
                        downstream[cycle_idx] = parent[cycle_idx];
                    }
                    changed = true;
                    break;
                }
                path.push(current);
                match downstream[current] {
                    Some(next) => current = next,
                    None => break,
                }
            }
        }
        if !changed {
            break;
        }
    }
}

fn accumulate_contributing_area(
    conditioning: &ConditioningState,
    downstream: &[Option<usize>],
    ocean: &[bool],
) -> Vec<f32> {
    let mut contributing_area = vec![0.0; downstream.len()];
    let mut order: Vec<_> = (0..downstream.len()).collect();
    order.sort_by(|a, b| {
        conditioning.hydro_elevation[*b]
            .total_cmp(&conditioning.hydro_elevation[*a])
            .then_with(|| conditioning.rank[*b].cmp(&conditioning.rank[*a]))
    });

    for idx in order {
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

fn assign_basin_ids(
    world: &World,
    ocean: &[bool],
    downstream: &[Option<usize>],
    lake_id: &[Option<u32>],
    basin_offset: u32,
) -> Vec<Option<u32>> {
    let mut basin_id = vec![None; world.tiles.len()];
    let mut mouth_to_basin = HashMap::<usize, u32>::new();
    let mut next_basin = 0_u32;

    for idx in 0..world.tiles.len() {
        if ocean[idx] {
            continue;
        }
        let mut current = idx;
        let mut guard = 0;
        while guard < world.tiles.len() {
            if ocean[current] {
                break;
            }
            if let Some(lake) = lake_id[current] {
                basin_id[idx] = Some(lake);
                break;
            }
            match downstream[current] {
                Some(next) => {
                    if ocean[next] {
                        let basin = *mouth_to_basin.entry(current).or_insert_with(|| {
                            let id = basin_offset + next_basin;
                            next_basin += 1;
                            id
                        });
                        basin_id[idx] = Some(basin);
                        break;
                    }
                    current = next;
                }
                None => break,
            }
            guard += 1;
        }
    }

    basin_id
}

fn classify_surfaces(
    world: &World,
    ocean: &[bool],
    contributing_area: &[f32],
    lake_id: &[Option<u32>],
) -> Vec<Surface> {
    let mut surfaces = vec![Surface::Land; world.tiles.len()];
    let thresholds = river_thresholds(world);

    for idx in 0..world.tiles.len() {
        if ocean[idx] {
            surfaces[idx] = Surface::Ocean;
        } else if lake_id[idx].is_some() {
            surfaces[idx] = Surface::Lake;
        } else if contributing_area[idx]
            >= thresholds.stream
                * (0.82
                    + (1.0 - sample_seed_field(world.seed, idx % world.width, idx / world.width, 22, 0xD1F1_0404))
                        * 0.42)
        {
            surfaces[idx] = Surface::River;
        }
    }

    for idx in 0..world.tiles.len() {
        if surfaces[idx] != Surface::Land {
            continue;
        }
        let (x, y) = world.coords(idx);
        if world
            .neighbors8(x, y)
            .any(|(nx, ny)| surfaces[world.idx(nx, ny)] == Surface::Ocean)
        {
            surfaces[idx] = Surface::Coast;
        }
    }

    surfaces
}

pub(super) fn apply_channel_carving(world: &mut World, hydrology: &HydrologyState) {
    let thresholds = river_thresholds(world);

    for idx in 0..world.tiles.len() {
        if hydrology.surfaces[idx] != Surface::River {
            continue;
        }
        let discharge = hydrology.contributing_area[idx];
        let ratio = (discharge / thresholds.stream).max(1.0);
        let band_multiplier = if discharge >= thresholds.trunk {
            1.75
        } else if discharge >= thresholds.secondary {
            1.25
        } else {
            1.0
        };
        let local_slope = hydrology.downstream[idx]
            .map(|next| {
                let (x, y) = world.coords(idx);
                let (nx, ny) = world.coords(next);
                (hydrology.hydro_elevation[idx] - hydrology.hydro_elevation[next]).max(0.0)
                    / neighbor_distance(x, y, nx, ny)
            })
            .unwrap_or(0.0);
        let slope_factor = if local_slope < 0.008 { 1.25 } else { 0.95 };
        let carve = (0.0045 + ratio.ln() * 0.0135) * band_multiplier * slope_factor;
        let carve = carve.clamp(0.0, 0.085);
        world.tiles[idx].raw_elevation = (world.tiles[idx].raw_elevation - carve).max(0.0);

        let (x, y) = world.coords(idx);
        let neighbors: Vec<_> = world.neighbors8(x, y).collect();
        for (nx, ny) in neighbors {
            let nidx = world.idx(nx, ny);
            if hydrology.surfaces[nidx] == Surface::Ocean {
                continue;
            }
            let distance = neighbor_distance(x, y, nx, ny);
            let neighbor_relief =
                (world.tiles[nidx].raw_elevation - world.tiles[idx].raw_elevation).max(0.0);
            let side_factor = if hydrology.surfaces[nidx] == Surface::River {
                0.38
            } else if distance > 1.0 {
                0.12
            } else {
                0.22
            };
            let relief_factor = (0.5 + neighbor_relief * 1.8).clamp(0.5, 1.4);
            let lateral_carve = carve * side_factor * relief_factor;
            world.tiles[nidx].raw_elevation =
                (world.tiles[nidx].raw_elevation - lateral_carve).max(0.0);
        }
    }
}

pub(super) fn apply_hydrology_to_world(world: &mut World, ocean: &[bool], hydrology: &HydrologyState) {
    for idx in 0..world.tiles.len() {
        world.tiles[idx].hydro_elevation = hydrology.hydro_elevation[idx];
        world.tiles[idx].contributing_area = hydrology.contributing_area[idx];
        world.tiles[idx].downstream = hydrology.downstream[idx];
        world.tiles[idx].surface = hydrology.surfaces[idx];
        world.tiles[idx].basin_id = hydrology.basin_id[idx];
        world.tiles[idx].lake_id = hydrology.lake_id[idx];
        world.tiles[idx].water_level = hydrology.water_level[idx];
        if ocean[idx] {
            world.tiles[idx].water_level = Some(world.sea_level);
        }
    }
}

fn river_thresholds(world: &World) -> RiverThresholds {
    let area = (world.width * world.height) as f32;
    let stream = (area * 0.00075).max(12.0);
    RiverThresholds {
        stream,
        secondary: stream * 6.5,
        trunk: stream * 18.0,
    }
}
