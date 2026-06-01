use serde::{Deserialize, Serialize};

pub const LEGACY_WORLD_SIZE: u32 = 384;
pub const DEFAULT_WORLD_SIZE: u32 = 768;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldConfig {
    pub seed: u64,
    pub width: usize,
    pub height: usize,
    pub sea_level: f32,
    pub temperature_bias: f32,
    pub moisture_bias: f32,
    pub rainfall_scale: f32,
    /// Tiles per world unit. Controls geographic scale independently of pixel count.
    /// The default is a 768-cell one-world-unit map; set to 384 for the original
    /// density over a larger extent, or 0 to fit the whole output into one world unit.
    pub world_size: u32,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            seed: 1,
            width: DEFAULT_WORLD_SIZE as usize,
            height: DEFAULT_WORLD_SIZE as usize,
            sea_level: 0.52,
            temperature_bias: 0.0,
            moisture_bias: 0.0,
            rainfall_scale: 1.0,
            world_size: DEFAULT_WORLD_SIZE,
        }
    }
}

impl WorldConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.width < 32 || self.height < 32 {
            return Err("width and height must be at least 32".into());
        }
        if self.width > 4096 || self.height > 4096 {
            return Err("width and height must be at most 4096".into());
        }
        if !(0.2..=0.8).contains(&self.sea_level) {
            return Err("sea level must be between 0.2 and 0.8".into());
        }
        if !(0.25..=4.0).contains(&self.rainfall_scale) {
            return Err("rainfall_scale must be between 0.25 and 4.0".into());
        }
        if self.world_size != 0 && self.world_size < 32 {
            return Err("world_size must be 0 (auto) or at least 32".into());
        }
        Ok(())
    }
}
