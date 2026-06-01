use serde::{Deserialize, Serialize};

use crate::config::LEGACY_WORLD_SIZE;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Surface {
    Ocean,
    Coast,
    Land,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Biome {
    Ocean,
    Coast,
    PolarDesert,
    Tundra,
    BorealForest,
    TemperateGrassland,
    TemperateForest,
    Woodland,
    Wetland,
    Freshwater,
    Foothills,
    Steppe,
    Desert,
    Savanna,
    TropicalForest,
    Rainforest,
    Alpine,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MountainFeature {
    #[default]
    None,
    Foothill,
    AlpineSlope,
    Ridge,
    Summit,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Landform {
    Water,
    Shore,
    #[default]
    Plain,
    Hill,
    Valley,
    Ridge,
    Peak,
    Basin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Tile {
    pub raw_elevation: f32,
    pub elevation_shade: f32,
    pub slope: f32,
    pub relief: f32,
    pub runoff: f32,
    pub flow_accumulation: f32,
    pub river: f32,
    pub river_depth: f32,
    pub river_width: f32,
    pub river_order: u8,
    pub lake_depth: f32,
    pub water_body_id: u32,
    pub lake_inflow: f32,
    pub lake_outlet: bool,
    pub spill_discharge: f32,
    pub erosion: f32,
    pub flow_direction: i8,
    pub temperature: f32,
    pub moisture: f32,
    pub precipitation: f32,
    pub continentality: f32,
    pub ocean_distance: u16,
    pub surface: Surface,
    pub biome: Biome,
    pub mountain_feature: MountainFeature,
    pub landform: Landform,
    pub terrain_texture: f32,
    pub ecotone_strength: f32,
    pub shore_influence: f32,
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            raw_elevation: 0.0,
            elevation_shade: 0.5,
            slope: 0.0,
            relief: 0.0,
            runoff: 0.0,
            flow_accumulation: 0.0,
            river: 0.0,
            river_depth: 0.0,
            river_width: 0.0,
            river_order: 0,
            lake_depth: 0.0,
            water_body_id: 0,
            lake_inflow: 0.0,
            lake_outlet: false,
            spill_discharge: 0.0,
            erosion: 0.0,
            flow_direction: -1,
            temperature: 0.0,
            moisture: 0.0,
            precipitation: 0.0,
            continentality: 0.0,
            ocean_distance: u16::MAX,
            surface: Surface::Ocean,
            biome: Biome::Ocean,
            mountain_feature: MountainFeature::None,
            landform: Landform::Plain,
            terrain_texture: 0.5,
            ecotone_strength: 0.0,
            shore_influence: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct World {
    pub seed: u64,
    pub width: usize,
    pub height: usize,
    pub sea_level: f32,
    pub world_size: u32,
    pub tiles: Vec<Tile>,
}

impl World {
    pub fn new(seed: u64, width: usize, height: usize, sea_level: f32, world_size: u32) -> Self {
        Self {
            seed,
            width,
            height,
            sea_level,
            world_size,
            tiles: vec![Tile::default(); width * height],
        }
    }

    /// Tiles per world unit. Resolves the 0 sentinel to min(width, height),
    /// which preserves the legacy single-world-unit behavior for explicit opt-in.
    pub fn effective_world_size(&self) -> f32 {
        if self.world_size == 0 {
            self.width.min(self.height) as f32
        } else {
            self.world_size as f32
        }
    }

    pub fn high_detail_scale(&self) -> f32 {
        (self.effective_world_size() / LEGACY_WORLD_SIZE as f32).max(1.0)
    }

    pub fn high_detail_cell_area(&self) -> f32 {
        let scale = self.high_detail_scale();
        1.0 / (scale * scale)
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    pub fn idx(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    pub fn coords(&self, idx: usize) -> (usize, usize) {
        (idx % self.width, idx / self.width)
    }

    pub fn in_bounds(&self, x: isize, y: isize) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height
    }

    pub fn neighbors8(&self, x: usize, y: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
        const DIRS: [(isize, isize); 8] = [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ];
        DIRS.into_iter().filter_map(move |(dx, dy)| {
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            self.in_bounds(nx, ny).then_some((nx as usize, ny as usize))
        })
    }

    pub fn neighbor_indices8(&self, idx: usize) -> impl Iterator<Item = usize> + '_ {
        let (x, y) = self.coords(idx);
        self.neighbors8(x, y).map(|(nx, ny)| self.idx(nx, ny))
    }
}
