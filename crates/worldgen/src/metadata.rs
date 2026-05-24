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
    pub highest_elevation: f32,
    pub biome_counts: Vec<(Biome, usize)>,
}

pub fn build_metadata(world: &World, config: &WorldConfig) -> WorldMetadata {
    let mut land_tiles = 0;
    let mut ocean_tiles = 0;
    let mut river_tiles = 0;
    let mut lake_tiles = 0;
    let mut highest_elevation = f32::MIN;
    let mut counts = std::collections::BTreeMap::<String, (Biome, usize)>::new();

    for tile in &world.tiles {
        highest_elevation = highest_elevation.max(tile.elevation);
        match tile.surface {
            Surface::Ocean => ocean_tiles += 1,
            Surface::River => {
                river_tiles += 1;
                land_tiles += 1;
            }
            Surface::Lake => lake_tiles += 1,
            Surface::Land | Surface::Coast => land_tiles += 1,
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
        highest_elevation,
        biome_counts,
    }
}
