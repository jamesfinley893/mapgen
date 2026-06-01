use crate::generate::{smoothstep, value_noise};
mod biome_style;
use coast::draw_coastline;
use colors::{apply_snow_overlay_in_place, land_base_colors, soften_biome_edges};
use image::{Rgba, RgbaImage};
use rivers::draw_rivers;
use shading::{build_land_vertices, compute_hillshade, draw_tile_hillshaded, lerp_rgba, offset};

mod coast;
mod colors;
mod rivers;
mod shading;

use crate::{Biome, Surface, World};

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
    let land_vertices = build_land_vertices(world, &hillshade, &land_colors);
    let ocean_vertices = build_ocean_vertices(world);

    draw_base_layer(
        &mut image,
        world,
        scale,
        &land_vertices,
        &ocean_vertices,
        &hillshade,
    );
    draw_land_features(&mut image, world, scale);

    image
}

fn build_hillshade(world: &World) -> Vec<f32> {
    let mut hillshade = (0..world.tiles.len())
        .map(|idx| {
            if is_water_layer(world.tiles[idx].biome) {
                return 0.0;
            }
            let (x, y) = world.coords(idx);
            compute_hillshade(world, x, y)
        })
        .collect::<Vec<_>>();
    smooth_hillshade_in_place(world, &mut hillshade, 1);
    hillshade
}

fn smooth_hillshade_in_place(world: &World, hillshade: &mut Vec<f32>, passes: usize) {
    let mut scratch = vec![0.0_f32; hillshade.len()];

    for _ in 0..passes {
        for idx in 0..world.tiles.len() {
            if is_water_layer(world.tiles[idx].biome) {
                scratch[idx] = 0.0;
                continue;
            }

            let (x, y) = world.coords(idx);
            let mut sum = hillshade[idx] * 5.0;
            let mut weight = 5.0_f32;
            for (nx, ny) in world.neighbors8(x, y) {
                let nidx = world.idx(nx, ny);
                if is_water_layer(world.tiles[nidx].biome) {
                    continue;
                }
                sum += hillshade[nidx];
                weight += 1.0;
            }

            scratch[idx] = sum / weight;
        }

        std::mem::swap(hillshade, &mut scratch);
    }
}

fn is_water_layer(biome: Biome) -> bool {
    matches!(biome, Biome::Ocean | Biome::Freshwater)
}

fn build_land_colors(world: &World, scale: u32) -> Vec<Rgba<u8>> {
    // Pre-compute land base colors, soften biome-boundary edges, then apply snow.
    // Snow must come after softening so partially-snowed tiles don't bleed white
    // into neighboring biomes through the blend pass.
    let land_colors = land_base_colors(world, scale);
    let mut land_colors = soften_biome_edges(world, &land_colors);
    apply_snow_overlay_in_place(world, &mut land_colors);
    land_colors
}

fn draw_base_layer(
    image: &mut RgbaImage,
    world: &World,
    scale: u32,
    land_vertices: &shading::LandVertexGrid,
    ocean_vertices: &OceanVertexGrid,
    hillshade: &[f32],
) {
    for (idx, tile) in world.tiles.iter().enumerate() {
        let (x, y) = world.coords(idx);

        if matches!(tile.biome, Biome::Ocean) {
            draw_ocean_tile(image, world, ocean_vertices, x, y, scale);
        } else if matches!(tile.biome, Biome::Freshwater) {
            draw_freshwater_tile(image, world, idx, x, y, scale);
        } else {
            draw_tile_hillshaded(image, land_vertices, world, hillshade, x as u32, y as u32, scale);
        }
    }
}

fn draw_ocean_tile(
    image: &mut RgbaImage,
    world: &World,
    ocean_vertices: &OceanVertexGrid,
    x: usize,
    y: usize,
    scale: u32,
) {
    let ox = x as u32 * scale;
    let oy = y as u32 * scale;
    let v00 = ocean_vertices.get(x, y);
    let v10 = ocean_vertices.get(x + 1, y);
    let v01 = ocean_vertices.get(x, y + 1);
    let v11 = ocean_vertices.get(x + 1, y + 1);
    let s = scale as f32;

    for py in 0..scale {
        for px in 0..scale {
            let fx = (px as f32 + 0.5) / s;
            let fy = (py as f32 + 0.5) / s;
            let depth = v00.depth * (1.0 - fx) * (1.0 - fy)
                + v10.depth * fx * (1.0 - fy)
                + v01.depth * (1.0 - fx) * fy
                + v11.depth * fx * fy;
            let temperature = v00.temperature * (1.0 - fx) * (1.0 - fy)
                + v10.temperature * fx * (1.0 - fy)
                + v01.temperature * (1.0 - fx) * fy
                + v11.temperature * fx * fy;
            let gx = (ox + px) as usize;
            let gy = (oy + py) as usize;
            image.put_pixel(
                ox + px,
                oy + py,
                ocean_textured_color(world.seed, depth, temperature, gx, gy),
            );
        }
    }
}

fn draw_freshwater_tile(
    image: &mut RgbaImage,
    world: &World,
    idx: usize,
    x: usize,
    y: usize,
    scale: u32,
) {
    let ox = x as u32 * scale;
    let oy = y as u32 * scale;
    let tile = &world.tiles[idx];
    // Teal depth ramp matching the river palette. Tiles touching the shore render
    // shallower as a simple depth cue; open-water tiles render at full depth.
    let shallow = Rgba([86, 144, 156, 255]);
    let deep = Rgba([34, 84, 108, 255]);
    let depth_t = smoothstep(0.05, 0.80, tile.lake_depth.clamp(0.0, 1.0));
    let depth_t = if freshwater_is_shore(world, x, y) {
        depth_t * 0.45
    } else {
        depth_t
    };
    let base = lerp_rgba(shallow, deep, depth_t);

    for py in 0..scale {
        for px in 0..scale {
            let gx = (ox + px) as usize;
            let gy = (oy + py) as usize;
            let ripple = value_noise(world.seed ^ 0x3E7F_91C2, gx, gy, 7) - 0.5;
            image.put_pixel(ox + px, oy + py, offset(base, (ripple * 5.0) as i16));
        }
    }
}

fn freshwater_is_shore(world: &World, x: usize, y: usize) -> bool {
    world.neighbors8(x, y).any(|(nx, ny)| {
        !matches!(world.tiles[world.idx(nx, ny)].biome, Biome::Freshwater)
    })
}

#[derive(Clone, Copy)]
struct OceanVertex {
    depth: f32,
    temperature: f32,
}

struct OceanVertexGrid {
    width: usize,
    vertices: Vec<OceanVertex>,
}

impl OceanVertexGrid {
    fn get(&self, x: usize, y: usize) -> OceanVertex {
        self.vertices[y * self.width + x]
    }
}

fn build_ocean_vertices(world: &World) -> OceanVertexGrid {
    let width = world.width + 1;
    let height = world.height + 1;
    let mut vertices = Vec::with_capacity(width * height);

    for y in 0..height {
        for x in 0..width {
            vertices.push(ocean_vertex_at(world, x as isize, y as isize));
        }
    }

    OceanVertexGrid { width, vertices }
}

fn ocean_vertex_at(world: &World, x: isize, y: isize) -> OceanVertex {
    let mut ocean_depth = 0.0_f32;
    let mut ocean_temperature = 0.0_f32;
    let mut ocean_count = 0.0_f32;
    let mut ambient_temperature = 0.0_f32;
    let mut ambient_count = 0.0_f32;

    for dy in [-1_isize, 0] {
        for dx in [-1_isize, 0] {
            let tx = x + dx;
            let ty = y + dy;
            if !world.in_bounds(tx, ty) {
                continue;
            }
            let tile = &world.tiles[world.idx(tx as usize, ty as usize)];
            ambient_temperature += tile.temperature;
            ambient_count += 1.0;
            if tile.surface == Surface::Ocean || tile.biome == Biome::Ocean {
                ocean_depth += ocean_tile_depth(world, tile.raw_elevation);
                ocean_temperature += tile.temperature;
                ocean_count += 1.0;
            }
        }
    }

    if ambient_count <= f32::EPSILON {
        return OceanVertex {
            depth: 0.0,
            temperature: 0.5,
        };
    }

    let fallback_depth = if ocean_count > 0.0 {
        ocean_depth / ocean_count
    } else {
        0.0
    };
    let fallback_temperature = if ocean_count > 0.0 {
        ocean_temperature / ocean_count
    } else {
        ambient_temperature / ambient_count
    };

    let mut depth = 0.0_f32;
    let mut temperature = 0.0_f32;
    let mut weight = 0.0_f32;
    for dy in [-1_isize, 0] {
        for dx in [-1_isize, 0] {
            let tx = x + dx;
            let ty = y + dy;
            if !world.in_bounds(tx, ty) {
                continue;
            }
            let tile = &world.tiles[world.idx(tx as usize, ty as usize)];
            if tile.surface == Surface::Ocean || tile.biome == Biome::Ocean {
                depth += ocean_tile_depth(world, tile.raw_elevation);
                temperature += tile.temperature;
            } else {
                depth += land_adjacent_shelf_depth(world, fallback_depth, tile.raw_elevation);
                temperature += fallback_temperature;
            }
            weight += 1.0;
        }
    }

    OceanVertex {
        depth: depth / weight,
        temperature: temperature / weight,
    }
}

fn ocean_tile_depth(world: &World, raw_elevation: f32) -> f32 {
    (world.sea_level - raw_elevation).max(0.0)
}

fn land_adjacent_shelf_depth(world: &World, fallback_depth: f32, land_elevation: f32) -> f32 {
    let land_height = (land_elevation - world.sea_level).max(0.0);
    let steep_shore = smoothstep(0.035, 0.22, land_height);

    (0.016 + steep_shore * 0.018 + fallback_depth.min(0.12) * 0.12).clamp(0.012, 0.046)
}

fn ocean_color_for_depth(depth: f32) -> Rgba<u8> {
    let shelf_t = (1.0 - smoothstep(0.0, 0.048, depth)).clamp(0.0, 1.0);
    let deep_t = smoothstep(0.06, 0.26, depth).clamp(0.0, 1.0);
    let shelf_color = Rgba([58, 132, 182, 255]);
    let ocean_color = Rgba([38, 84, 148, 255]);
    let abyss_color = Rgba([18, 46, 102, 255]);
    lerp_rgba(
        lerp_rgba(ocean_color, shelf_color, shelf_t),
        abyss_color,
        deep_t,
    )
}

fn ocean_textured_color(seed: u64, depth: f32, temperature: f32, x: usize, y: usize) -> Rgba<u8> {
    let mut color = ocean_color_for_depth(depth);
    let broad = value_noise(seed ^ 0x4F3C_2A19, x, y, 18) - 0.5;
    let ripple = value_noise(seed ^ 0x8B2F_67D1, x + y / 3, y + x / 5, 6) - 0.5;
    let shallow = 1.0 - smoothstep(0.018, 0.14, depth);
    let deep = smoothstep(0.10, 0.30, depth);
    let texture = broad * 3.6 + ripple * (2.4 + shallow * 2.2);
    color = offset(color, (texture * (1.0 - deep * 0.42)) as i16);

    if shallow > 0.0 {
        let caustic = value_noise(seed ^ 0x2D9A_5EF3, x + y / 2, y + x / 2, 5);
        let glint = smoothstep(0.56, 0.92, caustic) * shallow * 0.20;
        color = lerp_rgba(color, Rgba([126, 190, 202, 255]), shallow * 0.055);
        color = offset(color, (glint * 10.0) as i16);
    }

    let reef = tropical_shelf_strength(seed, depth, temperature, x, y);
    if reef > 0.0 {
        let lagoon = Rgba([84, 178, 188, 255]);
        let sand = Rgba([124, 194, 190, 255]);
        let patch = smoothstep(0.50, 0.92, value_noise(seed ^ 0xA61D_43B9, x, y, 9));
        color = lerp_rgba(color, lerp_rgba(lagoon, sand, patch * 0.34), reef * 0.34);
        color = offset(color, (reef * patch * 8.0) as i16);
    }

    let ice = polar_ice_strength(seed, depth, temperature, x, y);
    if ice > 0.0 {
        let floe = value_noise(seed ^ 0x6B91_E8D4, x + y / 4, y + x / 6, 7);
        let ice_color = Rgba([206, 224, 224, 255]);
        color = lerp_rgba(
            color,
            ice_color,
            ice * (0.26 + smoothstep(0.42, 0.90, floe) * 0.16),
        );
        color = offset(color, (ice * smoothstep(0.68, 0.94, floe) * 8.0) as i16);
    }

    color
}

fn tropical_shelf_strength(seed: u64, depth: f32, temperature: f32, x: usize, y: usize) -> f32 {
    let warm = smoothstep(0.66, 0.86, temperature);
    if warm <= 0.0 {
        return 0.0;
    }

    let shelf = smoothstep(0.006, 0.030, depth) * (1.0 - smoothstep(0.055, 0.145, depth));
    if shelf <= 0.0 {
        return 0.0;
    }

    let broken_reef = smoothstep(0.34, 0.84, value_noise(seed ^ 0x3AC7_29D1, x, y, 15));

    (warm * shelf * (0.52 + broken_reef * 0.48)).clamp(0.0, 1.0)
}

fn polar_ice_strength(seed: u64, depth: f32, temperature: f32, x: usize, y: usize) -> f32 {
    let cold = 1.0 - smoothstep(0.08, 0.24, temperature);
    if cold <= 0.0 {
        return 0.0;
    }

    let shelf = 1.0 - smoothstep(0.05, 0.24, depth);
    if shelf <= 0.0 {
        return 0.0;
    }

    let broken_pack = smoothstep(0.38, 0.82, value_noise(seed ^ 0x31AF_C402, x, y, 13));
    (cold * (0.24 + shelf * 0.76) * (0.50 + broken_pack * 0.50)).clamp(0.0, 1.0)
}

fn draw_land_features(image: &mut RgbaImage, world: &World, scale: u32) {
    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.biome == Biome::Coast {
            draw_coastline(image, world, idx, scale);
        }
    }
    draw_rivers(image, world, scale);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocean_temperature_effects_skip_ineligible_water() {
        assert_eq!(tropical_shelf_strength(7, 0.04, 0.50, 4, 9), 0.0);
        assert_eq!(tropical_shelf_strength(7, 0.20, 0.90, 4, 9), 0.0);
        assert_eq!(polar_ice_strength(7, 0.04, 0.50, 4, 9), 0.0);
        assert_eq!(polar_ice_strength(7, 0.30, 0.02, 4, 9), 0.0);
    }

    #[test]
    fn ocean_temperature_effects_still_apply_when_eligible() {
        assert!(tropical_shelf_strength(7, 0.04, 0.90, 4, 9) > 0.0);
        assert!(polar_ice_strength(7, 0.04, 0.02, 4, 9) > 0.0);
    }
}
