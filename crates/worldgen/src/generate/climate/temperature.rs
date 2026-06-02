use noise::OpenSimplex;

use crate::generate::util::{octave_noise, smoothstep};
use crate::{World, WorldConfig};

use super::fields::ClimateFields;

pub(super) fn sample_temperature(
    world: &World,
    config: &WorldConfig,
    fields: &ClimateFields,
    climate: &OpenSimplex,
    x: usize,
    y: usize,
    lat: f32,
) -> f32 {
    let idx = world.idx(x, y);
    let elevation = world.tiles[idx].elevation;
    let climate_noise = octave_noise(climate, x as f64 * 0.008, y as f64 * 0.008, 3, 0.5, 2.0);
    let seasonal_noise = octave_noise(
        climate,
        x as f64 * 0.004 - 19.0,
        y as f64 * 0.004 + 31.0,
        2,
        0.5,
        2.0,
    );
    let lowland = 1.0 - ((elevation - world.sea_level) / 0.24).clamp(0.0, 1.0);
    let equatorial_warmth = (1.0 - lat.powf(1.08)).clamp(0.0, 1.0);
    let subtropical_cooling = smoothstep(0.16, 0.34, lat) * (1.0_f32 - smoothstep(0.46, 0.68, lat));
    let nw = fields.nearby_water[idx];
    let maritime_temp = nw * 0.06 + (1.0 - fields.regional_continentality[idx]) * 0.06;

    (equatorial_warmth * 0.82 - subtropical_cooling * 0.04
        + climate_noise * 0.13
        + seasonal_noise * 0.08
        + maritime_temp
        - elevation * 0.34
        - fields.regional_continentality[idx] * 0.07 * lowland
        + config.temperature_bias)
        .clamp(0.0, 1.0)
}
