use noise::{NoiseFn, OpenSimplex};

use crate::World;

use super::DIAGONAL_COST;

pub(crate) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(crate) fn hash01(seed: u64, x: usize, y: usize) -> f32 {
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

pub(super) fn octave_noise(
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

pub(super) fn ridge_noise(noise: &OpenSimplex, x: f64, y: f64, octaves: usize) -> f32 {
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

pub(super) fn latitude_factor(y: usize, height: usize) -> f32 {
    let lat = y as f32 / (height.saturating_sub(1).max(1)) as f32;
    ((lat - 0.5).abs()) * 2.0
}

pub(super) fn normalize(v: (f32, f32)) -> (f32, f32) {
    let len = (v.0 * v.0 + v.1 * v.1).sqrt();
    if len <= f32::EPSILON {
        (0.0, 0.0)
    } else {
        (v.0 / len, v.1 / len)
    }
}

pub(super) fn direction_vector(from: (usize, usize), to: (usize, usize)) -> Option<(f32, f32)> {
    let dx = to.0 as isize - from.0 as isize;
    let dy = to.1 as isize - from.1 as isize;
    if dx == 0 && dy == 0 {
        None
    } else {
        Some(normalize((dx as f32, dy as f32)))
    }
}

pub(super) fn neighbor_distance(x: usize, y: usize, nx: usize, ny: usize) -> f32 {
    if x != nx && y != ny {
        DIAGONAL_COST
    } else {
        1.0
    }
}

pub(super) fn local_aspect(world: &World, x: usize, y: usize) -> (f32, f32) {
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

pub(super) fn local_aspect_on_values(
    values: &[f32],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
) -> (f32, f32) {
    let sample = |sx: isize, sy: isize| -> f32 {
        let cx = sx.clamp(0, width.saturating_sub(1) as isize) as usize;
        let cy = sy.clamp(0, height.saturating_sub(1) as isize) as usize;
        values[cy * width + cx]
    };
    let x = x as isize;
    let y = y as isize;
    let dx = sample(x + 1, y) - sample(x - 1, y);
    let dy = sample(x, y + 1) - sample(x, y - 1);
    normalize((-dx, -dy))
}

pub(super) fn sample_seed_field(
    seed: u64,
    x: usize,
    y: usize,
    cell_size: usize,
    channel: u64,
) -> f32 {
    let cell = cell_size.max(1) as f32;
    let fx = x as f32 / cell;
    let fy = y as f32 / cell;
    let x0 = fx.floor() as usize;
    let y0 = fy.floor() as usize;
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;
    let x1 = x0 + 1;
    let y1 = y0 + 1;

    let v00 = hash01(seed.wrapping_add(channel), x0, y0);
    let v10 = hash01(seed.wrapping_add(channel), x1, y0);
    let v01 = hash01(seed.wrapping_add(channel), x0, y1);
    let v11 = hash01(seed.wrapping_add(channel), x1, y1);

    let sx = smoothstep(0.0, 1.0, tx);
    let sy = smoothstep(0.0, 1.0, ty);
    let ix0 = v00 + (v10 - v00) * sx;
    let ix1 = v01 + (v11 - v01) * sx;
    (ix0 + (ix1 - ix0) * sy).clamp(0.0, 1.0)
}
