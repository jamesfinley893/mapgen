use std::collections::VecDeque;

use noise::OpenSimplex;

use crate::{Surface, World, WorldConfig};

use super::util::{hash01, latitude_factor, normalize, octave_noise, smoothstep};

pub(super) fn populate_base_climate(
    world: &mut World,
    config: &WorldConfig,
    ocean: &[bool],
    distance_to_ocean: &[u16],
    climate: &OpenSimplex,
) {
    let nearby_water = vec![0.0_f32; world.tiles.len()];
    let regional_continentality = compute_regional_continentality(world, ocean);
    populate_climate_from_fields(
        world,
        config,
        ocean,
        distance_to_ocean,
        climate,
        &nearby_water,
        &regional_continentality,
    );
}

pub(super) fn populate_climate(
    world: &mut World,
    config: &WorldConfig,
    ocean: &[bool],
    distance_to_ocean: &[u16],
    climate: &OpenSimplex,
) {
    let nearby_water = compute_nearby_water(world);
    let regional_continentality = compute_regional_continentality(world, ocean);
    populate_climate_from_fields(
        world,
        config,
        ocean,
        distance_to_ocean,
        climate,
        &nearby_water,
        &regional_continentality,
    );
}

fn populate_climate_from_fields(
    world: &mut World,
    config: &WorldConfig,
    ocean: &[bool],
    distance_to_ocean: &[u16],
    climate: &OpenSimplex,
    nearby_water: &[f32],
    regional_continentality: &[f32],
) {
    let wind_tilt = prevailing_wind_angle(world.seed);
    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            let elevation = world.tiles[idx].raw_elevation;
            let lat = latitude_factor(y, world.height);
            let wind = wind_at_latitude(wind_tilt, lat);
            let climate_noise =
                octave_noise(climate, x as f64 * 0.008, y as f64 * 0.008, 3, 0.5, 2.0);
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
            let subtropical_cooling =
                smoothstep(0.16, 0.34, lat) * (1.0_f32 - smoothstep(0.46, 0.68, lat));
            let maritime_temp =
                nearby_water[idx] * 0.06 + (1.0 - regional_continentality[idx]) * 0.06;
            let temperature = (equatorial_warmth * 0.82 - subtropical_cooling * 0.04
                + climate_noise * 0.13
                + seasonal_noise * 0.08
                + maritime_temp
                - elevation * 0.34
                - regional_continentality[idx] * 0.07 * lowland
                + config.temperature_bias)
                .clamp(0.0, 1.0);
            let fields = MoistureFields {
                ocean,
                distance_to_ocean,
                climate,
                nearby_water,
                regional_continentality,
                wind,
            };
            let moisture = (moisture_value(world, &fields, x, y) * config.rainfall_scale
                + config.moisture_bias)
                .clamp(0.0, 1.0);
            world.tiles[idx].temperature = temperature;
            world.tiles[idx].moisture = moisture;
            world.tiles[idx].precipitation = moisture;
        }
    }
}

fn compute_nearby_water(world: &World) -> Vec<f32> {
    let mut nearby = vec![0.0_f32; world.tiles.len()];
    for idx in 0..world.tiles.len() {
        if matches!(
            world.tiles[idx].surface,
            Surface::Ocean | Surface::Lake | Surface::River
        ) {
            nearby[idx] = 1.0;
            let (x, y) = world.coords(idx);
            for (nx, ny) in world.neighbors8(x, y) {
                let nidx = world.idx(nx, ny);
                nearby[nidx] = nearby[nidx].max(0.35);
            }
        }
    }
    nearby
}

fn compute_regional_continentality(world: &World, ocean: &[bool]) -> Vec<f32> {
    let mut field = vec![0.0_f32; world.tiles.len()];
    let max_extent = (world.width.max(world.height) as f32 * 0.36).max(1.0);
    for idx in 0..world.tiles.len() {
        if ocean[idx] {
            continue;
        }
        let (x, y) = world.coords(idx);
        let mut ocean_hits = 0.0_f32;
        let mut weighted_distance = 0.0_f32;
        for (dx, dy, weight) in [
            (-1_isize, 0_isize, 1.0_f32),
            (1, 0, 1.0),
            (0, -1, 0.85),
            (0, 1, 0.85),
        ] {
            for step in 1..=18 {
                let nx = x as isize + dx * step;
                let ny = y as isize + dy * step;
                if !world.in_bounds(nx, ny) {
                    break;
                }
                let nidx = world.idx(nx as usize, ny as usize);
                if ocean[nidx] {
                    ocean_hits += weight;
                    weighted_distance += (step as f32 / 18.0) * weight;
                    break;
                }
            }
        }
        let openness = (ocean_hits / 3.7).clamp(0.0, 1.0);
        let mean_fetch = if ocean_hits > f32::EPSILON {
            (weighted_distance / ocean_hits).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let distance_bias = (((x.min(world.width - 1 - x) + y.min(world.height - 1 - y)) as f32)
            / max_extent)
            .clamp(0.0, 1.0);
        field[idx] =
            (distance_bias * 0.5 + mean_fetch * 0.35 + (1.0 - openness) * 0.25).clamp(0.0, 1.0);
    }
    field
}

pub(super) fn fill_ocean_distance(world: &World, ocean: &[bool]) -> Vec<u16> {
    let mut out = vec![u16::MAX; world.tiles.len()];
    let mut queue = VecDeque::new();
    for (idx, is_ocean) in ocean.iter().enumerate() {
        if *is_ocean {
            out[idx] = 0;
            queue.push_back(idx);
        }
    }
    while let Some(idx) = queue.pop_front() {
        let (x, y) = world.coords(idx);
        let next_dist = out[idx].saturating_add(1);
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            if out[nidx] > next_dist {
                out[nidx] = next_dist;
                queue.push_back(nidx);
            }
        }
    }
    out
}

struct MoistureFields<'a> {
    ocean: &'a [bool],
    distance_to_ocean: &'a [u16],
    climate: &'a OpenSimplex,
    nearby_water: &'a [f32],
    regional_continentality: &'a [f32],
    wind: (f32, f32),
}

fn moisture_value(world: &World, fields: &MoistureFields<'_>, x: usize, y: usize) -> f32 {
    let idx = world.idx(x, y);
    if fields.ocean[idx] {
        return 1.0;
    }

    let ocean_influence = 1.0
        - (fields.distance_to_ocean[idx] as f32 / (world.width.max(world.height) as f32 * 0.45))
            .clamp(0.0, 1.0);
    let shadow = rain_shadow(world, fields.ocean, fields.wind, x, y);
    let lat = latitude_factor(y, world.height);
    // Wider transitions break the sharp moisture stripe at the Hadley cell boundary.
    let subtropical_dryness = smoothstep(0.12, 0.36, lat) * (1.0_f32 - smoothstep(0.40, 0.66, lat));
    let equatorial_wetness = (1.0_f32 - smoothstep(0.0, 0.40, lat)).clamp(0.0, 1.0);
    let polar_dryness = smoothstep(0.68, 0.92, lat);
    let zonal =
        (0.22 + equatorial_wetness * 0.32 - subtropical_dryness * 0.22 - polar_dryness * 0.08)
            .clamp(0.0, 1.0);
    let noise = octave_noise(
        fields.climate,
        x as f64 * 0.014 + 7.0,
        y as f64 * 0.014 - 9.0,
        4,
        0.55,
        2.0,
    );
    let monsoon = octave_noise(
        fields.climate,
        x as f64 * 0.006 - 41.0,
        y as f64 * 0.006 + 17.0,
        3,
        0.55,
        2.0,
    );
    let continentality = fields.regional_continentality[idx];
    let lowland = 1.0 - ((world.tiles[idx].raw_elevation - world.sea_level) / 0.24).clamp(0.0, 1.0);

    // Shift weight from the latitude-band (zonal) and directional rain-shadow terms toward
    // noise, so biome zones are geographically varied rather than strict horizontal bands.
    (ocean_influence * 0.34
        + zonal * 0.18
        + noise * 0.22
        + monsoon * 0.08 * equatorial_wetness
        + shadow * 0.14
        + fields.nearby_water[idx] * 0.16
        - continentality * 0.20 * (0.7 + subtropical_dryness * 0.45) * lowland)
        .clamp(0.0, 1.0)
}

// Returns angle offset (radians) from pure westerly. Varies per seed so each world
// has a distinct prevailing wind direction (±PI/4 range).
fn prevailing_wind_angle(seed: u64) -> f32 {
    (hash01(seed, 0x7135_DE00, 1) - 0.5) * std::f32::consts::FRAC_PI_2
}

// Hadley cell model: tropical easterlies, mid-latitude westerlies, polar easterlies.
// world_tilt rotates the whole pattern so each seed has a distinct slant.
fn wind_at_latitude(world_tilt: f32, lat: f32) -> (f32, f32) {
    let zonal = if !(0.24..=0.70).contains(&lat) {
        -1.0_f32 // easterlies
    } else {
        1.0_f32 // westerlies
    };
    let (s, c) = world_tilt.sin_cos();
    normalize((zonal * c, zonal * s))
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
        barrier += (world.tiles[nidx].raw_elevation - world.sea_level).max(0.0) * 0.09;
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
