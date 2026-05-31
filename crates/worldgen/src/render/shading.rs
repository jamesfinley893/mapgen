use image::{Rgba, RgbaImage};

use crate::generate::{hash01, smoothstep, value_noise};
use crate::{Biome, MountainFeature, World};

use super::biome_style::coastal_hinterland_biome;

pub(super) fn offset(color: Rgba<u8>, delta: i16) -> Rgba<u8> {
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

pub(super) fn draw_tile_hillshaded(
    image: &mut RgbaImage,
    vertices: &LandVertexGrid,
    world: &World,
    x: u32,
    y: u32,
    scale: u32,
) {
    let ox = x * scale;
    let oy = y * scale;
    let tx = x as usize;
    let ty = y as usize;
    let center_idx = world.idx(tx, ty);
    let center_tile = &world.tiles[center_idx];
    let center_biome = center_tile.biome;
    let texture_biome = if center_biome == Biome::Coast {
        coastal_hinterland_biome(center_tile.temperature, center_tile.moisture)
    } else {
        center_biome
    };
    let v00 = vertices.get(tx, ty);
    let v10 = vertices.get(tx + 1, ty);
    let v01 = vertices.get(tx, ty + 1);
    let v11 = vertices.get(tx + 1, ty + 1);
    let b00 = texture_biome;
    let get_texture_biome = |cx: usize, cy: usize| -> Biome {
        let cx = cx.min(world.width.saturating_sub(1));
        let cy = cy.min(world.height.saturating_sub(1));
        let nidx = world.idx(cx, cy);
        if matches!(world.tiles[nidx].biome, Biome::Ocean | Biome::Freshwater) {
            b00
        } else {
            visual_texture_biome(world, nidx)
        }
    };
    let b10 = get_texture_biome(tx + 1, ty);
    let b01 = get_texture_biome(tx, ty + 1);
    let b11 = get_texture_biome(tx + 1, ty + 1);
    let texture_edge = b10 != b00 || b01 != b00 || b11 != b00;
    let s = scale as f32;
    let textured = scale >= 3;
    let height_above_sea = (center_tile.raw_elevation - world.sea_level).max(0.0);
    let land_texture_relief = smoothstep(0.04, 0.34, height_above_sea);
    let rugged = (center_tile.relief + center_tile.slope * 0.75).clamp(0.0, 1.0);
    let talus_strength = mountain_talus_strength(world, center_idx);
    let snowfield_strength = mountain_snowfield_strength(world, center_idx);
    let mountain_spine = mountain_spine(world, center_idx);
    let biome_strength = match texture_biome {
        Biome::Alpine | Biome::Foothills => 1.18,
        Biome::TemperateForest
        | Biome::BorealForest
        | Biome::Rainforest
        | Biome::TropicalForest
        | Biome::Wetland => 0.82,
        Biome::Desert | Biome::PolarDesert => 0.74,
        Biome::Ocean => 0.0,
        _ => 0.92,
    };

    for py in 0..scale {
        for px in 0..scale {
            let fx = (px as f32 + 0.5) / s;
            let fy = (py as f32 + 0.5) / s;
            let shade = v00.shade * (1.0 - fx) * (1.0 - fy)
                + v10.shade * fx * (1.0 - fy)
                + v01.shade * (1.0 - fx) * fy
                + v11.shade * fx * fy;
            let base_color = bilerp_rgba(v00.color, v10.color, v01.color, v11.color, fx, fy);
            let shade_factor = match center_biome {
                Biome::Alpine => 0.34 + shade * 0.72,
                Biome::Foothills => 0.36 + shade * 0.68,
                _ => 0.37 + shade * 0.61,
            };
            let color = scale_rgb(base_color, shade_factor);
            // Aspect tinting: lit faces warm (+R, -B), shadowed faces cool (-R, +B).
            let tint = ((shade - 0.5) * 16.0) as i16;
            let mut color = Rgba([
                (color[0] as i16 + tint).clamp(0, 255) as u8,
                color[1],
                (color[2] as i16 - tint).clamp(0, 255) as u8,
                255,
            ]);
            if textured {
                let gx = (ox + px) as usize;
                let gy = (oy + py) as usize;
                let coarse = hash01(world.seed ^ 0x95A7_1D41, gx / 5, gy / 5);
                let fine = hash01(world.seed ^ 0x2C67_54ED, gx / 2, gy / 2);
                let grain = coarse * 0.72 + fine * 0.28 - 0.5;
                let detail = (grain
                    * (4.0 + land_texture_relief * 4.2 + rugged * 18.0)
                    * biome_strength) as i16;
                color = offset(color, detail);
                color = if texture_edge {
                    apply_edge_blended_land_cover_texture(
                        color, b00, b10, b01, b11, fx, fy, world.seed, gx, gy,
                    )
                } else {
                    apply_land_cover_texture(color, b00, world.seed, gx, gy)
                };
                if talus_strength > 0.0 {
                    color = apply_mountain_talus_texture(
                        color,
                        talus_strength,
                        shade,
                        world.seed,
                        gx,
                        gy,
                    );
                }
                color = apply_mountain_feature_texture(
                    color,
                    center_tile.mountain_feature,
                    shade,
                    rugged,
                    world.seed,
                    gx,
                    gy,
                );
                if let Some(spine) = mountain_spine {
                    color = apply_mountain_spine_texture(
                        color, spine, fx, fy, shade, world.seed, gx, gy,
                    );
                }
                if snowfield_strength > 0.0 {
                    color = apply_broken_snowfield_texture(
                        color,
                        snowfield_strength,
                        center_tile.mountain_feature,
                        shade,
                        world.seed,
                        gx,
                        gy,
                    );
                }
            }
            image.put_pixel(ox + px, oy + py, color);
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct LandVertex {
    shade: f32,
    color: Rgba<u8>,
}

pub(super) struct LandVertexGrid {
    width: usize,
    vertices: Vec<LandVertex>,
}

#[derive(Clone, Copy)]
struct MountainSpine {
    axis_x: f32,
    axis_y: f32,
    strength: f32,
}

impl LandVertexGrid {
    fn get(&self, x: usize, y: usize) -> LandVertex {
        self.vertices[y * self.width + x]
    }
}

pub(super) fn build_land_vertices(
    world: &World,
    hillshade: &[f32],
    land_colors: &[Rgba<u8>],
) -> LandVertexGrid {
    let width = world.width + 1;
    let height = world.height + 1;
    let mut vertices = Vec::with_capacity(width * height);

    for y in 0..height {
        for x in 0..width {
            vertices.push(land_vertex_at(
                world,
                hillshade,
                land_colors,
                x as isize,
                y as isize,
            ));
        }
    }

    LandVertexGrid { width, vertices }
}

fn land_vertex_at(
    world: &World,
    hillshade: &[f32],
    land_colors: &[Rgba<u8>],
    x: isize,
    y: isize,
) -> LandVertex {
    let mut r = 0.0_f32;
    let mut g = 0.0_f32;
    let mut b = 0.0_f32;
    let mut shade = 0.0_f32;
    let mut weight = 0.0_f32;

    for dy in [-1_isize, 0] {
        for dx in [-1_isize, 0] {
            let tx = x + dx;
            let ty = y + dy;
            if !world.in_bounds(tx, ty) {
                continue;
            }
            let idx = world.idx(tx as usize, ty as usize);
            if matches!(world.tiles[idx].biome, Biome::Ocean | Biome::Freshwater) {
                continue;
            }
            let color = land_colors[idx];
            r += color[0] as f32;
            g += color[1] as f32;
            b += color[2] as f32;
            shade += hillshade[idx];
            weight += 1.0;
        }
    }

    if weight <= f32::EPSILON {
        return LandVertex {
            shade: 0.0,
            color: Rgba([0, 0, 0, 255]),
        };
    }

    LandVertex {
        shade: shade / weight,
        color: Rgba([
            (r / weight).round() as u8,
            (g / weight).round() as u8,
            (b / weight).round() as u8,
            255,
        ]),
    }
}

pub(super) fn compute_hillshade(world: &World, x: usize, y: usize) -> f32 {
    let center_biome = world.tiles[world.idx(x, y)].biome;
    let center_raw_elev = world.tiles[world.idx(x, y)].raw_elevation;
    let visual_elevation = |elevation: f32| -> f32 {
        let height_above_sea = (elevation - world.sea_level).max(0.0);
        height_above_sea / (height_above_sea + 0.74)
    };
    // Use the actual land elevation field across biome boundaries so the render
    // remains faithful to terrain height; transform it through a visual curve so
    // extreme peaks do not alias into black one-tile cliffs.
    let get_visual_elev = |xi: isize, yi: isize| -> f32 {
        let cx = xi.clamp(0, world.width as isize - 1) as usize;
        let cy = yi.clamp(0, world.height as isize - 1) as usize;
        let neighbor = &world.tiles[world.idx(cx, cy)];
        match neighbor.biome {
            Biome::Ocean => 0.0,
            Biome::Freshwater => visual_elevation(center_raw_elev),
            _ => visual_elevation(neighbor.raw_elevation),
        }
    };
    let xi = x as isize;
    let yi = y as isize;
    let dz_dx = get_visual_elev(xi + 1, yi) - get_visual_elev(xi - 1, yi);
    let dz_dy = get_visual_elev(xi, yi + 1) - get_visual_elev(xi, yi - 1);
    // Adaptive z_scale: mountains get dramatic relief, plains stay gentle.
    let height_above_sea = (center_raw_elev - world.sea_level).max(0.0);
    let visual_height = height_above_sea / (height_above_sea + 0.82);
    let lowland_relief =
        smoothstep(0.04, 0.22, height_above_sea) * (1.0 - smoothstep(0.30, 0.70, height_above_sea));
    let z_scale = match center_biome {
        Biome::Alpine => 10.0 + visual_height * 31.0,
        Biome::Foothills => 7.5 + visual_height * 25.0,
        _ => 6.5 + visual_height * 24.0 + lowland_relief * 3.0,
    };
    let nx = -dz_dx * z_scale;
    let ny = 1.0_f32;
    let nz = -dz_dy * z_scale;
    let len = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-6);
    // Light from NW at 45° elevation
    let inv_sqrt3 = 1.0_f32 / 3.0_f32.sqrt();
    ((nx * (-inv_sqrt3) + ny * inv_sqrt3 + nz * (-inv_sqrt3)) / len).clamp(0.0, 1.0)
}

fn mountain_talus_strength(world: &World, idx: usize) -> f32 {
    let tile = &world.tiles[idx];
    if tile.biome != Biome::Foothills {
        return 0.0;
    }

    let (x, y) = world.coords(idx);
    let mut alpine_neighbors = 0.0_f32;
    let mut max_drop = 0.0_f32;
    for (nx, ny) in world.neighbors8(x, y) {
        let neighbor = &world.tiles[world.idx(nx, ny)];
        if neighbor.biome != Biome::Alpine {
            continue;
        }
        alpine_neighbors += 1.0;
        max_drop = max_drop.max(neighbor.raw_elevation - tile.raw_elevation);
    }

    if alpine_neighbors <= 0.0 {
        return 0.0;
    }

    let contact = (alpine_neighbors / 3.0).clamp(0.0, 1.0);
    let drop = smoothstep(0.035, 0.22, max_drop);
    let rugged = smoothstep(0.025, 0.11, tile.relief + tile.slope * 0.75);
    (contact * drop * (0.50 + rugged * 0.50)).clamp(0.0, 1.0)
}

fn mountain_snowfield_strength(world: &World, idx: usize) -> f32 {
    let tile = &world.tiles[idx];
    if tile.biome != Biome::Alpine {
        return 0.0;
    }

    let height_above_sea = (tile.raw_elevation - world.sea_level).max(0.0);
    let high = smoothstep(0.30, 0.58, height_above_sea);
    let cold = 1.0 - smoothstep(0.08, 0.34, tile.temperature);
    let feature = match tile.mountain_feature {
        MountainFeature::Summit => 1.0,
        MountainFeature::Ridge => 0.82,
        MountainFeature::AlpineSlope => 0.48,
        MountainFeature::Foothill | MountainFeature::None => 0.36,
    };

    (high * cold * feature).clamp(0.0, 1.0)
}

fn mountain_spine(world: &World, idx: usize) -> Option<MountainSpine> {
    let tile = &world.tiles[idx];
    if tile.biome != Biome::Alpine {
        return None;
    }

    let feature = match tile.mountain_feature {
        MountainFeature::Summit => 0.35,
        MountainFeature::Ridge => 0.82,
        MountainFeature::AlpineSlope | MountainFeature::Foothill | MountainFeature::None => {
            return None;
        }
    };
    let height_above_sea = (tile.raw_elevation - world.sea_level).max(0.0);
    let height = smoothstep(0.24, 0.54, height_above_sea);
    let rugged = smoothstep(0.025, 0.12, tile.relief + tile.slope * 0.80);
    let strength = (feature * (0.54 + height * 0.26 + rugged * 0.20)).clamp(0.0, 1.0);
    if strength <= 0.10 {
        return None;
    }

    let (x, y) = world.coords(idx);
    let elevation_at = |dx: isize, dy: isize| -> f32 {
        let nx = x as isize + dx;
        let ny = y as isize + dy;
        if world.in_bounds(nx, ny) {
            world.tiles[world.idx(nx as usize, ny as usize)].raw_elevation
        } else {
            tile.raw_elevation
        }
    };
    let dz_dx = elevation_at(1, 0) - elevation_at(-1, 0);
    let dz_dy = elevation_at(0, 1) - elevation_at(0, -1);
    let gradient_len = (dz_dx * dz_dx + dz_dy * dz_dy).sqrt();

    let (axis_x, axis_y) = if gradient_len > 0.001 {
        (-dz_dy / gradient_len, dz_dx / gradient_len)
    } else if let Some(axis) = mountain_neighbor_axis(world, idx) {
        axis
    } else {
        let turn = hash01(world.seed ^ 0xAF17_3C85, x, y) * std::f32::consts::TAU;
        (turn.cos(), turn.sin())
    };

    Some(MountainSpine {
        axis_x,
        axis_y,
        strength,
    })
}

fn mountain_neighbor_axis(world: &World, idx: usize) -> Option<(f32, f32)> {
    let tile = &world.tiles[idx];
    let (x, y) = world.coords(idx);
    let mut xx = 0.0_f32;
    let mut yy = 0.0_f32;
    let mut xy = 0.0_f32;
    let mut weight_sum = 0.0_f32;

    for (nx, ny) in world.neighbors8(x, y) {
        let neighbor = &world.tiles[world.idx(nx, ny)];
        if neighbor.biome != Biome::Alpine {
            continue;
        }

        let elevation_weight = smoothstep(-0.10, 0.03, neighbor.raw_elevation - tile.raw_elevation);
        let feature_weight = match neighbor.mountain_feature {
            MountainFeature::Summit => 1.00,
            MountainFeature::Ridge => 0.85,
            MountainFeature::AlpineSlope => 0.45,
            MountainFeature::Foothill | MountainFeature::None => 0.25,
        };
        let weight = elevation_weight * feature_weight;
        if weight <= 0.0 {
            continue;
        }

        let dx = nx as f32 - x as f32;
        let dy = ny as f32 - y as f32;
        xx += dx * dx * weight;
        yy += dy * dy * weight;
        xy += dx * dy * weight;
        weight_sum += weight;
    }

    if weight_sum <= 0.0 {
        return None;
    }

    let anisotropy = ((xx - yy) * (xx - yy) + 4.0 * xy * xy).sqrt() / (xx + yy).max(1e-6);
    if anisotropy < 0.12 {
        return None;
    }

    let angle = 0.5 * (2.0 * xy).atan2(xx - yy);
    Some((angle.cos(), angle.sin()))
}

fn scale_rgb(color: Rgba<u8>, factor: f32) -> Rgba<u8> {
    Rgba([
        (color[0] as f32 * factor).clamp(0.0, 255.0) as u8,
        (color[1] as f32 * factor).clamp(0.0, 255.0) as u8,
        (color[2] as f32 * factor).clamp(0.0, 255.0) as u8,
        color[3],
    ])
}

fn apply_land_cover_texture(
    color: Rgba<u8>,
    biome: Biome,
    seed: u64,
    x: usize,
    y: usize,
) -> Rgba<u8> {
    if matches!(biome, Biome::Coast | Biome::Ocean | Biome::Freshwater) {
        return color;
    }

    let broad = value_noise(seed ^ 0x6C8E_9CF5, x, y, 9) - 0.5;
    let fine = value_noise(seed ^ 0x51A9_37BD, x, y, 4) - 0.5;
    let texture = broad * 0.65 + fine * 0.35;

    match biome {
        Biome::TemperateForest => {
            let canopy = (texture * 12.0) as i16;
            add_rgb(color, -canopy / 2, canopy, -canopy / 3)
        }
        Biome::BorealForest => {
            let canopy = (texture * 10.0) as i16;
            apply_boreal_conifer_texture(
                add_rgb(color, -canopy / 2, canopy / 2, canopy / 4),
                seed,
                x,
                y,
            )
        }
        Biome::TropicalForest => {
            let canopy = (texture * 12.0) as i16;
            apply_tropical_broadleaf_texture(
                add_rgb(color, -canopy / 2, canopy, -canopy / 3),
                seed,
                x,
                y,
            )
        }
        Biome::Rainforest => {
            let canopy = (texture * 15.0) as i16;
            apply_rainforest_canopy_texture(
                add_rgb(color, -canopy / 2, canopy, -canopy / 2),
                seed,
                x,
                y,
            )
        }
        Biome::Woodland => {
            let canopy = (texture * 7.0) as i16;
            apply_woodland_mosaic_texture(
                add_rgb(color, -canopy / 3, canopy, -canopy / 4),
                seed,
                x,
                y,
            )
        }
        Biome::Wetland => {
            let reeds = (texture * 10.0) as i16;
            let color = add_rgb(color, -reeds / 4, reeds, -reeds / 2);
            apply_wetland_pools(color, seed, x, y)
        }
        Biome::TemperateGrassland => {
            let grass = (texture * 8.0) as i16;
            apply_meadow_texture(add_rgb(color, grass / 2, grass, -grass / 3), seed, x, y)
        }
        Biome::Savanna => {
            let grass = (texture * 8.0) as i16;
            apply_savanna_scrub_texture(add_rgb(color, grass / 2, grass, -grass / 3), seed, x, y)
        }
        Biome::Steppe => {
            let warmth = (texture * 7.0) as i16;
            apply_steppe_tussock_texture(
                add_rgb(color, warmth, warmth / 2, -warmth / 2),
                seed,
                x,
                y,
            )
        }
        Biome::Desert => apply_desert_dune_texture(color, seed, x, y),
        Biome::Tundra => {
            let moss = (texture * 5.0) as i16;
            apply_tundra_frost_texture(add_rgb(color, -moss / 3, moss / 2, moss / 3), seed, x, y)
        }
        Biome::PolarDesert => {
            let gravel = (texture * 4.0) as i16;
            apply_polar_desert_texture(
                add_rgb(color, gravel / 3, gravel / 3, gravel / 2),
                seed,
                x,
                y,
            )
        }
        Biome::Alpine => {
            let rock = (texture * 6.0) as i16;
            apply_alpine_crag_texture(add_rgb(color, rock / 2, rock / 2, rock), seed, x, y)
        }
        Biome::Foothills => {
            let rock = (texture * 5.0) as i16;
            apply_foothill_mosaic_texture(add_rgb(color, rock / 3, rock / 4, rock / 2), seed, x, y)
        }
        Biome::Coast | Biome::Ocean | Biome::Freshwater => color,
    }
}

fn apply_edge_blended_land_cover_texture(
    color: Rgba<u8>,
    b00: Biome,
    b10: Biome,
    b01: Biome,
    b11: Biome,
    fx: f32,
    fy: f32,
    seed: u64,
    x: usize,
    y: usize,
) -> Rgba<u8> {
    let primary = b00;
    let primary_color = apply_land_cover_texture(color, primary, seed, x, y);
    let w00 = (1.0 - fx) * (1.0 - fy);
    let w10 = fx * (1.0 - fy);
    let w01 = (1.0 - fx) * fy;
    let w11 = fx * fy;
    let mut primary_weight = w00;
    let mut strongest_edge_biome = primary;
    let mut strongest_edge_weight = 0.0_f32;

    for (biome, weight) in [(b10, w10), (b01, w01), (b11, w11)] {
        if biome == primary {
            primary_weight += weight;
        } else if weight > strongest_edge_weight {
            strongest_edge_biome = biome;
            strongest_edge_weight = weight;
        }
    }

    if strongest_edge_biome == primary || strongest_edge_weight <= 0.0 {
        return primary_color;
    }

    let edge_strength = smoothstep(0.18, 0.58, 1.0 - primary_weight);
    if edge_strength <= 0.0 {
        return primary_color;
    }

    let edge_color = apply_land_cover_texture(color, strongest_edge_biome, seed, x, y);
    lerp_rgba(primary_color, edge_color, edge_strength * 0.74)
}

fn visual_texture_biome(world: &World, idx: usize) -> Biome {
    let tile = &world.tiles[idx];
    if tile.biome == Biome::Coast {
        coastal_hinterland_biome(tile.temperature, tile.moisture)
    } else {
        tile.biome
    }
}

fn add_rgb(color: Rgba<u8>, dr: i16, dg: i16, db: i16) -> Rgba<u8> {
    Rgba([
        (color[0] as i16 + dr).clamp(0, 255) as u8,
        (color[1] as i16 + dg).clamp(0, 255) as u8,
        (color[2] as i16 + db).clamp(0, 255) as u8,
        color[3],
    ])
}

fn apply_desert_dune_texture(color: Rgba<u8>, seed: u64, x: usize, y: usize) -> Rgba<u8> {
    let broad = value_noise(seed ^ 0xD38A_71C4, x, y, 20) - 0.5;
    let drift = value_noise(seed ^ 0x91E3_4B2D, x + y / 2, y + x / 4, 11) - 0.5;
    let fine = value_noise(seed ^ 0xC6D4_E2A9, x, y, 5) - 0.5;
    let phase = x as f32 * 0.18 + y as f32 * 0.075 + drift * 2.4;
    let wave = phase.sin() * 0.5 + 0.5;
    let crest = smoothstep(0.72, 0.98, wave);
    let trough = 1.0 - smoothstep(0.18, 0.46, wave);
    let warmth = (broad * 7.0 + fine * 3.0 + crest * 8.0 - trough * 5.0) as i16;
    let shadow = (trough * 7.0) as i16;

    add_rgb(
        color,
        warmth - shadow / 3,
        warmth / 2 - shadow / 2,
        -warmth / 3 - shadow,
    )
}

fn apply_meadow_texture(color: Rgba<u8>, seed: u64, x: usize, y: usize) -> Rgba<u8> {
    let meadow = smoothstep(
        0.54,
        0.90,
        value_noise(seed ^ 0x6EAD_0F13, x + y / 4, y + x / 6, 13),
    );
    let seed_heads = smoothstep(0.70, 0.96, value_noise(seed ^ 0xC11D_7A55, x, y, 5));
    let green = (meadow * 7.0) as i16;
    let straw = (seed_heads * 5.0) as i16;

    add_rgb(color, straw / 2, green + straw / 3, -green / 3 - straw / 2)
}

fn apply_boreal_conifer_texture(color: Rgba<u8>, seed: u64, x: usize, y: usize) -> Rgba<u8> {
    let conifer_stand = smoothstep(
        0.44,
        0.86,
        value_noise(seed ^ 0x3B91_D7E5, x + y / 6, y + x / 5, 14),
    );
    let needle_clump = smoothstep(0.52, 0.90, value_noise(seed ^ 0xB8C4_117A, x, y, 5));
    let cold_shadow = smoothstep(
        0.58,
        0.94,
        value_noise(seed ^ 0x712E_64C9, x + y / 2, y + x / 7, 8),
    );
    let needles = (conifer_stand * 10.0 + needle_clump * 6.0) as i16;
    let shadow = (cold_shadow * 10.0) as i16;

    add_rgb(
        color,
        -needles / 2 - shadow,
        needles / 2 - shadow / 2,
        needles / 5 - shadow / 4,
    )
}

fn apply_tropical_broadleaf_texture(color: Rgba<u8>, seed: u64, x: usize, y: usize) -> Rgba<u8> {
    let broadleaf = smoothstep(
        0.42,
        0.84,
        value_noise(seed ^ 0xA4E7_39C1, x + y / 5, y + x / 6, 11),
    );
    let sun_gap = smoothstep(0.64, 0.94, value_noise(seed ^ 0x59F0_B23D, x, y, 6));
    let understory = smoothstep(
        0.52,
        0.90,
        value_noise(seed ^ 0xD63A_0E51, x + y / 3, y + x / 5, 8),
    );
    let leaf = (broadleaf * 12.0) as i16;
    let light = (sun_gap * 8.0) as i16;
    let dark = (understory * 5.0) as i16;

    add_rgb(
        color,
        light / 2 - leaf / 3 - dark / 2,
        leaf + light / 2 - dark / 3,
        -leaf / 2 + light / 4 - dark / 2,
    )
}

fn apply_rainforest_canopy_texture(color: Rgba<u8>, seed: u64, x: usize, y: usize) -> Rgba<u8> {
    let canopy = smoothstep(
        0.36,
        0.82,
        value_noise(seed ^ 0x91D4_AA37, x + y / 4, y + x / 5, 10),
    );
    let understory_shadow = smoothstep(0.48, 0.90, value_noise(seed ^ 0x4EE7_2B19, x, y, 5));
    let wet_highlight = smoothstep(
        0.72,
        0.97,
        value_noise(seed ^ 0xC75B_68D2, x + y / 2, y + x / 3, 6),
    ) * (1.0 - understory_shadow * 0.45);
    let leaf = (canopy * 15.0) as i16;
    let shadow = (understory_shadow * 13.0) as i16;
    let wet = (wet_highlight * 9.0) as i16;

    add_rgb(
        color,
        wet / 3 - leaf / 2 - shadow,
        leaf + wet / 2 - shadow / 2,
        wet - leaf / 2 - shadow / 3,
    )
}

fn apply_woodland_mosaic_texture(color: Rgba<u8>, seed: u64, x: usize, y: usize) -> Rgba<u8> {
    let canopy_patch = smoothstep(
        0.46,
        0.86,
        value_noise(seed ^ 0xD24B_8173, x + y / 5, y + x / 4, 12),
    );
    let understory = smoothstep(0.52, 0.90, value_noise(seed ^ 0x457E_B6D1, x, y, 7));
    let open_ground = smoothstep(
        0.58,
        0.92,
        value_noise(seed ^ 0xA9F3_0C27, x + y / 2, y + x / 6, 5),
    ) * (1.0 - canopy_patch * 0.62);
    let shade = (canopy_patch * 15.0) as i16;
    let grass = (understory * 8.0) as i16;
    let ground = (open_ground * 14.0) as i16;

    add_rgb(
        color,
        ground / 2 - shade * 2 / 3,
        grass + shade - ground / 4,
        -shade / 2 - ground / 2,
    )
}

fn apply_savanna_scrub_texture(color: Rgba<u8>, seed: u64, x: usize, y: usize) -> Rgba<u8> {
    let scrub_patch = smoothstep(
        0.58,
        0.90,
        value_noise(seed ^ 0x8C4F_22D1, x + y / 5, y + x / 7, 14),
    );
    let break_noise = value_noise(seed ^ 0xE2B9_6A31, x, y, 8);
    let scrub = scrub_patch * (1.0 - smoothstep(0.62, 0.90, break_noise));
    let dry_grass = smoothstep(0.60, 0.94, value_noise(seed ^ 0xB67D_18E3, x, y, 9));
    let shrub = (scrub * 25.0) as i16;
    let straw = (dry_grass * 12.0) as i16;

    add_rgb(
        color,
        straw / 2 - shrub,
        shrub / 4 + straw / 2,
        -shrub - straw / 2,
    )
}

fn apply_steppe_tussock_texture(color: Rgba<u8>, seed: u64, x: usize, y: usize) -> Rgba<u8> {
    let dry_grass = smoothstep(
        0.48,
        0.88,
        value_noise(seed ^ 0xEE72_34BD, x + y / 7, y + x / 5, 13),
    );
    let bare_soil = smoothstep(0.62, 0.92, value_noise(seed ^ 0x4B28_D9A1, x, y, 10));
    let drift = value_noise(seed ^ 0xB817_A4C5, x + y / 3, y, 11) - 0.5;
    let phase = x as f32 * 0.11 + y as f32 * 0.055 + drift * 2.2;
    let windrow = smoothstep(0.56, 0.96, phase.sin() * 0.5 + 0.5);
    let straw = (dry_grass * 12.0 + windrow * 10.0) as i16;
    let soil = (bare_soil * (1.0 - dry_grass * 0.45) * 16.0) as i16;

    add_rgb(
        color,
        straw / 2 + soil / 4,
        straw / 2 - soil,
        -straw / 2 - soil,
    )
}

fn apply_tundra_frost_texture(color: Rgba<u8>, seed: u64, x: usize, y: usize) -> Rgba<u8> {
    let lichen = smoothstep(
        0.50,
        0.88,
        value_noise(seed ^ 0x7A1D_3E55, x + y / 5, y + x / 4, 11),
    );
    let frost = smoothstep(0.62, 0.94, value_noise(seed ^ 0xF205_71C9, x, y, 6));
    let polygon = value_noise(seed ^ 0xA73E_C011, x + y / 3, y + x / 2, 4);
    let crack = (1.0 - smoothstep(0.18, 0.36, polygon)).max(smoothstep(0.72, 0.92, polygon));
    let green = (lichen * 9.0) as i16;
    let ice = (frost * 8.0) as i16;
    let shadow = (crack * 7.0) as i16;

    add_rgb(
        color,
        ice / 2 - green / 3 - shadow,
        green + ice / 2 - shadow / 2,
        ice - green / 4 - shadow / 3,
    )
}

fn apply_polar_desert_texture(color: Rgba<u8>, seed: u64, x: usize, y: usize) -> Rgba<u8> {
    let snow_scour = smoothstep(
        0.50,
        0.90,
        value_noise(seed ^ 0xE17A_50A7, x + y / 6, y + x / 5, 13),
    );
    let exposed_gravel = smoothstep(0.58, 0.92, value_noise(seed ^ 0x94D0_27B3, x, y, 5));
    let wind = value_noise(seed ^ 0xC9B4_F012, x + y / 2, y, 7) - 0.5;
    let snow = (snow_scour * 9.0 + wind * 4.0) as i16;
    let gravel = (exposed_gravel * 8.0) as i16;

    add_rgb(
        color,
        snow - gravel / 2,
        snow - gravel / 3,
        snow + 2 - gravel / 4,
    )
}

fn apply_alpine_crag_texture(color: Rgba<u8>, seed: u64, x: usize, y: usize) -> Rgba<u8> {
    let warp = value_noise(seed ^ 0x5C2D_EA91, x + y / 4, y + x / 5, 10) - 0.5;
    let phase = x as f32 * 0.23 - y as f32 * 0.31 + warp * 2.6;
    let rib = smoothstep(0.58, 0.96, phase.sin() * 0.5 + 0.5);
    let fracture = value_noise(seed ^ 0xE42B_17C5, x + y / 2, y + x / 3, 5);
    let crack = ((1.0 - smoothstep(0.18, 0.38, fracture)).max(smoothstep(0.76, 0.94, fracture)))
        * (0.55 + rib * 0.45);
    let scree =
        smoothstep(0.52, 0.90, value_noise(seed ^ 0x39B6_C812, x, y, 4)) * (1.0 - rib * 0.45);
    let lift = (rib * 10.0 + scree * 4.0) as i16;
    let shadow = (crack * 12.0) as i16;

    add_rgb(color, lift - shadow, lift - shadow, lift + 2 - shadow / 2)
}

fn apply_mountain_talus_texture(
    color: Rgba<u8>,
    strength: f32,
    shade: f32,
    seed: u64,
    x: usize,
    y: usize,
) -> Rgba<u8> {
    let strength = strength.clamp(0.0, 1.0);
    let warp = value_noise(seed ^ 0x71B4_9A2D, x + y / 3, y + x / 5, 12) - 0.5;
    let phase = x as f32 * 0.20 + y as f32 * 0.34 + warp * 2.8;
    let chute = smoothstep(0.58, 0.96, phase.sin() * 0.5 + 0.5);
    let rubble = smoothstep(0.48, 0.90, value_noise(seed ^ 0xB3E6_4127, x, y, 4));
    let shadow_face = 1.0 - smoothstep(0.18, 0.56, shade);
    let stone = (chute * 14.0 + rubble * 8.0) * strength;
    let shadow = (chute * shadow_face * 8.0 + rubble * 2.5) * strength;
    let scree = Rgba([134, 132, 124, 255]);
    let color = lerp_rgba(
        color,
        scree,
        (strength * (0.12 + chute * 0.18 + rubble * 0.10)).clamp(0.0, 0.42),
    );

    add_rgb(
        color,
        (stone * 0.95 - shadow * 0.55) as i16,
        (stone * 0.28 - shadow * 0.75) as i16,
        (stone * 1.05 - shadow * 0.30) as i16,
    )
}

fn apply_broken_snowfield_texture(
    color: Rgba<u8>,
    strength: f32,
    feature: MountainFeature,
    shade: f32,
    seed: u64,
    x: usize,
    y: usize,
) -> Rgba<u8> {
    let strength = strength.clamp(0.0, 1.0);
    let drift = value_noise(seed ^ 0xD89A_2F14, x + y / 5, y + x / 7, 11) - 0.5;
    let phase = x as f32 * 0.16 - y as f32 * 0.24 + drift * 2.7;
    let wind_slab = smoothstep(0.56, 0.94, phase.sin() * 0.5 + 0.5);
    let pocket = smoothstep(0.50, 0.88, value_noise(seed ^ 0x6F31_C8A5, x, y, 6));
    let lit = smoothstep(0.48, 0.86, shade);
    let feature_bias = match feature {
        MountainFeature::Summit => 0.31,
        MountainFeature::Ridge => 0.18,
        MountainFeature::AlpineSlope => 0.08,
        MountainFeature::Foothill | MountainFeature::None => 0.04,
    };
    let snow = (feature_bias + wind_slab * 0.30 + pocket * 0.24 + lit * 0.18) * strength;
    if snow <= 0.02 {
        return color;
    }

    let ice_shadow = (1.0 - lit) * strength * smoothstep(0.58, 0.94, pocket);
    let scoured_rock = (1.0 - smoothstep(0.22, 0.52, pocket)) * (1.0 - wind_slab * 0.55) * strength;
    let snow_color = lerp_rgba(Rgba([224, 230, 232, 255]), Rgba([250, 252, 252, 255]), lit);
    let color = lerp_rgba(color, snow_color, snow.clamp(0.0, 0.64));
    add_rgb(
        color,
        -(ice_shadow * 6.0 + scoured_rock * 11.0) as i16,
        -(ice_shadow * 2.0 + scoured_rock * 10.0) as i16,
        (ice_shadow * 10.0 - scoured_rock * 6.0) as i16,
    )
}

fn apply_mountain_spine_texture(
    color: Rgba<u8>,
    spine: MountainSpine,
    fx: f32,
    fy: f32,
    shade: f32,
    seed: u64,
    x: usize,
    y: usize,
) -> Rgba<u8> {
    let cx = fx - 0.5;
    let cy = fy - 0.5;
    let normal_x = -spine.axis_y;
    let normal_y = spine.axis_x;
    let signed_across = cx * normal_x + cy * normal_y;
    let along = cx * spine.axis_x + cy * spine.axis_y;
    let broad_warp = value_noise(seed ^ 0x32D4_A719, x + y / 4, y + x / 5, 8) - 0.5;
    let fine_warp = value_noise(seed ^ 0x7EC1_5DB3, x, y, 3) - 0.5;
    let warped_across =
        signed_across + broad_warp * 0.050 + fine_warp * 0.018 + along.sin() * 0.010;
    let distance = warped_across.abs();
    let crest = (1.0 - smoothstep(0.020, 0.110, distance)) * spine.strength;
    let shoulder = smoothstep(0.065, 0.220, distance)
        * (1.0 - smoothstep(0.230, 0.460, distance))
        * spine.strength;
    let lee_side = smoothstep(-0.04, 0.18, warped_across);
    let lit = smoothstep(0.42, 0.86, shade);
    let color = lerp_rgba(
        color,
        Rgba([220, 220, 212, 255]),
        (crest * (0.12 + lit * 0.18)).clamp(0.0, 0.32),
    );

    let ridge_light = crest * (8.0 + lit * 15.0);
    let side_shadow = shoulder * lee_side * (7.0 + (1.0 - lit) * 12.0);
    add_rgb(
        color,
        (ridge_light - side_shadow * 0.95) as i16,
        (ridge_light - side_shadow * 0.85) as i16,
        (ridge_light * 0.70 + crest * 3.0 - side_shadow * 0.45) as i16,
    )
}

fn apply_foothill_mosaic_texture(color: Rgba<u8>, seed: u64, x: usize, y: usize) -> Rgba<u8> {
    let grassy_slope = smoothstep(
        0.48,
        0.88,
        value_noise(seed ^ 0x2F71_BC9D, x + y / 6, y + x / 5, 12),
    );
    let exposed_stone = smoothstep(0.58, 0.92, value_noise(seed ^ 0xA6C3_47E1, x, y, 5));
    let dry_soil = smoothstep(
        0.54,
        0.90,
        value_noise(seed ^ 0x7D1E_92B5, x + y / 3, y + x / 4, 8),
    ) * (1.0 - grassy_slope * 0.45);
    let grass = (grassy_slope * 10.0) as i16;
    let stone = (exposed_stone * 11.0) as i16;
    let soil = (dry_soil * 8.0) as i16;

    add_rgb(
        color,
        soil / 2 + stone / 3 - grass / 3,
        grass + soil / 4 + stone / 5,
        stone - grass / 2 - soil / 3,
    )
}

fn apply_wetland_pools(color: Rgba<u8>, seed: u64, x: usize, y: usize) -> Rgba<u8> {
    let pool_noise = value_noise(seed ^ 0x57A6_D1C3, x + y / 2, y + x / 3, 5);
    let pool_shape = smoothstep(0.62, 0.90, pool_noise);
    if pool_shape <= 0.0 {
        return color;
    }

    let reed_breaks = value_noise(seed ^ 0xA1E4_9B2D, x, y, 11);
    let pool = pool_shape * (1.0 - smoothstep(0.58, 0.82, reed_breaks));
    if pool <= 0.0 {
        return color;
    }

    let water = Rgba([58, 118, 118, 255]);
    let glint = smoothstep(0.74, 0.96, pool_noise) * 0.10;
    offset(lerp_rgba(color, water, 0.20 * pool), (glint * 12.0) as i16)
}

fn apply_mountain_feature_texture(
    color: Rgba<u8>,
    feature: MountainFeature,
    shade: f32,
    rugged: f32,
    seed: u64,
    x: usize,
    y: usize,
) -> Rgba<u8> {
    if feature == MountainFeature::None {
        return color;
    }

    let ridge_grain = value_noise(seed ^ 0xB4C1_AE57, x, y, 3) - 0.5;
    let lit_face = smoothstep(0.52, 0.92, shade);
    let shadow_face = 1.0 - smoothstep(0.16, 0.54, shade);
    let band_warp = value_noise(seed ^ 0xC93D_28AF, x + y / 5, y + x / 4, 9) - 0.5;
    let band =
        ((x as f32 * 0.30 - y as f32 * 0.18 + band_warp * 2.4).sin() * 0.5 + 0.5).clamp(0.0, 1.0);
    let rib = smoothstep(0.66, 0.98, band);
    let gully = (1.0 - smoothstep(0.18, 0.48, band)) * shadow_face;
    let rugged = rugged.clamp(0.0, 1.0);

    match feature {
        MountainFeature::Summit => {
            let fracture = value_noise(seed ^ 0xD177_A1B5, x, y, 7);
            let crest = (15.0
                + lit_face * 21.0
                + rugged * 24.0
                + ridge_grain * 9.0
                + rib * lit_face * 11.0) as i16;
            let crack =
                (((1.0 - fracture) * shadow_face + gully * 0.75) * (11.0 + rugged * 20.0)) as i16;
            add_rgb(color, crest - crack, crest - crack, crest + 4 - crack / 2)
        }
        MountainFeature::Ridge => {
            let fracture = value_noise(seed ^ 0xD177_A1B5, x, y, 7);
            let crest =
                (8.0 + lit_face * 13.0 + rugged * 15.0 + ridge_grain * 7.0 + rib * 9.0) as i16;
            let crack = (((1.0 - fracture) * shadow_face + gully) * (7.0 + rugged * 17.0)) as i16;
            add_rgb(color, crest - crack, crest - crack, crest + 3 - crack / 2)
        }
        MountainFeature::AlpineSlope => {
            let scree = (ridge_grain * (5.0 + rugged * 11.0) + gully * -5.0) as i16;
            add_rgb(color, scree / 2, scree / 2, scree)
        }
        MountainFeature::Foothill => {
            let shoulder = (ridge_grain * (3.0 + rugged * 6.0)) as i16;
            add_rgb(color, shoulder / 2, shoulder / 3, shoulder / 4)
        }
        MountainFeature::None => color,
    }
}

pub(super) fn lerp_rgba(a: Rgba<u8>, b: Rgba<u8>, t: f32) -> Rgba<u8> {
    Rgba([
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t).clamp(0.0, 255.0) as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t).clamp(0.0, 255.0) as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t).clamp(0.0, 255.0) as u8,
        255,
    ])
}

fn bilerp_rgba(
    c00: Rgba<u8>,
    c10: Rgba<u8>,
    c01: Rgba<u8>,
    c11: Rgba<u8>,
    fx: f32,
    fy: f32,
) -> Rgba<u8> {
    lerp_rgba(lerp_rgba(c00, c10, fx), lerp_rgba(c01, c11, fx), fy)
}

// Bilinearly-interpolated value noise gives spatially-coherent render variation
// without storing another generated field.
pub(super) fn sample_noise(seed: u64, x: usize, y: usize, cell: usize) -> f32 {
    value_noise(seed, x, y, cell)
}
