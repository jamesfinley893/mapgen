use serde::{Deserialize, Serialize};

use crate::{Biome, Surface, World, WorldConfig};

const BIOMES: [Biome; 17] = [
    Biome::Ocean,
    Biome::Coast,
    Biome::PolarDesert,
    Biome::Tundra,
    Biome::BorealForest,
    Biome::TemperateGrassland,
    Biome::TemperateForest,
    Biome::Woodland,
    Biome::Wetland,
    Biome::Freshwater,
    Biome::Foothills,
    Biome::Steppe,
    Biome::Desert,
    Biome::Savanna,
    Biome::TropicalForest,
    Biome::Rainforest,
    Biome::Alpine,
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldMetadata {
    pub seed: u64,
    pub width: usize,
    pub height: usize,
    pub sea_level: f32,
    pub temperature_bias: f32,
    pub moisture_bias: f32,
    pub rainfall_scale: f32,
    pub world_size: u32,
    pub effective_world_size: f32,
    pub land_tiles: usize,
    pub ocean_tiles: usize,
    pub highest_elevation: f32,
    pub mean_land_slope: f32,
    pub mean_land_relief: f32,
    pub mean_land_runoff: f32,
    pub mean_land_continentality: f32,
    pub river_tiles: usize,
    pub strongest_river: f32,
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
    land_slope_sum: f32,
    land_relief_sum: f32,
    land_runoff_sum: f32,
    land_continentality_sum: f32,
    river_tiles: usize,
    strongest_river: f32,
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
        world_size: config.world_size,
        effective_world_size: world.effective_world_size(),
        land_tiles: tile_summary.land_tiles,
        ocean_tiles: tile_summary.ocean_tiles,
        highest_elevation: tile_summary.highest_elevation,
        mean_land_slope: tile_summary.land_slope_sum / tile_summary.land_tiles.max(1) as f32,
        mean_land_relief: tile_summary.land_relief_sum / tile_summary.land_tiles.max(1) as f32,
        mean_land_runoff: tile_summary.land_runoff_sum / tile_summary.land_tiles.max(1) as f32,
        mean_land_continentality: tile_summary.land_continentality_sum
            / tile_summary.land_tiles.max(1) as f32,
        river_tiles: tile_summary.river_tiles,
        strongest_river: tile_summary.strongest_river,
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
    let mut land_slope_sum = 0.0_f32;
    let mut land_relief_sum = 0.0_f32;
    let mut land_runoff_sum = 0.0_f32;
    let mut land_continentality_sum = 0.0_f32;
    let mut river_tiles = 0_usize;
    let mut strongest_river = 0.0_f32;
    let mut alpine_tiles = 0_usize;
    let mut foothill_tiles = 0_usize;
    let mut biome_counts = [0_usize; BIOMES.len()];

    for tile in &world.tiles {
        highest_elevation = highest_elevation.max(tile.raw_elevation);
        if tile.surface == Surface::Ocean {
            ocean_tiles += 1;
        } else {
            land_tiles += 1;
            land_slope_sum += tile.slope;
            land_relief_sum += tile.relief;
            land_runoff_sum += tile.runoff;
            land_continentality_sum += tile.continentality;
            if tile.river > 0.08 {
                river_tiles += 1;
            }
            strongest_river = strongest_river.max(tile.river);
        }
        if tile.biome == Biome::Alpine {
            alpine_tiles += 1;
        } else if tile.biome == Biome::Foothills {
            foothill_tiles += 1;
        }
        biome_counts[biome_index(tile.biome)] += 1;
    }

    let mut biome_counts = BIOMES
        .iter()
        .copied()
        .zip(biome_counts)
        .filter(|(_, count)| *count > 0)
        .collect::<Vec<_>>();
    biome_counts.sort_by(|(left_biome, left_count), (right_biome, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| biome_name(*left_biome).cmp(biome_name(*right_biome)))
    });

    TileSummary {
        land_tiles,
        ocean_tiles,
        highest_elevation,
        land_slope_sum,
        land_relief_sum,
        land_runoff_sum,
        land_continentality_sum,
        river_tiles,
        strongest_river,
        alpine_tiles,
        foothill_tiles,
        biome_counts,
    }
}

fn biome_index(biome: Biome) -> usize {
    match biome {
        Biome::Ocean => 0,
        Biome::Coast => 1,
        Biome::PolarDesert => 2,
        Biome::Tundra => 3,
        Biome::BorealForest => 4,
        Biome::TemperateGrassland => 5,
        Biome::TemperateForest => 6,
        Biome::Woodland => 7,
        Biome::Wetland => 8,
        Biome::Freshwater => 9,
        Biome::Foothills => 10,
        Biome::Steppe => 11,
        Biome::Desert => 12,
        Biome::Savanna => 13,
        Biome::TropicalForest => 14,
        Biome::Rainforest => 15,
        Biome::Alpine => 16,
    }
}

fn biome_name(biome: Biome) -> &'static str {
    match biome {
        Biome::Ocean => "Ocean",
        Biome::Coast => "Coast",
        Biome::PolarDesert => "PolarDesert",
        Biome::Tundra => "Tundra",
        Biome::BorealForest => "BorealForest",
        Biome::TemperateGrassland => "TemperateGrassland",
        Biome::TemperateForest => "TemperateForest",
        Biome::Woodland => "Woodland",
        Biome::Wetland => "Wetland",
        Biome::Freshwater => "Freshwater",
        Biome::Foothills => "Foothills",
        Biome::Steppe => "Steppe",
        Biome::Desert => "Desert",
        Biome::Savanna => "Savanna",
        Biome::TropicalForest => "TropicalForest",
        Biome::Rainforest => "Rainforest",
        Biome::Alpine => "Alpine",
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
