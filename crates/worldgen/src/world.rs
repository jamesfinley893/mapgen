use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Tile {
    pub elevation: f32,
    pub slope: f32,
    pub relief: f32,
    pub temperature: f32,
    pub moisture: f32,
    pub precipitation: f32,
    pub continentality: f32,
    pub ocean_distance: u16,
    pub biome: Biome,
    pub mountain_feature: MountainFeature,
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            elevation: 0.0,
            slope: 0.0,
            relief: 0.0,
            temperature: 0.0,
            moisture: 0.0,
            precipitation: 0.0,
            continentality: 0.0,
            ocean_distance: u16::MAX,
            biome: Biome::Ocean,
            mountain_feature: MountainFeature::None,
        }
    }
}

impl Tile {
    pub fn is_ocean(&self) -> bool {
        self.biome == Biome::Ocean
    }

    pub fn is_coast(&self) -> bool {
        self.biome == Biome::Coast
    }

    pub fn is_land(&self) -> bool {
        !self.is_ocean()
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

    pub fn is_ocean(&self, idx: usize) -> bool {
        self.tiles[idx].is_ocean()
    }

    pub fn is_coast(&self, idx: usize) -> bool {
        self.tiles[idx].is_coast()
    }

    pub fn is_land(&self, idx: usize) -> bool {
        self.tiles[idx].is_land()
    }
}
