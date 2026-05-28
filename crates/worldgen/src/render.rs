use image::{Rgba, RgbaImage};

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

    // Pre-compute hillshade for every tile (including ocean, for use as bilinear corners).
    let hillshade: Vec<f32> = (0..world.tiles.len())
        .map(|idx| {
            let (x, y) = world.coords(idx);
            compute_hillshade(world, x, y)
        })
        .collect();

    for (idx, tile) in world.tiles.iter().enumerate() {
        let (x, y) = world.coords(idx);
        let mut color = biome_color(tile.biome);
        let variation = hash01(world.seed, x, y);

        if matches!(tile.biome, Biome::Ocean) {
            let depth = ((world.sea_level - tile.raw_elevation).max(0.0) * 50.0) as i16;
            color = offset(color, -depth);
            let near_coast = world
                .neighbors8(x, y)
                .any(|(nx, ny)| !matches!(world.tiles[world.idx(nx, ny)].biome, Biome::Ocean));
            if near_coast {
                color = offset(color, 34);
            }
            draw_tile(&mut image, x as u32, y as u32, scale, color);
        } else {
            let elev_shade = ((tile.raw_elevation - world.sea_level) * 22.0) as i16;
            color = offset(color, elev_shade + ((variation * 8.0) as i16 - 4));
            // Snow: fades in above a temperature-dependent snow line
            let snow_line =
                (world.sea_level + 0.28 + tile.temperature * 0.18).min(world.sea_level + 0.46);
            let snow = ((tile.raw_elevation - snow_line) / 0.10).clamp(0.0, 1.0);
            if snow > 0.0 {
                color = lerp_rgba(color, Rgba([240, 244, 246, 255]), snow);
            }
            draw_tile_hillshaded(
                &mut image, &hillshade, world, x as u32, y as u32, scale, color,
            );
        }

        if matches!(tile.biome, Biome::Alpine) {
            draw_peak(&mut image, x as u32, y as u32, scale);
        } else if matches!(tile.biome, Biome::Foothills) {
            draw_hills(&mut image, x as u32, y as u32, scale);
        } else if matches!(tile.biome, Biome::Desert | Biome::PolarDesert) {
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
            draw_coastline(&mut image, x as u32, y as u32, scale);
        }
    }

    for (idx, tile) in world.tiles.iter().enumerate() {
        let (x, y) = world.coords(idx);
        if tile.biome == Biome::Lake {
            draw_lake(&mut image, x as u32, y as u32, scale);
        }
    }

    let thresholds = river_thresholds(world);
    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface == Surface::River {
            let flow = tile.discharge.max(1.0);
            draw_river(&mut image, world, idx, scale, flow, thresholds);
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

fn draw_hills(image: &mut RgbaImage, x: u32, y: u32, scale: u32) {
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
                if d >= 0.76 && d <= 1.30 {
                    put_pixel_checked(image, ox as i32 + px as i32, oy as i32 + py as i32, outline);
                }
            }
        }
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

fn draw_river(
    image: &mut RgbaImage,
    world: &World,
    idx: usize,
    scale: u32,
    flow: f32,
    thresholds: RiverThresholds,
) {
    let (x, y) = world.coords(idx);
    let color = river_color(flow, thresholds);
    let radius = river_radius_px(world, idx, flow, scale, thresholds);
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
struct RiverThresholds {
    secondary: f32,
    trunk: f32,
}

fn river_thresholds(world: &World) -> RiverThresholds {
    let mut discharge: Vec<_> = world
        .tiles
        .iter()
        .filter_map(|tile| (tile.surface == Surface::River).then_some(tile.discharge))
        .collect();
    discharge.sort_by(|a, b| a.total_cmp(b));
    if discharge.is_empty() {
        return RiverThresholds {
            secondary: f32::INFINITY,
            trunk: f32::INFINITY,
        };
    }
    RiverThresholds {
        secondary: discharge[discharge.len() * 58 / 100],
        trunk: discharge[discharge.len() * 84 / 100],
    }
}

fn river_color(flow: f32, thresholds: RiverThresholds) -> Rgba<u8> {
    if flow > thresholds.trunk {
        Rgba([49, 132, 201, 255])
    } else if flow > thresholds.secondary {
        Rgba([66, 160, 219, 255])
    } else {
        Rgba([95, 185, 235, 255])
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
            0 | 1 => i32::from(!steep && (lowland || excess >= 1.45)),
            2 => i32::from(!steep || excess >= 1.8),
            3 | 4 => i32::from(!steep && (lowland || excess >= 2.2)) + 1,
            _ => {
                let width = scale as f32
                    * (0.66 + (excess.clamp(1.0, 4.0) - 1.0) * 0.10 + f32::from(lowland) * 0.08
                        - f32::from(steep) * 0.12);
                ((width.round() as i32) / 2).clamp(1, (scale as i32).max(1))
            }
        }
    } else if tile.channel_order >= 2 || flow >= thresholds.secondary {
        i32::from(scale >= 3 && !steep)
    } else {
        0
    }
}

fn tile_center_px(x: usize, y: usize, scale: u32) -> (i32, i32) {
    (
        (x as u32 * scale + scale / 2) as i32,
        (y as u32 * scale + scale / 2) as i32,
    )
}

fn draw_thick_line(
    image: &mut RgbaImage,
    start: (i32, i32),
    end: (i32, i32),
    radius: i32,
    color: Rgba<u8>,
) {
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let steps = dx.abs().max(dy.abs()).max(1);

    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let x = (start.0 as f32 + dx as f32 * t).round() as i32;
        let y = (start.1 as f32 + dy as f32 * t).round() as i32;
        draw_disc(image, (x, y), radius, color);
    }
}

fn draw_disc(image: &mut RgbaImage, center: (i32, i32), radius: i32, color: Rgba<u8>) {
    let radius = radius.max(0);
    let radius_sq = radius * radius;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy <= radius_sq {
                put_pixel_checked(image, center.0 + dx, center.1 + dy, color);
            }
        }
    }
}

fn put_pixel_checked(image: &mut RgbaImage, x: i32, y: i32, color: Rgba<u8>) {
    if x >= 0 && y >= 0 && (x as u32) < image.width() && (y as u32) < image.height() {
        image.put_pixel(x as u32, y as u32, color);
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

fn draw_tile_hillshaded(
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
    let get_hs = |cx: usize, cy: usize| -> f32 {
        let cx = cx.min(world.width.saturating_sub(1));
        let cy = cy.min(world.height.saturating_sub(1));
        hillshade[world.idx(cx, cy)]
    };
    let h00 = get_hs(tx, ty);
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
            let color = scale_rgb(base_color, 0.38 + shade * 0.62);
            image.put_pixel(ox + px, oy + py, color);
        }
    }
}

fn compute_hillshade(world: &World, x: usize, y: usize) -> f32 {
    let get_elev = |xi: isize, yi: isize| -> f32 {
        let cx = xi.clamp(0, world.width as isize - 1) as usize;
        let cy = yi.clamp(0, world.height as isize - 1) as usize;
        world.tiles[world.idx(cx, cy)].raw_elevation
    };
    let xi = x as isize;
    let yi = y as isize;
    // Central-difference gradient in tile space
    let dz_dx = get_elev(xi + 1, yi) - get_elev(xi - 1, yi);
    let dz_dy = get_elev(xi, yi + 1) - get_elev(xi, yi - 1);
    // Surface normal in (east, up, south) space; z_scale controls perceived steepness
    let z_scale = 6.0_f32;
    let nx = -dz_dx * z_scale;
    let ny = 1.0_f32;
    let nz = -dz_dy * z_scale;
    let len = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-6);
    // Light from NW at 45° elevation: normalize(-1, 1, -1)
    let inv_sqrt3 = 1.0_f32 / 3.0_f32.sqrt();
    ((nx * (-inv_sqrt3) + ny * inv_sqrt3 + nz * (-inv_sqrt3)) / len).clamp(0.0, 1.0)
}

fn scale_rgb(color: Rgba<u8>, factor: f32) -> Rgba<u8> {
    Rgba([
        ((color[0] as f32 * factor) as u8).min(255),
        ((color[1] as f32 * factor) as u8).min(255),
        ((color[2] as f32 * factor) as u8).min(255),
        color[3],
    ])
}

fn lerp_rgba(a: Rgba<u8>, b: Rgba<u8>, t: f32) -> Rgba<u8> {
    Rgba([
        ((a[0] as f32 + (b[0] as f32 - a[0] as f32) * t) as u8).min(255),
        ((a[1] as f32 + (b[1] as f32 - a[1] as f32) * t) as u8).min(255),
        ((a[2] as f32 + (b[2] as f32 - a[2] as f32) * t) as u8).min(255),
        255,
    ])
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
