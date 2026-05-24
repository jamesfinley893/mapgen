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
    Steppe,
    Desert,
    Savanna,
    TropicalForest,
    Rainforest,
    Alpine,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tile {
    pub elevation: f32,
    pub temperature: f32,
    pub moisture: f32,
    pub flow_accumulation: f32,
    pub surface: Surface,
    pub biome: Biome,
    pub downstream: Option<usize>,
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            elevation: 0.0,
            temperature: 0.0,
            moisture: 0.0,
            flow_accumulation: 0.0,
            surface: Surface::Ocean,
            biome: Biome::Ocean,
            downstream: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct World {
    pub seed: u64,
    pub width: usize,
    pub height: usize,
    pub sea_level: f32,
    pub tiles: Vec<Tile>,
}

impl World {
    pub fn new(seed: u64, width: usize, height: usize, sea_level: f32) -> Self {
        Self {
            seed,
            width,
            height,
            sea_level,
            tiles: vec![Tile::default(); width * height],
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
