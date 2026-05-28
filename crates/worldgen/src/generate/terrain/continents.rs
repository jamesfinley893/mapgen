use super::{ContinentalConfig, ContinentalFields, PreparedCut, PreparedLobe};
use crate::generate::util::{hash01, smoothstep};

pub(super) fn build_continental_config(
    seed: u64,
    world_units_x: f32,
    world_units_y: f32,
) -> ContinentalConfig {
    let world_area = world_units_x * world_units_y;
    let scale = world_area.sqrt();
    // For a 1×1 world: 3–5 land lobes, 2–4 basins, 1–2 seaways. Scale counts with world size.
    let land_count = (3 + (seed as usize % 3)) + (scale * 2.0).floor() as usize;
    let basin_count = (2 + (seed.wrapping_mul(3) as usize % 3)) + (scale * 1.5).floor() as usize;
    let seaway_count = (1 + (seed.wrapping_mul(5) as usize % 2)) + (scale * 0.8).floor() as usize;

    let mut land_lobes = Vec::with_capacity(land_count);
    for i in 0..land_count {
        let cx = hash01(seed.wrapping_add(0xC011_1000), i * 7 + 1, 0) * (world_units_x * 1.4)
            - world_units_x * 0.2;
        let cy = hash01(seed.wrapping_add(0xC011_2000), i * 11 + 3, 0) * (world_units_y * 1.4)
            - world_units_y * 0.2;
        let angle = hash01(seed.wrapping_add(0xC011_3000), i * 13 + 5, 0) * std::f32::consts::TAU;
        let rx = 0.20 + hash01(seed.wrapping_add(0xC011_4000), i * 17 + 7, 0) * 0.30;
        let ry = 0.16 + hash01(seed.wrapping_add(0xC011_5000), i * 19 + 9, 0) * 0.26;
        let strength = 0.60 + hash01(seed.wrapping_add(0xC011_6000), i * 23 + 11, 0) * 0.34;
        let (sin_a, cos_a) = angle.sin_cos();
        land_lobes.push(PreparedLobe {
            cx,
            cy,
            sin_a,
            cos_a,
            rx,
            ry,
            strength,
        });
    }

    let mut basins = Vec::with_capacity(basin_count);
    for i in 0..basin_count {
        let cx = hash01(seed.wrapping_add(0xB451_1000), i * 7 + 2, 0) * (world_units_x * 1.4)
            - world_units_x * 0.2;
        let cy = hash01(seed.wrapping_add(0xB451_2000), i * 11 + 4, 0) * (world_units_y * 1.4)
            - world_units_y * 0.2;
        let angle = hash01(seed.wrapping_add(0xB451_3000), i * 13 + 6, 0) * std::f32::consts::TAU;
        let rx = 0.18 + hash01(seed.wrapping_add(0xB451_4000), i * 17 + 8, 0) * 0.24;
        let ry = 0.14 + hash01(seed.wrapping_add(0xB451_5000), i * 19 + 10, 0) * 0.22;
        let strength = 0.55 + hash01(seed.wrapping_add(0xB451_6000), i * 23 + 12, 0) * 0.35;
        let (sin_a, cos_a) = angle.sin_cos();
        basins.push(PreparedLobe {
            cx,
            cy,
            sin_a,
            cos_a,
            rx,
            ry,
            strength,
        });
    }

    let mut seaways = Vec::with_capacity(seaway_count);
    for i in 0..seaway_count {
        let cx = hash01(seed.wrapping_add(0x5EA0_1000), i * 5 + 1, 0) * (world_units_x * 1.4)
            - world_units_x * 0.2;
        let cy = hash01(seed.wrapping_add(0x5EA0_2000), i * 9 + 3, 0) * (world_units_y * 1.4)
            - world_units_y * 0.2;
        let angle = hash01(seed.wrapping_add(0x5EA0_3000), i * 13 + 5, 0) * std::f32::consts::TAU;
        let width = 0.035 + hash01(seed.wrapping_add(0x5EA0_4000), i * 17 + 7, 0) * 0.09;
        let extent = 0.28 + hash01(seed.wrapping_add(0x5EA0_5000), i * 19 + 9, 0) * 0.34;
        let strength = 0.55 + hash01(seed.wrapping_add(0x5EA0_6000), i * 23 + 11, 0) * 0.30;
        let (sin_a, cos_a) = angle.sin_cos();
        seaways.push(PreparedCut {
            cx,
            cy,
            sin_a,
            cos_a,
            width,
            extent,
            strength,
        });
    }

    ContinentalConfig {
        land_lobes,
        basins,
        seaways,
    }
}

pub(super) fn sample_continental_fields(
    cfg: &ContinentalConfig,
    xf: f32,
    yf: f32,
) -> ContinentalFields {
    let land_count = cfg.land_lobes.len();

    let mut strongest = 0.0_f32;
    let mut second = 0.0_f32;
    let mut land_sum = 0.0_f32;
    let mut interior_sum = 0.0_f32;
    for lobe in &cfg.land_lobes {
        let lobe_val = eval_lobe(lobe, xf, yf, 1.0) * lobe.strength;
        let core_val = eval_lobe(lobe, xf, yf, 0.62) * lobe.strength;
        land_sum += lobe_val;
        interior_sum += core_val;
        if lobe_val > strongest {
            second = strongest;
            strongest = lobe_val;
        } else if lobe_val > second {
            second = lobe_val;
        }
    }

    let mut basin_max = 0.0_f32;
    for basin in &cfg.basins {
        let val = eval_lobe(basin, xf, yf, 1.0) * basin.strength;
        basin_max = basin_max.max(val);
    }

    let mut seaway_cut = 0.0_f32;
    for cut in &cfg.seaways {
        seaway_cut = seaway_cut.max(eval_seaway(cut, xf, yf) * cut.strength);
    }

    let dominant = strongest.clamp(0.0, 1.0);
    let secondary = second.clamp(0.0, 1.0);
    let blended =
        (dominant * 0.60 + secondary * 0.24 + (land_sum / land_count as f32).min(1.0) * 0.22
            - basin_max * 0.22
            - seaway_cut * 0.18)
            .clamp(0.0, 1.0);
    let support = (blended + (dominant - secondary).max(0.0) * 0.08).clamp(0.0, 1.0);
    let interior = ((interior_sum / land_count as f32) * 0.72 + dominant * 0.22 - basin_max * 0.10)
        .clamp(0.0, 1.0);

    ContinentalFields {
        support,
        interior,
        seaway_cut,
        ocean_basin: basin_max,
        major_secondary_balance: (secondary - dominant * 0.55).max(0.0),
    }
}

// Evaluates a rotated elliptical influence field at (xf, yf).
// `scale` shrinks both radii (1.0 = full lobe, 0.62 = inner core).
fn eval_lobe(lobe: &PreparedLobe, xf: f32, yf: f32, scale: f32) -> f32 {
    let dx = xf - lobe.cx;
    let dy = yf - lobe.cy;
    let lx = dx * lobe.cos_a + dy * lobe.sin_a;
    let ly = -dx * lobe.sin_a + dy * lobe.cos_a;
    let rx = lobe.rx * scale;
    let ry = lobe.ry * scale;
    let radius = ((lx / rx).powi(2) + (ly / ry).powi(2)).sqrt();
    (1.0 - smoothstep(0.48, 1.40, radius)).clamp(0.0, 1.0)
}

fn eval_seaway(cut: &PreparedCut, xf: f32, yf: f32) -> f32 {
    let dx = xf - cut.cx;
    let dy = yf - cut.cy;
    let along = dx * cut.cos_a + dy * cut.sin_a;
    let perp = -dx * cut.sin_a + dy * cut.cos_a;
    let width_term = 1.0 - smoothstep(cut.width * 0.55, cut.width, perp.abs());
    let extent_term = 1.0 - smoothstep(cut.extent * 0.82, cut.extent, along.abs());
    (width_term * extent_term).clamp(0.0, 1.0)
}
