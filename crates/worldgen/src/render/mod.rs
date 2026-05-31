use crate::generate::{hash01, smoothstep};
use coast::draw_coastline;
use colors::{apply_snow_overlay, land_base_colors, soften_biome_edges};
use image::{Rgba, RgbaImage};
use shading::{compute_hillshade, draw_tile, draw_tile_hillshaded, lerp_rgba, offset};

mod coast;
mod colors;
mod shading;

use crate::{Biome, World};

#[derive(Debug, Clone, Copy)]
pub struct RenderConfig {
    pub scale: u32,
}

pub fn render_world(world: &World, config: RenderConfig) -> RgbaImage {
    let scale = config.scale.max(1);
    let width = world.width as u32 * scale;
    let height = world.height as u32 * scale;
    let mut image = RgbaImage::new(width, height);
    let hillshade = build_hillshade(world);
    let land_colors = build_land_colors(world, scale);

    draw_base_layer(&mut image, world, scale, &hillshade, &land_colors);
    draw_land_features(&mut image, world, scale);

    image
}

fn build_hillshade(world: &World) -> Vec<f32> {
    let raw = (0..world.tiles.len())
        .map(|idx| {
            let (x, y) = world.coords(idx);
            compute_hillshade(world, x, y)
        })
        .collect::<Vec<_>>();
    let softened = soften_hillshade(world, &raw);
    soften_hillshade(world, &softened)
}

fn soften_hillshade(world: &World, hillshade: &[f32]) -> Vec<f32> {
    let mut out = hillshade.to_vec();

    for idx in 0..world.tiles.len() {
        if world.tiles[idx].biome == Biome::Ocean {
            continue;
        }

        let (x, y) = world.coords(idx);
        let mut sum = hillshade[idx] * 5.0;
        let mut weight = 5.0_f32;
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            if world.tiles[nidx].biome == Biome::Ocean {
                continue;
            }
            sum += hillshade[nidx];
            weight += 1.0;
        }

        out[idx] = sum / weight;
    }

    out
}

fn build_land_colors(world: &World, scale: u32) -> Vec<Rgba<u8>> {
    // Pre-compute land base colors, soften biome-boundary edges, then apply snow.
    // Snow must come after softening so partially-snowed tiles don't bleed white
    // into neighboring biomes through the blend pass.
    let land_colors = land_base_colors(world, scale);
    let land_colors = soften_biome_edges(world, &land_colors);
    apply_snow_overlay(world, &land_colors)
}

fn draw_base_layer(
    image: &mut RgbaImage,
    world: &World,
    scale: u32,
    hillshade: &[f32],
    land_colors: &[Rgba<u8>],
) {
    for (idx, tile) in world.tiles.iter().enumerate() {
        let (x, y) = world.coords(idx);

        if matches!(tile.biome, Biome::Ocean) {
            draw_ocean_tile(image, world, idx, x, y, scale);
        } else {
            draw_tile_hillshaded(
                image,
                hillshade,
                world,
                x as u32,
                y as u32,
                scale,
                land_colors[idx],
            );
        }
    }
}

fn draw_ocean_tile(
    image: &mut RgbaImage,
    world: &World,
    idx: usize,
    x: usize,
    y: usize,
    scale: u32,
) {
    let tile = &world.tiles[idx];
    let variation = hash01(world.seed, x, y);
    let depth = (world.sea_level - tile.raw_elevation).max(0.0);
    let shelf_t = (1.0 - smoothstep(0.0, 0.048, depth)).clamp(0.0, 1.0);
    let deep_t = smoothstep(0.06, 0.26, depth).clamp(0.0, 1.0);
    let shelf_color = Rgba([58, 132, 182, 255]);
    let ocean_color = Rgba([38, 84, 148, 255]);
    let abyss_color = Rgba([18, 46, 102, 255]);
    let base = lerp_rgba(
        lerp_rgba(ocean_color, shelf_color, shelf_t),
        abyss_color,
        deep_t,
    );
    let tex = ((variation - 0.5) * 6.0) as i16;
    draw_tile(image, x as u32, y as u32, scale, offset(base, tex));
}

fn draw_land_features(image: &mut RgbaImage, world: &World, scale: u32) {
    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.biome != Biome::Ocean {
            draw_coast_feature(image, world, idx, scale);
        }
    }
}

fn draw_coast_feature(image: &mut RgbaImage, world: &World, idx: usize, scale: u32) {
    if world.tiles[idx].biome == Biome::Coast {
        draw_coastline(image, world, idx, scale);
    }
}
