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
    pub rainfall_scale: f32,
    pub render_scale: u32,
    pub world_size: u32,
    pub effective_world_size: f32,
    pub land_tiles: usize,
    pub ocean_tiles: usize,
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
    highest_elevation: f32,
    alpine_tiles: usize,
    foothill_tiles: usize,
    biome_counts: Vec<(Biome, usize)>,
}

pub fn build_metadata(world: &World, config: &WorldConfig) -> WorldMetadata {
    let tile_summary = collect_tile_summary(world);

    WorldMetadata {
        seed: world.seed,
        width: world.width,
        height: world.height,
        sea_level: world.sea_level,
        temperature_bias: config.temperature_bias,
        moisture_bias: config.moisture_bias,
        rainfall_scale: config.rainfall_scale,
        render_scale: config.render_scale,
        world_size: config.world_size,
        effective_world_size: world.effective_world_size(),
        land_tiles: tile_summary.land_tiles,
        ocean_tiles: tile_summary.ocean_tiles,
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
    let mut highest_elevation = f32::MIN;
    let mut alpine_tiles = 0_usize;
    let mut foothill_tiles = 0_usize;
    let mut counts = std::collections::BTreeMap::<String, (Biome, usize)>::new();

    for tile in &world.tiles {
        highest_elevation = highest_elevation.max(tile.raw_elevation);
        if tile.surface == Surface::Ocean {
            ocean_tiles += 1;
        } else {
            land_tiles += 1;
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
        highest_elevation,
        alpine_tiles,
        foothill_tiles,
        biome_counts,
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
