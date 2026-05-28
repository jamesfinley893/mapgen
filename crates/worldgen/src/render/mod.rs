use crate::generate::{hash01, smoothstep};
use crate::river::river_band_thresholds;
use colors::{apply_snow_overlay, land_base_colors, soften_biome_edges};
use image::{Rgba, RgbaImage};
use shading::{compute_hillshade, draw_tile, draw_tile_hillshaded, lerp_rgba, offset};
use symbols::{draw_dunes, draw_forest, draw_hills, draw_peak, draw_ridge};
use water::{draw_coastline, draw_lake, draw_river};

mod colors;
mod shading;
mod symbols;
mod water;

use crate::{Biome, MountainFeature, Surface, World, mountain_feature_for_tile};

#[derive(Debug, Clone, Copy)]
pub struct RenderConfig {
    pub scale: u32,
}

pub fn render_world(world: &World, config: RenderConfig) -> RgbaImage {
    let scale = config.scale.max(1);
    let width = world.width as u32 * scale;
    let height = world.height as u32 * scale;
    let mut image = RgbaImage::new(width, height);

    // Hillshade computed per tile; bilinear-interpolated at sub-pixel level.
    let hillshade: Vec<f32> = (0..world.tiles.len())
        .map(|idx| {
            let (x, y) = world.coords(idx);
            compute_hillshade(world, x, y)
        })
        .collect();

    // Pre-compute land base colors, soften biome-boundary edges, then apply snow.
    // Snow must come after softening so partially-snowed tiles don't bleed white
    // into neighboring biomes through the blend pass.
    let land_colors = land_base_colors(world, scale);
    let land_colors = soften_biome_edges(world, &land_colors);
    let land_colors = apply_snow_overlay(world, &land_colors);

    for (idx, tile) in world.tiles.iter().enumerate() {
        let (x, y) = world.coords(idx);
        let variation = hash01(world.seed, x, y);

        if matches!(tile.biome, Biome::Ocean) {
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
            draw_tile(&mut image, x as u32, y as u32, scale, offset(base, tex));
        } else {
            draw_tile_hillshaded(
                &mut image,
                &hillshade,
                world,
                x as u32,
                y as u32,
                scale,
                land_colors[idx],
            );
        }

        match mountain_feature_for_tile(world, idx) {
            MountainFeature::Summit => draw_peak(&mut image, x as u32, y as u32, scale),
            MountainFeature::Ridge => draw_ridge(&mut image, world, idx, scale),
            MountainFeature::AlpineSlope => {}
            MountainFeature::Foothill => draw_hills(&mut image, x as u32, y as u32, scale),
            MountainFeature::None => {}
        }

        if matches!(tile.biome, Biome::Desert | Biome::PolarDesert) {
            draw_dunes(&mut image, x as u32, y as u32, scale);
        } else if matches!(
            tile.biome,
            Biome::TemperateForest
                | Biome::BorealForest
                | Biome::Rainforest
                | Biome::TropicalForest
        ) {
            draw_forest(&mut image, x as u32, y as u32, scale);
        }

        if tile.biome == Biome::Coast {
            draw_coastline(&mut image, world, idx, scale);
        }
    }

    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.biome == Biome::Lake {
            draw_lake(&mut image, world, idx, scale);
        }
    }

    let thresholds = river_band_thresholds(world);
    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface == Surface::River {
            let flow = tile.discharge.max(1.0);
            draw_river(&mut image, world, idx, scale, flow, thresholds);
        }
    }

    image
}
