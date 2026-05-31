use noise::OpenSimplex;

mod continents;
mod normalize;
mod tectonics;

use crate::World;

use super::util::{latitude_factor, smoothstep};
use continents::build_continental_config;
use normalize::normalize_terrain;
use tectonics::{generate_plates, sample_tectonic_elevation};

const TERRAIN_RELAX_STEPS: usize = 18;

#[derive(Clone, Copy)]
struct Plate {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
}

#[derive(Clone, Copy)]
struct OrogenSample {
    basement: f32,
    axial_uplift: f32,
    shoulder_uplift: f32,
    plateau_support: f32,
    foreland_loading: f32,
    backarc_loading: f32,
    craton_stability: f32,
    basin_bias: f32,
}

#[derive(Clone, Copy)]
struct ContinentalFields {
    support: f32,
    interior: f32,
    seaway_cut: f32,
    ocean_basin: f32,
    major_secondary_balance: f32,
}

// Precomputed per-lobe parameters (seed-only, not tile-dependent).
struct PreparedLobe {
    cx: f32,
    cy: f32,
    sin_a: f32,
    cos_a: f32,
    rx: f32,
    ry: f32,
    strength: f32,
}

struct PreparedCut {
    cx: f32,
    cy: f32,
    sin_a: f32,
    cos_a: f32,
    width: f32,
    extent: f32,
    strength: f32,
}

struct ContinentalConfig {
    land_lobes: Vec<PreparedLobe>,
    basins: Vec<PreparedLobe>,
    seaways: Vec<PreparedCut>,
}

pub(super) fn populate_raw_elevation(world: &mut World, base: &OpenSimplex, ridge: &OpenSimplex) {
    let ws = world.effective_world_size();
    let world_units_x = world.width as f32 / ws;
    let world_units_y = world.height as f32 / ws;
    let plates = generate_plates(world);
    let continental_config = build_continental_config(world.seed, world_units_x, world_units_y);

    let mut basement = vec![0.0_f32; world.tiles.len()];
    let mut axial_uplift = vec![0.0_f32; world.tiles.len()];
    let mut shoulder_uplift = vec![0.0_f32; world.tiles.len()];
    let mut plateau_support = vec![0.0_f32; world.tiles.len()];
    let mut foreland_loading = vec![0.0_f32; world.tiles.len()];
    let mut backarc_loading = vec![0.0_f32; world.tiles.len()];
    let mut craton_stability = vec![0.0_f32; world.tiles.len()];
    let mut basin_bias = vec![0.0_f32; world.tiles.len()];

    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            let sample =
                sample_tectonic_elevation(world, base, ridge, &plates, &continental_config, x, y);
            basement[idx] = sample.basement;
            axial_uplift[idx] = sample.axial_uplift;
            shoulder_uplift[idx] = sample.shoulder_uplift;
            plateau_support[idx] = sample.plateau_support;
            foreland_loading[idx] = sample.foreland_loading;
            backarc_loading[idx] = sample.backarc_loading;
            craton_stability[idx] = sample.craton_stability;
            basin_bias[idx] = sample.basin_bias;
        }
    }

    let mut terrain = basement
        .iter()
        .zip(foreland_loading.iter())
        .zip(backarc_loading.iter())
        .map(|((base, foreland), backarc)| (base - foreland * 0.16 - backarc * 0.09).max(0.0))
        .collect::<Vec<_>>();
    let uplift_per_step = 0.145 / TERRAIN_RELAX_STEPS as f32;

    for step in 0..TERRAIN_RELAX_STEPS {
        let progress = (step + 1) as f32 / TERRAIN_RELAX_STEPS as f32;
        for idx in 0..terrain.len() {
            let uplift_core =
                smoothstep(0.48, 0.92, axial_uplift[idx] + shoulder_uplift[idx] * 0.24);
            let orogen_margin = smoothstep(
                0.16,
                0.54,
                shoulder_uplift[idx] + plateau_support[idx] * 0.55,
            );
            let elevation_damping = 1.0
                - smoothstep(0.54, 0.86, terrain[idx]) * (0.48 + orogen_margin * 0.18)
                - smoothstep(0.72, 0.94, terrain[idx]) * (0.22 + (1.0 - uplift_core) * 0.16);
            let uplift_add = axial_uplift[idx] * uplift_per_step * (0.98 + progress * 0.46)
                + shoulder_uplift[idx] * uplift_per_step * 0.34 * (0.72 + progress * 0.18)
                + plateau_support[idx] * uplift_per_step * 0.14;
            let core_boost =
                0.82 + uplift_core * 0.56 + axial_uplift[idx] * 0.12 - basin_bias[idx] * 0.12;
            terrain[idx] += uplift_add * elevation_damping.max(0.18) * core_boost;
        }

        let mut next = terrain.clone();

        for idx in 0..terrain.len() {
            let (x, y) = world.coords(idx);
            let current = terrain[idx];
            let is_ocean = current <= world.sea_level;
            let mut avg = 0.0;
            let mut count = 0.0;
            let mut max_neighbor_drop = 0.0_f32;
            let mut ocean_neighbors = 0.0_f32;

            for (nx, ny) in world.neighbors8(x, y) {
                let nidx = world.idx(nx, ny);
                let neighbor = terrain[nidx];
                avg += neighbor;
                count += 1.0;
                max_neighbor_drop = max_neighbor_drop.max((current - neighbor).max(0.0));
                if neighbor <= world.sea_level {
                    ocean_neighbors += 1.0;
                }
            }

            let avg_neighbor = if count > 0.0 { avg / count } else { current };
            let relief = (current - basement[idx]).max(0.0);
            let highland = smoothstep(0.52, 0.76, current);
            let alpine = smoothstep(0.7, 0.92, current);
            let super_alpine = smoothstep(0.82, 0.98, current);
            let uplift_core =
                smoothstep(0.48, 0.92, axial_uplift[idx] + shoulder_uplift[idx] * 0.24);
            let interior_high = highland * (1.0 - uplift_core);
            let shoulder_zone = smoothstep(
                0.12,
                0.48,
                shoulder_uplift[idx] + plateau_support[idx] * 0.4,
            );
            let plain_zone = smoothstep(0.38, 0.82, craton_stability[idx]) * (1.0 - uplift_core);
            let basin_zone = smoothstep(
                0.24,
                0.72,
                basin_bias[idx] + foreland_loading[idx] * 0.55 + backarc_loading[idx] * 0.45,
            );
            let lat = latitude_factor(y, world.height);
            let snowline = (0.84 - lat * 0.22 - plateau_support[idx] * 0.04).clamp(0.58, 0.88);
            let glacial_band = smoothstep(snowline, (snowline + 0.12).min(0.98), current);
            let ridge_crest = highland * (1.0 - smoothstep(0.012, 0.075, max_neighbor_drop));
            let coastal = if is_ocean {
                0.0
            } else {
                ocean_neighbors / count.max(1.0)
            };
            let diffusion = (avg_neighbor - current)
                * (0.019 * (1.0 - ridge_crest * 0.65)
                    + max_neighbor_drop * 0.034
                    + interior_high * 0.046 * (1.0 - ridge_crest * 0.55)
                    + shoulder_zone * 0.022
                    + plain_zone * 0.03
                    + basin_zone * 0.028
                    + coastal * 0.025
                    + alpine * 0.016 * (1.0 - uplift_core * 0.35)
                    + glacial_band * 0.02);
            let slope_failure = max_neighbor_drop
                * (0.011
                    + interior_high * 0.03
                    + shoulder_zone * 0.012
                    + alpine * 0.015 * (1.0 - uplift_core * 0.18)
                    + glacial_band * 0.012);
            let ridge_decay = relief
                * (0.013
                    + interior_high * 0.044
                    + shoulder_zone * 0.022
                    + alpine * 0.016 * (1.0 - uplift_core * 0.3))
                * (1.0 - axial_uplift[idx] * 0.22).max(0.48);
            let alpine_relax = interior_high * 0.018
                + shoulder_zone * 0.008
                + alpine * (0.012 + (1.0 - uplift_core) * 0.016)
                + super_alpine * (0.016 + (1.0 - uplift_core) * 0.02)
                + (current - 0.84).max(0.0) * (0.026 + (1.0 - uplift_core) * 0.026);
            let glacial_relief_softening = glacial_band
                * (0.004 + max_neighbor_drop * 0.012 + relief * 0.008)
                * (0.5 + axial_uplift[idx] * 0.62 + shoulder_uplift[idx] * 0.18);
            let coastal_planation = coastal
                * (0.0015 + plain_zone * 0.004 + basin_zone * 0.003)
                * (0.7 + progress * 0.3);
            let plain_planation = relief
                * (0.012 + plain_zone * 0.038 + basin_zone * 0.025)
                * (1.0 - uplift_core * 0.75);
            let shoulder_denudation = relief
                * (0.008 + shoulder_zone * 0.032 + interior_high * 0.018)
                * (1.0 - axial_uplift[idx] * 0.65);
            let basin_subsidence = (foreland_loading[idx] * 0.0105
                + backarc_loading[idx] * 0.007
                + basin_zone * 0.004
                + shoulder_zone * 0.0025 * (1.0 - uplift_core))
                * (0.62 + progress * 0.48);

            let land_delta = diffusion
                - ridge_decay
                - alpine_relax
                - slope_failure
                - glacial_relief_softening
                - plain_planation
                - shoulder_denudation
                - basin_subsidence
                - coastal_planation;

            next[idx] = if is_ocean {
                current
            } else {
                (current + land_delta).max(world.sea_level + 0.001)
            };
        }

        terrain = next;
    }

    normalize_terrain(&mut terrain, 0.02, 0.98);
    for (tile, value) in world.tiles.iter_mut().zip(terrain.into_iter()) {
        tile.raw_elevation = value;
    }

    // If the lobes happen to land mostly off-screen the config sea_level can leave
    // less than 25% of tiles as land. Lower sea_level just enough to clear that floor;
    // seeds with normal coverage are unaffected.
    const MIN_LAND_FRAC: f32 = 0.25;
    let mut elevs: Vec<f32> = world.tiles.iter().map(|t| t.raw_elevation).collect();
    elevs.sort_by(|a, b| a.total_cmp(b));
    let threshold_idx =
        ((elevs.len() as f32 * (1.0 - MIN_LAND_FRAC)) as usize).min(elevs.len().saturating_sub(1));
    world.sea_level = world.sea_level.min(elevs[threshold_idx]);
}
