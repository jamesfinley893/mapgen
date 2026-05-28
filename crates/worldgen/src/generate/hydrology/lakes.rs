use std::collections::{HashSet, VecDeque};

use crate::World;

use super::LakeData;

pub(super) fn identify_lakes(
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
    let ws = world.effective_world_size();
    // Higher thresholds cull small shallow depressions that produce fragmented lake patches.
    let area_threshold = (ws * ws * 0.0022).ceil() as usize;
    let area_threshold = area_threshold.max(12);
    let volume_threshold = (ws * ws * 0.00034).max(0.18);
    let depth_threshold = 0.025;

    for idx in 0..world.tiles.len() {
        if visited[idx]
            || ocean[idx]
            || lake_id[idx].is_some()
            || fill_depth[idx] <= depth_threshold
        {
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
                if visited[nidx]
                    || ocean[nidx]
                    || lake_id[nidx].is_some()
                    || fill_depth[nidx] <= depth_threshold
                {
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
        if avg_depth < 0.028 && max_depth < 0.058 {
            continue;
        }

        let in_region: HashSet<usize> = region.iter().copied().collect();
        let mut outlet = None;
        let mut outlet_level = f32::MAX;
        for &cell in &region {
            if let Some(next) = parent[cell]
                && !in_region.contains(&next)
                && hydro[cell] < outlet_level
            {
                outlet_level = hydro[cell];
                outlet = Some(next);
            }
        }

        if outlet.is_none() {
            continue;
        }

        let refined_region =
            refine_lake_region(world, hydro, fill_depth, &region, outlet_level, max_depth);
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

    mask.iter()
        .enumerate()
        .filter_map(|(idx, included)| included.then_some(idx))
        .collect()
}
