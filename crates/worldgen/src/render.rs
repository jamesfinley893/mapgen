use image::{Rgba, RgbaImage};

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

    for (idx, tile) in world.tiles.iter().enumerate() {
        let (x, y) = world.coords(idx);
        let mut color = biome_color(tile.biome);
        let variation = hash01(world.seed, x, y);
        let shade = ((tile.raw_elevation - world.sea_level) * 60.0) as i16;
        color = offset(color, shade + ((variation * 10.0) as i16 - 5));

        if matches!(tile.biome, Biome::Ocean) {
            let depth = ((world.sea_level - tile.raw_elevation).max(0.0) * 50.0) as i16;
            color = offset(color, -depth);
        }

        draw_tile(&mut image, x as u32, y as u32, scale, color);

        if matches!(tile.biome, Biome::Alpine) {
            draw_peak(&mut image, x as u32, y as u32, scale);
        } else if matches!(tile.biome, Biome::Foothills) {
            draw_hills(&mut image, x as u32, y as u32, scale);
        } else if matches!(tile.biome, Biome::Desert | Biome::PolarDesert) {
            draw_dunes(&mut image, x as u32, y as u32, scale);
        } else if matches!(tile.biome, Biome::TemperateForest | Biome::BorealForest | Biome::Rainforest | Biome::TropicalForest) {
            draw_forest(&mut image, x as u32, y as u32, scale);
        }

        if tile.biome == Biome::Coast {
            draw_coastline(&mut image, x as u32, y as u32, scale);
        }
    }

    for (idx, tile) in world.tiles.iter().enumerate() {
        let (x, y) = world.coords(idx);
        if tile.biome == Biome::Lake {
            draw_lake(&mut image, x as u32, y as u32, scale);
        } else if tile.surface == crate::Surface::River {
            let flow = tile.contributing_area.max(1.0);
            draw_river(&mut image, world, x as u32, y as u32, scale, flow);
        }
    }

    image
}

fn biome_color(biome: Biome) -> Rgba<u8> {
    match biome {
        Biome::Ocean => Rgba([44, 92, 153, 255]),
        Biome::Coast => Rgba([198, 192, 131, 255]),
        Biome::Lake => Rgba([60, 139, 191, 255]),
        Biome::PolarDesert => Rgba([216, 222, 220, 255]),
        Biome::Tundra => Rgba([163, 180, 138, 255]),
        Biome::BorealForest => Rgba([76, 126, 76, 255]),
        Biome::TemperateGrassland => Rgba([152, 175, 93, 255]),
        Biome::TemperateForest => Rgba([89, 140, 83, 255]),
        Biome::Woodland => Rgba([117, 153, 88, 255]),
        Biome::Foothills => Rgba([138, 151, 116, 255]),
        Biome::Steppe => Rgba([172, 169, 101, 255]),
        Biome::Desert => Rgba([214, 195, 132, 255]),
        Biome::Savanna => Rgba([171, 174, 85, 255]),
        Biome::TropicalForest => Rgba([65, 147, 78, 255]),
        Biome::Rainforest => Rgba([42, 122, 58, 255]),
        Biome::Alpine => Rgba([147, 149, 145, 255]),
    }
}

fn draw_tile(image: &mut RgbaImage, x: u32, y: u32, scale: u32, color: Rgba<u8>) {
    let ox = x * scale;
    let oy = y * scale;
    for py in 0..scale {
        for px in 0..scale {
            image.put_pixel(ox + px, oy + py, color);
        }
    }
}

fn draw_peak(image: &mut RgbaImage, x: u32, y: u32, scale: u32) {
    let ox = x * scale;
    let oy = y * scale;
    let white = Rgba([230, 231, 228, 255]);
    let dark = Rgba([118, 120, 119, 255]);
    for i in 0..scale {
        image.put_pixel(ox + i, oy + i.min(scale - 1), white);
        if i > 0 {
            image.put_pixel(ox + scale - i, oy + i - 1, dark);
        }
    }
}

fn draw_hills(image: &mut RgbaImage, x: u32, y: u32, scale: u32) {
    let ox = x * scale;
    let oy = y * scale;
    let dark = Rgba([102, 111, 84, 255]);
    if scale > 1 {
        for px in 0..scale {
            let py = if px < scale / 2 { scale / 2 } else { scale / 2 + px.saturating_sub(scale / 2) / 2 };
            image.put_pixel(ox + px, oy + py.min(scale - 1), dark);
        }
    } else {
        image.put_pixel(ox, oy, dark);
    }
}

fn draw_forest(image: &mut RgbaImage, x: u32, y: u32, scale: u32) {
    let ox = x * scale;
    let oy = y * scale;
    let c = Rgba([30, 74, 34, 255]);
    image.put_pixel(ox + scale / 2, oy + scale / 3, c);
    if scale > 2 {
        image.put_pixel(ox + scale / 3, oy + (scale * 2 / 3), c);
        image.put_pixel(ox + (scale * 2 / 3), oy + (scale * 2 / 3), c);
    }
}

fn draw_dunes(image: &mut RgbaImage, x: u32, y: u32, scale: u32) {
    let ox = x * scale;
    let oy = y * scale;
    let c = Rgba([185, 160, 97, 255]);
    for px in 0..scale {
        let py = (px / 2).min(scale - 1);
        image.put_pixel(ox + px, oy + py, c);
    }
}

fn draw_coastline(image: &mut RgbaImage, x: u32, y: u32, scale: u32) {
    let ox = x * scale;
    let oy = y * scale;
    let c = Rgba([233, 225, 166, 255]);
    for px in 0..scale {
        image.put_pixel(ox + px, oy, c);
    }
}

fn draw_lake(image: &mut RgbaImage, x: u32, y: u32, scale: u32) {
    let ox = x * scale;
    let oy = y * scale;
    let c = Rgba([80, 176, 220, 255]);
    for py in 1..scale.saturating_sub(1) {
        for px in 1..scale.saturating_sub(1) {
            image.put_pixel(ox + px, oy + py, c);
        }
    }
}

fn draw_river(image: &mut RgbaImage, world: &World, x: u32, y: u32, scale: u32, flow: f32) {
    let ox = x * scale;
    let oy = y * scale;
    let base = ((world.width * world.height) as f32 * 0.00075).max(12.0);
    let c = if flow > base * 18.0 {
        Rgba([49, 132, 201, 255])
    } else if flow > base * 6.5 {
        Rgba([66, 160, 219, 255])
    } else {
        Rgba([95, 185, 235, 255])
    };
    let mid = scale / 2;
    for py in 0..scale {
        image.put_pixel(ox + mid, oy + py, c);
        if flow > base * 6.5 && scale > 2 && mid > 0 {
            image.put_pixel(ox + mid - 1, oy + py, c);
        }
        if flow > base * 18.0 && scale > 3 && mid + 1 < scale {
            image.put_pixel(ox + mid + 1, oy + py, c);
        }
    }
}

fn offset(color: Rgba<u8>, delta: i16) -> Rgba<u8> {
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

fn hash01(seed: u64, x: usize, y: usize) -> f32 {
    let mut z = seed
        .wrapping_add((x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add((y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F));
    z ^= z >> 30;
    z = z.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z ^= z >> 27;
    z = z.wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z as f64 / u64::MAX as f64) as f32
}
