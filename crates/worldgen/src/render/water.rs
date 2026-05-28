use image::{Rgba, RgbaImage};

use crate::generate::smoothstep;
use crate::river::river_discharge_percentiles;
use crate::{Biome, Surface, World};

use super::shading::{draw_disc, draw_thick_line, lerp_rgba, put_pixel_checked, tile_center_px};

pub(super) fn draw_coastline(image: &mut RgbaImage, world: &World, idx: usize, scale: u32) {
    let (x, y) = world.coords(idx);
    let ox = (x as u32) * scale;
    let oy = (y as u32) * scale;
    let c = Rgba([218, 210, 158, 255]);
    // Draw the beach line only on sides that face ocean, so it follows the actual shore.
    for (dx, dy) in [(-1_isize, 0_isize), (1, 0), (0, -1), (0, 1)] {
        let nx = x as isize + dx;
        let ny = y as isize + dy;
        if !world.in_bounds(nx, ny) {
            continue;
        }
        if !matches!(
            world.tiles[world.idx(nx as usize, ny as usize)].surface,
            Surface::Ocean
        ) {
            continue;
        }
        match (dx, dy) {
            (0, -1) => {
                (0..scale).for_each(|px| put_pixel_checked(image, (ox + px) as i32, oy as i32, c))
            }
            (0, 1) => (0..scale).for_each(|px| {
                put_pixel_checked(image, (ox + px) as i32, (oy + scale - 1) as i32, c)
            }),
            (-1, 0) => {
                (0..scale).for_each(|py| put_pixel_checked(image, ox as i32, (oy + py) as i32, c))
            }
            (1, 0) => (0..scale).for_each(|py| {
                put_pixel_checked(image, (ox + scale - 1) as i32, (oy + py) as i32, c)
            }),
            _ => {}
        }
    }
}

pub(super) fn draw_lake(image: &mut RgbaImage, world: &World, idx: usize, scale: u32) {
    let (x, y) = world.coords(idx);
    let ox = (x as u32) * scale;
    let oy = (y as u32) * scale;
    let tile = &world.tiles[idx];
    let depth = tile
        .water_level
        .map(|wl| (wl - tile.raw_elevation).max(0.0))
        .unwrap_or(0.0);
    let deep_t = smoothstep(0.0, 0.065, depth);
    let c = lerp_rgba(Rgba([76, 164, 218, 255]), Rgba([46, 122, 186, 255]), deep_t);
    for py in 0..scale {
        for px in 0..scale {
            image.put_pixel(ox + px, oy + py, c);
        }
    }
}

pub(super) fn draw_river(
    image: &mut RgbaImage,
    world: &World,
    idx: usize,
    scale: u32,
    flow: f32,
    thresholds: RiverThresholds,
) {
    let radius = river_radius_px(world, idx, flow, scale, thresholds);
    if radius < 0 {
        return;
    }
    let (x, y) = world.coords(idx);
    let color = river_color(flow, thresholds);
    let start = tile_center_px(x, y, scale);

    if let Some(next) = world.tiles[idx].downstream {
        let (nx, ny) = world.coords(next);
        let dx = nx.abs_diff(x);
        let dy = ny.abs_diff(y);
        if dx <= 1 && dy <= 1 {
            draw_thick_line(image, start, tile_center_px(nx, ny, scale), radius, color);
            return;
        }
    }

    draw_disc(image, start, radius, color);
}

#[derive(Clone, Copy)]
pub(super) struct RiverThresholds {
    secondary: f32,
    trunk: f32,
}

pub(super) fn river_thresholds(world: &World) -> RiverThresholds {
    let (secondary, trunk) = river_discharge_percentiles(world, 58, 84);
    RiverThresholds { secondary, trunk }
}

fn river_color(flow: f32, thresholds: RiverThresholds) -> Rgba<u8> {
    if flow > thresholds.trunk {
        Rgba([42, 118, 192, 255])
    } else if flow > thresholds.secondary {
        Rgba([58, 148, 214, 255])
    } else {
        Rgba([88, 176, 228, 255])
    }
}

fn river_radius_px(
    world: &World,
    idx: usize,
    flow: f32,
    scale: u32,
    thresholds: RiverThresholds,
) -> i32 {
    let tile = &world.tiles[idx];
    let height_above_sea = tile.raw_elevation - world.sea_level;
    let lowland = height_above_sea < 0.16;
    let steep = matches!(tile.biome, Biome::Alpine | Biome::Foothills) || height_above_sea > 0.32;

    if tile.channel_order >= 3 || flow >= thresholds.trunk {
        let excess = flow / thresholds.trunk;
        match scale {
            // Trunk rivers always visible; guarantee at least 1px even at minimum scale.
            0 | 1 => 1,
            2 => i32::from(!steep) + 1,
            3 | 4 => i32::from(!steep && (lowland || excess >= 1.8)) + 1,
            _ => {
                let width = scale as f32
                    * (0.72 + (excess.clamp(1.0, 5.0) - 1.0) * 0.10 + f32::from(lowland) * 0.10
                        - f32::from(steep) * 0.10);
                ((width.round() as i32) / 2).clamp(1, (scale as i32 * 2).max(2))
            }
        }
    } else if tile.channel_order >= 2 || flow >= thresholds.secondary {
        match scale {
            0 | 1 => 0,
            2 | 3 => i32::from(!steep),
            _ => i32::from(!steep) + i32::from(!steep && lowland),
        }
    } else {
        // Headwaters: only draw where each tile occupies enough pixels to show them clearly.
        if scale >= 3 { 0 } else { -1 }
    }
}
