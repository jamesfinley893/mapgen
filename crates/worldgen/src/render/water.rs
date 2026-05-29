use image::{Rgba, RgbaImage};

use crate::generate::{hash01, smoothstep};
use crate::{Biome, Surface, World};

use super::colors::RIVER_COLOR;
use super::shading::{lerp_rgba, put_pixel_checked, tile_center_px};

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

pub(super) fn draw_river_tile(
    image: &mut RgbaImage,
    world: &World,
    idx: usize,
    scale: u32,
    land_colors: &[Rgba<u8>],
) {
    let (x, y) = world.coords(idx);
    let ox = (x as u32) * scale;
    let oy = (y as u32) * scale;
    let bank = river_bank_width(scale);
    let bank_f = bank as f32;

    for py in 0..scale {
        for px in 0..scale {
            let mut color = RIVER_COLOR;
            for (edge_dist, neighbor) in [
                (px, neighbor_idx(world, x, y, -1, 0)),
                (scale - 1 - px, neighbor_idx(world, x, y, 1, 0)),
                (py, neighbor_idx(world, x, y, 0, -1)),
                (scale - 1 - py, neighbor_idx(world, x, y, 0, 1)),
            ] {
                if edge_dist >= bank {
                    continue;
                }
                let Some(nidx) = neighbor else {
                    continue;
                };
                if is_open_water(world.tiles[nidx].surface) {
                    continue;
                }
                let edge_t = 1.0 - edge_dist as f32 / bank_f;
                color = lerp_rgba(color, land_colors[nidx], edge_t * 0.26);
                let cut = bank_cut_strength(world, idx, nidx);
                if cut > 0.0 {
                    color = lerp_rgba(color, Rgba([24, 52, 94, 255]), edge_t * cut * 0.22);
                }
            }
            image.put_pixel(ox + px, oy + py, color);
        }
    }
}

pub(super) fn draw_river_banks(image: &mut RgbaImage, world: &World, idx: usize, scale: u32) {
    if scale <= 1 {
        return;
    }
    let (x, y) = world.coords(idx);
    let ox = (x as u32) * scale;
    let oy = (y as u32) * scale;
    let bank = river_bank_width(scale);
    let bank_shadow = Rgba([39, 50, 45, 255]);

    for (dx, dy, edge) in [
        (-1, 0, BankEdge::West),
        (1, 0, BankEdge::East),
        (0, -1, BankEdge::North),
        (0, 1, BankEdge::South),
    ] {
        let Some(nidx) = neighbor_idx(world, x, y, dx, dy) else {
            continue;
        };
        if world.tiles[nidx].surface != Surface::River {
            continue;
        }
        let cut = bank_cut_strength(world, nidx, idx);
        if cut <= 0.0 {
            continue;
        }
        for i in 0..scale {
            for d in 0..bank {
                let t = 1.0 - d as f32 / bank as f32;
                let alpha = 0.24 * cut * t;
                let (px, py) = match edge {
                    BankEdge::West => (d, i),
                    BankEdge::East => (scale - 1 - d, i),
                    BankEdge::North => (i, d),
                    BankEdge::South => (i, scale - 1 - d),
                };
                let bg = *image.get_pixel(ox + px, oy + py);
                image.put_pixel(ox + px, oy + py, lerp_rgba(bg, bank_shadow, alpha));
            }
        }
    }
}

// River tiles and connector strokes use the same flat colour so streams don't
// change hue as order, biome, or render scale changes.
pub(super) fn draw_river(image: &mut RgbaImage, world: &World, idx: usize, scale: u32) {
    let tile = &world.tiles[idx];
    let radius: i32 = match (tile.channel_order, scale) {
        (_, 1) => return, // scale=1: tile colour is sufficient
        (3..=4, 2) => 1,
        (3..=4, _) => ((tile.river_width * scale as f32 * 0.26).round() as i32).clamp(1, 7),
        (2, 3..=u32::MAX) => ((tile.river_width * scale as f32 * 0.18).round() as i32).clamp(0, 3),
        (1, 4..=u32::MAX) => 0,
        _ => return,
    };
    let (x, y) = world.coords(idx);
    let start = river_point_px(world, idx, scale);

    if let Some(next) = tile.downstream {
        let (nx, ny) = world.coords(next);
        if nx.abs_diff(x) <= 1 && ny.abs_diff(y) <= 1 {
            draw_river_line(
                image,
                start,
                river_point_px(world, next, scale),
                radius,
                RIVER_COLOR,
            );
            draw_rapid_marks(image, world, idx, scale);
            return;
        }
    }
    draw_river_disc(image, start, radius, RIVER_COLOR);
    draw_rapid_marks(image, world, idx, scale);
}

fn river_point_px(world: &World, idx: usize, scale: u32) -> (i32, i32) {
    let (x, y) = world.coords(idx);
    let center = tile_center_px(x, y, scale);
    let Some(next) = world.tiles[idx].downstream else {
        return center;
    };
    let (nx, ny) = world.coords(next);
    let fx = (nx as i32 - x as i32).signum();
    let fy = (ny as i32 - y as i32).signum();
    if fx == 0 && fy == 0 {
        return center;
    }
    let cross = (-fy, fx);
    let offset = world.tiles[idx].river_lateral_offset
        * world.tiles[idx].river_sinuosity
        * (scale as f32 * 0.24).min(2.2);
    (
        center.0 + (cross.0 as f32 * offset).round() as i32,
        center.1 + (cross.1 as f32 * offset).round() as i32,
    )
}

fn river_drop(world: &World, idx: usize) -> f32 {
    let tile = &world.tiles[idx];
    tile.downstream
        .map(|next| (tile.raw_elevation - world.tiles[next].raw_elevation).max(0.0))
        .unwrap_or(0.0)
}

fn bank_cut_strength(world: &World, river_idx: usize, land_idx: usize) -> f32 {
    if is_open_water(world.tiles[land_idx].surface) {
        return 0.0;
    }
    let river = &world.tiles[river_idx];
    let land = &world.tiles[land_idx];
    let bank_rise = land.raw_elevation - river.raw_elevation;
    let steepness = smoothstep(0.014, 0.095, bank_rise);
    if steepness <= 0.0 {
        return 0.0;
    }

    let height_above_sea = (river.raw_elevation - world.sea_level).max(0.0);
    let relief = smoothstep(0.05, 0.38, height_above_sea);
    let terrain = match land.biome {
        Biome::Alpine => 1.0,
        Biome::Foothills => 0.82,
        _ => 0.42 + relief * 0.42,
    };
    (steepness * terrain).clamp(0.0, 1.0)
}

fn river_bank_width(scale: u32) -> u32 {
    (scale / 4).clamp(1, 3).min(scale)
}

fn neighbor_idx(world: &World, x: usize, y: usize, dx: isize, dy: isize) -> Option<usize> {
    let nx = x as isize + dx;
    let ny = y as isize + dy;
    world
        .in_bounds(nx, ny)
        .then(|| world.idx(nx as usize, ny as usize))
}

fn is_open_water(surface: Surface) -> bool {
    matches!(surface, Surface::River | Surface::Lake | Surface::Ocean)
}

enum BankEdge {
    West,
    East,
    North,
    South,
}

fn draw_rapid_marks(image: &mut RgbaImage, world: &World, idx: usize, scale: u32) {
    if scale < 3 {
        return;
    }
    let drop = river_drop(world, idx);
    let drop_t = smoothstep(0.03, 0.085, drop);
    if drop_t <= 0.0 {
        return;
    }

    let (x, y) = world.coords(idx);
    let chance = 0.12 + drop_t * 0.26;
    if hash01(world.seed ^ 0xA7C5_4E2D, x, y) > chance {
        return;
    }

    let Some(next) = world.tiles[idx].downstream else {
        return;
    };
    let (nx, ny) = world.coords(next);
    let fx = nx as i32 - x as i32;
    let fy = ny as i32 - y as i32;
    if fx.abs() > 1 || fy.abs() > 1 || (fx == 0 && fy == 0) {
        return;
    }

    let center = tile_center_px(x, y, scale);
    let cross = (-fy.signum(), fx.signum());
    let cross = if cross == (0, 0) { (0, 1) } else { cross };
    let flow = (fx.signum(), fy.signum());
    let marks = if scale >= 5 && drop_t > 0.58 && hash01(world.seed ^ 0x65D9_EB13, x, y) < 0.45 {
        2
    } else {
        1
    };
    let half_len = (scale as i32 / 3).clamp(1, 3);
    let color = Rgba([168, 190, 205, 255]);
    let alpha = 0.20 + drop_t * 0.12;

    for mark in 0..marks {
        let offset = if marks == 1 {
            0
        } else if mark == 0 {
            -(scale as i32 / 5).max(1)
        } else {
            (scale as i32 / 5).max(1)
        };
        let cx = center.0 + flow.0 * offset;
        let cy = center.1 + flow.1 * offset;
        draw_alpha_line(
            image,
            (cx - cross.0 * half_len, cy - cross.1 * half_len),
            (cx + cross.0 * half_len, cy + cross.1 * half_len),
            color,
            alpha,
        );
    }
}

// Hard disc for radius<=1 (sharp thin lines); soft feathered edge for larger radii.
fn draw_river_disc(image: &mut RgbaImage, center: (i32, i32), radius: i32, color: Rgba<u8>) {
    let r = radius.max(0);
    if r <= 1 {
        let r_sq = r * r;
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r_sq {
                    let px = center.0 + dx;
                    let py = center.1 + dy;
                    if px >= 0
                        && py >= 0
                        && (px as u32) < image.width()
                        && (py as u32) < image.height()
                    {
                        image.put_pixel(px as u32, py as u32, color);
                    }
                }
            }
        }
    } else {
        let rf = r as f32;
        let search = r + 1;
        for dy in -search..=search {
            for dx in -search..=search {
                let dist = ((dx * dx + dy * dy) as f32).sqrt();
                let alpha = (rf + 0.5 - dist).clamp(0.0, 1.0);
                if alpha <= 0.0 {
                    continue;
                }
                let px = center.0 + dx;
                let py = center.1 + dy;
                if px < 0 || py < 0 || (px as u32) >= image.width() || (py as u32) >= image.height()
                {
                    continue;
                }
                if alpha >= 1.0 {
                    image.put_pixel(px as u32, py as u32, color);
                } else {
                    let bg = *image.get_pixel(px as u32, py as u32);
                    image.put_pixel(px as u32, py as u32, lerp_rgba(bg, color, alpha));
                }
            }
        }
    }
}

// Euclidean step count keeps stamp density uniform on diagonal segments.
fn draw_river_line(
    image: &mut RgbaImage,
    start: (i32, i32),
    end: (i32, i32),
    radius: i32,
    color: Rgba<u8>,
) {
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let steps = (((dx * dx + dy * dy) as f32).sqrt().ceil() as i32).max(1);
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let x = (start.0 as f32 + dx as f32 * t).round() as i32;
        let y = (start.1 as f32 + dy as f32 * t).round() as i32;
        draw_river_disc(image, (x, y), radius, color);
    }
}

fn draw_alpha_line(
    image: &mut RgbaImage,
    start: (i32, i32),
    end: (i32, i32),
    color: Rgba<u8>,
    alpha: f32,
) {
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let steps = dx.abs().max(dy.abs()).max(1);
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let x = (start.0 as f32 + dx as f32 * t).round() as i32;
        let y = (start.1 as f32 + dy as f32 * t).round() as i32;
        if x < 0 || y < 0 || (x as u32) >= image.width() || (y as u32) >= image.height() {
            continue;
        }
        let bg = *image.get_pixel(x as u32, y as u32);
        image.put_pixel(x as u32, y as u32, lerp_rgba(bg, color, alpha));
    }
}
