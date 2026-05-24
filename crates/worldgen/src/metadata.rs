use serde::{Deserialize, Serialize};

use crate::{Biome, Surface, World, WorldConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldMetadata {
    pub seed: u64,
    pub width: usize,
    pub height: usize,
    pub sea_level: f32,
    pub temperature_bias: f32,
    pub moisture_bias: f32,
    pub render_scale: u32,
    pub land_tiles: usize,
    pub ocean_tiles: usize,
    pub river_tiles: usize,
    pub lake_tiles: usize,
    pub lake_count: usize,
    pub total_lake_area: usize,
    pub largest_basin_area: usize,
    pub max_river_discharge: f32,
    pub river_band_counts: [usize; 3],
    pub longest_trunk_length: usize,
    pub confined_trunk_fraction: f32,
    pub average_trunk_confinement: f32,
    pub highest_elevation: f32,
    pub alpine_fraction: f32,
    pub foothill_fraction: f32,
    pub largest_contiguous_alpine_region: usize,
    pub largest_contiguous_foothill_region: usize,
    pub biome_counts: Vec<(Biome, usize)>,
}

pub fn build_metadata(world: &World, config: &WorldConfig) -> WorldMetadata {
    let mut land_tiles = 0;
    let mut ocean_tiles = 0;
    let mut river_tiles = 0;
    let mut lake_tiles = 0;
    let mut highest_elevation = f32::MIN;
    let mut max_river_discharge = 0.0_f32;
    let mut river_band_counts = [0_usize; 3];
    let mut alpine_tiles = 0_usize;
    let mut foothill_tiles = 0_usize;
    let mut counts = std::collections::BTreeMap::<String, (Biome, usize)>::new();
    let mut lake_ids = std::collections::BTreeSet::new();
    let mut basin_counts = std::collections::HashMap::<u32, usize>::new();
    let thresholds = river_thresholds(world);

    for tile in &world.tiles {
        highest_elevation = highest_elevation.max(tile.raw_elevation);
        match tile.surface {
            Surface::Ocean => ocean_tiles += 1,
            Surface::River => {
                river_tiles += 1;
                land_tiles += 1;
                max_river_discharge = max_river_discharge.max(tile.contributing_area);
                let band = if tile.contributing_area >= thresholds.1 {
                    2
                } else if tile.contributing_area >= thresholds.0 {
                    1
                } else {
                    0
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
    let (confined_trunk_fraction, average_trunk_confinement) =
        trunk_confinement_stats(world, thresholds.1);

    WorldMetadata {
        seed: world.seed,
        width: world.width,
        height: world.height,
        sea_level: config.sea_level,
        temperature_bias: config.temperature_bias,
        moisture_bias: config.moisture_bias,
        render_scale: config.render_scale,
        land_tiles,
        ocean_tiles,
        river_tiles,
        lake_tiles,
        lake_count: lake_ids.len(),
        total_lake_area: lake_tiles,
        largest_basin_area: basin_counts.into_values().max().unwrap_or(0),
        max_river_discharge,
        river_band_counts,
        longest_trunk_length: longest_trunk_length(world, thresholds.1),
        confined_trunk_fraction,
        average_trunk_confinement,
        highest_elevation,
        alpine_fraction: alpine_tiles as f32 / land_tiles.max(1) as f32,
        foothill_fraction: foothill_tiles as f32 / land_tiles.max(1) as f32,
        largest_contiguous_alpine_region: largest_biome_region(world, Biome::Alpine),
        largest_contiguous_foothill_region: largest_biome_region(world, Biome::Foothills),
        biome_counts,
    }
}

fn river_thresholds(world: &World) -> (f32, f32) {
    let stream = ((world.width * world.height) as f32 * 0.00075).max(12.0);
    (stream * 6.5, stream * 18.0)
}

fn longest_trunk_length(world: &World, trunk_threshold: f32) -> usize {
    let mut best = 0;
    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface != Surface::River || tile.contributing_area < trunk_threshold {
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

fn trunk_confinement_stats(world: &World, trunk_threshold: f32) -> (f32, f32) {
    let mut trunk_tiles = 0_usize;
    let mut confined = 0_usize;
    let mut total = 0.0_f32;

    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface != Surface::River || tile.contributing_area < trunk_threshold {
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
