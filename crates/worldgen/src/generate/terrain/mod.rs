use noise::OpenSimplex;

mod continents;
mod erosion;
mod tectonics;

use crate::World;

use super::EROSION_STEPS;
use super::util::{latitude_factor, sample_seed_field, smoothstep};
use continents::build_continental_config;
use erosion::{normalize_terrain, routing_field_vector, simulate_erosion_flow};
use tectonics::{generate_plates, sample_tectonic_elevation};

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

struct ErosionFlow {
    downstream: Vec<Option<usize>>,
    contributing_area: Vec<f32>,
    local_slope: Vec<f32>,
    confinement: Vec<f32>,
    valley_scale: Vec<f32>,
    transport_capacity: Vec<f32>,
    sediment_flux: Vec<f32>,
    deposition: Vec<f32>,
    floodplain_scale: Vec<f32>,
    is_ocean: Vec<bool>,
}

pub(super) fn populate_raw_elevation(world: &mut World, base: &OpenSimplex, ridge: &OpenSimplex) {
    let ws = world.effective_world_size();
    let world_units_x = world.width as f32 / ws;
    let world_units_y = world.height as f32 / ws;
    let plates = generate_plates(world);
    let continental_config = build_continental_config(world.seed, world_units_x, world_units_y);
    // Scale noise cell sizes so texture frequency stays geographically consistent at any resolution.
    let res_x = (world.width as f32 / ws).max(1.0);
    let n = world.tiles.len();
    let routing_noise_field: Vec<(f32, f32)> = (0..n)
        .map(|idx| {
            let (x, y) = world.coords(idx);
            routing_field_vector(
                world.seed,
                x,
                y,
                (20.0 * res_x).round().max(1.0) as usize,
                1,
            )
        })
        .collect();
    let flow_opportunity: Vec<f32> = (0..n)
        .map(|idx| {
            let (x, y) = world.coords(idx);
            sample_seed_field(
                world.seed,
                x,
                y,
                (24.0 * res_x).round().max(1.0) as usize,
                0xA11E_0001,
            )
        })
        .collect();
    let trib_opportunity: Vec<f32> = (0..n)
        .map(|idx| {
            let (x, y) = world.coords(idx);
            sample_seed_field(
                world.seed,
                x,
                y,
                (28.0 * res_x).round().max(1.0) as usize,
                0xA11E_0101,
            )
        })
        .collect();
    let meander_field: Vec<f32> = (0..n)
        .map(|idx| {
            let (x, y) = world.coords(idx);
            sample_seed_field(
                world.seed,
                x,
                y,
                (18.0 * res_x).round().max(1.0) as usize,
                0xA11E_0002,
            ) * 2.0
                - 1.0
        })
        .collect();
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
    let uplift_per_step = 0.145 / EROSION_STEPS as f32;

    // Pre-carve proto-valleys before the erosion loop. On a convex mountain
    // surface drainage diverges, so the stream-power positive-feedback loop
    // (lower elevation → more flow → deeper incision) never starts — most
    // tiles accumulate only CA=5–50, making the stream-power term negligible.
    // One flow-accumulation pass here creates slight concavities along the
    // major drainage paths; the 18 erosion steps amplify them into real valleys.
    {
        let seed_flow = simulate_erosion_flow(
            world,
            &terrain,
            &routing_noise_field,
            &flow_opportunity,
            &trib_opportunity,
            &meander_field,
        );
        for idx in 0..terrain.len() {
            if seed_flow.is_ocean[idx] {
                continue;
            }
            let ca = seed_flow.contributing_area[idx];
            if ca < 50.0 {
                continue;
            }
            let highland = smoothstep(0.52, 0.76, terrain[idx]);
            let ca_scale = smoothstep(50.0, 1200.0, ca);
            terrain[idx] = (terrain[idx] - ca_scale * highland * 0.055).max(0.0);
        }
    }

    let mut valley_memory = vec![0.0_f32; terrain.len()];

    for step in 0..EROSION_STEPS {
        let progress = (step + 1) as f32 / EROSION_STEPS as f32;
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

        let flow = simulate_erosion_flow(
            world,
            &terrain,
            &routing_noise_field,
            &flow_opportunity,
            &trib_opportunity,
            &meander_field,
        );
        for idx in 0..terrain.len() {
            if flow.is_ocean[idx] {
                valley_memory[idx] *= 0.86;
                continue;
            }
            let highland_path = smoothstep(0.50, 0.74, terrain[idx]);
            let drainage_strength = smoothstep(24.0, 2400.0, flow.contributing_area[idx])
                * (0.62 + smoothstep(0.002, 0.07, flow.local_slope[idx]) * 0.38)
                * highland_path;
            valley_memory[idx] = (valley_memory[idx] * 0.88).max(drainage_strength);
        }
        let mut next = terrain.clone();
        let mut lateral_erosion = vec![0.0_f32; terrain.len()];

        for idx in 0..terrain.len() {
            let (x, y) = world.coords(idx);
            let current = terrain[idx];
            let mut avg = 0.0;
            let mut count = 0.0;
            let mut max_neighbor_drop = 0.0_f32;

            for (nx, ny) in world.neighbors8(x, y) {
                let nidx = world.idx(nx, ny);
                let neighbor = terrain[nidx];
                avg += neighbor;
                count += 1.0;
                max_neighbor_drop = max_neighbor_drop.max((current - neighbor).max(0.0));
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
            let ca = flow.contributing_area[idx].max(1.0);
            let slope = flow.local_slope[idx];
            // Stream power law concentrates incision along high-CA paths. Old ln(CA)*slope
            // gave trunk rivers (lower slope) less erosion than steep headwaters — the
            // correct physics inverts this: large upstream catchments drive valley deepening.
            // The log-area base term preserves baseline erosion in low-discharge zones.
            let stream_power_val = ca.powf(0.55) * slope.max(0.0008).powf(0.70);
            let incision = if flow.is_ocean[idx] {
                0.0
            } else {
                let discharge = ca.ln();
                let relief_factor = 0.58
                    + relief * 2.7
                    + interior_high * 1.18
                    + alpine * (0.62 + uplift_core * 0.72);
                let tectonic_factor = 0.28 + axial_uplift[idx] * 1.54 + shoulder_uplift[idx] * 0.42;
                let alluvial_brake = 1.0 - flow.floodplain_scale[idx] * 0.55;
                (discharge * slope * 0.005 + stream_power_val * 0.0018)
                    * relief_factor
                    * tectonic_factor
                    * alluvial_brake
            };
            let confinement = flow.confinement[idx];
            let valley_scale = flow.valley_scale[idx];
            let floodplain_scale = flow.floodplain_scale[idx];
            let deposition = flow.deposition[idx];
            let sediment_flux = flow.sediment_flux[idx];
            let transport_capacity = flow.transport_capacity[idx];
            let persistent_valley = valley_memory[idx] * highland;
            let trunk_persistence = persistent_valley * smoothstep(80.0, 2200.0, ca);
            let sediment_ratio = if transport_capacity <= f32::EPSILON {
                0.0
            } else {
                (sediment_flux / transport_capacity).clamp(0.0, 2.5)
            };
            // Interfluve crests (low contributing area, high elevation) should stay sharp;
            // diffusion alone rounds them into broad domes, erasing watershed divides.
            let ridge_crest = (1.0 - smoothstep(1.0, 15.0, ca)) * highland;
            let diffusion = (avg_neighbor - current)
                * (0.019 * (1.0 - ridge_crest * 0.65)
                    + max_neighbor_drop * 0.034
                    + interior_high * 0.046 * (1.0 - ridge_crest * 0.55)
                    + shoulder_zone * 0.022
                    + plain_zone * 0.03
                    + basin_zone * 0.028
                    + valley_scale * 0.022 * (1.0 - confinement)
                    + floodplain_scale * (0.02 + sediment_ratio.min(1.0) * 0.012)
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
            let glacial_erosion = glacial_band
                * (0.004 + max_neighbor_drop * 0.012 + relief * 0.008)
                * (0.5 + axial_uplift[idx] * 0.62 + shoulder_uplift[idx] * 0.18);
            let valley_floor_lowering = valley_scale
                * (1.0 - confinement)
                * (0.0015 + plain_zone * 0.003 + basin_zone * 0.0025 + highland * 0.0030)
                * (0.55 + smoothstep(0.0, 0.09, flow.local_slope[idx]));
            let persistent_corridor_lowering = persistent_valley
                * (0.0035 + trunk_persistence * 0.018 + stream_power_val.min(12.0) * 0.0016)
                * (0.55 + (1.0 - confinement) * 0.45)
                * (0.7 + progress * 0.55);
            let alluvial_fill = deposition
                * floodplain_scale
                * (0.006
                    + plain_zone * 0.01
                    + basin_zone * 0.012
                    + sediment_ratio.min(1.2) * 0.004)
                * (0.65 + (1.0 - confinement) * 0.35);
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

            next[idx] = (current - incision + diffusion
                - ridge_decay
                - alpine_relax
                - slope_failure
                - glacial_erosion
                - plain_planation
                - shoulder_denudation
                - basin_subsidence
                - valley_floor_lowering
                - persistent_corridor_lowering
                + alluvial_fill)
                .max(0.0);

            if !flow.is_ocean[idx]
                && (valley_scale > 0.02 || persistent_valley > 0.04)
                && let Some(next_idx) = flow.downstream[idx]
            {
                let (nx, ny) = world.coords(next_idx);
                let step_x = (nx as isize - x as isize).signum();
                let step_y = (ny as isize - y as isize).signum();
                if step_x != 0 || step_y != 0 {
                    let side_a = (-step_y, step_x);
                    let side_b = (step_y, -step_x);
                    let lateral_strength = (valley_scale
                        * (1.0 - confinement)
                        * (0.0024 + basin_zone * 0.003 + plain_zone * 0.002)
                        + highland * stream_power_val * 0.0035) // valley-wall undercutting in mountain terrain
                        + (floodplain_scale * (0.0045 + basin_zone * 0.0045 + plain_zone * 0.0035))
                        + persistent_valley * (0.0035 + trunk_persistence * 0.010);
                    let lateral_strength = lateral_strength * (0.65 + (1.0 - uplift_core) * 0.35);
                    // Trunk rivers in highland zones carve broad U/V valleys; extend the
                    // lateral erosion radius so the valley floor spans multiple tiles.
                    let trunk_valley = smoothstep(100.0, 2000.0, ca) * highland;
                    let lat_distances: &[(isize, f32)] = if trunk_persistence > 0.42 {
                        &[(1, 1.0), (2, 0.62), (3, 0.34), (4, 0.18), (5, 0.09)]
                    } else if trunk_valley > 0.35 || persistent_valley > 0.22 {
                        &[(1, 1.0), (2, 0.50), (3, 0.22), (4, 0.10)]
                    } else {
                        &[(1, 1.0), (2, 0.45)]
                    };
                    for &(distance, weight) in lat_distances {
                        for side in [side_a, side_b] {
                            let sx = x as isize + side.0 * distance;
                            let sy = y as isize + side.1 * distance;
                            if !world.in_bounds(sx, sy) {
                                continue;
                            }
                            let sidx = world.idx(sx as usize, sy as usize);
                            let height_above = (terrain[sidx] - current).max(0.0);
                            let carve = lateral_strength * weight * (0.45 + height_above * 3.8);
                            lateral_erosion[sidx] += carve.min(0.024 + trunk_persistence * 0.014);
                        }
                    }
                    if floodplain_scale > 0.08 {
                        for (distance, weight) in [(1_isize, 0.55_f32), (2_isize, 0.32_f32)] {
                            for side in [side_a, side_b] {
                                let sx = x as isize + side.0 * distance;
                                let sy = y as isize + side.1 * distance;
                                if !world.in_bounds(sx, sy) {
                                    continue;
                                }
                                let sidx = world.idx(sx as usize, sy as usize);
                                let build = deposition
                                    * floodplain_scale
                                    * weight
                                    * (0.003 + basin_zone * 0.0035 + plain_zone * 0.0025);
                                next[sidx] = (next[sidx] + build.min(0.006)).min(1.0);
                            }
                        }
                    }
                }
            }
        }

        for idx in 0..next.len() {
            next[idx] = (next[idx] - lateral_erosion[idx]).max(0.0);
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
