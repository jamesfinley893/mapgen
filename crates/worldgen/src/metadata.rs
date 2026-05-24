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
    pub highest_elevation: f32,
    pub biome_counts: Vec<(Biome, usize)>,
}

pub fn build_metadata(world: &World, config: &WorldConfig) -> WorldMetadata {
    let mut land_tiles = 0;
    let mut ocean_tiles = 0;
    let mut river_tiles = 0;
    let mut lake_tiles = 0;
    let mut highest_elevation = f32::MIN;
    let mut max_river_discharge = 0.0_f32;
    let mut counts = std::collections::BTreeMap::<String, (Biome, usize)>::new();
    let mut lake_ids = std::collections::BTreeSet::new();
    let mut basin_counts = std::collections::HashMap::<u32, usize>::new();

    for tile in &world.tiles {
        highest_elevation = highest_elevation.max(tile.raw_elevation);
        match tile.surface {
            Surface::Ocean => ocean_tiles += 1,
            Surface::River => {
                river_tiles += 1;
                land_tiles += 1;
                max_river_discharge = max_river_discharge.max(tile.contributing_area);
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
        counts
            .entry(format!("{:?}", tile.biome))
            .and_modify(|entry| entry.1 += 1)
            .or_insert((tile.biome, 1));
    }

    let mut biome_counts: Vec<_> = counts.into_values().collect();
    biome_counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));

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
        highest_elevation,
        biome_counts,
    }
}
