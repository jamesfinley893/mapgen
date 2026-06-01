use image::{Rgba, RgbaImage};

use crate::generate::smoothstep;
use crate::{Biome, Landform, MountainFeature, Surface, Tile, World};

mod biome_style;

use biome_style::coastal_hinterland_biome;

pub fn render_world(world: &World) -> RgbaImage {
    let mut image = RgbaImage::new(world.width as u32, world.height as u32);

    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            image.put_pixel(x as u32, y as u32, tile_color(world, idx));
        }
    }

    image
}

fn tile_color(world: &World, idx: usize) -> Rgba<u8> {
    let tile = &world.tiles[idx];

    if tile.surface == Surface::Ocean || tile.biome == Biome::Ocean {
        return ocean_color(world, tile);
    }

    let base = if tile.biome == Biome::Freshwater || tile.lake_depth > 0.0 {
        freshwater_color(tile)
    } else {
        land_color(world, tile)
    };

    if is_channel(tile) {
        blend(base, channel_color(tile), channel_alpha(tile))
    } else {
        base
    }
}

fn is_channel(tile: &Tile) -> bool {
    tile.surface != Surface::Ocean
        && tile.lake_depth <= 0.0
        && (tile.river_order > 0 || tile.river > 0.18 || tile.spill_discharge > 0.04)
}

fn ocean_color(world: &World, tile: &Tile) -> Rgba<u8> {
    let depth = (world.sea_level - tile.raw_elevation).max(0.0);
    let shelf_t = (1.0 - smoothstep(0.0, 0.048, depth)).clamp(0.0, 1.0);
    let deep_t = smoothstep(0.06, 0.26, depth).clamp(0.0, 1.0);
    let shelf_color = Rgba([58, 132, 182, 255]);
    let ocean_color = Rgba([38, 84, 148, 255]);
    let abyss_color = Rgba([18, 46, 102, 255]);
    let mut color = lerp_rgba(
        lerp_rgba(ocean_color, shelf_color, shelf_t),
        abyss_color,
        deep_t,
    );

    let warm = smoothstep(0.66, 0.86, tile.temperature);
    let shallow = smoothstep(0.006, 0.030, depth) * (1.0 - smoothstep(0.055, 0.145, depth));
    if warm > 0.0 && shallow > 0.0 {
        color = lerp_rgba(color, Rgba([84, 178, 188, 255]), warm * shallow * 0.36);
    }

    let cold = 1.0 - smoothstep(0.08, 0.24, tile.temperature);
    let ice_shelf = 1.0 - smoothstep(0.05, 0.24, depth);
    if cold > 0.0 && ice_shelf > 0.0 {
        color = lerp_rgba(color, Rgba([206, 224, 224, 255]), cold * ice_shelf * 0.38);
    }

    let shore = tile.shore_influence.clamp(0.0, 1.0);
    if shore > 0.0 {
        color = lerp_rgba(color, Rgba([78, 156, 190, 255]), shore * 0.26);
    }

    color
}

fn freshwater_color(tile: &Tile) -> Rgba<u8> {
    let shallow = Rgba([86, 144, 156, 255]);
    let deep = Rgba([34, 84, 108, 255]);
    let depth_t = smoothstep(0.05, 0.80, tile.lake_depth.clamp(0.0, 1.0));
    let mut color = lerp_rgba(shallow, deep, depth_t);
    let shore = tile.shore_influence.clamp(0.0, 1.0);
    if shore > 0.0 {
        color = lerp_rgba(color, Rgba([96, 166, 170, 255]), shore * 0.30);
    }
    color
}

fn land_color(world: &World, tile: &Tile) -> Rgba<u8> {
    let height_above_sea = (tile.raw_elevation - world.sea_level).max(0.0);
    let rugged = (tile.relief + tile.slope * 0.75).clamp(0.0, 1.0);
    let shade = if tile.elevation_shade.is_finite() {
        tile.elevation_shade.clamp(0.0, 1.0)
    } else {
        0.5
    };

    let mut color = if tile.biome == Biome::Alpine {
        alpine_color(height_above_sea, rugged)
    } else if tile.surface == Surface::Coast || tile.biome == Biome::Coast {
        coast_color(tile)
    } else {
        let visual_biome = visual_land_biome(tile);
        let mut color = biome_color_climatic(visual_biome, tile.temperature, tile.moisture);
        let tint_strength = smoothstep(0.04, 0.30, height_above_sea) * 0.28;
        if tint_strength > 0.0 {
            color = lerp_rgba(color, elevation_tint(height_above_sea), tint_strength);
        }
        color
    };

    color = apply_landform_style(
        color,
        tile.landform,
        height_above_sea,
        rugged,
        tile.moisture,
    );
    color = apply_ecotone_tone(color, tile);
    color = offset(color, (height_above_sea * 22.0).min(18.0) as i16);
    color = scale_rgb(color, shade_factor(tile.biome, shade));
    color = apply_mountain_feature(color, tile.mountain_feature, shade, rugged);
    color = apply_terrain_texture(color, tile, rugged);
    color = apply_shore_style(color, tile);

    let snow = tile_snow_cover(world, tile);
    if snow > 0.0 {
        color = lerp_rgba(color, Rgba([240, 244, 248, 255]), snow);
    }

    // A wet simulated tile should read wetter even without cross-pixel overlays.
    if tile.moisture > 0.55 && !matches!(tile.biome, Biome::Alpine | Biome::Foothills) {
        let wet = smoothstep(0.55, 0.92, tile.moisture) * 0.10;
        color = lerp_rgba(color, Rgba([74, 132, 82, 255]), wet);
    }

    color
}

fn apply_landform_style(
    color: Rgba<u8>,
    landform: Landform,
    height_above_sea: f32,
    rugged: f32,
    moisture: f32,
) -> Rgba<u8> {
    match landform {
        Landform::Water | Landform::Plain => color,
        Landform::Shore => lerp_rgba(color, Rgba([220, 205, 142, 255]), 0.18),
        Landform::Hill => {
            let strength = (0.08 + rugged * 0.12 + height_above_sea * 0.08).clamp(0.08, 0.22);
            lerp_rgba(color, Rgba([168, 152, 102, 255]), strength)
        }
        Landform::Valley => {
            let strength = (0.11 + moisture * 0.08).clamp(0.11, 0.20);
            offset(lerp_rgba(color, Rgba([70, 128, 88, 255]), strength), -3)
        }
        Landform::Ridge => {
            let strength = (0.16 + rugged * 0.10).clamp(0.16, 0.28);
            offset(lerp_rgba(color, Rgba([150, 142, 122, 255]), strength), 5)
        }
        Landform::Peak => {
            let strength = (0.24 + rugged * 0.14).clamp(0.24, 0.38);
            offset(lerp_rgba(color, Rgba([190, 190, 182, 255]), strength), 8)
        }
        Landform::Basin => offset(lerp_rgba(color, Rgba([158, 146, 88, 255]), 0.14), -4),
    }
}

fn apply_ecotone_tone(color: Rgba<u8>, tile: &Tile) -> Rgba<u8> {
    let ecotone = tile.ecotone_strength.clamp(0.0, 1.0);
    if ecotone <= 0.0 {
        return color;
    }

    let tint = ecotone_tint(tile.temperature, tile.moisture);
    let toned = lerp_rgba(color, tint, ecotone * 0.12);
    offset(toned, (ecotone * 4.0) as i16)
}

fn ecotone_tint(temperature: f32, moisture: f32) -> Rgba<u8> {
    let warm = smoothstep(0.46, 0.82, temperature);
    let wet = smoothstep(0.42, 0.78, moisture);
    let dry = 1.0 - smoothstep(0.18, 0.48, moisture);
    let green = lerp_rgba(Rgba([150, 154, 96, 255]), Rgba([70, 136, 82, 255]), wet);
    let warm_dry = lerp_rgba(green, Rgba([194, 174, 92, 255]), warm * dry * 0.72);
    lerp_rgba(
        warm_dry,
        Rgba([126, 152, 112, 255]),
        (1.0 - warm) * wet * 0.35,
    )
}

fn apply_terrain_texture(color: Rgba<u8>, tile: &Tile, rugged: f32) -> Rgba<u8> {
    let texture = tile.terrain_texture.clamp(0.0, 1.0) - 0.5;
    if texture.abs() <= 0.001 {
        return color;
    }

    let landform_gain = match tile.landform {
        Landform::Peak | Landform::Ridge => 0.16,
        Landform::Hill | Landform::Valley => 0.13,
        Landform::Basin | Landform::Shore => 0.10,
        Landform::Plain => 0.08,
        Landform::Water => 0.0,
    };
    let factor = 1.0 + texture * (0.14 + rugged * 0.10 + landform_gain);
    scale_rgb(color, factor.clamp(0.78, 1.24))
}

fn apply_shore_style(color: Rgba<u8>, tile: &Tile) -> Rgba<u8> {
    let shore = tile.shore_influence.clamp(0.0, 1.0);
    if shore <= 0.0 {
        return color;
    }

    let channel = smoothstep(0.035, 0.72, tile.river.max(tile.spill_discharge * 0.85));
    let tint = if channel > 0.05 && tile.surface == Surface::Land {
        Rgba([76, 130, 96, 255])
    } else {
        Rgba([220, 204, 138, 255])
    };
    let strength = if channel > 0.05 {
        shore * (0.08 + channel * 0.10)
    } else {
        shore * 0.18
    };
    lerp_rgba(color, tint, strength.clamp(0.0, 0.24))
}

fn visual_land_biome(tile: &Tile) -> Biome {
    if tile.biome == Biome::Coast {
        coastal_hinterland_biome(tile.temperature, tile.moisture)
    } else {
        tile.biome
    }
}

fn alpine_color(height_above_sea: f32, rugged: f32) -> Rgba<u8> {
    let alpine_t = smoothstep(0.24, 0.70, height_above_sea);
    lerp_rgba(
        lerp_rgba(Rgba([82, 84, 84, 255]), Rgba([108, 106, 100, 255]), rugged),
        lerp_rgba(
            Rgba([166, 166, 160, 255]),
            Rgba([186, 186, 180, 255]),
            rugged,
        ),
        (alpine_t * 0.86 + rugged * 0.14).clamp(0.0, 1.0),
    )
}

fn coast_color(tile: &Tile) -> Rgba<u8> {
    let hinterland = biome_color_climatic(
        coastal_hinterland_biome(tile.temperature, tile.moisture),
        tile.temperature,
        tile.moisture,
    );
    let rugged = smoothstep(0.026, 0.115, tile.relief + tile.slope * 0.75);
    let shore = lerp_rgba(
        Rgba([218, 205, 154, 255]),
        Rgba([154, 150, 132, 255]),
        rugged,
    );
    lerp_rgba(shore, hinterland, 0.34 + rugged * 0.22)
}

fn shade_factor(biome: Biome, shade: f32) -> f32 {
    match biome {
        Biome::Alpine => 0.56 + shade * 0.70,
        Biome::Foothills => 0.58 + shade * 0.66,
        _ => 0.66 + shade * 0.56,
    }
}

fn apply_mountain_feature(
    color: Rgba<u8>,
    feature: MountainFeature,
    shade: f32,
    rugged: f32,
) -> Rgba<u8> {
    match feature {
        MountainFeature::Summit => offset(color, (14.0 + shade * 12.0) as i16),
        MountainFeature::Ridge => offset(color, (6.0 + rugged * 10.0 + shade * 5.0) as i16),
        MountainFeature::AlpineSlope => offset(color, (rugged * 5.0) as i16),
        MountainFeature::Foothill => offset(color, (rugged * 3.0) as i16),
        MountainFeature::None => color,
    }
}

fn tile_snow_cover(world: &World, tile: &Tile) -> f32 {
    let height_above_sea = (tile.raw_elevation - world.sea_level).max(0.0);

    let (snow_line, melt_band, max_cover) = match tile.biome {
        Biome::Alpine => {
            let (line_offset, max_cover) = match tile.mountain_feature {
                MountainFeature::Summit => (0.04, 0.58),
                MountainFeature::Ridge => (0.08, 0.34),
                MountainFeature::AlpineSlope
                | MountainFeature::None
                | MountainFeature::Foothill => (0.10, 0.16),
            };
            let snow_line = (world.sea_level + 0.28 + tile.temperature * 0.20 + line_offset)
                .min(world.sea_level + 0.56);
            (snow_line, 0.12, max_cover)
        }
        Biome::Foothills => {
            if tile.temperature > 0.28 || height_above_sea < 0.34 {
                return 0.0;
            }
            let snow_line =
                (world.sea_level + 0.34 + tile.temperature * 0.14).min(world.sea_level + 0.54);
            (snow_line, 0.12, 0.12)
        }
        Biome::Tundra | Biome::PolarDesert => {
            if tile.temperature > 0.16 || height_above_sea < 0.28 {
                return 0.0;
            }
            let snow_line =
                (world.sea_level + 0.32 + tile.temperature * 0.16).min(world.sea_level + 0.52);
            (snow_line, 0.14, 0.42)
        }
        _ => return 0.0,
    };

    ((tile.raw_elevation - snow_line) / melt_band).clamp(0.0, max_cover)
}

fn channel_color(tile: &Tile) -> Rgba<u8> {
    let flow = tile.river.max(tile.spill_discharge * 0.85).clamp(0.0, 1.0);
    let order = if tile.river_order > 0 {
        ((tile.river_order.min(4) as f32 - 1.0) / 3.0).clamp(0.0, 1.0)
    } else {
        smoothstep(0.05, 0.50, flow)
    };
    let depth = (order * 0.45 + tile.river_depth.clamp(0.0, 1.0) * 0.40 + tile.river_width * 0.15)
        .clamp(0.0, 1.0);
    lerp_rgba(Rgba([76, 138, 152, 255]), Rgba([28, 76, 104, 255]), depth)
}

fn channel_alpha(tile: &Tile) -> f32 {
    let flow = tile.river.max(tile.spill_discharge * 0.85).clamp(0.0, 1.0);
    let order = if tile.river_order > 0 {
        ((tile.river_order.min(4) as f32 - 1.0) / 3.0).clamp(0.0, 1.0)
    } else {
        smoothstep(0.05, 0.50, flow)
    };
    (0.48 + flow * 0.16 + order * 0.16 + tile.river_depth * 0.12 + tile.river_width * 0.08)
        .clamp(0.0, 0.92)
}

fn biome_color_climatic(biome: Biome, temperature: f32, moisture: f32) -> Rgba<u8> {
    let base = biome_color(biome);
    match biome {
        Biome::Steppe | Biome::TemperateGrassland => {
            let dryness = (1.0 - (moisture.clamp(0.15, 0.45) - 0.15) / 0.30).max(0.0);
            add_rgb(
                base,
                (dryness * 12.0) as i16,
                (dryness * 2.0) as i16,
                -(dryness * 10.0) as i16,
            )
        }
        Biome::Desert => {
            let heat = (temperature - 0.4).clamp(0.0, 0.45) / 0.45;
            add_rgb(base, (heat * 10.0) as i16, 0, -(heat * 10.0) as i16)
        }
        Biome::Savanna => {
            let dry = (1.0 - (moisture.clamp(0.2, 0.4) - 0.2) / 0.2).max(0.0);
            let dr = (dry * 8.0) as i16;
            add_rgb(base, dr, dr / 2, -dr)
        }
        Biome::BorealForest => {
            let cold = (1.0 - (temperature.clamp(0.12, 0.32) - 0.12) / 0.20).max(0.0);
            let d = (cold * 7.0) as i16;
            add_rgb(base, -d, -d, -d)
        }
        Biome::Tundra => {
            let wet = (moisture.clamp(0.3, 0.7) - 0.3) / 0.4;
            let g = (wet * 8.0) as i16;
            add_rgb(base, -g / 2, g, 0)
        }
        Biome::Wetland => {
            let wet = moisture.clamp(0.45, 0.90);
            let dark = ((wet - 0.45) / 0.45 * 10.0) as i16;
            add_rgb(base, -dark / 2, dark / 2, -dark)
        }
        _ => base,
    }
}

fn biome_color(biome: Biome) -> Rgba<u8> {
    match biome {
        Biome::Ocean => Rgba([38, 84, 148, 255]),
        Biome::Coast => Rgba([204, 198, 148, 255]),
        Biome::PolarDesert => Rgba([212, 220, 218, 255]),
        Biome::Tundra => Rgba([148, 168, 126, 255]),
        Biome::BorealForest => Rgba([64, 112, 68, 255]),
        Biome::TemperateGrassland => Rgba([158, 184, 90, 255]),
        Biome::TemperateForest => Rgba([80, 138, 76, 255]),
        Biome::Woodland => Rgba([106, 150, 78, 255]),
        Biome::Wetland => Rgba([92, 128, 78, 255]),
        Biome::Freshwater => Rgba([58, 128, 148, 255]),
        Biome::Foothills => Rgba([160, 148, 110, 255]),
        Biome::Steppe => Rgba([176, 168, 96, 255]),
        Biome::Desert => Rgba([218, 196, 126, 255]),
        Biome::Savanna => Rgba([186, 180, 76, 255]),
        Biome::TropicalForest => Rgba([56, 148, 70, 255]),
        Biome::Rainforest => Rgba([34, 112, 52, 255]),
        Biome::Alpine => Rgba([144, 146, 142, 255]),
    }
}

fn elevation_tint(height_above_sea: f32) -> Rgba<u8> {
    if height_above_sea < 0.20 {
        let s = (height_above_sea - 0.06).max(0.0) / 0.14;
        lerp_rgba(Rgba([144, 138, 90, 255]), Rgba([148, 122, 82, 255]), s)
    } else if height_above_sea < 0.34 {
        let s = (height_above_sea - 0.20) / 0.14;
        lerp_rgba(Rgba([148, 122, 82, 255]), Rgba([132, 116, 98, 255]), s)
    } else {
        Rgba([132, 116, 98, 255])
    }
}

fn lerp_rgba(a: Rgba<u8>, b: Rgba<u8>, t: f32) -> Rgba<u8> {
    let t = t.clamp(0.0, 1.0);
    Rgba([
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).round() as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).round() as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t).round() as u8,
        (a[3] as f32 + (b[3] as f32 - a[3] as f32) * t).round() as u8,
    ])
}

fn blend(base: Rgba<u8>, overlay: Rgba<u8>, alpha: f32) -> Rgba<u8> {
    lerp_rgba(base, overlay, alpha)
}

fn scale_rgb(color: Rgba<u8>, factor: f32) -> Rgba<u8> {
    Rgba([
        (color[0] as f32 * factor).clamp(0.0, 255.0) as u8,
        (color[1] as f32 * factor).clamp(0.0, 255.0) as u8,
        (color[2] as f32 * factor).clamp(0.0, 255.0) as u8,
        color[3],
    ])
}

fn offset(color: Rgba<u8>, delta: i16) -> Rgba<u8> {
    add_rgb(color, delta, delta, delta)
}

fn add_rgb(color: Rgba<u8>, dr: i16, dg: i16, db: i16) -> Rgba<u8> {
    Rgba([
        (color[0] as i16 + dr).clamp(0, 255) as u8,
        (color[1] as i16 + dg).clamp(0, 255) as u8,
        (color[2] as i16 + db).clamp(0, 255) as u8,
        color[3],
    ])
}
