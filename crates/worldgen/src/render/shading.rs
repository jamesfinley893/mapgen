use image::{Rgba, RgbaImage};

use crate::Biome;
use crate::World;
use crate::generate::{hash01, smoothstep};

pub(super) fn draw_tile(image: &mut RgbaImage, x: u32, y: u32, scale: u32, color: Rgba<u8>) {
    let ox = x * scale;
    let oy = y * scale;
    for py in 0..scale {
        for px in 0..scale {
            image.put_pixel(ox + px, oy + py, color);
        }
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
    let center_tile = &world.tiles[world.idx(tx, ty)];
    let center_biome = center_tile.biome;
    let h00 = hillshade[world.idx(tx, ty)];
    let get_hs = |cx: usize, cy: usize| -> f32 {
        let cx = cx.min(world.width.saturating_sub(1));
        let cy = cy.min(world.height.saturating_sub(1));
        // Land height is continuous across biome boundaries; only avoid blending
        // with ocean tiles, which use their own depth rendering.
        if world.tiles[world.idx(cx, cy)].biome == Biome::Ocean {
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
            let shade_factor = match center_biome {
                Biome::Alpine => 0.34 + shade * 0.72,
                Biome::Foothills => 0.36 + shade * 0.68,
                _ => 0.37 + shade * 0.61,
            };
            let color = scale_rgb(base_color, shade_factor);
            // Aspect tinting: lit faces warm (+R, -B), shadowed faces cool (-R, +B).
            let tint = ((shade - 0.5) * 16.0) as i16;
            let mut color = Rgba([
                (color[0] as i16 + tint).clamp(0, 255) as u8,
                color[1],
                (color[2] as i16 - tint).clamp(0, 255) as u8,
                255,
            ]);
            if scale >= 3 {
                let gx = (ox + px) as usize;
                let gy = (oy + py) as usize;
                let coarse = hash01(world.seed ^ 0x95A7_1D41, gx / 5, gy / 5);
                let fine = hash01(world.seed ^ 0x2C67_54ED, gx / 2, gy / 2);
                let grain = coarse * 0.72 + fine * 0.28 - 0.5;
                let height_above_sea = (center_tile.raw_elevation - world.sea_level).max(0.0);
                let relief = smoothstep(0.04, 0.34, height_above_sea);
                let biome_strength = match center_biome {
                    Biome::Alpine | Biome::Foothills => 1.18,
                    Biome::TemperateForest
                    | Biome::BorealForest
                    | Biome::Rainforest
                    | Biome::TropicalForest => 0.82,
                    Biome::Desert | Biome::PolarDesert => 0.74,
                    Biome::Ocean => 0.0,
                    _ => 0.92,
                };
                let rugged = (center_tile.relief + center_tile.slope * 0.75).clamp(0.0, 1.0);
                let detail = (grain * (4.0 + relief * 4.2 + rugged * 18.0) * biome_strength) as i16;
                color = offset(color, detail);
            }
            image.put_pixel(ox + px, oy + py, color);
        }
    }
}

pub(super) fn compute_hillshade(world: &World, x: usize, y: usize) -> f32 {
    let center_biome = world.tiles[world.idx(x, y)].biome;
    let center_raw_elev = world.tiles[world.idx(x, y)].raw_elevation;
    let visual_elevation = |elevation: f32| -> f32 {
        let height_above_sea = (elevation - world.sea_level).max(0.0);
        height_above_sea / (height_above_sea + 0.74)
    };
    // Use the actual land elevation field across biome boundaries so the render
    // remains faithful to terrain height; transform it through a visual curve so
    // extreme peaks do not alias into black one-tile cliffs.
    let get_visual_elev = |xi: isize, yi: isize| -> f32 {
        let cx = xi.clamp(0, world.width as isize - 1) as usize;
        let cy = yi.clamp(0, world.height as isize - 1) as usize;
        let neighbor = &world.tiles[world.idx(cx, cy)];
        if neighbor.biome == Biome::Ocean {
            0.0
        } else {
            visual_elevation(neighbor.raw_elevation)
        }
    };
    let xi = x as isize;
    let yi = y as isize;
    let dz_dx = get_visual_elev(xi + 1, yi) - get_visual_elev(xi - 1, yi);
    let dz_dy = get_visual_elev(xi, yi + 1) - get_visual_elev(xi, yi - 1);
    // Adaptive z_scale: mountains get dramatic relief, plains stay gentle.
    let height_above_sea = (center_raw_elev - world.sea_level).max(0.0);
    let visual_height = height_above_sea / (height_above_sea + 0.82);
    let lowland_relief =
        smoothstep(0.04, 0.22, height_above_sea) * (1.0 - smoothstep(0.30, 0.70, height_above_sea));
    let z_scale = match center_biome {
        Biome::Alpine => 10.0 + visual_height * 31.0,
        Biome::Foothills => 7.5 + visual_height * 25.0,
        _ => 6.5 + visual_height * 24.0 + lowland_relief * 3.0,
    };
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
