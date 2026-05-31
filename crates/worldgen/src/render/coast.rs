use image::{Rgba, RgbaImage};

use crate::{Surface, World};

const COASTLINE_COLOR: Rgba<u8> = Rgba([218, 210, 158, 255]);

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
        let neighbor = &world.tiles[world.idx(nx as usize, ny as usize)];
        if neighbor.surface == Surface::Ocean {
            draw_coastline_edge(image, x, y, scale, edge);
        }
    }
}

fn blend_pixel(image: &mut RgbaImage, x: u32, y: u32, color: Rgba<u8>, alpha: f32) {
    if x >= image.width() || y >= image.height() {
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

fn draw_coastline_edge(image: &mut RgbaImage, x: usize, y: usize, scale: u32, edge: Edge) {
    let ox = x as u32 * scale;
    let oy = y as u32 * scale;
    let width = (scale / 4).max(1);
    for py in 0..scale {
        for px in 0..scale {
            let near = match edge {
                Edge::West => px < width,
                Edge::East => px >= scale.saturating_sub(width),
                Edge::North => py < width,
                Edge::South => py >= scale.saturating_sub(width),
            };
            if near {
                blend_pixel(image, ox + px, oy + py, COASTLINE_COLOR, 0.72);
            }
        }
    }
}
