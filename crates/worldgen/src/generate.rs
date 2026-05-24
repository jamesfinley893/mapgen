use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, VecDeque};

use noise::{NoiseFn, OpenSimplex};

use crate::{Biome, Surface, World, WorldConfig};

const HYDRO_EPSILON: f32 = 0.0001;
const DIAGONAL_COST: f32 = std::f32::consts::SQRT_2;
const EROSION_STEPS: usize = 18;

pub fn generate_world(config: &WorldConfig) -> Result<World, String> {
    config.validate()?;

    let mut world = World::new(config.seed, config.width, config.height, config.sea_level);

    let base = OpenSimplex::new(config.seed as u32);
    let ridge = OpenSimplex::new(config.seed.wrapping_add(1) as u32);
    let climate = OpenSimplex::new(config.seed.wrapping_add(2) as u32);

    populate_raw_elevation(&mut world, &base, &ridge);

    let mut ocean = classify_ocean(&world);
    let hydrology = simulate_hydrology(&world, &ocean);
    apply_channel_carving(&mut world, &hydrology);

    ocean = classify_ocean(&world);
    let hydrology = simulate_hydrology(&world, &ocean);
    apply_hydrology_to_world(&mut world, &ocean, &hydrology);

    let distance_to_ocean = fill_ocean_distance(&world, &ocean);
    populate_climate(&mut world, config, &ocean, &distance_to_ocean, &climate);
    assign_biomes(&mut world);

    Ok(world)
}

#[derive(Debug, Clone)]
struct HydrologyState {
    hydro_elevation: Vec<f32>,
    downstream: Vec<Option<usize>>,
    contributing_area: Vec<f32>,
    surfaces: Vec<Surface>,
    lake_id: Vec<Option<u32>>,
    water_level: Vec<Option<f32>>,
    basin_id: Vec<Option<u32>>,
}

struct RiverThresholds {
    stream: f32,
    secondary: f32,
    trunk: f32,
}

#[derive(Clone, Copy, Debug)]
struct QueueCell {
    level: f32,
    idx: usize,
}

struct ConditioningState {
    hydro_elevation: Vec<f32>,
    fill_depth: Vec<f32>,
    parent: Vec<Option<usize>>,
    rank: Vec<usize>,
}

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

impl PartialEq for QueueCell {
    fn eq(&self, other: &Self) -> bool {
        self.idx == other.idx && self.level.to_bits() == other.level.to_bits()
    }
}

impl Eq for QueueCell {}

impl Ord for QueueCell {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .level
            .total_cmp(&self.level)
            .then_with(|| other.idx.cmp(&self.idx))
    }
}

impl PartialOrd for QueueCell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn populate_raw_elevation(world: &mut World, base: &OpenSimplex, ridge: &OpenSimplex) {
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
            let orogen_margin = smoothstep(0.16, 0.54, shoulder_uplift[idx] + plateau_support[idx] * 0.55);
            let elevation_damping = 1.0
                - smoothstep(0.54, 0.86, terrain[idx]) * (0.48 + orogen_margin * 0.18)
                - smoothstep(0.72, 0.94, terrain[idx]) * (0.22 + (1.0 - uplift_core) * 0.16);
            let uplift_add = axial_uplift[idx] * uplift_per_step * (0.98 + progress * 0.46)
                + shoulder_uplift[idx] * uplift_per_step * 0.34 * (0.72 + progress * 0.18)
                + plateau_support[idx] * uplift_per_step * 0.14;
            let core_boost = 0.82 + uplift_core * 0.56 + axial_uplift[idx] * 0.12 - basin_bias[idx] * 0.12;
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
            let shoulder_zone = smoothstep(0.12, 0.48, shoulder_uplift[idx] + plateau_support[idx] * 0.4);
            let plain_zone = smoothstep(0.38, 0.82, craton_stability[idx]) * (1.0 - uplift_core);
            let basin_zone = smoothstep(0.24, 0.72, basin_bias[idx] + foreland_loading[idx] * 0.55 + backarc_loading[idx] * 0.45);
            let lat = latitude_factor(y, world.height);
            let snowline = (0.84 - lat * 0.22 - plateau_support[idx] * 0.04).clamp(0.58, 0.88);
            let glacial_band = smoothstep(snowline, (snowline + 0.12).min(0.98), current);
            let incision = if flow.is_ocean[idx] {
                0.0
            } else {
                let discharge = flow.contributing_area[idx].max(1.0).ln();
                let slope = flow.local_slope[idx];
                let relief_factor =
                    0.58 + relief * 2.7 + interior_high * 1.18 + alpine * (0.62 + uplift_core * 0.72);
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
                * (0.013 + interior_high * 0.044 + shoulder_zone * 0.022 + alpine * 0.016 * (1.0 - uplift_core * 0.3))
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
                * (0.006 + plain_zone * 0.01 + basin_zone * 0.012 + sediment_ratio.min(1.2) * 0.004)
                * (0.65 + (1.0 - confinement) * 0.35);
            let plain_planation = relief
                * (0.012 + plain_zone * 0.038 + basin_zone * 0.025)
                * (1.0 - uplift_core * 0.75);
            let shoulder_denudation = relief
                * (0.008 + shoulder_zone * 0.032 + interior_high * 0.018)
                * (1.0 - axial_uplift[idx] * 0.65);
            let basin_subsidence =
                (foreland_loading[idx] * 0.0105
                    + backarc_loading[idx] * 0.007
                    + basin_zone * 0.004
                    + shoulder_zone * 0.0025 * (1.0 - uplift_core))
                    * (0.62 + progress * 0.48);

            next[idx] = (current - incision + diffusion - ridge_decay - alpine_relax - slope_failure - glacial_erosion - plain_planation
                - shoulder_denudation - basin_subsidence - valley_floor_lowering + alluvial_fill)
                .max(0.0);

            if !flow.is_ocean[idx] && valley_scale > 0.02 {
                if let Some(next_idx) = flow.downstream[idx] {
                    let (nx, ny) = world.coords(next_idx);
                    let step_x = (nx as isize - x as isize).signum();
                    let step_y = (ny as isize - y as isize).signum();
                    if step_x != 0 || step_y != 0 {
                        let side_a = (-step_y, step_x);
                        let side_b = (step_y, -step_x);
                        let lateral_strength = (valley_scale * (1.0 - confinement) * (0.0024 + basin_zone * 0.003 + plain_zone * 0.002))
                            + (floodplain_scale * (0.0045 + basin_zone * 0.0045 + plain_zone * 0.0035));
                        let lateral_strength = lateral_strength * (0.65 + (1.0 - uplift_core) * 0.35);
                        for (distance, weight) in [(1_isize, 1.0_f32), (2_isize, 0.45_f32)] {
                            for side in [side_a, side_b] {
                                let sx = x as isize + side.0 * distance;
                                let sy = y as isize + side.1 * distance;
                                if !world.in_bounds(sx, sy) {
                                    continue;
                                }
                                let sidx = world.idx(sx as usize, sy as usize);
                                let height_above = (terrain[sidx] - current).max(0.0);
                                let carve = lateral_strength * weight * (0.45 + height_above * 3.4);
                                lateral_erosion[sidx] += carve.min(0.015);
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
    let segment_noise = octave_noise(base, xf64 * 3.4 + 23.0, yf64 * 3.4 - 19.0, 3, 0.56, 2.0);
    let transfer_noise = octave_noise(base, xf64 * 6.8 - 31.0, yf64 * 6.8 + 7.0, 2, 0.5, 2.0);
    let basin_noise = octave_noise(base, xf64 * 2.8 - 17.0, yf64 * 2.8 + 29.0, 3, 0.52, 2.0);
    let plateau_noise = octave_noise(base, xf64 * 1.9 + 37.0, yf64 * 1.9 - 15.0, 3, 0.5, 2.0);
    let plain_bands = octave_noise(base, xf64 * 1.25 - 41.0, yf64 * 1.25 + 33.0, 3, 0.54, 2.0);

    let dx = xf - 0.5;
    let dy = yf - 0.5;
    let radial = (dx * dx + dy * dy).sqrt();
    let edge_falloff = smoothstep(0.34, 0.78, radial);
    let continent_mask = (continent * 0.72 + shelves * 0.18 + craton * 0.16 - edge_falloff * 0.42).clamp(0.0, 1.0);

    let tectonics = sample_uplift_field(plates, xf, yf);
    let land_mask = smoothstep(0.38, 0.72, continent_mask);
    let segmentation = smoothstep(0.42, 0.78, segment_noise) * 0.75 + smoothstep(0.52, 0.86, ridge_detail) * 0.25;
    let transfer_gap = 1.0 - smoothstep(0.58, 0.84, transfer_noise) * 0.62;
    let boundary_wide = smoothstep(0.08, 0.72, tectonics);
    let boundary_mid = smoothstep(0.22, 0.82, tectonics);
    let boundary_narrow = smoothstep(0.48, 0.94, tectonics);
    let axial_uplift = (boundary_narrow * segmentation * transfer_gap * (0.55 + ridge_detail * 0.35) * land_mask)
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
    let convergence = ((rel_velocity.0 * normal.0 + rel_velocity.1 * normal.1) * 0.5 + 0.5).clamp(0.0, 1.0);
    let shear = ((rel_velocity.0 * -normal.1 + rel_velocity.1 * normal.0).abs()).clamp(0.0, 1.0);
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
            let score = drop / distance + alignment * 0.02 - if distance > 1.0 { 0.004 } else { 0.0 };
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

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hash01(seed: u64, x: usize, y: usize) -> f32 {
    let mut z = seed
        .wrapping_add((x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add((y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F));
    z ^= z >> 30;
    z = z.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z ^= z >> 27;
    z = z.wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z as f64 / u64::MAX as f64) as f32
}

fn classify_ocean(world: &World) -> Vec<bool> {
    world.tiles.iter().map(|tile| tile.raw_elevation <= world.sea_level).collect()
}

fn populate_climate(
    world: &mut World,
    config: &WorldConfig,
    ocean: &[bool],
    distance_to_ocean: &[u16],
    climate: &OpenSimplex,
) {
    let nearby_water = compute_nearby_water(world);

    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            let elevation = world.tiles[idx].raw_elevation;
            let lat = latitude_factor(y, world.height);
            let climate_noise = octave_noise(climate, x as f64 * 0.008, y as f64 * 0.008, 3, 0.5, 2.0);
            let temperature =
                (1.0 - lat * 1.15 + climate_noise * 0.12 - elevation * 0.35 + config.temperature_bias)
                    .clamp(0.0, 1.0);
            let moisture = (moisture_value(world, ocean, distance_to_ocean, climate, &nearby_water, x, y)
                + config.moisture_bias)
                .clamp(0.0, 1.0);
            world.tiles[idx].temperature = temperature;
            world.tiles[idx].moisture = moisture;
        }
    }
}

fn compute_nearby_water(world: &World) -> Vec<f32> {
    let mut nearby = vec![0.0_f32; world.tiles.len()];
    for idx in 0..world.tiles.len() {
        if matches!(world.tiles[idx].surface, Surface::Ocean | Surface::Lake | Surface::River) {
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

fn simulate_hydrology(world: &World, ocean: &[bool]) -> HydrologyState {
    let conditioning = condition_terrain(world, ocean);
    let provisional = identify_lakes(
        world,
        ocean,
        &conditioning.hydro_elevation,
        &conditioning.fill_depth,
        &conditioning.parent,
    );
    let mut downstream = build_downstream(world, ocean, &conditioning, &provisional.lake_id);
    break_downstream_cycles(&mut downstream, &conditioning.parent, ocean);
    let contributing_area = accumulate_contributing_area(&conditioning, &downstream, ocean);
    let basin_id = assign_basin_ids(world, ocean, &downstream, &provisional.lake_id, provisional.lake_count);
    let surfaces = classify_surfaces(world, ocean, &contributing_area, &provisional.lake_id);

    HydrologyState {
        hydro_elevation: conditioning.hydro_elevation,
        downstream,
        contributing_area,
        surfaces,
        lake_id: provisional.lake_id,
        water_level: provisional.water_level,
        basin_id,
    }
}

fn condition_terrain(world: &World, ocean: &[bool]) -> ConditioningState {
    let count = world.tiles.len();
    let mut hydro = vec![0.0; count];
    let mut fill_depth = vec![0.0; count];
    let mut parent = vec![None; count];
    let mut rank = vec![usize::MAX; count];
    let mut visited = vec![false; count];
    let mut heap = BinaryHeap::new();
    let mut next_rank = 0_usize;

    for idx in 0..count {
        if ocean[idx] {
            visited[idx] = true;
            hydro[idx] = world.tiles[idx].raw_elevation;
            rank[idx] = next_rank;
            next_rank += 1;
            heap.push(QueueCell {
                level: hydro[idx],
                idx,
            });
        }
    }

    while let Some(cell) = heap.pop() {
        let (x, y) = world.coords(cell.idx);
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            if visited[nidx] {
                continue;
            }
            visited[nidx] = true;
            let raw = world.tiles[nidx].raw_elevation;
            let raised = raw.max(cell.level);
            hydro[nidx] = raised;
            fill_depth[nidx] = (raised - raw).max(0.0);
            parent[nidx] = Some(cell.idx);
            rank[nidx] = next_rank;
            next_rank += 1;
            heap.push(QueueCell {
                level: raised + HYDRO_EPSILON,
                idx: nidx,
            });
        }
    }

    ConditioningState {
        hydro_elevation: hydro,
        fill_depth,
        parent,
        rank,
    }
}

struct LakeData {
    lake_id: Vec<Option<u32>>,
    water_level: Vec<Option<f32>>,
    lake_count: u32,
}

fn identify_lakes(
    world: &World,
    ocean: &[bool],
    hydro: &[f32],
    fill_depth: &[f32],
    parent: &[Option<usize>],
) -> LakeData {
    let mut lake_id = vec![None; world.tiles.len()];
    let mut water_level = vec![None; world.tiles.len()];
    let mut visited = vec![false; world.tiles.len()];
    let mut next_lake_id = 0_u32;
    let area_threshold = ((world.width * world.height) as f32 * 0.00075).ceil() as usize;
    let area_threshold = area_threshold.max(6);
    let volume_threshold = ((world.width * world.height) as f32 * 0.00011).max(0.06);
    let depth_threshold = 0.018;

    for idx in 0..world.tiles.len() {
        if visited[idx] || ocean[idx] || fill_depth[idx] <= depth_threshold {
            continue;
        }
        let mut region = Vec::new();
        let mut queue = VecDeque::from([idx]);
        visited[idx] = true;

        while let Some(current) = queue.pop_front() {
            region.push(current);
            let (x, y) = world.coords(current);
            for (nx, ny) in world.neighbors8(x, y) {
                let nidx = world.idx(nx, ny);
                if visited[nidx] || ocean[nidx] || fill_depth[nidx] <= depth_threshold {
                    continue;
                }
                if (hydro[nidx] - hydro[current]).abs() > 0.02 {
                    continue;
                }
                visited[nidx] = true;
                queue.push_back(nidx);
            }
        }

        let volume: f32 = region.iter().map(|&cell| fill_depth[cell]).sum();
        let avg_depth = volume / region.len() as f32;
        let max_depth = region
            .iter()
            .map(|&cell| fill_depth[cell])
            .fold(0.0_f32, f32::max);
        if region.len() < area_threshold && volume < volume_threshold {
            continue;
        }
        if avg_depth < 0.024 && max_depth < 0.05 {
            continue;
        }

        let mut in_region = vec![false; world.tiles.len()];
        for &cell in &region {
            in_region[cell] = true;
        }
        let mut outlet = None;
        let mut outlet_level = f32::MAX;
        for &cell in &region {
            if let Some(next) = parent[cell] {
                if !in_region[next] && hydro[cell] < outlet_level {
                    outlet_level = hydro[cell];
                    outlet = Some(next);
                }
            }
        }

        if outlet.is_none() {
            continue;
        }

        for &cell in &region {
            lake_id[cell] = Some(next_lake_id);
            water_level[cell] = Some(hydro[cell]);
        }
        next_lake_id += 1;
    }

    LakeData {
        lake_id,
        water_level,
        lake_count: next_lake_id,
    }
}

fn build_downstream(
    world: &World,
    ocean: &[bool],
    conditioning: &ConditioningState,
    lake_id: &[Option<u32>],
) -> Vec<Option<usize>> {
    let mut downstream = vec![None; world.tiles.len()];
    let mut order: Vec<_> = (0..world.tiles.len()).collect();
    order.sort_by(|a, b| {
        conditioning.hydro_elevation[*b]
            .total_cmp(&conditioning.hydro_elevation[*a])
            .then_with(|| world.tiles[*b].raw_elevation.total_cmp(&world.tiles[*a].raw_elevation))
            .then_with(|| conditioning.rank[*b].cmp(&conditioning.rank[*a]))
    });

    for idx in order {
        if ocean[idx] {
            continue;
        }
        let (x, y) = world.coords(idx);
        let current_hydro = conditioning.hydro_elevation[idx];
        let current_raw = world.tiles[idx].raw_elevation;
        let aspect = local_aspect(world, x, y);
        let persistence = conditioning.parent[idx].and_then(|parent| {
            let (px, py) = world.coords(parent);
            direction_vector((x, y), (px, py))
        });
        let mut best = conditioning.parent[idx];
        let mut best_score = conditioning
            .parent[idx]
            .map(|parent| {
                candidate_score(
                    world,
                    idx,
                    parent,
                    current_hydro,
                    current_raw,
                    aspect,
                    persistence,
                    conditioning,
                    true,
                )
            })
            .unwrap_or(f32::MIN);

        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            if lake_id[idx].is_some() && lake_id[idx] == lake_id[nidx] {
                continue;
            }
            let neighbor_hydro = conditioning.hydro_elevation[nidx];
            if neighbor_hydro > current_hydro + HYDRO_EPSILON {
                continue;
            }
            let is_flat_or_equal = (neighbor_hydro - current_hydro).abs() <= HYDRO_EPSILON;
            if is_flat_or_equal && conditioning.rank[nidx] >= conditioning.rank[idx] {
                continue;
            }
            let score = candidate_score(
                world,
                idx,
                nidx,
                current_hydro,
                current_raw,
                aspect,
                persistence,
                conditioning,
                nidx == conditioning.parent[idx].unwrap_or(usize::MAX),
            );
            if score > best_score {
                best_score = score;
                best = Some(nidx);
            }
        }
        downstream[idx] = best;
    }

    downstream
}

fn candidate_score(
    world: &World,
    idx: usize,
    next: usize,
    current_hydro: f32,
    current_raw: f32,
    aspect: (f32, f32),
    persistence: Option<(f32, f32)>,
    conditioning: &ConditioningState,
    is_parent: bool,
) -> f32 {
    let (x, y) = world.coords(idx);
    let (nx, ny) = world.coords(next);
    let distance = neighbor_distance(x, y, nx, ny);
    let dir = direction_vector((x, y), (nx, ny)).unwrap_or((0.0, 0.0));
    let hydro_drop = (current_hydro - conditioning.hydro_elevation[next]).max(0.0);
    let raw_drop = (current_raw - world.tiles[next].raw_elevation).max(-0.08);
    let slope = hydro_drop / distance;
    let raw_slope = raw_drop / distance;
    let alignment = dir.0 * aspect.0 + dir.1 * aspect.1;
    let persistence_bonus = persistence
        .map(|prev| (dir.0 * prev.0 + dir.1 * prev.1).max(-0.5))
        .unwrap_or(0.0);
    let diagonal_penalty = if distance > 1.0 {
        if hydro_drop <= HYDRO_EPSILON {
            0.32
        } else if slope < 0.012 {
            0.16
        } else {
            0.02
        }
    } else {
        0.0
    };
    let flat_bonus = if hydro_drop <= HYDRO_EPSILON {
        raw_slope.max(0.0) * 2.2 + alignment * 0.08
    } else {
        0.0
    };

    slope * 10.0
        + raw_slope.max(0.0) * 3.0
        + alignment * 0.35
        + persistence_bonus * 0.08
        + flat_bonus
        + if is_parent { 0.03 } else { 0.0 }
        - diagonal_penalty
}

fn break_downstream_cycles(downstream: &mut [Option<usize>], parent: &[Option<usize>], ocean: &[bool]) {
    for _ in 0..4 {
        let mut changed = false;
        for start in 0..downstream.len() {
            if ocean[start] {
                continue;
            }
            let mut path = Vec::new();
            let mut current = start;
            loop {
                if ocean[current] {
                    break;
                }
                if let Some(pos) = path.iter().position(|&idx| idx == current) {
                    for &cycle_idx in &path[pos..] {
                        downstream[cycle_idx] = parent[cycle_idx];
                    }
                    changed = true;
                    break;
                }
                path.push(current);
                match downstream[current] {
                    Some(next) => current = next,
                    None => break,
                }
            }
        }
        if !changed {
            break;
        }
    }
}

fn accumulate_contributing_area(
    conditioning: &ConditioningState,
    downstream: &[Option<usize>],
    ocean: &[bool],
) -> Vec<f32> {
    let mut contributing_area = vec![0.0; downstream.len()];
    let mut order: Vec<_> = (0..downstream.len()).collect();
    order.sort_by(|a, b| {
        conditioning.hydro_elevation[*b]
            .total_cmp(&conditioning.hydro_elevation[*a])
            .then_with(|| conditioning.rank[*b].cmp(&conditioning.rank[*a]))
    });

    for idx in order {
        if ocean[idx] {
            continue;
        }
        contributing_area[idx] += 1.0;
        if let Some(next) = downstream[idx] {
            contributing_area[next] += contributing_area[idx];
        }
    }
    contributing_area
}

fn assign_basin_ids(
    world: &World,
    ocean: &[bool],
    downstream: &[Option<usize>],
    lake_id: &[Option<u32>],
    basin_offset: u32,
) -> Vec<Option<u32>> {
    let mut basin_id = vec![None; world.tiles.len()];
    let mut mouth_to_basin = HashMap::<usize, u32>::new();
    let mut next_basin = 0_u32;

    for idx in 0..world.tiles.len() {
        if ocean[idx] {
            continue;
        }
        let mut current = idx;
        let mut guard = 0;
        while guard < world.tiles.len() {
            if ocean[current] {
                break;
            }
            if let Some(lake) = lake_id[current] {
                basin_id[idx] = Some(lake);
                break;
            }
            match downstream[current] {
                Some(next) => {
                    if ocean[next] {
                        let basin = *mouth_to_basin.entry(current).or_insert_with(|| {
                            let id = basin_offset + next_basin;
                            next_basin += 1;
                            id
                        });
                        basin_id[idx] = Some(basin);
                        break;
                    }
                    current = next;
                }
                None => break,
            }
            guard += 1;
        }
    }

    basin_id
}

fn classify_surfaces(
    world: &World,
    ocean: &[bool],
    contributing_area: &[f32],
    lake_id: &[Option<u32>],
) -> Vec<Surface> {
    let mut surfaces = vec![Surface::Land; world.tiles.len()];
    let thresholds = river_thresholds(world);

    for idx in 0..world.tiles.len() {
        if ocean[idx] {
            surfaces[idx] = Surface::Ocean;
        } else if lake_id[idx].is_some() {
            surfaces[idx] = Surface::Lake;
        } else if contributing_area[idx] >= thresholds.stream {
            surfaces[idx] = Surface::River;
        }
    }

    for idx in 0..world.tiles.len() {
        if surfaces[idx] != Surface::Land {
            continue;
        }
        let (x, y) = world.coords(idx);
        if world
            .neighbors8(x, y)
            .any(|(nx, ny)| surfaces[world.idx(nx, ny)] == Surface::Ocean)
        {
            surfaces[idx] = Surface::Coast;
        }
    }

    surfaces
}

fn apply_channel_carving(world: &mut World, hydrology: &HydrologyState) {
    let thresholds = river_thresholds(world);

    for idx in 0..world.tiles.len() {
        if hydrology.surfaces[idx] != Surface::River {
            continue;
        }
        let discharge = hydrology.contributing_area[idx];
        let ratio = (discharge / thresholds.stream).max(1.0);
        let band_multiplier = if discharge >= thresholds.trunk {
            1.75
        } else if discharge >= thresholds.secondary {
            1.25
        } else {
            1.0
        };
        let local_slope = hydrology.downstream[idx]
            .map(|next| {
                let (x, y) = world.coords(idx);
                let (nx, ny) = world.coords(next);
                (hydrology.hydro_elevation[idx] - hydrology.hydro_elevation[next]).max(0.0)
                    / neighbor_distance(x, y, nx, ny)
            })
            .unwrap_or(0.0);
        let slope_factor = if local_slope < 0.008 { 1.25 } else { 0.95 };
        let carve = (0.0045 + ratio.ln() * 0.0135) * band_multiplier * slope_factor;
        let carve = carve.clamp(0.0, 0.085);
        world.tiles[idx].raw_elevation = (world.tiles[idx].raw_elevation - carve).max(0.0);

        let (x, y) = world.coords(idx);
        let neighbors: Vec<_> = world.neighbors8(x, y).collect();
        for (nx, ny) in neighbors {
            let nidx = world.idx(nx, ny);
            if hydrology.surfaces[nidx] == Surface::Ocean {
                continue;
            }
            let distance = neighbor_distance(x, y, nx, ny);
            let neighbor_relief = (world.tiles[nidx].raw_elevation - world.tiles[idx].raw_elevation).max(0.0);
            let side_factor = if hydrology.surfaces[nidx] == Surface::River {
                0.38
            } else if distance > 1.0 {
                0.12
            } else {
                0.22
            };
            let relief_factor = (0.5 + neighbor_relief * 1.8).clamp(0.5, 1.4);
            let lateral_carve = carve * side_factor * relief_factor;
            world.tiles[nidx].raw_elevation = (world.tiles[nidx].raw_elevation - lateral_carve).max(0.0);
        }
    }
}

fn apply_hydrology_to_world(world: &mut World, ocean: &[bool], hydrology: &HydrologyState) {
    for idx in 0..world.tiles.len() {
        world.tiles[idx].hydro_elevation = hydrology.hydro_elevation[idx];
        world.tiles[idx].contributing_area = hydrology.contributing_area[idx];
        world.tiles[idx].downstream = hydrology.downstream[idx];
        world.tiles[idx].surface = hydrology.surfaces[idx];
        world.tiles[idx].basin_id = hydrology.basin_id[idx];
        world.tiles[idx].lake_id = hydrology.lake_id[idx];
        world.tiles[idx].water_level = hydrology.water_level[idx];
        if ocean[idx] {
            world.tiles[idx].water_level = Some(world.sea_level);
        }
    }
}

fn octave_noise(
    noise: &OpenSimplex,
    x: f64,
    y: f64,
    octaves: usize,
    persistence: f64,
    lacunarity: f64,
) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut freq = 1.0;
    let mut norm = 0.0;
    for _ in 0..octaves {
        sum += noise.get([x * freq, y * freq]) * amp;
        norm += amp;
        amp *= persistence;
        freq *= lacunarity;
    }
    (((sum / norm) as f32) * 0.5 + 0.5).clamp(0.0, 1.0)
}

fn ridge_noise(noise: &OpenSimplex, x: f64, y: f64, octaves: usize) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut freq = 1.0;
    let mut norm = 0.0;
    for _ in 0..octaves {
        let v = noise.get([x * freq, y * freq]).abs();
        sum += (1.0 - v) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.1;
    }
    (sum / norm) as f32
}

fn latitude_factor(y: usize, height: usize) -> f32 {
    let lat = y as f32 / (height.saturating_sub(1).max(1)) as f32;
    ((lat - 0.5).abs()) * 2.0
}

fn fill_ocean_distance(world: &World, ocean: &[bool]) -> Vec<u16> {
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

fn moisture_value(
    world: &World,
    ocean: &[bool],
    distance_to_ocean: &[u16],
    climate: &OpenSimplex,
    nearby_water: &[f32],
    x: usize,
    y: usize,
) -> f32 {
    let idx = world.idx(x, y);
    if ocean[idx] {
        return 1.0;
    }

    let ocean_influence = 1.0 - (distance_to_ocean[idx] as f32 / (world.width.max(world.height) as f32 * 0.45))
        .clamp(0.0, 1.0);
    let rain_shadow = rain_shadow(world, ocean, x, y);
    let lat = latitude_factor(y, world.height);
    let zonal = (1.0 - (lat - 0.35).abs() * 1.4).clamp(0.0, 1.0);
    let noise = octave_noise(climate, x as f64 * 0.014 + 7.0, y as f64 * 0.014 - 9.0, 4, 0.55, 2.0);

    (ocean_influence * 0.44 + zonal * 0.20 + noise * 0.17 + rain_shadow * 0.11 + nearby_water[idx] * 0.18)
        .clamp(0.0, 1.0)
}

fn river_thresholds(world: &World) -> RiverThresholds {
    let area = (world.width * world.height) as f32;
    let stream = (area * 0.00075).max(12.0);
    RiverThresholds {
        stream,
        secondary: stream * 6.5,
        trunk: stream * 18.0,
    }
}

fn local_aspect(world: &World, x: usize, y: usize) -> (f32, f32) {
    let sample = |sx: isize, sy: isize| -> f32 {
        let cx = sx.clamp(0, world.width.saturating_sub(1) as isize) as usize;
        let cy = sy.clamp(0, world.height.saturating_sub(1) as isize) as usize;
        world.tiles[world.idx(cx, cy)].raw_elevation
    };
    let x = x as isize;
    let y = y as isize;
    let dx = sample(x + 1, y) - sample(x - 1, y);
    let dy = sample(x, y + 1) - sample(x, y - 1);
    normalize((-dx, -dy))
}

fn normalize(v: (f32, f32)) -> (f32, f32) {
    let len = (v.0 * v.0 + v.1 * v.1).sqrt();
    if len <= f32::EPSILON {
        (0.0, 0.0)
    } else {
        (v.0 / len, v.1 / len)
    }
}

fn direction_vector(from: (usize, usize), to: (usize, usize)) -> Option<(f32, f32)> {
    let dx = to.0 as isize - from.0 as isize;
    let dy = to.1 as isize - from.1 as isize;
    if dx == 0 && dy == 0 {
        None
    } else {
        Some(normalize((dx as f32, dy as f32)))
    }
}

fn neighbor_distance(x: usize, y: usize, nx: usize, ny: usize) -> f32 {
    if x != nx && y != ny {
        DIAGONAL_COST
    } else {
        1.0
    }
}

fn rain_shadow(world: &World, ocean: &[bool], x: usize, y: usize) -> f32 {
    let mut moisture = 0.0_f32;
    let mut height_barrier = 0.0_f32;

    for step in 1..=12 {
        let nx = x.saturating_sub(step);
        let idx = world.idx(nx, y);
        if ocean[idx] {
            moisture += 0.08;
            break;
        }
        height_barrier += (world.tiles[idx].raw_elevation - world.sea_level).max(0.0) * 0.12;
    }

    for step in 1..=8 {
        let nx = (x + step).min(world.width - 1);
        let idx = world.idx(nx, y);
        if ocean[idx] {
            moisture += 0.04;
            break;
        }
    }

    (moisture - height_barrier).clamp(0.0, 1.0)
}

fn assign_biomes(world: &mut World) {
    let mut biomes = Vec::with_capacity(world.tiles.len());
    for idx in 0..world.tiles.len() {
        biomes.push(biome_for_world_tile(world, idx));
    }
    for (tile, biome) in world.tiles.iter_mut().zip(biomes.into_iter()) {
        tile.biome = biome;
    }
}

fn biome_for_world_tile(world: &World, idx: usize) -> Biome {
    let tile = &world.tiles[idx];
    let support = mountain_support(world, idx);
    let proximity = mountain_proximity(world, idx);
    let trunk_river = trunk_river_proximity(world, idx);
    let local_relief = local_relief(world, idx);
    biome_for_tile_with_support(
        tile.surface,
        tile.raw_elevation,
        world.sea_level,
        tile.temperature,
        tile.moisture,
        support,
        proximity,
        trunk_river,
        local_relief,
    )
}

fn mountain_support(world: &World, idx: usize) -> f32 {
    let (x, y) = world.coords(idx);
    let high_threshold = world.sea_level + 0.24;
    let alpine_threshold = world.sea_level + 0.34;
    let mut support = 0.0_f32;
    let mut total = 0.0_f32;

    for dy in -2..=2 {
        for dx in -2..=2 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if !world.in_bounds(nx, ny) {
                continue;
            }
            let nidx = world.idx(nx as usize, ny as usize);
            let dist = dx.abs().max(dy.abs()) as f32;
            let weight = if dist <= 1.0 { 1.0 } else { 0.45 };
            let elev = world.tiles[nidx].raw_elevation;
            total += weight;
            if elev > alpine_threshold {
                support += weight;
            } else if elev > high_threshold {
                support += weight * 0.55;
            }
        }
    }

    if total <= f32::EPSILON {
        0.0
    } else {
        (support / total).clamp(0.0, 1.0)
    }
}

fn mountain_proximity(world: &World, idx: usize) -> f32 {
    let (x, y) = world.coords(idx);
    let alpine_threshold = world.sea_level + 0.38;
    let ridge_threshold = world.sea_level + 0.32;
    let mut support = 0.0_f32;
    let mut total = 0.0_f32;

    for dy in -4..=4 {
        for dx in -4..=4 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if !world.in_bounds(nx, ny) {
                continue;
            }
            let nidx = world.idx(nx as usize, ny as usize);
            let dist = ((dx * dx + dy * dy) as f32).sqrt();
            let weight = (1.0 / (1.0 + dist)).clamp(0.12, 0.7);
            let elev = world.tiles[nidx].raw_elevation;
            total += weight;
            if elev > alpine_threshold {
                support += weight;
            } else if elev > ridge_threshold {
                support += weight * 0.45;
            }
        }
    }

    if total <= f32::EPSILON {
        0.0
    } else {
        (support / total).clamp(0.0, 1.0)
    }
}

fn local_relief(world: &World, idx: usize) -> f32 {
    let (x, y) = world.coords(idx);
    let current = world.tiles[idx].raw_elevation;
    let mut max_drop = 0.0_f32;
    let mut max_rise = 0.0_f32;
    for (nx, ny) in world.neighbors8(x, y) {
        let elev = world.tiles[world.idx(nx, ny)].raw_elevation;
        max_drop = max_drop.max((current - elev).max(0.0));
        max_rise = max_rise.max((elev - current).max(0.0));
    }
    (max_drop + max_rise * 0.5).clamp(0.0, 1.0)
}

fn trunk_river_proximity(world: &World, idx: usize) -> f32 {
    let (x, y) = world.coords(idx);
    let trunk_threshold = (((world.width * world.height) as f32 * 0.00075).max(12.0)) * 18.0;
    let mut influence = 0.0_f32;
    let mut total = 0.0_f32;

    for dy in -3..=3 {
        for dx in -3..=3 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if !world.in_bounds(nx, ny) {
                continue;
            }
            let nidx = world.idx(nx as usize, ny as usize);
            let dist = ((dx * dx + dy * dy) as f32).sqrt();
            let weight = (1.0 / (1.0 + dist)).clamp(0.14, 0.75);
            total += weight;
            let neighbor = &world.tiles[nidx];
            if neighbor.surface == Surface::River && neighbor.contributing_area >= trunk_threshold {
                influence += weight;
            }
        }
    }

    if total <= f32::EPSILON {
        0.0
    } else {
        (influence / total).clamp(0.0, 1.0)
    }
}

pub fn biome_for_tile(
    surface: Surface,
    elevation: f32,
    sea_level: f32,
    temperature: f32,
    moisture: f32,
) -> Biome {
    biome_for_tile_with_support(surface, elevation, sea_level, temperature, moisture, 1.0, 1.0, 0.0, 0.08)
}

fn biome_for_tile_with_support(
    surface: Surface,
    elevation: f32,
    sea_level: f32,
    temperature: f32,
    moisture: f32,
    support: f32,
    proximity: f32,
    trunk_river: f32,
    relief: f32,
) -> Biome {
    match surface {
        Surface::Ocean => Biome::Ocean,
        Surface::Coast => Biome::Coast,
        Surface::Lake => Biome::Lake,
        Surface::River => {
            if elevation > sea_level + 0.38 && support > 0.48 {
                Biome::Alpine
            } else if elevation > sea_level + 0.30
                && support > 0.22
                && proximity > 0.18
                && relief > 0.04
                && (trunk_river < 0.18 || (support > 0.34 && proximity > 0.28))
            {
                Biome::Foothills
            } else {
                land_biome(temperature, (moisture + 0.18).clamp(0.0, 1.0), elevation, sea_level)
            }
        }
        Surface::Land => {
            land_biome_with_support(
                temperature,
                moisture,
                elevation,
                sea_level,
                support,
                proximity,
                trunk_river,
                relief,
            )
        }
    }
}

fn land_biome(temperature: f32, moisture: f32, elevation: f32, sea_level: f32) -> Biome {
    land_biome_with_support(temperature, moisture, elevation, sea_level, 1.0, 1.0, 0.0, 0.08)
}

fn land_biome_with_support(
    temperature: f32,
    moisture: f32,
    elevation: f32,
    sea_level: f32,
    support: f32,
    proximity: f32,
    trunk_river: f32,
    relief: f32,
) -> Biome {
    if elevation > sea_level + 0.38 && support > 0.5 {
        return Biome::Alpine;
    }
    if elevation > sea_level + 0.31
        && support > 0.24
        && proximity > 0.2
        && relief > 0.04
        && (trunk_river < 0.2 || (support > 0.35 && proximity > 0.3))
    {
        return Biome::Foothills;
    }
    if temperature < 0.12 {
        return if moisture < 0.35 { Biome::PolarDesert } else { Biome::Tundra };
    }
    if temperature < 0.28 {
        return if moisture < 0.30 { Biome::Steppe } else { Biome::BorealForest };
    }
    if temperature < 0.48 {
        if moisture < 0.18 {
            Biome::Desert
        } else if moisture < 0.35 {
            Biome::TemperateGrassland
        } else if moisture < 0.62 {
            Biome::Woodland
        } else {
            Biome::TemperateForest
        }
    } else if temperature < 0.72 {
        if moisture < 0.16 {
            Biome::Desert
        } else if moisture < 0.38 {
            Biome::Savanna
        } else if moisture < 0.66 {
            Biome::Woodland
        } else {
            Biome::TropicalForest
        }
    } else if moisture < 0.18 {
        Biome::Desert
    } else if moisture < 0.42 {
        Biome::Savanna
    } else if moisture < 0.72 {
        Biome::TropicalForest
    } else {
        Biome::Rainforest
    }
}
