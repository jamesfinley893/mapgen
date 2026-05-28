use image::{Rgba, RgbaImage};

use crate::World;
use crate::generate::hash01;

pub(super) fn draw_tile(image: &mut RgbaImage, x: u32, y: u32, scale: u32, color: Rgba<u8>) {
    let ox = x * scale;
    let oy = y * scale;
    for py in 0..scale {
        for px in 0..scale {
            image.put_pixel(ox + px, oy + py, color);
        }
    }
}

pub(super) fn tile_center_px(x: usize, y: usize, scale: u32) -> (i32, i32) {
    (
        (x as u32 * scale + scale / 2) as i32,
        (y as u32 * scale + scale / 2) as i32,
    )
}

pub(super) fn draw_thick_line(
    image: &mut RgbaImage,
    start: (i32, i32),
    end: (i32, i32),
    radius: i32,
    color: Rgba<u8>,
) {
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let steps = dx.abs().max(dy.abs()).max(1);

    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let x = (start.0 as f32 + dx as f32 * t).round() as i32;
        let y = (start.1 as f32 + dy as f32 * t).round() as i32;
        draw_disc(image, (x, y), radius, color);
    }
}

pub(super) fn draw_disc(image: &mut RgbaImage, center: (i32, i32), radius: i32, color: Rgba<u8>) {
    let radius = radius.max(0);
    let radius_sq = radius * radius;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= radius_sq {
                put_pixel_checked(image, center.0 + dx, center.1 + dy, color);
            }
        }
    }
}

pub(super) fn put_pixel_checked(image: &mut RgbaImage, x: i32, y: i32, color: Rgba<u8>) {
    if x >= 0 && y >= 0 && (x as u32) < image.width() && (y as u32) < image.height() {
        image.put_pixel(x as u32, y as u32, color);
    }
}

pub(super) fn offset(color: Rgba<u8>, delta: i16) -> Rgba<u8> {
    let mut out = [0_u8; 4];
    for (i, channel) in color.0.iter().enumerate() {
        if i == 3 {
            out[i] = *channel;
        } else {
            out[i] = ((*channel as i16 + delta).clamp(0, 255)) as u8;
        }
    }
    Rgba(out)
}

pub(super) fn draw_tile_hillshaded(
    image: &mut RgbaImage,
    hillshade: &[f32],
    world: &World,
    x: u32,
    y: u32,
    scale: u32,
    base_color: Rgba<u8>,
) {
    let ox = x * scale;
    let oy = y * scale;
    let tx = x as usize;
    let ty = y as usize;
    let center_biome = world.tiles[world.idx(tx, ty)].biome;
    let h00 = hillshade[world.idx(tx, ty)];
    let get_hs = |cx: usize, cy: usize| -> f32 {
        let cx = cx.min(world.width.saturating_sub(1));
        let cy = cy.min(world.height.saturating_sub(1));
        // Don't interpolate hillshade across biome boundaries — a bright Alpine
        // face would otherwise bleed into adjacent forest/grassland tiles.
        if world.tiles[world.idx(cx, cy)].biome != center_biome {
            h00
        } else {
            hillshade[world.idx(cx, cy)]
        }
    };
    let h10 = get_hs(tx + 1, ty);
    let h01 = get_hs(tx, ty + 1);
    let h11 = get_hs(tx + 1, ty + 1);
    let s = scale as f32;
    for py in 0..scale {
        for px in 0..scale {
            let fx = (px as f32 + 0.5) / s;
            let fy = (py as f32 + 0.5) / s;
            let shade = h00 * (1.0 - fx) * (1.0 - fy)
                + h10 * fx * (1.0 - fy)
                + h01 * (1.0 - fx) * fy
                + h11 * fx * fy;
            let color = scale_rgb(base_color, 0.28 + shade * 0.72);
            // Aspect tinting: lit faces warm (+R, -B), shadowed faces cool (-R, +B).
            let tint = ((shade - 0.5) * 16.0) as i16;
            let color = Rgba([
                (color[0] as i16 + tint).clamp(0, 255) as u8,
                color[1],
                (color[2] as i16 - tint).clamp(0, 255) as u8,
                255,
            ]);
            image.put_pixel(ox + px, oy + py, color);
        }
    }
}

pub(super) fn compute_hillshade(world: &World, x: usize, y: usize) -> f32 {
    let center_biome = world.tiles[world.idx(x, y)].biome;
    let center_elev = world.tiles[world.idx(x, y)].raw_elevation;
    // Don't let a neighbor from a different biome drive the gradient — a forest
    // tile at the base of a mountain would otherwise inherit the mountain's steep
    // slope and render as a bright halo.
    let get_elev = |xi: isize, yi: isize| -> f32 {
        let cx = xi.clamp(0, world.width as isize - 1) as usize;
        let cy = yi.clamp(0, world.height as isize - 1) as usize;
        if world.tiles[world.idx(cx, cy)].biome != center_biome {
            center_elev
        } else {
            world.tiles[world.idx(cx, cy)].raw_elevation
        }
    };
    let xi = x as isize;
    let yi = y as isize;
    let dz_dx = get_elev(xi + 1, yi) - get_elev(xi - 1, yi);
    let dz_dy = get_elev(xi, yi + 1) - get_elev(xi, yi - 1);
    // Adaptive z_scale: mountains get dramatic relief, plains stay gentle.
    let elev = get_elev(xi, yi);
    let height_above_sea = (elev - world.sea_level).max(0.0);
    let z_scale = 4.0 + height_above_sea * 18.0;
    let nx = -dz_dx * z_scale;
    let ny = 1.0_f32;
    let nz = -dz_dy * z_scale;
    let len = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-6);
    // Light from NW at 45° elevation
    let inv_sqrt3 = 1.0_f32 / 3.0_f32.sqrt();
    ((nx * (-inv_sqrt3) + ny * inv_sqrt3 + nz * (-inv_sqrt3)) / len).clamp(0.0, 1.0)
}

fn scale_rgb(color: Rgba<u8>, factor: f32) -> Rgba<u8> {
    Rgba([
        (color[0] as f32 * factor).clamp(0.0, 255.0) as u8,
        (color[1] as f32 * factor).clamp(0.0, 255.0) as u8,
        (color[2] as f32 * factor).clamp(0.0, 255.0) as u8,
        color[3],
    ])
}

pub(super) fn lerp_rgba(a: Rgba<u8>, b: Rgba<u8>, t: f32) -> Rgba<u8> {
    Rgba([
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).clamp(0.0, 255.0) as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).clamp(0.0, 255.0) as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t).clamp(0.0, 255.0) as u8,
        255,
    ])
}

// Bilinearly-interpolated value noise — gives spatially-coherent variation
// within a biome without needing the generation-side noise functions.
pub(super) fn sample_noise(seed: u64, x: usize, y: usize, cell: usize) -> f32 {
    let cell = cell.max(1);
    let fx = x as f32 / cell as f32;
    let fy = y as f32 / cell as f32;
    let x0 = fx.floor() as usize;
    let y0 = fy.floor() as usize;
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;
    let sx = tx * tx * (3.0 - 2.0 * tx);
    let sy = ty * ty * (3.0 - 2.0 * ty);
    let v00 = hash01(seed, x0, y0);
    let v10 = hash01(seed, x0 + 1, y0);
    let v01 = hash01(seed, x0, y0 + 1);
    let v11 = hash01(seed, x0 + 1, y0 + 1);
    let ix0 = v00 + (v10 - v00) * sx;
    let ix1 = v01 + (v11 - v01) * sx;
    ix0 + (ix1 - ix0) * sy
}
