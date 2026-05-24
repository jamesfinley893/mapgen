use noise::OpenSimplex;

use crate::World;

use super::util::{
    direction_vector, hash01, latitude_factor, neighbor_distance, normalize, octave_noise,
    ridge_noise, smoothstep,
};
use super::EROSION_STEPS;

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
    let plates = generate_plates(world);
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
            let sample = sample_tectonic_elevation(world, base, ridge, &plates, x, y);
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

    for step in 0..EROSION_STEPS {
        let progress = (step + 1) as f32 / EROSION_STEPS as f32;
        for idx in 0..terrain.len() {
            let uplift_core =
                smoothstep(0.48, 0.92, axial_uplift[idx] + shoulder_uplift[idx] * 0.24);
            let orogen_margin =
                smoothstep(0.16, 0.54, shoulder_uplift[idx] + plateau_support[idx] * 0.55);
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

        let flow = simulate_erosion_flow(world, &terrain);
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
            let shoulder_zone =
                smoothstep(0.12, 0.48, shoulder_uplift[idx] + plateau_support[idx] * 0.4);
            let plain_zone = smoothstep(0.38, 0.82, craton_stability[idx]) * (1.0 - uplift_core);
            let basin_zone = smoothstep(
                0.24,
                0.72,
                basin_bias[idx] + foreland_loading[idx] * 0.55 + backarc_loading[idx] * 0.45,
            );
            let lat = latitude_factor(y, world.height);
            let snowline = (0.84 - lat * 0.22 - plateau_support[idx] * 0.04).clamp(0.58, 0.88);
            let glacial_band = smoothstep(snowline, (snowline + 0.12).min(0.98), current);
            let incision = if flow.is_ocean[idx] {
                0.0
            } else {
                let discharge = flow.contributing_area[idx].max(1.0).ln();
                let slope = flow.local_slope[idx];
                let relief_factor = 0.58
                    + relief * 2.7
                    + interior_high * 1.18
                    + alpine * (0.62 + uplift_core * 0.72);
                let tectonic_factor = 0.28 + axial_uplift[idx] * 1.54 + shoulder_uplift[idx] * 0.42;
                let alluvial_brake = 1.0 - flow.floodplain_scale[idx] * 0.55;
                discharge * slope * 0.012 * relief_factor * tectonic_factor * alluvial_brake
            };
            let confinement = flow.confinement[idx];
            let valley_scale = flow.valley_scale[idx];
            let floodplain_scale = flow.floodplain_scale[idx];
            let deposition = flow.deposition[idx];
            let sediment_flux = flow.sediment_flux[idx];
            let transport_capacity = flow.transport_capacity[idx];
            let sediment_ratio = if transport_capacity <= f32::EPSILON {
                0.0
            } else {
                (sediment_flux / transport_capacity).clamp(0.0, 2.5)
            };
            let diffusion = (avg_neighbor - current)
                * (0.019
                    + max_neighbor_drop * 0.034
                    + interior_high * 0.046
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
                * (0.0015 + plain_zone * 0.003 + basin_zone * 0.0025)
                * (0.55 + smoothstep(0.0, 0.09, flow.local_slope[idx]));
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

            next[idx] = (current
                - incision
                + diffusion
                - ridge_decay
                - alpine_relax
                - slope_failure
                - glacial_erosion
                - plain_planation
                - shoulder_denudation
                - basin_subsidence
                - valley_floor_lowering
                + alluvial_fill)
                .max(0.0);

            if !flow.is_ocean[idx] && valley_scale > 0.02 {
                if let Some(next_idx) = flow.downstream[idx] {
                    let (nx, ny) = world.coords(next_idx);
                    let step_x = (nx as isize - x as isize).signum();
                    let step_y = (ny as isize - y as isize).signum();
                    if step_x != 0 || step_y != 0 {
                        let side_a = (-step_y, step_x);
                        let side_b = (step_y, -step_x);
                        let lateral_strength = (valley_scale
                            * (1.0 - confinement)
                            * (0.0024 + basin_zone * 0.003 + plain_zone * 0.002))
                            + (floodplain_scale
                                * (0.0045 + basin_zone * 0.0045 + plain_zone * 0.0035));
                        let lateral_strength =
                            lateral_strength * (0.65 + (1.0 - uplift_core) * 0.35);
                        for (distance, weight) in [(1_isize, 1.0_f32), (2_isize, 0.45_f32)] {
                            for side in [side_a, side_b] {
                                let sx = x as isize + side.0 * distance;
                                let sy = y as isize + side.1 * distance;
                                if !world.in_bounds(sx, sy) {
                                    continue;
                                }
                                let sidx = world.idx(sx as usize, sy as usize);
                                let height_above = (terrain[sidx] - current).max(0.0);
                                let carve =
                                    lateral_strength * weight * (0.45 + height_above * 3.4);
                                lateral_erosion[sidx] += carve.min(0.015);
                            }
                        }
                        if floodplain_scale > 0.08 {
                            for (distance, weight) in [(1_isize, 0.55_f32), (2_isize, 0.32_f32)]
                            {
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
                                        * (0.003
                                            + basin_zone * 0.0035
                                            + plain_zone * 0.0025);
                                    next[sidx] = (next[sidx] + build.min(0.006)).min(1.0);
                                }
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
}

fn generate_plates(world: &World) -> Vec<Plate> {
    let approx = ((world.width * world.height) as f32 / 18000.0).round() as usize;
    let plate_count = approx.clamp(8, 18);
    let mut plates = Vec::with_capacity(plate_count);
    for i in 0..plate_count {
        let xf = hash01(world.seed.wrapping_add(17), i * 13 + 1, 0);
        let yf = hash01(world.seed.wrapping_add(29), i * 17 + 3, 0);
        let angle = hash01(world.seed.wrapping_add(41), i * 19 + 5, 0) * std::f32::consts::TAU;
        plates.push(Plate {
            x: xf,
            y: yf,
            vx: angle.cos(),
            vy: angle.sin(),
        });
    }
    plates
}

fn sample_tectonic_elevation(
    world: &World,
    base: &OpenSimplex,
    ridge: &OpenSimplex,
    plates: &[Plate],
    x: usize,
    y: usize,
) -> OrogenSample {
    let xf = x as f32 / world.width as f32;
    let yf = y as f32 / world.height as f32;
    let xf64 = xf as f64;
    let yf64 = yf as f64;

    let continent = octave_noise(base, xf64 * 1.15, yf64 * 1.15, 5, 0.53, 2.0);
    let shelves = octave_noise(base, xf64 * 2.4 + 11.0, yf64 * 2.4 - 7.0, 3, 0.55, 2.0);
    let plains = octave_noise(base, xf64 * 7.4 - 9.0, yf64 * 7.4 + 3.0, 4, 0.58, 2.15);
    let craton = octave_noise(base, xf64 * 0.65 + 2.0, yf64 * 0.65 - 4.0, 2, 0.5, 2.0);
    let ridge_detail = ridge_noise(ridge, xf64 * 4.2 + 13.0, yf64 * 4.2 - 6.0, 3);
    let segment_noise =
        octave_noise(base, xf64 * 3.4 + 23.0, yf64 * 3.4 - 19.0, 3, 0.56, 2.0);
    let transfer_noise =
        octave_noise(base, xf64 * 6.8 - 31.0, yf64 * 6.8 + 7.0, 2, 0.5, 2.0);
    let basin_noise = octave_noise(base, xf64 * 2.8 - 17.0, yf64 * 2.8 + 29.0, 3, 0.52, 2.0);
    let plateau_noise =
        octave_noise(base, xf64 * 1.9 + 37.0, yf64 * 1.9 - 15.0, 3, 0.5, 2.0);
    let plain_bands =
        octave_noise(base, xf64 * 1.25 - 41.0, yf64 * 1.25 + 33.0, 3, 0.54, 2.0);

    let dx = xf - 0.5;
    let dy = yf - 0.5;
    let radial = (dx * dx + dy * dy).sqrt();
    let edge_falloff = smoothstep(0.34, 0.78, radial);
    let continent_mask =
        (continent * 0.72 + shelves * 0.18 + craton * 0.16 - edge_falloff * 0.42)
            .clamp(0.0, 1.0);

    let tectonics = sample_uplift_field(plates, xf, yf);
    let land_mask = smoothstep(0.38, 0.72, continent_mask);
    let segmentation = smoothstep(0.42, 0.78, segment_noise) * 0.75
        + smoothstep(0.52, 0.86, ridge_detail) * 0.25;
    let transfer_gap = 1.0 - smoothstep(0.58, 0.84, transfer_noise) * 0.62;
    let boundary_wide = smoothstep(0.08, 0.72, tectonics);
    let boundary_mid = smoothstep(0.22, 0.82, tectonics);
    let boundary_narrow = smoothstep(0.48, 0.94, tectonics);
    let axial_uplift = (boundary_narrow
        * segmentation
        * transfer_gap
        * (0.55 + ridge_detail * 0.35)
        * land_mask)
        .clamp(0.0, 1.0);
    let shoulder_uplift = ((boundary_mid - boundary_narrow * 0.55).max(0.0)
        * (0.30 + segment_noise * 0.20)
        * land_mask)
        .clamp(0.0, 1.0);
    let plateau_support = (boundary_mid
        * smoothstep(0.56, 0.86, plateau_noise)
        * smoothstep(0.48, 0.86, ridge_detail)
        * (0.15 + axial_uplift * 0.28)
        * land_mask)
        .clamp(0.0, 1.0);
    let foreland_loading = ((boundary_wide - boundary_mid * 0.55).max(0.0)
        * smoothstep(0.34, 0.74, basin_noise)
        * (0.72 + boundary_mid * 0.18)
        * land_mask)
        .clamp(0.0, 1.0);
    let backarc_loading = ((boundary_mid - boundary_narrow * 0.8).max(0.0)
        * smoothstep(0.46, 0.82, 1.0 - basin_noise)
        * land_mask
        * 0.92)
        .clamp(0.0, 1.0);
    let craton_stability = (smoothstep(0.48, 0.84, craton)
        * smoothstep(0.34, 0.74, plain_bands)
        * (1.0 - boundary_mid * 0.75)
        * land_mask)
        .clamp(0.0, 1.0);
    let basin_bias = (smoothstep(0.44, 0.82, basin_noise)
        * (0.45 + (1.0 - boundary_narrow) * 0.4)
        * land_mask)
        .clamp(0.0, 1.0);

    let basement = (continent_mask * 0.52
        + plains * 0.12
        + craton * 0.16
        + plain_bands * 0.08
        - basin_bias * 0.10
        - edge_falloff * 0.12)
        .clamp(0.0, 1.0);

    OrogenSample {
        basement,
        axial_uplift,
        shoulder_uplift,
        plateau_support,
        foreland_loading,
        backarc_loading,
        craton_stability,
        basin_bias,
    }
}

fn sample_uplift_field(plates: &[Plate], xf: f32, yf: f32) -> f32 {
    let mut best = (usize::MAX, f32::MAX, 0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
    let mut second = (usize::MAX, f32::MAX, 0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);

    for (i, plate) in plates.iter().enumerate() {
        let dx = wrap_delta(xf - plate.x);
        let dy = wrap_delta(yf - plate.y);
        let dist2 = dx * dx + dy * dy;
        if dist2 < best.1 {
            second = best;
            best = (i, dist2, dx, dy, plate.vx, plate.vy);
        } else if dist2 < second.1 {
            second = (i, dist2, dx, dy, plate.vx, plate.vy);
        }
    }

    let boundary_gap = (second.1.sqrt() - best.1.sqrt()).abs();
    let boundary = (1.0 - smoothstep(0.01, 0.09, boundary_gap)).powf(1.85);
    let normal = normalize((second.2 - best.2, second.3 - best.3));
    let rel_velocity = (best.4 - second.4, best.5 - second.5);
    let convergence =
        ((rel_velocity.0 * normal.0 + rel_velocity.1 * normal.1) * 0.5 + 0.5).clamp(0.0, 1.0);
    let shear =
        ((rel_velocity.0 * -normal.1 + rel_velocity.1 * normal.0).abs()).clamp(0.0, 1.0);
    let orogeny = smoothstep(0.45, 0.92, convergence * 0.9 + shear * 0.18);

    boundary * orogeny
}

fn wrap_delta(delta: f32) -> f32 {
    if delta > 0.5 {
        delta - 1.0
    } else if delta < -0.5 {
        delta + 1.0
    } else {
        delta
    }
}

fn simulate_erosion_flow(world: &World, terrain: &[f32]) -> ErosionFlow {
    let mut is_ocean = vec![false; terrain.len()];
    for idx in 0..terrain.len() {
        is_ocean[idx] = terrain[idx] <= world.sea_level;
    }

    let mut downstream = vec![None; terrain.len()];
    let mut local_slope = vec![0.0_f32; terrain.len()];
    let mut order: Vec<_> = (0..terrain.len()).collect();
    order.sort_by(|a, b| terrain[*b].total_cmp(&terrain[*a]));

    for idx in 0..terrain.len() {
        if is_ocean[idx] {
            continue;
        }
        let (x, y) = world.coords(idx);
        let current = terrain[idx];
        let aspect = local_aspect_on_surface(world, terrain, x, y);
        let mut best = None;
        let mut best_score = f32::MIN;
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            let distance = neighbor_distance(x, y, nx, ny);
            let drop = current - terrain[nidx];
            if drop <= 0.0 {
                continue;
            }
            let dir = direction_vector((x, y), (nx, ny)).unwrap_or((0.0, 0.0));
            let alignment = dir.0 * aspect.0 + dir.1 * aspect.1;
            let score =
                drop / distance + alignment * 0.02 - if distance > 1.0 { 0.004 } else { 0.0 };
            if score > best_score {
                best_score = score;
                best = Some(nidx);
                local_slope[idx] = drop / distance;
            }
        }
        downstream[idx] = best;
    }

    let mut contributing_area = vec![0.0_f32; terrain.len()];
    for idx in order.iter().copied() {
        if is_ocean[idx] {
            continue;
        }
        contributing_area[idx] += 1.0;
        if let Some(next) = downstream[idx] {
            contributing_area[next] += contributing_area[idx];
        }
    }

    let mut confinement = vec![0.0_f32; terrain.len()];
    let mut valley_scale = vec![0.0_f32; terrain.len()];
    let mut transport_capacity = vec![0.0_f32; terrain.len()];
    let mut sediment_flux = vec![0.0_f32; terrain.len()];
    let mut deposition = vec![0.0_f32; terrain.len()];
    let mut floodplain_scale = vec![0.0_f32; terrain.len()];
    for idx in 0..terrain.len() {
        if is_ocean[idx] {
            continue;
        }
        let Some(next) = downstream[idx] else {
            continue;
        };
        let conf = flow_confinement(world, terrain, idx, next);
        let discharge = contributing_area[idx].max(1.0).ln();
        let slope = local_slope[idx];
        confinement[idx] = conf;
        valley_scale[idx] = smoothstep(3.1, 5.7, discharge)
            * (1.0 - smoothstep(0.045, 0.17, slope))
            * (0.58 + (1.0 - conf) * 0.42);
        transport_capacity[idx] =
            (contributing_area[idx].max(1.0).ln().powf(1.28) * slope.max(0.0008).sqrt()) * 0.9;
    }

    for idx in order.iter().copied() {
        if is_ocean[idx] {
            continue;
        }
        let slope = local_slope[idx];
        let discharge = contributing_area[idx].max(1.0).ln();
        let local_supply = (0.015 + slope * 0.22 + valley_scale[idx] * 0.06)
            * (0.45 + discharge * 0.12)
            * (0.75 + confinement[idx] * 0.35);
        let incoming = sediment_flux[idx] + local_supply;
        let capacity = transport_capacity[idx];
        let deposited = (incoming - capacity).max(0.0);
        deposition[idx] = deposited.min(0.18);
        let carried = (incoming - deposition[idx] * 0.72).max(0.0);
        if let Some(next) = downstream[idx] {
            sediment_flux[next] += carried;
        }
        let low_slope = 1.0 - smoothstep(0.03, 0.12, slope);
        let overcapacity = if incoming <= f32::EPSILON {
            0.0
        } else {
            (deposition[idx] / incoming).clamp(0.0, 1.0)
        };
        floodplain_scale[idx] = smoothstep(2.7, 5.6, discharge)
            * low_slope
            * (0.45 + overcapacity * 0.55)
            * (0.6 + (1.0 - confinement[idx]) * 0.4);
    }

    ErosionFlow {
        downstream,
        contributing_area,
        local_slope,
        confinement,
        valley_scale,
        transport_capacity,
        sediment_flux,
        deposition,
        floodplain_scale,
        is_ocean,
    }
}

fn flow_confinement(world: &World, terrain: &[f32], idx: usize, next: usize) -> f32 {
    let (x, y) = world.coords(idx);
    let (nx, ny) = world.coords(next);
    let dx = (nx as isize - x as isize).signum();
    let dy = (ny as isize - y as isize).signum();
    if dx == 0 && dy == 0 {
        return 0.0;
    }
    let current = terrain[idx];
    let side_a = (-dy, dx);
    let side_b = (dy, -dx);
    let mut rise = 0.0_f32;
    let mut weight_sum = 0.0_f32;

    for distance in 1..=2 {
        let weight = if distance == 1 { 1.0 } else { 0.55 };
        for side in [side_a, side_b] {
            let sx = x as isize + side.0 * distance;
            let sy = y as isize + side.1 * distance;
            if !world.in_bounds(sx, sy) {
                continue;
            }
            let sidx = world.idx(sx as usize, sy as usize);
            rise += (terrain[sidx] - current).max(0.0) * weight;
            weight_sum += weight;
        }
    }

    if weight_sum <= f32::EPSILON {
        0.0
    } else {
        smoothstep(0.015, 0.13, rise / weight_sum)
    }
}

fn local_aspect_on_surface(world: &World, terrain: &[f32], x: usize, y: usize) -> (f32, f32) {
    let sample = |sx: isize, sy: isize| -> f32 {
        let cx = sx.clamp(0, world.width.saturating_sub(1) as isize) as usize;
        let cy = sy.clamp(0, world.height.saturating_sub(1) as isize) as usize;
        terrain[world.idx(cx, cy)]
    };
    let x = x as isize;
    let y = y as isize;
    let dx = sample(x + 1, y) - sample(x - 1, y);
    let dy = sample(x, y + 1) - sample(x, y - 1);
    normalize((-dx, -dy))
}

fn normalize_terrain(terrain: &mut [f32], low_q: f32, high_q: f32) {
    let mut sorted = terrain.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let last = sorted.len().saturating_sub(1);
    let lo_idx = ((last as f32) * low_q).round() as usize;
    let hi_idx = ((last as f32) * high_q).round() as usize;
    let lo = sorted[lo_idx.min(last)];
    let hi = sorted[hi_idx.min(last)].max(lo + 0.0001);

    for value in terrain.iter_mut() {
        let mapped = ((*value - lo) / (hi - lo)).clamp(0.0, 1.0);
        let compressed = smoothstep(0.0, 1.0, mapped).powf(1.04);
        let top_tail = smoothstep(0.72, 1.0, compressed);
        *value = (compressed - top_tail * 0.09).clamp(0.0, 1.0);
    }
}
