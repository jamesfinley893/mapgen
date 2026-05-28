use noise::OpenSimplex;

use crate::World;

use super::continents::sample_continental_fields;
use super::{ContinentalConfig, OrogenSample, Plate};
use crate::generate::util::{hash01, normalize, octave_noise, ridge_noise, smoothstep};

pub(super) fn generate_plates(world: &World) -> Vec<Plate> {
    let ws = world.effective_world_size();
    let world_units_x = world.width as f32 / ws;
    let world_units_y = world.height as f32 / ws;
    let world_area = world_units_x * world_units_y;
    // Base count for a 1×1 world; scale linearly with geographic area.
    let base = (ws * ws / 18000.0).round() as usize;
    let approx = (base as f32 * world_area).round() as usize;
    let plate_count = approx.clamp(8, 120);
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
    let seaway_land_cut = continental.seaway_cut * smoothstep(0.18, 0.62, continental.support);
    let continental_density =
        (continental.support * 0.36 + continent * 0.40 + shelves * 0.10 + craton * 0.12
            - continental.ocean_basin * 0.18
            - seaway_land_cut * 0.14
            + continental.major_secondary_balance * 0.04)
            .clamp(0.0, 1.0);
    let continental_margin =
        (continental.support * 0.42 + shelf_break * 0.20 + margin_variation * 0.16
            - continental.ocean_basin * 0.18
            - seaway_land_cut * 0.14)
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
        (boundary_narrow * segmentation * transfer_gap * (0.55 + ridge_detail * 0.35) * land_mask)
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
        * (0.58 + continental.interior * 0.42)
        * (1.0 - boundary_mid * 0.75)
        * land_mask)
        .clamp(0.0, 1.0);
    let basin_bias = (smoothstep(0.44, 0.82, basin_noise)
        * (0.78 + seaway_land_cut * 0.34 + continental.ocean_basin * 0.18)
        * (0.45 + (1.0 - boundary_narrow) * 0.4)
        * land_mask)
        .clamp(0.0, 1.0);

    let basement = (continent_mask * 0.52
        + plains * 0.12
        + craton * 0.16
        + plain_bands * 0.08
        + continental.interior * 0.08
        - basin_bias * 0.10
        - continental.ocean_basin * 0.10
        - seaway_land_cut * 0.04)
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
        let dx = xf - plate.x;
        let dy = yf - plate.y;
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
    let shear = ((rel_velocity.0 * -normal.1 + rel_velocity.1 * normal.0).abs()).clamp(0.0, 1.0);
    let orogeny = smoothstep(0.45, 0.92, convergence * 0.9 + shear * 0.18);

    boundary * orogeny
}
