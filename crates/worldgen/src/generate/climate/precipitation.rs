use noise::OpenSimplex;

use crate::generate::util::{octave_noise, smoothstep};
use crate::{World, WorldConfig};

use super::fields::ClimateFields;

const RAIN_SHADOW_WORLD_FRACTION: f32 = 0.28;
const RAIN_SHADOW_MAP_FRACTION: f32 = 0.42;
const MAX_RAIN_SHADOW_STEPS: usize = 192;

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

        let maritime = self.fields.maritime_influence[idx];
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
        let continentality = self.fields.continentality[idx];
        let lowland = 1.0 - ((world.tiles[idx].elevation - world.sea_level) / 0.24).clamp(0.0, 1.0);

        // Keep latitude bands secondary to geography and noise so lowlands do not collapse
        // into strict horizontal biome zones.
        (maritime * 0.34
            + zonal * 0.18
            + noise * 0.22
            + monsoon * 0.08 * equatorial_wetness
            + shadow * 0.14
            - continentality * 0.20 * (0.7 + subtropical_dryness * 0.45) * lowland)
            .clamp(0.0, 1.0)
    }
}

struct WindScan {
    ocean_fetch: f32,
    barrier: f32,
}

fn rain_shadow(world: &World, ocean: &[bool], wind: (f32, f32), x: usize, y: usize) -> f32 {
    // Scan upwind for ocean fetch while normalizing intervening terrain barriers by range.
    let upwind = (-wind.0, -wind.1);
    let scan_steps = rain_shadow_scan_steps(world);
    let windward = scan_wind_path(world, ocean, upwind, x, y, scan_steps);
    let leeward = if windward.ocean_fetch <= 0.02 {
        scan_wind_path(world, ocean, wind, x, y, (scan_steps / 2).max(1)).ocean_fetch * 0.25
    } else {
        0.0
    };

    (windward.ocean_fetch + leeward - windward.barrier * 0.78).clamp(-0.65, 1.0)
}

fn scan_wind_path(
    world: &World,
    ocean: &[bool],
    direction: (f32, f32),
    x: usize,
    y: usize,
    scan_steps: usize,
) -> WindScan {
    let mut ocean_fetch = 0.0_f32;
    let mut barrier = 0.0_f32;
    let mut total_weight = 0.0_f32;

    for step in 1..=scan_steps {
        let nx = (x as f32 + direction.0 * step as f32).round() as isize;
        let ny = (y as f32 + direction.1 * step as f32).round() as isize;
        if !world.in_bounds(nx, ny) {
            break;
        }

        let nidx = world.idx(nx as usize, ny as usize);
        let proximity = 1.0 - smoothstep(0.0, 1.0, step as f32 / scan_steps as f32);
        total_weight += proximity;

        if ocean[nidx] {
            ocean_fetch += proximity;
        } else {
            let above_sea = (world.tiles[nidx].elevation - world.sea_level).max(0.0);
            barrier += smoothstep(0.05, 0.45, above_sea) * proximity;
        }
    }

    if total_weight <= f32::EPSILON {
        WindScan {
            ocean_fetch: 0.0,
            barrier: 0.0,
        }
    } else {
        WindScan {
            ocean_fetch: (ocean_fetch / total_weight).clamp(0.0, 1.0),
            barrier: (barrier / total_weight).clamp(0.0, 1.0),
        }
    }
}

fn rain_shadow_scan_steps(world: &World) -> usize {
    let max_available = world.width.max(world.height).saturating_sub(1).max(1);
    let min_steps = 8.min(max_available);
    let max_steps = MAX_RAIN_SHADOW_STEPS.min(max_available).max(min_steps);
    let world_steps = (world.effective_world_size() * RAIN_SHADOW_WORLD_FRACTION).round() as usize;
    let map_steps =
        (world.width.max(world.height) as f32 * RAIN_SHADOW_MAP_FRACTION).round() as usize;
    world_steps
        .min(map_steps)
        .max(1)
        .clamp(min_steps, max_steps)
}
