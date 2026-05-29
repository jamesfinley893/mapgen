use serde::{Deserialize, Serialize};

use crate::river::river_direction;
use crate::{Biome, Surface, World, WorldConfig, audit_rivers};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldMetadata {
    pub seed: u64,
    pub width: usize,
    pub height: usize,
    pub sea_level: f32,
    pub temperature_bias: f32,
    pub moisture_bias: f32,
    pub rainfall_scale: f32,
    pub runoff_scale: f32,
    pub channel_density: f32,
    pub render_scale: u32,
    pub world_size: u32,
    pub effective_world_size: f32,
    pub land_tiles: usize,
    pub ocean_tiles: usize,
    pub river_tiles: usize,
    pub lake_tiles: usize,
    pub lake_count: usize,
    pub total_lake_area: usize,
    pub largest_basin_area: usize,
    pub max_river_discharge: f32,
    pub mean_runoff: f32,
    pub mean_river_discharge: f32,
    pub max_stream_power: f32,
    pub river_band_counts: [usize; 3],
    pub river_source_count: usize,
    pub river_confluence_count: usize,
    pub river_mouth_count: usize,
    pub median_source_segment_length: usize,
    pub p90_source_segment_length: usize,
    pub dominant_river_direction_fraction: f32,
    pub longest_trunk_length: usize,
    pub trunk_straight_run_ratio: f32,
    pub tributary_spacing_variance: f32,
    pub mountain_exit_irregularity_score: f32,
    pub confined_trunk_fraction: f32,
    pub average_trunk_confinement: f32,
    pub highest_elevation: f32,
    pub alpine_fraction: f32,
    pub foothill_fraction: f32,
    pub largest_contiguous_alpine_region: usize,
    pub largest_contiguous_foothill_region: usize,
    pub biome_counts: Vec<(Biome, usize)>,
}

struct TileSummary {
    land_tiles: usize,
    ocean_tiles: usize,
    river_tiles: usize,
    lake_tiles: usize,
    lake_count: usize,
    total_lake_area: usize,
    largest_basin_area: usize,
    max_river_discharge: f32,
    mean_runoff: f32,
    mean_river_discharge: f32,
    max_stream_power: f32,
    river_band_counts: [usize; 3],
    highest_elevation: f32,
    alpine_tiles: usize,
    foothill_tiles: usize,
    biome_counts: Vec<(Biome, usize)>,
}

pub fn build_metadata(world: &World, config: &WorldConfig) -> WorldMetadata {
    let tile_summary = collect_tile_summary(world);
    let (confined_trunk_fraction, average_trunk_confinement) = trunk_confinement_stats(world);
    let trunk_straight_run_ratio = trunk_straight_run_ratio(world);
    let tributary_spacing_variance = tributary_spacing_variance(world);
    let mountain_exit_irregularity_score = mountain_exit_irregularity_score(world);
    let river_audit = audit_rivers(world);

    WorldMetadata {
        seed: world.seed,
        width: world.width,
        height: world.height,
        sea_level: world.sea_level,
        temperature_bias: config.temperature_bias,
        moisture_bias: config.moisture_bias,
        rainfall_scale: config.rainfall_scale,
        runoff_scale: config.runoff_scale,
        channel_density: config.channel_density,
        render_scale: config.render_scale,
        world_size: config.world_size,
        effective_world_size: world.effective_world_size(),
        land_tiles: tile_summary.land_tiles,
        ocean_tiles: tile_summary.ocean_tiles,
        river_tiles: tile_summary.river_tiles,
        lake_tiles: tile_summary.lake_tiles,
        lake_count: tile_summary.lake_count,
        total_lake_area: tile_summary.total_lake_area,
        largest_basin_area: tile_summary.largest_basin_area,
        max_river_discharge: tile_summary.max_river_discharge,
        mean_runoff: tile_summary.mean_runoff,
        mean_river_discharge: tile_summary.mean_river_discharge,
        max_stream_power: tile_summary.max_stream_power,
        river_band_counts: tile_summary.river_band_counts,
        river_source_count: river_audit.sources,
        river_confluence_count: river_audit.confluences,
        river_mouth_count: river_audit.mouths,
        median_source_segment_length: river_audit.segment_median,
        p90_source_segment_length: river_audit.segment_p90,
        dominant_river_direction_fraction: river_audit.dominant_direction_fraction,
        longest_trunk_length: longest_trunk_length(world),
        trunk_straight_run_ratio,
        tributary_spacing_variance,
        mountain_exit_irregularity_score,
        confined_trunk_fraction,
        average_trunk_confinement,
        highest_elevation: tile_summary.highest_elevation,
        alpine_fraction: tile_summary.alpine_tiles as f32 / tile_summary.land_tiles.max(1) as f32,
        foothill_fraction: tile_summary.foothill_tiles as f32
            / tile_summary.land_tiles.max(1) as f32,
        largest_contiguous_alpine_region: largest_biome_region(world, Biome::Alpine),
        largest_contiguous_foothill_region: largest_biome_region(world, Biome::Foothills),
        biome_counts: tile_summary.biome_counts,
    }
}

fn collect_tile_summary(world: &World) -> TileSummary {
    let mut land_tiles = 0;
    let mut ocean_tiles = 0;
    let mut river_tiles = 0;
    let mut lake_tiles = 0;
    let mut highest_elevation = f32::MIN;
    let mut max_river_discharge = 0.0_f32;
    let mut total_runoff = 0.0_f32;
    let mut total_river_discharge = 0.0_f32;
    let mut max_stream_power = 0.0_f32;
    let mut river_band_counts = [0_usize; 3];
    let mut alpine_tiles = 0_usize;
    let mut foothill_tiles = 0_usize;
    let mut counts = std::collections::BTreeMap::<String, (Biome, usize)>::new();
    let mut lake_ids = std::collections::BTreeSet::new();
    let mut basin_counts = std::collections::HashMap::<u32, usize>::new();

    for tile in &world.tiles {
        highest_elevation = highest_elevation.max(tile.raw_elevation);
        total_runoff += tile.runoff;
        max_stream_power = max_stream_power.max(tile.stream_power);
        match tile.surface {
            Surface::Ocean => ocean_tiles += 1,
            Surface::River => {
                river_tiles += 1;
                land_tiles += 1;
                total_river_discharge += tile.discharge;
                max_river_discharge = max_river_discharge.max(tile.discharge);
                let band = match tile.channel_order {
                    3 | 4 => 2,
                    2 => 1,
                    _ => 0,
                };
                river_band_counts[band] += 1;
            }
            Surface::Lake => {
                lake_tiles += 1;
                if let Some(lake_id) = tile.lake_id {
                    lake_ids.insert(lake_id);
                }
            }
            Surface::Land | Surface::Coast => land_tiles += 1,
        }
        if let Some(basin_id) = tile.basin_id {
            *basin_counts.entry(basin_id).or_default() += 1;
        }
        if tile.biome == Biome::Alpine {
            alpine_tiles += 1;
        } else if tile.biome == Biome::Foothills {
            foothill_tiles += 1;
        }
        counts
            .entry(format!("{:?}", tile.biome))
            .and_modify(|entry| entry.1 += 1)
            .or_insert((tile.biome, 1));
    }

    let mut biome_counts: Vec<_> = counts.into_values().collect();
    biome_counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));

    TileSummary {
        land_tiles,
        ocean_tiles,
        river_tiles,
        lake_tiles,
        lake_count: lake_ids.len(),
        total_lake_area: lake_tiles,
        largest_basin_area: basin_counts.into_values().max().unwrap_or(0),
        max_river_discharge,
        mean_runoff: total_runoff / world.tiles.len().max(1) as f32,
        mean_river_discharge: total_river_discharge / river_tiles.max(1) as f32,
        max_stream_power,
        river_band_counts,
        highest_elevation,
        alpine_tiles,
        foothill_tiles,
        biome_counts,
    }
}

fn longest_trunk_length(world: &World) -> usize {
    let mut best = 0;
    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface != Surface::River || tile.channel_order < 3 {
            continue;
        }
        let mut current = idx;
        let mut len = 0;
        let mut guard = 0;
        loop {
            let tile = &world.tiles[current];
            if tile.surface != Surface::River {
                break;
            }
            len += 1;
            match tile.downstream {
                Some(next) => current = next,
                None => break,
            }
            guard += 1;
            if guard >= world.tiles.len() {
                break;
            }
        }
        best = best.max(len);
    }
    best
}

fn trunk_confinement_stats(world: &World) -> (f32, f32) {
    let mut trunk_tiles = 0_usize;
    let mut confined = 0_usize;
    let mut total = 0.0_f32;

    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface != Surface::River || tile.channel_order < 3 {
            continue;
        }
        let Some(next) = tile.downstream else {
            continue;
        };
        let conf = local_trunk_confinement(world, idx, next);
        trunk_tiles += 1;
        if conf > 0.52 {
            confined += 1;
        }
        total += conf;
    }

    if trunk_tiles == 0 {
        (0.0, 0.0)
    } else {
        (
            confined as f32 / trunk_tiles as f32,
            total / trunk_tiles as f32,
        )
    }
}

fn trunk_straight_run_ratio(world: &World) -> f32 {
    let mut total = 0_usize;
    let mut straight = 0_usize;

    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface != Surface::River || tile.channel_order < 3 {
            continue;
        }
        let Some(next) = tile.downstream else {
            continue;
        };
        if world.tiles[next].surface != Surface::River || world.tiles[next].channel_order < 3 {
            continue;
        }
        let Some(next2) = world.tiles[next].downstream else {
            continue;
        };
        if world.tiles[next2].surface != Surface::River {
            continue;
        }
        total += 1;
        if river_direction(world, idx, next) == river_direction(world, next, next2) {
            straight += 1;
        }
    }

    if total == 0 {
        0.0
    } else {
        straight as f32 / total as f32
    }
}

fn tributary_spacing_variance(world: &World) -> f32 {
    let mut upstream = vec![Vec::new(); world.tiles.len()];
    for (idx, tile) in world.tiles.iter().enumerate() {
        if let Some(next) = tile.downstream {
            upstream[next].push(idx);
        }
    }

    let mut intervals = Vec::new();
    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface != Surface::River || tile.channel_order < 3 {
            continue;
        }
        let upstream_trunk = upstream[idx]
            .iter()
            .filter(|&&source| {
                let source_tile = &world.tiles[source];
                source_tile.surface == Surface::River && source_tile.channel_order >= 3
            })
            .count();
        if upstream_trunk != 0 {
            continue;
        }
        let mut current = idx;
        let mut since_junction = 0_usize;
        let mut guard = 0;
        while guard < world.tiles.len() {
            let tile = &world.tiles[current];
            if tile.surface != Surface::River || tile.channel_order < 3 {
                break;
            }
            let major_inputs = upstream[current]
                .iter()
                .filter(|&&source| {
                    let source_tile = &world.tiles[source];
                    source_tile.surface == Surface::River && source_tile.channel_order >= 2
                })
                .count();
            if major_inputs >= 2 {
                intervals.push(since_junction as f32);
                since_junction = 0;
            } else {
                since_junction += 1;
            }
            let Some(next) = tile.downstream else {
                break;
            };
            current = next;
            guard += 1;
        }
    }

    if intervals.len() < 2 {
        0.0
    } else {
        let mean = intervals.iter().sum::<f32>() / intervals.len() as f32;
        intervals
            .iter()
            .map(|interval| {
                let delta = interval - mean;
                delta * delta
            })
            .sum::<f32>()
            / intervals.len() as f32
    }
}

fn mountain_exit_irregularity_score(world: &World) -> f32 {
    let mut exits = 0_usize;
    let mut total_score = 0.0_f32;

    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface != Surface::River || !matches!(tile.biome, Biome::Alpine | Biome::Foothills)
        {
            continue;
        }
        let Some(next) = tile.downstream else {
            continue;
        };
        if matches!(world.tiles[next].biome, Biome::Alpine | Biome::Foothills) {
            continue;
        }
        exits += 1;
        let mut current = idx;
        let mut previous_dir = None;
        let mut bends = 0_usize;
        let mut steps = 0_usize;
        let mut guard = 0;
        while guard < 6 {
            let tile = &world.tiles[current];
            if tile.surface != Surface::River {
                break;
            }
            let Some(next) = tile.downstream else {
                break;
            };
            let dir = river_direction(world, current, next);
            if previous_dir.is_some() && previous_dir != Some(dir) {
                bends += 1;
            }
            previous_dir = Some(dir);
            steps += 1;
            if !matches!(world.tiles[next].biome, Biome::Alpine | Biome::Foothills) {
                current = next;
            } else {
                break;
            }
            guard += 1;
        }
        if steps > 0 {
            total_score += bends as f32 / steps as f32;
        }
    }

    if exits == 0 {
        0.0
    } else {
        total_score / exits as f32
    }
}

fn local_trunk_confinement(world: &World, idx: usize, next: usize) -> f32 {
    let (x, y) = world.coords(idx);
    let (nx, ny) = world.coords(next);
    let dx = (nx as isize - x as isize).signum();
    let dy = (ny as isize - y as isize).signum();
    if dx == 0 && dy == 0 {
        return 0.0;
    }
    let current = world.tiles[idx].raw_elevation;
    let side_a = (-dy, dx);
    let side_b = (dy, -dx);
    let mut rise = 0.0_f32;
    let mut weight_sum = 0.0_f32;

    for distance in 1..=2 {
        let weight = if distance == 1 { 1.0 } else { 0.55 };
        for side in [side_a, side_b] {
            let sx = x as isize + side.0 * distance;
            let sy = y as isize + side.1 * distance;
            if !world.in_bounds(sx, sy) {
                continue;
            }
            let sidx = world.idx(sx as usize, sy as usize);
            rise += (world.tiles[sidx].raw_elevation - current).max(0.0) * weight;
            weight_sum += weight;
        }
    }

    if weight_sum <= f32::EPSILON {
        0.0
    } else {
        let avg_rise = rise / weight_sum;
        ((avg_rise - 0.015) / (0.13 - 0.015)).clamp(0.0, 1.0)
    }
}

fn largest_biome_region(world: &World, biome: Biome) -> usize {
    let mut visited = vec![false; world.tiles.len()];
    let mut best = 0;

    for start in 0..world.tiles.len() {
        if visited[start] || world.tiles[start].biome != biome {
            continue;
        }
        let mut queue = std::collections::VecDeque::from([start]);
        visited[start] = true;
        let mut size = 0;

        while let Some(idx) = queue.pop_front() {
            size += 1;
            let (x, y) = world.coords(idx);
            for (nx, ny) in world.neighbors8(x, y) {
                let nidx = world.idx(nx, ny);
                if !visited[nidx] && world.tiles[nidx].biome == biome {
                    visited[nidx] = true;
                    queue.push_back(nidx);
                }
            }
        }

        best = best.max(size);
    }

    best
}
