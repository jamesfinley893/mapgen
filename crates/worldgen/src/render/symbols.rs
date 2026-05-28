use image::{Rgba, RgbaImage};

use crate::World;

use super::shading::{draw_thick_line, put_pixel_checked, tile_center_px};

pub(super) fn draw_peak(image: &mut RgbaImage, x: u32, y: u32, scale: u32) {
    if scale < 2 {
        return;
    }
    let ox = x * scale;
    let oy = y * scale;
    let snow = Rgba([240, 244, 246, 255]);
    let lit = Rgba([195, 200, 197, 255]);
    let shadow = Rgba([88, 91, 89, 255]);
    let s = scale as f32;
    let apex_x = s * 0.50;
    let apex_y = s * 0.08;
    let base_y = s * 0.90;
    let base_l = s * 0.06;
    let base_r = s * 0.94;
    let snow_y = apex_y + (base_y - apex_y) * 0.42;

    // Explicit apex pixel so the peak tip is always visible
    put_pixel_checked(image, ox as i32 + (scale / 2) as i32, oy as i32, snow);

    for py in 0..scale {
        let pf_y = py as f32 + 0.5;
        if pf_y < apex_y || pf_y > base_y {
            continue;
        }
        let t = (pf_y - apex_y) / (base_y - apex_y);
        let lx = apex_x + (base_l - apex_x) * t;
        let rx = apex_x + (base_r - apex_x) * t;
        let mid_x = (lx + rx) * 0.5;
        for px in 0..scale {
            let pf_x = px as f32 + 0.5;
            if pf_x < lx || pf_x > rx {
                continue;
            }
            let color = if pf_y < snow_y {
                snow
            } else if pf_x <= mid_x {
                lit
            } else {
                shadow
            };
            put_pixel_checked(image, ox as i32 + px as i32, oy as i32 + py as i32, color);
        }
    }
}

pub(super) fn draw_ridge(image: &mut RgbaImage, world: &World, idx: usize, scale: u32) {
    if scale < 2 {
        return;
    }

    let (x, y) = world.coords(idx);
    let (axis_x, axis_y) = ridge_axis(world, x, y);
    let center = tile_center_px(x, y, scale);
    let reach = (scale as f32 * 0.42).round() as i32;
    let start = (
        center.0 - (axis_x * reach as f32).round() as i32,
        center.1 - (axis_y * reach as f32).round() as i32,
    );
    let end = (
        center.0 + (axis_x * reach as f32).round() as i32,
        center.1 + (axis_y * reach as f32).round() as i32,
    );

    draw_thick_line(image, start, end, 0, Rgba([80, 78, 72, 255]));
    if scale >= 5 {
        let normal = (-axis_y, axis_x);
        let lit_start = (
            start.0 + (normal.0 * 1.0).round() as i32,
            start.1 + (normal.1 * 1.0).round() as i32,
        );
        let lit_end = (
            end.0 + (normal.0 * 1.0).round() as i32,
            end.1 + (normal.1 * 1.0).round() as i32,
        );
        draw_thick_line(image, lit_start, lit_end, 0, Rgba([154, 156, 148, 255]));
    }
}

fn ridge_axis(world: &World, x: usize, y: usize) -> (f32, f32) {
    let pairs = [
        ((1_isize, 0_isize), (-1_isize, 0_isize), (1.0_f32, 0.0_f32)),
        ((0, 1), (0, -1), (0.0, 1.0)),
        ((1, 1), (-1, -1), (0.707, 0.707)),
        ((1, -1), (-1, 1), (0.707, -0.707)),
    ];

    let mut best = (1.0_f32, 0.0_f32);
    let mut best_score = f32::NEG_INFINITY;
    for &(a, b, axis) in &pairs {
        let score =
            sample_elevation(world, x, y, a.0, a.1) + sample_elevation(world, x, y, b.0, b.1);
        if score > best_score {
            best_score = score;
            best = axis;
        }
    }

    best
}

fn sample_elevation(world: &World, x: usize, y: usize, dx: isize, dy: isize) -> f32 {
    let nx = x as isize + dx;
    let ny = y as isize + dy;
    if world.in_bounds(nx, ny) {
        world.tiles[world.idx(nx as usize, ny as usize)].raw_elevation
    } else {
        world.tiles[world.idx(x, y)].raw_elevation
    }
}

pub(super) fn draw_hills(image: &mut RgbaImage, x: u32, y: u32, scale: u32) {
    if scale < 2 {
        return;
    }
    let ox = x * scale;
    let oy = y * scale;
    let outline = Rgba([74, 84, 58, 255]);
    let s = scale as f32;
    // Two overlapping hill arcs — classic cartographic foothills symbol
    for &(cx_f, cy_f, rx_f, ry_f) in &[
        (0.60_f32, 0.58_f32, 0.27_f32, 0.17_f32),
        (0.30_f32, 0.65_f32, 0.18_f32, 0.12_f32),
    ] {
        let cx = cx_f * s;
        let cy = cy_f * s;
        let rx = rx_f * s;
        let ry = ry_f * s;
        for py in 0..scale {
            for px in 0..scale {
                let dx = (px as f32 + 0.5) - cx;
                let dy = (py as f32 + 0.5) - cy;
                if dy > ry * 0.18 {
                    continue;
                }
                let d = (dx / rx).powi(2) + (dy / ry).powi(2);
                if (0.76..=1.30).contains(&d) {
                    put_pixel_checked(image, ox as i32 + px as i32, oy as i32 + py as i32, outline);
                }
            }
        }
    }
}

pub(super) fn draw_forest(image: &mut RgbaImage, x: u32, y: u32, scale: u32) {
    if scale < 2 {
        return;
    }
    let ox = x * scale;
    let oy = y * scale;
    let c = Rgba([30, 74, 34, 255]);
    image.put_pixel(ox + scale / 2, oy + scale / 3, c);
    if scale > 2 {
        image.put_pixel(ox + scale / 3, oy + (scale * 2 / 3), c);
        image.put_pixel(ox + (scale * 2 / 3), oy + (scale * 2 / 3), c);
    }
}

pub(super) fn draw_dunes(image: &mut RgbaImage, x: u32, y: u32, scale: u32) {
    if scale < 3 {
        return;
    }
    let ox = x * scale;
    let oy = y * scale;
    let c = Rgba([185, 160, 97, 255]);
    for px in 0..scale {
        let py = (px / 2).min(scale - 1);
        image.put_pixel(ox + px, oy + py, c);
    }
}
