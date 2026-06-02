use noise::OpenSimplex;

use crate::generate::util::{octave_noise, smoothstep};
use crate::{World, WorldConfig};

use super::fields::ClimateFields;

pub(super) struct PrecipitationModel<'a> {
    fields: &'a ClimateFields,
    climate: &'a OpenSimplex,
}

impl<'a> PrecipitationModel<'a> {
    pub(super) fn new(fields: &'a ClimateFields, climate: &'a OpenSimplex) -> Self {
        Self { fields, climate }
    }

    pub(super) fn sample_precipitation(
        &self,
        world: &World,
        config: &WorldConfig,
        wind: (f32, f32),
        x: usize,
        y: usize,
        lat: f32,
    ) -> f32 {
        (self.raw_precipitation(world, wind, x, y, lat) * config.rainfall_scale
            + config.moisture_bias)
            .clamp(0.0, 1.0)
    }

    fn raw_precipitation(
        &self,
        world: &World,
        wind: (f32, f32),
        x: usize,
        y: usize,
        lat: f32,
    ) -> f32 {
        let idx = world.idx(x, y);
        if self.fields.ocean[idx] {
            return 1.0;
        }

        let ocean_influence = 1.0
            - (self.fields.distance_to_ocean[idx] as f32
                / (world.width.max(world.height) as f32 * 0.45))
                .clamp(0.0, 1.0);
        let shadow = rain_shadow(world, &self.fields.ocean, wind, x, y);
        // Wider transitions break the sharp moisture stripe at the Hadley cell boundary.
        let subtropical_dryness =
            smoothstep(0.12, 0.36, lat) * (1.0_f32 - smoothstep(0.40, 0.66, lat));
        let equatorial_wetness = (1.0_f32 - smoothstep(0.0, 0.40, lat)).clamp(0.0, 1.0);
        let polar_dryness = smoothstep(0.68, 0.92, lat);
        let zonal =
            (0.22 + equatorial_wetness * 0.32 - subtropical_dryness * 0.22 - polar_dryness * 0.08)
                .clamp(0.0, 1.0);
        let noise = octave_noise(
            self.climate,
            x as f64 * 0.014 + 7.0,
            y as f64 * 0.014 - 9.0,
            4,
            0.55,
            2.0,
        );
        let monsoon = octave_noise(
            self.climate,
            x as f64 * 0.006 - 41.0,
            y as f64 * 0.006 + 17.0,
            3,
            0.55,
            2.0,
        );
        let continentality = self.fields.regional_continentality[idx];
        let lowland = 1.0 - ((world.tiles[idx].elevation - world.sea_level) / 0.24).clamp(0.0, 1.0);

        // Shift weight from the latitude-band (zonal) and directional rain-shadow terms toward
        // noise, so biome zones are geographically varied rather than strict horizontal bands.
        (ocean_influence * 0.34
            + zonal * 0.18
            + noise * 0.22
            + monsoon * 0.08 * equatorial_wetness
            + shadow * 0.14
            + self.fields.nearby_water[idx] * 0.16
            - continentality * 0.20 * (0.7 + subtropical_dryness * 0.45) * lowland)
            .clamp(0.0, 1.0)
    }
}

fn rain_shadow(world: &World, ocean: &[bool], wind: (f32, f32), x: usize, y: usize) -> f32 {
    // Scan upwind for a moisture source, accumulating terrain barriers along the way.
    let upwind = (-wind.0, -wind.1);
    let mut moisture = 0.0_f32;
    let mut barrier = 0.0_f32;
    let mut found_ocean = false;

    for step in 1_usize..=16 {
        let nx = (x as f32 + upwind.0 * step as f32).round() as isize;
        let ny = (y as f32 + upwind.1 * step as f32).round() as isize;
        if !world.in_bounds(nx, ny) {
            break;
        }
        let nidx = world.idx(nx as usize, ny as usize);
        if ocean[nidx] {
            // Moisture decays with distance from coast so far-inland tiles still dry out.
            let proximity = 1.0 - (step as f32 - 1.0) / 16.0;
            moisture += 0.12 * proximity.max(0.03);
            found_ocean = true;
            break;
        }
        barrier += (world.tiles[nidx].elevation - world.sea_level).max(0.0) * 0.09;
    }

    if !found_ocean {
        // Leeward scan: minor contribution from the downwind direction.
        for step in 1_usize..=8 {
            let nx = (x as f32 + wind.0 * step as f32).round() as isize;
            let ny = (y as f32 + wind.1 * step as f32).round() as isize;
            if !world.in_bounds(nx, ny) {
                break;
            }
            let nidx = world.idx(nx as usize, ny as usize);
            if ocean[nidx] {
                moisture += 0.04;
                break;
            }
        }
    }

    (moisture - barrier * 0.60).clamp(0.0, 1.0)
}
