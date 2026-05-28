use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Surface {
    Ocean,
    Coast,
    Land,
    Lake,
    River,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Biome {
    Ocean,
    Coast,
    Lake,
    PolarDesert,
    Tundra,
    BorealForest,
    TemperateGrassland,
    TemperateForest,
    Woodland,
    Foothills,
    Steppe,
    Desert,
    Savanna,
    TropicalForest,
    Rainforest,
    Alpine,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Tile {
    pub raw_elevation: f32,
    pub hydro_elevation: f32,
    pub temperature: f32,
    pub moisture: f32,
    pub contributing_area: f32,
    pub precipitation: f32,
    pub runoff: f32,
    pub discharge: f32,
    pub stream_power: f32,
    pub channel_order: u8,
    pub surface: Surface,
    pub biome: Biome,
    pub downstream: Option<usize>,
    pub basin_id: Option<u32>,
    pub lake_id: Option<u32>,
    pub water_level: Option<f32>,
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            raw_elevation: 0.0,
            hydro_elevation: 0.0,
            temperature: 0.0,
            moisture: 0.0,
            contributing_area: 0.0,
            precipitation: 0.0,
            runoff: 0.0,
            discharge: 0.0,
            stream_power: 0.0,
            channel_order: 0,
            surface: Surface::Ocean,
            biome: Biome::Ocean,
            downstream: None,
            basin_id: None,
            lake_id: None,
            water_level: None,
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

    /// Tiles per world unit. Resolves the 0 sentinel to min(width, height).
    pub fn effective_world_size(&self) -> f32 {
        if self.world_size == 0 {
            self.width.min(self.height) as f32
        } else {
            self.world_size as f32
        }
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
}
