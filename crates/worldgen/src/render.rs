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

    // Hillshade computed per tile; bilinear-interpolated at sub-pixel level.
    let hillshade: Vec<f32> = (0..world.tiles.len())
        .map(|idx| {
            let (x, y) = world.coords(idx);
            compute_hillshade(world, x, y)
        })
        .collect();

    // Pre-compute land base colors then soften biome-boundary edges.
    let land_colors = land_base_colors(world);
    let land_colors = soften_biome_edges(world, &land_colors);

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
            let base = lerp_rgba(lerp_rgba(ocean_color, shelf_color, shelf_t), abyss_color, deep_t);
            let tex = ((variation - 0.5) * 6.0) as i16;
            draw_tile(&mut image, x as u32, y as u32, scale, offset(base, tex));
        } else {
            draw_tile_hillshaded(
                &mut image, &hillshade, world, x as u32, y as u32, scale, land_colors[idx],
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
            draw_coastline(&mut image, world, idx, scale);
        }
    }

    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.biome == Biome::Lake {
            draw_lake(&mut image, world, idx, scale);
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

fn land_base_colors(world: &World) -> Vec<Rgba<u8>> {
    (0..world.tiles.len())
        .map(|idx| {
            let tile = &world.tiles[idx];
            let (x, y) = world.coords(idx);
            let mut color = biome_color_climatic(tile.biome, tile.temperature, tile.moisture);
            if !matches!(tile.biome, Biome::Ocean | Biome::Lake) {
                // Alpine elevation gradient: rocky-brown at lower slopes → cool gray at summit.
                // This runs before the snow overlay so the rock shows through the transition zone.
                if matches!(tile.biome, Biome::Alpine) {
                    let height_above_sea = (tile.raw_elevation - world.sea_level).max(0.0);
                    let alpine_t = ((height_above_sea - 0.08) / 0.26).clamp(0.0, 1.0);
                    color = lerp_rgba(Rgba([136, 118, 98, 255]), Rgba([152, 150, 146, 255]), alpine_t);
                }
                let variation = hash01(world.seed, x, y);
                // Medium-scale coherent noise breaks up large uniform biome areas.
                let regional = sample_noise(world.seed.wrapping_add(0xCAFE_BABE), x, y, 14);
                let elev_shade = ((tile.raw_elevation - world.sea_level) * 20.0) as i16;
                let micro = (variation * 12.0) as i16 - 6;
                let macro_v = ((regional - 0.5) * 10.0) as i16;
                color = offset(color, elev_shade + micro + macro_v);
                // Permanent snow line: high enough that only true glaciers/snowfields appear.
                // Polar lowlands are cold-looking via biome color; no seasonal snow overlay.
                let snow_line = (world.sea_level + 0.26 + tile.temperature * 0.20)
                    .min(world.sea_level + 0.46);
                // Cap at 0.75 so rocky texture and hillshading show through even on glaciers.
                let snow = ((tile.raw_elevation - snow_line) / 0.10).clamp(0.0, 0.75);
                if snow > 0.0 {
                    color = lerp_rgba(color, Rgba([240, 244, 248, 255]), snow);
                }
                // Riparian zone: dry biomes adjacent to rivers or lakes get a slight
                // green push representing water-side vegetation.
                if matches!(
                    tile.biome,
                    Biome::Steppe
                        | Biome::TemperateGrassland
                        | Biome::Savanna
                        | Biome::Desert
                        | Biome::PolarDesert
                        | Biome::Tundra
                        | Biome::Foothills
                ) {
                    let near_water = world.neighbors8(x, y).any(|(nx, ny)| {
                        matches!(
                            world.tiles[world.idx(nx, ny)].surface,
                            Surface::River | Surface::Lake
                        )
                    });
                    if near_water {
                        color = Rgba([
                            (color[0] as i16 - 5).clamp(0, 255) as u8,
                            (color[1] as i16 + 8).clamp(0, 255) as u8,
                            (color[2] as i16 - 4).clamp(0, 255) as u8,
                            255,
                        ]);
                    }
                }
            }
            color
        })
        .collect()
}

// One pass of weighted neighbour blending at biome boundaries.
// Tiles deep in a biome are unchanged; tiles at edges blend ≈10–30% with neighbours.
fn soften_biome_edges(world: &World, colors: &[Rgba<u8>]) -> Vec<Rgba<u8>> {
    let mut out = colors.to_vec();
    for idx in 0..world.tiles.len() {
        let my_biome = world.tiles[idx].biome;
        if matches!(my_biome, Biome::Ocean | Biome::Lake) {
            continue;
        }
        let (x, y) = world.coords(idx);
        let mut r = colors[idx][0] as f32 * 10.0;
        let mut g = colors[idx][1] as f32 * 10.0;
        let mut b = colors[idx][2] as f32 * 10.0;
        let mut w = 10.0_f32;
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            let nb = world.tiles[nidx].biome;
            if nb == my_biome || matches!(nb, Biome::Ocean | Biome::Lake) {
                continue;
            }
            r += colors[nidx][0] as f32;
            g += colors[nidx][1] as f32;
            b += colors[nidx][2] as f32;
            w += 1.0;
        }
        out[idx] = Rgba([(r / w) as u8, (g / w) as u8, (b / w) as u8, 255]);
    }
    out
}

fn biome_color_climatic(biome: Biome, temperature: f32, moisture: f32) -> Rgba<u8> {
    let base = biome_color(biome);
    match biome {
        Biome::Steppe | Biome::TemperateGrassland => {
            // Drier steppe = warmer/golden; moister = cooler green
            let dryness = (1.0 - (moisture.clamp(0.15, 0.45) - 0.15) / 0.30).max(0.0);
            let dr = (dryness * 12.0) as i16;
            let dg = (dryness * 2.0) as i16;
            let db = (dryness * 10.0) as i16;
            Rgba([
                (base[0] as i16 + dr).clamp(0, 255) as u8,
                (base[1] as i16 + dg).clamp(0, 255) as u8,
                (base[2] as i16 - db).clamp(0, 255) as u8,
                255,
            ])
        }
        Biome::Desert => {
            // Hot deserts more orange, cooler deserts more grey-brown
            let heat = ((temperature - 0.4).clamp(0.0, 0.45) / 0.45) as f32;
            let hr = (heat * 10.0) as i16;
            Rgba([
                (base[0] as i16 + hr).clamp(0, 255) as u8,
                base[1],
                (base[2] as i16 - hr).clamp(0, 255) as u8,
                255,
            ])
        }
        Biome::Savanna => {
            // Dry savanna more golden; moist savanna slightly greener
            let dry = (1.0 - (moisture.clamp(0.2, 0.4) - 0.2) / 0.2).max(0.0);
            let dr = (dry * 8.0) as i16;
            Rgba([
                (base[0] as i16 + dr).clamp(0, 255) as u8,
                (base[1] as i16 + dr / 2).clamp(0, 255) as u8,
                (base[2] as i16 - dr).clamp(0, 255) as u8,
                255,
            ])
        }
        Biome::BorealForest => {
            // Cold boreal = darker/denser; warmer margins = slightly lighter
            let cold = (1.0 - (temperature.clamp(0.12, 0.32) - 0.12) / 0.20).max(0.0);
            let d = (cold * 7.0) as i16;
            Rgba([
                (base[0] as i16 - d).clamp(0, 255) as u8,
                (base[1] as i16 - d).clamp(0, 255) as u8,
                (base[2] as i16 - d).clamp(0, 255) as u8,
                255,
            ])
        }
        Biome::Tundra => {
            // Wetter tundra = slightly greener; drier = more grey
            let wet = (moisture.clamp(0.3, 0.7) - 0.3) / 0.4;
            let g = (wet * 8.0) as i16;
            Rgba([
                (base[0] as i16 - g / 2).clamp(0, 255) as u8,
                (base[1] as i16 + g).clamp(0, 255) as u8,
                base[2],
                255,
            ])
        }
        _ => base,
    }
}

fn biome_color(biome: Biome) -> Rgba<u8> {
    match biome {
        Biome::Ocean => Rgba([38, 84, 148, 255]),
        Biome::Coast => Rgba([204, 198, 148, 255]),
        Biome::Lake => Rgba([62, 148, 204, 255]),
        Biome::PolarDesert => Rgba([212, 220, 218, 255]),
        Biome::Tundra => Rgba([148, 168, 126, 255]),
        Biome::BorealForest => Rgba([64, 112, 68, 255]),
        Biome::TemperateGrassland => Rgba([158, 184, 90, 255]),
        Biome::TemperateForest => Rgba([80, 138, 76, 255]),
        Biome::Woodland => Rgba([106, 150, 78, 255]),
        Biome::Foothills => Rgba([148, 132, 96, 255]),
        Biome::Steppe => Rgba([176, 168, 96, 255]),
        Biome::Desert => Rgba([218, 196, 126, 255]),
        Biome::Savanna => Rgba([186, 180, 76, 255]),
        Biome::TropicalForest => Rgba([56, 148, 70, 255]),
        Biome::Rainforest => Rgba([34, 112, 52, 255]),
        Biome::Alpine => Rgba([144, 146, 142, 255]),
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

fn draw_coastline(image: &mut RgbaImage, world: &World, idx: usize, scale: u32) {
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
        if !matches!(world.tiles[world.idx(nx as usize, ny as usize)].surface, Surface::Ocean) {
            continue;
        }
        match (dx, dy) {
            (0, -1) => (0..scale).for_each(|px| put_pixel_checked(image, (ox + px) as i32, oy as i32, c)),
            (0, 1) => (0..scale).for_each(|px| put_pixel_checked(image, (ox + px) as i32, (oy + scale - 1) as i32, c)),
            (-1, 0) => (0..scale).for_each(|py| put_pixel_checked(image, ox as i32, (oy + py) as i32, c)),
            (1, 0) => (0..scale).for_each(|py| put_pixel_checked(image, (ox + scale - 1) as i32, (oy + py) as i32, c)),
            _ => {}
        }
    }
}

fn draw_lake(image: &mut RgbaImage, world: &World, idx: usize, scale: u32) {
    let (x, y) = world.coords(idx);
    let ox = (x as u32) * scale;
    let oy = (y as u32) * scale;
    let tile = &world.tiles[idx];
    let depth = tile.water_level.map(|wl| (wl - tile.raw_elevation).max(0.0)).unwrap_or(0.0);
    let deep_t = smoothstep(0.0, 0.065, depth);
    let c = lerp_rgba(Rgba([76, 164, 218, 255]), Rgba([46, 122, 186, 255]), deep_t);
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
            0 | 1 => i32::from(!steep && (lowland || excess >= 1.3)),
            2 => 1,
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
            // Expanded shadow range for more dramatic relief (was 0.46–1.0).
            let color = scale_rgb(base_color, 0.26 + shade * 0.74);
            // Aspect tinting: lit faces warm (+R, -B), shadowed faces cool (-R, +B).
            let tint = ((shade - 0.5) * 16.0) as i16;
            let color = Rgba([
                (color[0] as i16 + tint).clamp(0, 255) as u8,
                color[1],
                (color[2] as i16 - tint).clamp(0, 255) as u8,
                255,
            ]);
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
    let dz_dx = get_elev(xi + 1, yi) - get_elev(xi - 1, yi);
    let dz_dy = get_elev(xi, yi + 1) - get_elev(xi, yi - 1);
    // Adaptive z_scale: mountains get dramatic relief, plains stay gentle.
    let elev = get_elev(xi, yi);
    let height_above_sea = (elev - world.sea_level).max(0.0);
    let z_scale = 4.0 + height_above_sea * 22.0;
    let nx = -dz_dx * z_scale;
    let ny = 1.0_f32;
    let nz = -dz_dy * z_scale;
    let len = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-6);
    // Light from NW at 45° elevation
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

// Bilinearly-interpolated value noise — gives spatially-coherent variation
// within a biome without needing the generation-side noise functions.
fn sample_noise(seed: u64, x: usize, y: usize, cell: usize) -> f32 {
    let cell = cell.max(1);
    let fx = x as f32 / cell as f32;
    let fy = y as f32 / cell as f32;
    let x0 = fx.floor() as usize;
    let y0 = fy.floor() as usize;
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;
    let sx = tx * tx * (3.0 - 2.0 * tx);
    let sy = ty * ty * (3.0 - 2.0 * ty);
    let v00 = hash01(seed, x0, y0);
    let v10 = hash01(seed, x0 + 1, y0);
    let v01 = hash01(seed, x0, y0 + 1);
    let v11 = hash01(seed, x0 + 1, y0 + 1);
    let ix0 = v00 + (v10 - v00) * sx;
    let ix1 = v01 + (v11 - v01) * sx;
    ix0 + (ix1 - ix0) * sy
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
