use noise::OpenSimplex;

use crate::World;

use super::continents::{ContinentalConfig, sample_continental_fields};
use crate::generate::util::{hash01, normalize, octave_noise, ridge_noise, smoothstep};

#[derive(Clone, Copy)]
pub(super) struct Plate {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
}

#[derive(Clone, Copy)]
pub(super) struct OrogenSample {
    pub(super) basement: f32,
    pub(super) axial_uplift: f32,
    pub(super) shoulder_uplift: f32,
    pub(super) plateau_support: f32,
    pub(super) foreland_loading: f32,
    pub(super) backarc_loading: f32,
    pub(super) craton_stability: f32,
    pub(super) basin_bias: f32,
}

pub(super) fn generate_plates(world: &World) -> Vec<Plate> {
    let ws = world.effective_world_size();
    let world_units_x = world.width as f32 / ws;
    let world_units_y = world.height as f32 / ws;
    let world_area = world_units_x * world_units_y;
    // Base count for a 1×1 world; scale linearly with geographic area.
    let base = (ws * ws / 12000.0).round() as usize;
    let approx = (base as f32 * world_area).round() as usize;
    let plate_count = approx.clamp(12, 140);
    let mut plates = Vec::with_capacity(plate_count);
    for i in 0..plate_count {
        let xf = hash01(world.seed.wrapping_add(17), i * 13 + 1, 0) * world_units_x;
        let yf = hash01(world.seed.wrapping_add(29), i * 17 + 3, 0) * world_units_y;
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

pub(super) fn sample_tectonic_elevation(
    world: &World,
    base: &OpenSimplex,
    ridge: &OpenSimplex,
    plates: &[Plate],
    cfg: &ContinentalConfig,
    x: usize,
    y: usize,
) -> OrogenSample {
    let ws = world.effective_world_size();
    let xf = x as f32 / ws;
    let yf = y as f32 / ws;
    let xf64 = xf as f64;
    let yf64 = yf as f64;
    let continental = sample_continental_fields(cfg, xf, yf);

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
    let shelf_break = octave_noise(base, xf64 * 0.78 - 13.0, yf64 * 0.78 + 17.0, 2, 0.5, 2.0);
    let margin_variation = octave_noise(base, xf64 * 1.6 + 51.0, yf64 * 1.6 - 27.0, 3, 0.55, 2.0);

    // Noise provides the broad-scale organic continent texture; lobe support shifts
    // which noise regions become land without replacing the noise signal entirely.
    let continental_density =
        (continental.support * 0.36 + continent * 0.40 + shelves * 0.10 + craton * 0.12
            - continental.ocean_basin * 0.18
            + continental.major_secondary_balance * 0.04)
            .clamp(0.0, 1.0);
    let continental_margin =
        (continental.support * 0.42 + shelf_break * 0.20 + margin_variation * 0.16
            - continental.ocean_basin * 0.20)
            .clamp(0.0, 1.0);
    let continent_mask = (continental_density * 0.72
        + continental_margin * 0.12
        + continental.interior * 0.10
        + plain_bands * 0.06)
        .clamp(0.0, 1.0);

    let tectonics = sample_uplift_field(plates, xf, yf);
    let land_mask = smoothstep(0.38, 0.72, continent_mask);
    let segmentation =
        smoothstep(0.42, 0.78, segment_noise) * 0.75 + smoothstep(0.52, 0.86, ridge_detail) * 0.25;
    let transfer_gap = 1.0 - smoothstep(0.58, 0.84, transfer_noise) * 0.62;
    let boundary_wide = smoothstep(0.08, 0.72, tectonics);
    let boundary_mid = smoothstep(0.22, 0.82, tectonics);
    let boundary_narrow = smoothstep(0.48, 0.94, tectonics);
    let axial_uplift =
        (boundary_narrow * segmentation * transfer_gap * (0.68 + ridge_detail * 0.42) * land_mask)
            .clamp(0.0, 1.0);
    let shoulder_uplift = ((boundary_mid - boundary_narrow * 0.45).max(0.0)
        * (0.42 + segment_noise * 0.30)
        * land_mask)
        .clamp(0.0, 1.0);
    let plateau_support = (boundary_mid
        * smoothstep(0.56, 0.86, plateau_noise)
        * smoothstep(0.48, 0.86, ridge_detail)
        * (0.13 + axial_uplift * 0.26)
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
        * (0.58 + continental.interior * 0.42)
        * (1.0 - boundary_mid * 0.75)
        * land_mask)
        .clamp(0.0, 1.0);
    let basin_bias = (smoothstep(0.44, 0.82, basin_noise)
        * (0.78 + continental.ocean_basin * 0.28)
        * (0.45 + (1.0 - boundary_narrow) * 0.4)
        * land_mask)
        .clamp(0.0, 1.0);

    let basement = (continent_mask * 0.52
        + plains * 0.12
        + craton * 0.16
        + plain_bands * 0.08
        + continental.interior * 0.08
        - basin_bias * 0.10
        - continental.ocean_basin * 0.12)
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
    let mut total_weight = 0.0_f32;
    let mut max_weight = 0.0_f32;
    let mut mean_velocity = (0.0_f32, 0.0_f32);

    for plate in plates {
        let dx = xf - plate.x;
        let dy = yf - plate.y;
        let weight = plate_influence(dx * dx + dy * dy);
        total_weight += weight;
        max_weight = max_weight.max(weight);
        mean_velocity.0 += plate.vx * weight;
        mean_velocity.1 += plate.vy * weight;
    }

    if total_weight <= f32::EPSILON {
        return 0.0;
    }

    mean_velocity.0 /= total_weight;
    mean_velocity.1 /= total_weight;
    let dominance = max_weight / total_weight;
    let plate_mixing = (1.0 - dominance).clamp(0.0, 1.0);

    let mut velocity_variance = 0.0_f32;
    let mut convergence = 0.0_f32;
    let mut shear = 0.0_f32;
    for plate in plates {
        let dx = xf - plate.x;
        let dy = yf - plate.y;
        let weight = plate_influence(dx * dx + dy * dy) / total_weight;
        let relative_velocity = (plate.vx - mean_velocity.0, plate.vy - mean_velocity.1);
        let radial = normalize((dx, dy));
        velocity_variance += weight
            * (relative_velocity.0 * relative_velocity.0
                + relative_velocity.1 * relative_velocity.1);
        convergence +=
            weight * (relative_velocity.0 * radial.0 + relative_velocity.1 * radial.1).max(0.0);
        shear += weight * (relative_velocity.0 * -radial.1 + relative_velocity.1 * radial.0).abs();
    }

    let mixed_boundary = smoothstep(0.18, 0.48, plate_mixing);
    let strain = smoothstep(0.12, 0.56, velocity_variance.sqrt());
    let compression = smoothstep(0.16, 0.58, convergence + shear * 0.16);

    (mixed_boundary * strain * compression).clamp(0.0, 1.0)
}

fn plate_influence(dist2: f32) -> f32 {
    1.0 / (dist2 + 0.006).powf(1.35)
}
