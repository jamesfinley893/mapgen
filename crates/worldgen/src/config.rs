use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldConfig {
    pub seed: u64,
    pub width: usize,
    pub height: usize,
    pub sea_level: f32,
    pub temperature_bias: f32,
    pub moisture_bias: f32,
    pub render_scale: u32,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            seed: 1,
            width: 384,
            height: 384,
            sea_level: 0.52,
            temperature_bias: 0.0,
            moisture_bias: 0.0,
            render_scale: 4,
        }
    }
}

impl WorldConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.width < 32 || self.height < 32 {
            return Err("width and height must be at least 32".into());
        }
        if self.width > 2048 || self.height > 2048 {
            return Err("width and height must be at most 2048".into());
        }
        if !(0.2..=0.8).contains(&self.sea_level) {
            return Err("sea level must be between 0.2 and 0.8".into());
        }
        if self.render_scale == 0 || self.render_scale > 32 {
            return Err("render scale must be between 1 and 32".into());
        }
        Ok(())
    }
}
