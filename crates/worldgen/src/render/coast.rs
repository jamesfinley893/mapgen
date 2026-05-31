use image::{Rgba, RgbaImage};

use crate::generate::{hash01, smoothstep};
use crate::{Surface, World};

const BEACH_COLOR: Rgba<u8> = Rgba([218, 205, 154, 255]);
const ROCKY_COAST_COLOR: Rgba<u8> = Rgba([154, 150, 132, 255]);
const SURF_COLOR: Rgba<u8> = Rgba([190, 218, 218, 255]);
const SHALLOW_COLOR: Rgba<u8> = Rgba([96, 164, 194, 255]);

pub(super) fn draw_coastline(image: &mut RgbaImage, world: &World, idx: usize, scale: u32) {
    let (x, y) = world.coords(idx);
    for (dx, dy, edge) in [
        (-1, 0, Edge::West),
        (1, 0, Edge::East),
        (0, -1, Edge::North),
        (0, 1, Edge::South),
    ] {
        let nx = x as isize + dx;
        let ny = y as isize + dy;
        if !world.in_bounds(nx, ny) {
            continue;
        }
        let nidx = world.idx(nx as usize, ny as usize);
        let neighbor = &world.tiles[nidx];
        if neighbor.surface == Surface::Ocean {
            draw_coastal_transition(image, world, idx, nidx, x, y, scale, edge);
        }
    }

    for (dx, dy, corner) in [
        (-1, -1, Corner::NorthWest),
        (1, -1, Corner::NorthEast),
        (-1, 1, Corner::SouthWest),
        (1, 1, Corner::SouthEast),
    ] {
        let nx = x as isize + dx;
        let ny = y as isize + dy;
        if !world.in_bounds(nx, ny) {
            continue;
        }
        let nidx = world.idx(nx as usize, ny as usize);
        let neighbor = &world.tiles[nidx];
        if neighbor.surface == Surface::Ocean {
            draw_coastal_corner_transition(image, world, idx, nidx, x, y, scale, corner);
        }
    }
}

fn blend_pixel(image: &mut RgbaImage, x: u32, y: u32, color: Rgba<u8>, alpha: f32) {
    if alpha <= 0.0 || x >= image.width() || y >= image.height() {
        return;
    }
    let base = image.get_pixel(x, y);
    let blend = |a: u8, b: u8| -> u8 {
        (a as f32 * (1.0 - alpha) + b as f32 * alpha).clamp(0.0, 255.0) as u8
    };
    image.put_pixel(
        x,
        y,
        Rgba([
            blend(base[0], color[0]),
            blend(base[1], color[1]),
            blend(base[2], color[2]),
            255,
        ]),
    );
}

#[derive(Clone, Copy)]
enum Edge {
    West,
    East,
    North,
    South,
}

#[derive(Clone, Copy)]
enum Corner {
    NorthWest,
    NorthEast,
    SouthWest,
    SouthEast,
}

fn draw_coastal_transition(
    image: &mut RgbaImage,
    world: &World,
    idx: usize,
    ocean_idx: usize,
    x: usize,
    y: usize,
    scale: u32,
    edge: Edge,
) {
    let ox = x as u32 * scale;
    let oy = y as u32 * scale;
    let rugged = coastal_ruggedness(world, idx);
    let land_width = (scale as f32 * (0.34 - rugged * 0.16)).max(1.0);
    let water_width = (scale / 2).max(1);
    let beach_color = lerp_color(BEACH_COLOR, ROCKY_COAST_COLOR, rugged * 0.78);
    let beach_alpha = 0.34 * (1.0 - rugged * 0.58);
    let surf_alpha = 0.20 + rugged * 0.10;

    for along in 0..scale {
        let intrusion = coastline_intrusion(world, idx, ocean_idx, x, y, scale, edge, along);
        for cross in 0..scale {
            let (px, py, land_distance) = edge_pixel_from_along_cross(along, cross, scale, edge);

            if land_distance <= intrusion && intrusion > 0.0 {
                let t = 1.0 - land_distance / intrusion.max(0.5);
                blend_pixel(image, ox + px, oy + py, SHALLOW_COLOR, 0.44 * t);
                blend_pixel(image, ox + px, oy + py, SURF_COLOR, surf_alpha * t * t);
            }

            let beach_distance = (land_distance - intrusion).abs();
            if beach_distance < land_width {
                let t = 1.0 - beach_distance / land_width;
                blend_pixel(image, ox + px, oy + py, beach_color, beach_alpha * t);
            }
        }
    }

    let (wx, wy) = match edge {
        Edge::West => (ox.saturating_sub(water_width), oy),
        Edge::East => (ox + scale, oy),
        Edge::North => (ox, oy.saturating_sub(water_width)),
        Edge::South => (ox, oy + scale),
    };
    for outer in 0..scale {
        for inner in 0..water_width {
            let (px, py, water_distance) = match edge {
                Edge::West => (wx + inner, wy + outer, water_width - inner),
                Edge::East => (wx + inner, wy + outer, inner + 1),
                Edge::North => (wx + outer, wy + inner, water_width - inner),
                Edge::South => (wx + outer, wy + inner, inner + 1),
            };
            if px >= image.width() || py >= image.height() {
                continue;
            }

            let t = 1.0 - (water_distance.saturating_sub(1) as f32 / water_width as f32);
            blend_pixel(image, px, py, SHALLOW_COLOR, 0.28 * t);
            blend_pixel(image, px, py, SURF_COLOR, 0.18 * t * t);
        }
    }
}

fn edge_pixel_from_along_cross(along: u32, cross: u32, scale: u32, edge: Edge) -> (u32, u32, f32) {
    let distance = cross as f32 + 0.5;
    match edge {
        Edge::West => (cross, along, distance),
        Edge::East => (
            scale.saturating_sub(1).saturating_sub(cross),
            along,
            distance,
        ),
        Edge::North => (along, cross, distance),
        Edge::South => (
            along,
            scale.saturating_sub(1).saturating_sub(cross),
            distance,
        ),
    }
}

fn coastline_intrusion(
    world: &World,
    idx: usize,
    ocean_idx: usize,
    x: usize,
    y: usize,
    scale: u32,
    edge: Edge,
    along: u32,
) -> f32 {
    let coast = &world.tiles[idx];
    let ocean = &world.tiles[ocean_idx];
    let land_height = (coast.raw_elevation - world.sea_level).max(0.0);
    let ocean_depth = (world.sea_level - ocean.raw_elevation).max(0.0);
    let low_land = 1.0 - smoothstep(0.015, 0.095, land_height);
    let shallow_sea = 1.0 - smoothstep(0.035, 0.18, ocean_depth);
    let rugged = coastal_ruggedness(world, idx);
    let s = scale as f32;
    let base = s * (0.06 + low_land * 0.15 + shallow_sea * 0.08) * (1.0 - rugged * 0.45);
    let wobble =
        coastline_wobble(world.seed, x, y, scale, edge, along) * s * 0.22 * (1.0 - rugged * 0.30);

    (base + wobble).clamp(0.0, s * 0.42)
}

fn coastline_wobble(seed: u64, x: usize, y: usize, scale: u32, edge: Edge, along: u32) -> f32 {
    if scale <= 1 {
        return 0.0;
    }

    let edge_id = match edge {
        Edge::West => 0,
        Edge::East => 1,
        Edge::North => 2,
        Edge::South => 3,
    };
    let t = along as f32 / scale.saturating_sub(1) as f32;
    let sx = x * 7 + edge_id * 19;
    let sy = y * 7 + edge_id * 23;
    let a = hash01(seed ^ 0xC0A5_71E5, sx, sy);
    let b = hash01(seed ^ 0xC0A5_71E5, sx + 5, sy + 11);
    let c = hash01(seed ^ 0xC0A5_71E5, sx + 13, sy + 3);
    let v = if t < 0.5 {
        a + (b - a) * smoothstep(0.0, 0.5, t)
    } else {
        b + (c - b) * smoothstep(0.5, 1.0, t)
    };

    v - 0.5
}

fn draw_coastal_corner_transition(
    image: &mut RgbaImage,
    world: &World,
    idx: usize,
    ocean_idx: usize,
    x: usize,
    y: usize,
    scale: u32,
    corner: Corner,
) {
    let ox = x as u32 * scale;
    let oy = y as u32 * scale;
    let rugged = coastal_ruggedness(world, idx);
    let land_radius = (scale as f32 * (0.62 - rugged * 0.20)).max(1.0);
    let water_width = (scale / 2).max(1);
    let water_radius = coastline_corner_intrusion(world, idx, ocean_idx, x, y, scale, corner);
    let beach_color = lerp_color(BEACH_COLOR, ROCKY_COAST_COLOR, rugged * 0.78);
    let beach_alpha = 0.28 * (1.0 - rugged * 0.58);
    let surf_alpha = 0.15 + rugged * 0.08;

    for py in 0..scale {
        for px in 0..scale {
            let (dx, dy) = corner_land_distance(px, py, scale, corner);
            let distance = (dx * dx + dy * dy).sqrt();
            if distance > land_radius {
                continue;
            }

            if distance <= water_radius && water_radius > 0.0 {
                let t = 1.0 - distance / water_radius.max(0.5);
                blend_pixel(image, ox + px, oy + py, SHALLOW_COLOR, 0.50 * t);
                blend_pixel(image, ox + px, oy + py, SURF_COLOR, surf_alpha * t * t);
            }

            let beach_distance = (distance - water_radius).abs();
            if beach_distance < land_radius && distance >= water_radius * 0.70 {
                let t = 1.0 - beach_distance / land_radius;
                blend_pixel(image, ox + px, oy + py, beach_color, beach_alpha * t * t);
            }
        }
    }

    for iy in 0..water_width {
        for ix in 0..water_width {
            let Some((px, py)) = corner_water_pixel(ox, oy, ix, iy, scale, corner) else {
                continue;
            };
            if px >= image.width() || py >= image.height() {
                continue;
            }

            let distance = (((ix + 1) * (ix + 1) + (iy + 1) * (iy + 1)) as f32).sqrt();
            let max_distance = water_width as f32 * std::f32::consts::SQRT_2;
            let t = (1.0 - (distance - 1.0).max(0.0) / max_distance).clamp(0.0, 1.0);
            blend_pixel(image, px, py, SHALLOW_COLOR, 0.20 * t);
            blend_pixel(image, px, py, SURF_COLOR, surf_alpha * t * t);
        }
    }
}

fn coastal_ruggedness(world: &World, idx: usize) -> f32 {
    let tile = &world.tiles[idx];
    smoothstep(0.026, 0.115, tile.relief + tile.slope * 0.75)
}

fn coastline_corner_intrusion(
    world: &World,
    idx: usize,
    ocean_idx: usize,
    x: usize,
    y: usize,
    scale: u32,
    corner: Corner,
) -> f32 {
    let coast = &world.tiles[idx];
    let ocean = &world.tiles[ocean_idx];
    let land_height = (coast.raw_elevation - world.sea_level).max(0.0);
    let ocean_depth = (world.sea_level - ocean.raw_elevation).max(0.0);
    let low_land = 1.0 - smoothstep(0.015, 0.095, land_height);
    let shallow_sea = 1.0 - smoothstep(0.035, 0.18, ocean_depth);
    let rugged = coastal_ruggedness(world, idx);
    let s = scale as f32;
    let base = s * (0.08 + low_land * 0.16 + shallow_sea * 0.09) * (1.0 - rugged * 0.45);
    let wobble =
        coastline_corner_wobble(world.seed, x, y, corner) * s * 0.15 * (1.0 - rugged * 0.30);

    (base + wobble).clamp(0.0, s * 0.46)
}

fn coastline_corner_wobble(seed: u64, x: usize, y: usize, corner: Corner) -> f32 {
    let corner_id = match corner {
        Corner::NorthWest => 0,
        Corner::NorthEast => 1,
        Corner::SouthWest => 2,
        Corner::SouthEast => 3,
    };
    hash01(
        seed ^ 0x4D19_BC7A,
        x * 11 + corner_id * 29,
        y * 11 + corner_id * 31,
    ) - 0.5
}

fn lerp_color(a: Rgba<u8>, b: Rgba<u8>, t: f32) -> Rgba<u8> {
    let t = t.clamp(0.0, 1.0);
    Rgba([
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).clamp(0.0, 255.0) as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).clamp(0.0, 255.0) as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t).clamp(0.0, 255.0) as u8,
        255,
    ])
}

fn corner_land_distance(px: u32, py: u32, scale: u32, corner: Corner) -> (f32, f32) {
    match corner {
        Corner::NorthWest => (px as f32, py as f32),
        Corner::NorthEast => (scale.saturating_sub(1).saturating_sub(px) as f32, py as f32),
        Corner::SouthWest => (px as f32, scale.saturating_sub(1).saturating_sub(py) as f32),
        Corner::SouthEast => (
            scale.saturating_sub(1).saturating_sub(px) as f32,
            scale.saturating_sub(1).saturating_sub(py) as f32,
        ),
    }
}

fn corner_water_pixel(
    ox: u32,
    oy: u32,
    ix: u32,
    iy: u32,
    scale: u32,
    corner: Corner,
) -> Option<(u32, u32)> {
    match corner {
        Corner::NorthWest => Some((ox.checked_sub(ix + 1)?, oy.checked_sub(iy + 1)?)),
        Corner::NorthEast => Some((ox + scale + ix, oy.checked_sub(iy + 1)?)),
        Corner::SouthWest => Some((ox.checked_sub(ix + 1)?, oy + scale + iy)),
        Corner::SouthEast => Some((ox + scale + ix, oy + scale + iy)),
    }
}
