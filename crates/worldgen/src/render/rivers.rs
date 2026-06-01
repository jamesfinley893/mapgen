use image::{Rgba, RgbaImage};

use crate::generate::smoothstep;
use crate::{Surface, Tile, World};

use super::shading::lerp_rgba;

const RIVER_TILE_MIN: f32 = 0.035;
const CHANNEL_TILE_MIN: f32 = 0.105;

pub(super) fn draw_rivers(
    image: &mut RgbaImage,
    world: &World,
    scale: u32,
    land_colors: &[Rgba<u8>],
) {
    let states = channel_render_states(world);
    let downstream = river_downstream_indices(world, &states);
    let mouths = river_mouths(world, &states, &downstream);

    draw_regional_hydration_tiles(image, world, scale, &states);
    draw_alluvial_tiles(image, world, scale, &states);
    draw_estuary_tiles(image, world, scale, &mouths);
    draw_channel_tiles(image, world, scale, &states, land_colors);
}

#[derive(Clone, Copy, Default)]
struct ChannelRenderState {
    strength: f32,
    depth: f32,
    width: f32,
    spill: f32,
    low_gradient: f32,
    confinement: f32,
    turbidity: f32,
    mass: f32,
}

impl ChannelRenderState {
    fn new(tile: &Tile) -> Self {
        let strength = visual_river_strength(tile);
        let depth = visual_river_depth(tile);
        let width = visual_river_width(tile);
        let gradient = tile.slope + tile.relief * 0.55;
        let low_gradient = 1.0 - smoothstep(0.014, 0.088, gradient);
        let confinement = smoothstep(0.040, 0.170, tile.slope + tile.relief * 0.72);
        let mass = hydraulic_mass(strength, depth, width, tile.spill_discharge);
        let turbidity = turbidity_strength(tile, depth, width, mass, low_gradient);

        Self {
            strength,
            depth,
            width,
            spill: tile.spill_discharge.clamp(0.0, 1.0),
            low_gradient,
            confinement,
            turbidity,
            mass,
        }
    }
}

fn channel_render_states(world: &World) -> Vec<ChannelRenderState> {
    world
        .tiles
        .iter()
        .map(|tile| {
            if is_renderable_channel(tile) {
                ChannelRenderState::new(tile)
            } else {
                ChannelRenderState::default()
            }
        })
        .collect()
}

fn draw_regional_hydration_tiles(
    image: &mut RgbaImage,
    world: &World,
    scale: u32,
    states: &[ChannelRenderState],
) {
    for (idx, state) in states.iter().copied().enumerate() {
        if state.mass <= 0.0 {
            continue;
        }

        let amount = regional_hydration_strength(world, idx, state);
        if amount <= 0.0 {
            continue;
        }

        let radius = regional_hydration_radius(state);
        let reach = radius.ceil() as isize;
        let (x, y) = world.coords(idx);
        for dy in -reach..=reach {
            for dx in -reach..=reach {
                let tx = x as isize + dx;
                let ty = y as isize + dy;
                if !world.in_bounds(tx, ty) {
                    continue;
                }

                let target_idx = world.idx(tx as usize, ty as usize);
                if target_idx == idx {
                    continue;
                }

                draw_hydration_tile(image, world, idx, target_idx, scale, radius, amount);
            }
        }
    }
}

fn draw_hydration_tile(
    image: &mut RgbaImage,
    world: &World,
    source_idx: usize,
    target_idx: usize,
    scale: u32,
    radius: f32,
    amount: f32,
) {
    let source = &world.tiles[source_idx];
    let target = &world.tiles[target_idx];
    if target.surface == Surface::Ocean || target.lake_depth > 0.0 {
        return;
    }

    let (sx, sy) = world.coords(source_idx);
    let (tx, ty) = world.coords(target_idx);
    let tile_distance = ((tx as f32 - sx as f32).powi(2) + (ty as f32 - sy as f32).powi(2)).sqrt();
    if tile_distance > radius {
        return;
    }

    let target_dryness = (1.0 - target.moisture.clamp(0.0, 1.0)).clamp(0.0, 1.0);
    let target_upland_guard = smoothstep(
        0.20,
        0.58,
        (target.raw_elevation - world.sea_level).max(0.0),
    );
    let target_rugged_guard = smoothstep(0.045, 0.150, target.slope + target.relief * 0.50);
    let acceptance = (0.36 + target_dryness * 0.44)
        * (1.0 - target_upland_guard * 0.52)
        * (1.0 - target_rugged_guard * 0.42);
    if acceptance <= 0.0 {
        return;
    }

    let coverage = 1.0 - smoothstep(radius * 0.24, radius, tile_distance);
    let alpha = amount * acceptance * (0.018 + coverage * 0.105);
    draw_full_tile_overlay(
        image,
        world,
        target_idx,
        scale,
        regional_hydration_color(source.moisture, source.temperature),
        alpha.clamp(0.0, 0.14),
    );
}

fn draw_alluvial_tiles(
    image: &mut RgbaImage,
    world: &World,
    scale: u32,
    states: &[ChannelRenderState],
) {
    for (idx, state) in states.iter().copied().enumerate() {
        if state.mass <= 0.0 {
            continue;
        }

        let alluvial = floodplain_strength(world, idx, state);
        if alluvial <= 0.0 {
            continue;
        }

        let tile = &world.tiles[idx];
        let alpha = (0.028 + alluvial * 0.125 + state.width * 0.038).clamp(0.0, 0.20);
        draw_full_tile_overlay(
            image,
            world,
            idx,
            scale,
            floodplain_color(tile.moisture, tile.temperature),
            alpha,
        );
    }
}

fn draw_estuary_tiles(image: &mut RgbaImage, world: &World, scale: u32, mouths: &[RiverMouth]) {
    for mouth in mouths {
        let alpha = (0.080 + mouth.width * 0.170 + mouth.depth * 0.120 + mouth.strength * 0.070)
            .clamp(0.0, 0.36);
        let color = estuary_color(mouth.strength, mouth.depth);
        draw_full_tile_overlay(image, world, mouth.ocean_idx, scale, color, alpha);
        draw_full_tile_overlay(
            image,
            world,
            mouth.river_idx,
            scale,
            color,
            (alpha * 0.45).clamp(0.0, 0.18),
        );
    }
}

fn draw_channel_tiles(
    image: &mut RgbaImage,
    world: &World,
    scale: u32,
    states: &[ChannelRenderState],
    land_colors: &[Rgba<u8>],
) {
    let mut paints = vec![ChannelTilePaint::default(); world.tiles.len()];
    for (idx, state) in states.iter().copied().enumerate() {
        if state.mass <= 0.0 {
            continue;
        }
        accumulate_channel_footprint(world, idx, state, land_colors, &mut paints);
    }

    let vertices = channel_paint_vertices(world, &paints);
    draw_channel_paint_field(image, world, scale, &vertices);
}

#[derive(Clone, Copy, Default)]
struct ChannelTilePaint {
    bed_alpha: f32,
    bed_weight: f32,
    bed_r: f32,
    bed_g: f32,
    bed_b: f32,
    water_alpha: f32,
    water_weight: f32,
    water_r: f32,
    water_g: f32,
    water_b: f32,
}

impl ChannelTilePaint {
    fn add_bed(&mut self, color: Rgba<u8>, alpha: f32) {
        let alpha = alpha.clamp(0.0, 1.0);
        if alpha <= 0.0 {
            return;
        }

        self.bed_alpha = 1.0 - (1.0 - self.bed_alpha) * (1.0 - alpha);
        self.bed_weight += alpha;
        self.bed_r += color[0] as f32 * alpha;
        self.bed_g += color[1] as f32 * alpha;
        self.bed_b += color[2] as f32 * alpha;
    }

    fn add_water(&mut self, color: Rgba<u8>, alpha: f32) {
        let alpha = alpha.clamp(0.0, 1.0);
        if alpha <= 0.0 {
            return;
        }

        self.water_alpha = 1.0 - (1.0 - self.water_alpha) * (1.0 - alpha);
        self.water_weight += alpha;
        self.water_r += color[0] as f32 * alpha;
        self.water_g += color[1] as f32 * alpha;
        self.water_b += color[2] as f32 * alpha;
    }

    fn bed_color(self) -> Option<Rgba<u8>> {
        average_color(self.bed_r, self.bed_g, self.bed_b, self.bed_weight)
    }

    fn water_color(self) -> Option<Rgba<u8>> {
        average_color(self.water_r, self.water_g, self.water_b, self.water_weight)
    }
}

fn average_color(r: f32, g: f32, b: f32, weight: f32) -> Option<Rgba<u8>> {
    if weight <= 0.0 {
        return None;
    }

    Some(Rgba([
        (r / weight).clamp(0.0, 255.0) as u8,
        (g / weight).clamp(0.0, 255.0) as u8,
        (b / weight).clamp(0.0, 255.0) as u8,
        255,
    ]))
}

#[derive(Clone, Copy)]
struct ChannelVertexPaint {
    bed_alpha: f32,
    bed_color: Rgba<u8>,
    water_alpha: f32,
    water_color: Rgba<u8>,
}

impl Default for ChannelVertexPaint {
    fn default() -> Self {
        Self {
            bed_alpha: 0.0,
            bed_color: Rgba([0, 0, 0, 255]),
            water_alpha: 0.0,
            water_color: Rgba([0, 0, 0, 255]),
        }
    }
}

struct ChannelPaintVertexGrid {
    width: usize,
    vertices: Vec<ChannelVertexPaint>,
}

impl ChannelPaintVertexGrid {
    fn get(&self, x: usize, y: usize) -> ChannelVertexPaint {
        self.vertices[y * self.width + x]
    }
}

fn channel_paint_vertices(world: &World, paints: &[ChannelTilePaint]) -> ChannelPaintVertexGrid {
    let width = world.width + 1;
    let height = world.height + 1;
    let mut vertices = Vec::with_capacity(width * height);

    for y in 0..height {
        for x in 0..width {
            vertices.push(channel_vertex_paint_at(
                world, paints, x as isize, y as isize,
            ));
        }
    }

    ChannelPaintVertexGrid { width, vertices }
}

fn channel_vertex_paint_at(
    world: &World,
    paints: &[ChannelTilePaint],
    x: isize,
    y: isize,
) -> ChannelVertexPaint {
    let mut bed_alpha = 0.0_f32;
    let mut bed_r = 0.0_f32;
    let mut bed_g = 0.0_f32;
    let mut bed_b = 0.0_f32;
    let mut bed_weight = 0.0_f32;
    let mut water_alpha = 0.0_f32;
    let mut water_r = 0.0_f32;
    let mut water_g = 0.0_f32;
    let mut water_b = 0.0_f32;
    let mut water_weight = 0.0_f32;

    for dy in [-1_isize, 0] {
        for dx in [-1_isize, 0] {
            let tx = x + dx;
            let ty = y + dy;
            if !world.in_bounds(tx, ty) {
                continue;
            }

            let paint = paints[world.idx(tx as usize, ty as usize)];
            if let Some(color) = paint.bed_color() {
                let weight = paint.bed_alpha;
                bed_alpha += paint.bed_alpha;
                bed_r += color[0] as f32 * weight;
                bed_g += color[1] as f32 * weight;
                bed_b += color[2] as f32 * weight;
                bed_weight += weight;
            }
            if let Some(color) = paint.water_color() {
                let weight = paint.water_alpha;
                water_alpha += paint.water_alpha;
                water_r += color[0] as f32 * weight;
                water_g += color[1] as f32 * weight;
                water_b += color[2] as f32 * weight;
                water_weight += weight;
            }
        }
    }

    ChannelVertexPaint {
        bed_alpha: channel_vertex_alpha(bed_alpha, 0.48),
        bed_color: average_color(bed_r, bed_g, bed_b, bed_weight).unwrap_or(Rgba([0, 0, 0, 255])),
        water_alpha: channel_vertex_alpha(water_alpha, 0.72),
        water_color: average_color(water_r, water_g, water_b, water_weight)
            .unwrap_or(Rgba([0, 0, 0, 255])),
    }
}

fn channel_vertex_alpha(alpha: f32, max_alpha: f32) -> f32 {
    (alpha / 2.55).clamp(0.0, max_alpha)
}

fn draw_channel_paint_field(
    image: &mut RgbaImage,
    world: &World,
    scale: u32,
    vertices: &ChannelPaintVertexGrid,
) {
    let s = scale as f32;
    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            let tile = &world.tiles[idx];
            if tile.surface == Surface::Ocean || tile.lake_depth > 0.0 {
                continue;
            }

            let v00 = vertices.get(x, y);
            let v10 = vertices.get(x + 1, y);
            let v01 = vertices.get(x, y + 1);
            let v11 = vertices.get(x + 1, y + 1);
            let ox = x as u32 * scale;
            let oy = y as u32 * scale;
            for py in 0..scale {
                for px in 0..scale {
                    let fx = (px as f32 + 0.5) / s;
                    let fy = (py as f32 + 0.5) / s;
                    let bed_alpha = bilerp_scalar(
                        v00.bed_alpha,
                        v10.bed_alpha,
                        v01.bed_alpha,
                        v11.bed_alpha,
                        fx,
                        fy,
                    );
                    if bed_alpha > 0.0 {
                        let color = bilerp_rgba(
                            v00.bed_color,
                            v10.bed_color,
                            v01.bed_color,
                            v11.bed_color,
                            fx,
                            fy,
                        );
                        blend_pixel(image, ox + px, oy + py, color, bed_alpha);
                    }

                    let water_alpha = bilerp_scalar(
                        v00.water_alpha,
                        v10.water_alpha,
                        v01.water_alpha,
                        v11.water_alpha,
                        fx,
                        fy,
                    );
                    if water_alpha > 0.0 {
                        let color = bilerp_rgba(
                            v00.water_color,
                            v10.water_color,
                            v01.water_color,
                            v11.water_color,
                            fx,
                            fy,
                        );
                        blend_pixel(image, ox + px, oy + py, color, water_alpha);
                    }
                }
            }
        }
    }
}

fn accumulate_channel_footprint(
    world: &World,
    idx: usize,
    state: ChannelRenderState,
    land_colors: &[Rgba<u8>],
    paints: &mut [ChannelTilePaint],
) {
    let density = channel_tile_density(state);
    if density <= 0.0 {
        return;
    }

    let bed_radius = channel_footprint_radius(state);
    let water_radius = channel_water_radius(state);
    let reach = bed_radius.ceil() as isize;
    let (x, y) = world.coords(idx);
    let water_color = channel_water_color(state);
    let water_presence = smoothstep(0.12, 0.82, state.width).max(state.strength * 0.35)
        * (0.42 + state.depth * 0.42 + state.strength * 0.22);

    for dy in -reach..=reach {
        for dx in -reach..=reach {
            let tx = x as isize + dx;
            let ty = y as isize + dy;
            if !world.in_bounds(tx, ty) {
                continue;
            }

            let target_idx = world.idx(tx as usize, ty as usize);
            let target = &world.tiles[target_idx];
            if target.surface == Surface::Ocean || target.lake_depth > 0.0 {
                continue;
            }

            let distance = ((dx * dx + dy * dy) as f32).sqrt();
            let bed_coverage = channel_footprint_coverage(distance, bed_radius);
            if bed_coverage <= 0.0 {
                continue;
            }

            let bed_color = channel_bed_color(target, land_colors[target_idx], state);
            let bed_alpha =
                (0.044 + state.width * 0.165 + state.depth * 0.072 + state.strength * 0.034)
                    * density
                    * bed_coverage;
            paints[target_idx].add_bed(bed_color, bed_alpha.clamp(0.0, 0.32));

            if water_presence > 0.0 {
                let water_coverage = channel_water_coverage(distance, water_radius);
                if water_coverage <= 0.0 {
                    continue;
                }

                let core =
                    (1.0 - smoothstep(water_radius * 0.30, water_radius, distance)).clamp(0.0, 1.0);
                let water_alpha =
                    (0.090 + state.width * 0.300 + state.depth * 0.170 + state.spill * 0.060)
                        * water_presence
                        * density
                        * water_coverage
                        * (0.72 + core * 0.28);
                paints[target_idx].add_water(water_color, water_alpha.clamp(0.0, 0.74));
            }
        }
    }
}

fn channel_footprint_radius(state: ChannelRenderState) -> f32 {
    let width_radius = smoothstep(0.30, 1.0, state.width) * 1.12;
    let depth_radius = smoothstep(0.42, 0.92, state.depth) * 0.18;
    let spill_radius = smoothstep(0.12, 0.80, state.spill) * 0.18;

    (0.52 + width_radius + depth_radius + spill_radius).clamp(0.52, 1.82)
}

fn channel_footprint_coverage(distance: f32, radius: f32) -> f32 {
    if distance <= 0.0 {
        return 1.0;
    }

    1.0 - smoothstep((radius * 0.36).max(0.34), radius, distance)
}

fn channel_water_radius(state: ChannelRenderState) -> f32 {
    let width_radius = smoothstep(0.34, 1.0, state.width) * 0.96;
    let depth_radius = smoothstep(0.46, 0.96, state.depth) * 0.16;
    let spill_radius = smoothstep(0.14, 0.82, state.spill) * 0.14;

    (0.38 + width_radius + depth_radius + spill_radius).clamp(0.38, 1.46)
}

fn channel_water_coverage(distance: f32, radius: f32) -> f32 {
    if distance <= 0.0 {
        return 1.0;
    }

    1.0 - smoothstep((radius * 0.30).max(0.24), radius, distance)
}

fn draw_full_tile_overlay(
    image: &mut RgbaImage,
    world: &World,
    idx: usize,
    scale: u32,
    color: Rgba<u8>,
    alpha: f32,
) {
    if alpha <= 0.0 {
        return;
    }

    let (x, y) = world.coords(idx);
    let ox = x as u32 * scale;
    let oy = y as u32 * scale;
    let alpha = alpha.clamp(0.0, 1.0);

    for py in 0..scale {
        for px in 0..scale {
            blend_pixel(image, ox + px, oy + py, color, alpha);
        }
    }
}

fn channel_tile_density(state: ChannelRenderState) -> f32 {
    let persistent =
        smoothstep(0.09, 0.86, state.width) * (0.40 + state.depth * 0.38 + state.strength * 0.22);
    let spill = smoothstep(0.015, 0.28, state.spill) * (0.34 + state.depth * 0.30);
    persistent.max(spill).clamp(0.0, 1.0)
}

fn floodplain_strength(world: &World, idx: usize, state: ChannelRenderState) -> f32 {
    let tile = &world.tiles[idx];
    if tile.surface == Surface::Ocean
        || tile.lake_depth > 0.0
        || (tile.river <= CHANNEL_TILE_MIN && tile.spill_discharge <= 0.04)
    {
        return 0.0;
    }

    let height_above_sea = (tile.raw_elevation - world.sea_level).max(0.0);
    let lowland = 1.0 - smoothstep(0.14, 0.44, height_above_sea);
    let low_gradient = 1.0 - smoothstep(0.020, 0.095, tile.slope + tile.relief * 0.55);

    (state.mass * (0.50 + state.depth * 0.18 + state.width * 0.32) * lowland * low_gradient)
        .clamp(0.0, 1.0)
}

fn regional_hydration_strength(world: &World, idx: usize, state: ChannelRenderState) -> f32 {
    let tile = &world.tiles[idx];
    if tile.surface == Surface::Ocean || tile.lake_depth > 0.0 {
        return 0.0;
    }

    let depth_signal = smoothstep(0.08, 0.90, state.depth);
    if depth_signal <= 0.0 {
        return 0.0;
    }

    let height_above_sea = (tile.raw_elevation - world.sea_level).max(0.0);
    let lowland = 1.0 - smoothstep(0.18, 0.56, height_above_sea);
    let rugged_guard = smoothstep(0.045, 0.160, tile.slope + tile.relief * 0.55);
    let dry_response = 0.36 + (1.0 - tile.moisture.clamp(0.0, 1.0)) * 0.44;

    (depth_signal
        * state.mass
        * dry_response
        * (0.42 + lowland * 0.58)
        * (1.0 - rugged_guard * 0.44))
        .clamp(0.0, 1.0)
}

fn regional_hydration_radius(state: ChannelRenderState) -> f32 {
    (0.48 + smoothstep(0.08, 0.92, state.depth) * 0.92 + state.width * 0.36).clamp(0.55, 1.80)
}

fn is_renderable_channel(tile: &Tile) -> bool {
    tile.surface != Surface::Ocean
        && tile.lake_depth <= 0.0
        && (tile.river > RIVER_TILE_MIN || tile.spill_discharge > 0.015)
}

fn visual_river_strength(tile: &Tile) -> f32 {
    tile.river.max(tile.spill_discharge * 0.72).clamp(0.0, 1.0)
}

fn visual_river_depth(tile: &Tile) -> f32 {
    if tile.river <= RIVER_TILE_MIN && tile.spill_discharge <= 0.0 {
        return 0.0;
    }

    if tile.river_depth > 0.0 {
        return tile.river_depth.clamp(0.0, 1.0);
    }

    (smoothstep(CHANNEL_TILE_MIN, 0.95, visual_river_strength(tile))
        * (0.20 + visual_river_strength(tile) * 0.58)
        + tile.spill_discharge * 0.16)
        .clamp(0.0, 1.0)
}

fn visual_river_width(tile: &Tile) -> f32 {
    if tile.river <= RIVER_TILE_MIN && tile.spill_discharge <= 0.0 {
        return 0.0;
    }

    if tile.river_width > 0.0 {
        return tile.river_width.clamp(0.0, 1.0);
    }

    (smoothstep(0.08, 0.94, visual_river_strength(tile))
        * (0.18 + visual_river_strength(tile) * 0.56)
        + tile.spill_discharge * 0.12)
        .clamp(0.0, 1.0)
}

fn hydraulic_mass(strength: f32, depth: f32, width: f32, spill: f32) -> f32 {
    (width * 0.50 + depth * 0.34 + strength * 0.12 + spill * 0.04).clamp(0.0, 1.0)
}

fn turbidity_strength(tile: &Tile, depth: f32, width: f32, mass: f32, low_gradient: f32) -> f32 {
    let broad = smoothstep(0.26, 0.82, width);
    let deep = smoothstep(0.26, 0.86, depth);
    let dryness_supply = (1.0 - tile.moisture.clamp(0.0, 1.0)) * 0.30 + 0.70;

    (low_gradient * broad * deep * dryness_supply * (0.42 + mass * 0.58)).clamp(0.0, 1.0)
}

fn floodplain_color(moisture: f32, temperature: f32) -> Rgba<u8> {
    let dry = (1.0 - moisture.clamp(0.0, 0.75) / 0.75).clamp(0.0, 1.0);
    let warm = temperature.clamp(0.0, 1.0);
    Rgba([
        (116.0 + dry * 30.0 + warm * 6.0) as u8,
        (138.0 + (1.0 - dry) * 18.0 + warm * 3.0) as u8,
        (86.0 + (1.0 - dry) * 12.0) as u8,
        255,
    ])
}

fn channel_bed_color(tile: &Tile, base_color: Rgba<u8>, state: ChannelRenderState) -> Rgba<u8> {
    let wet_bed = lerp_rgba(
        base_color,
        Rgba([82, 126, 92, 255]),
        0.24 + state.mass * 0.12,
    );
    let floodplain = floodplain_color(tile.moisture, tile.temperature);
    let silty_bed = lerp_rgba(wet_bed, floodplain, 0.38 + state.low_gradient * 0.24);
    let incised_bed = lerp_rgba(
        base_color,
        Rgba([64, 86, 80, 255]),
        0.36 + state.depth * 0.20,
    );
    let incision =
        (state.confinement * 0.58 + (1.0 - state.low_gradient) * 0.20 + state.depth * 0.10)
            .clamp(0.0, 0.88);
    let dry_silt = (1.0 - tile.moisture.clamp(0.0, 1.0)) * state.low_gradient;

    lerp_rgba(
        silty_bed,
        incised_bed,
        (incision - state.turbidity * 0.18 - dry_silt * 0.08).clamp(0.0, 0.82),
    )
}

fn channel_water_color(state: ChannelRenderState) -> Rgba<u8> {
    // Clear teal-blue that reads as water against green/tan terrain and matches
    // the lake palette, rather than the old desaturated olive that blended in.
    let shallow = Rgba([84, 146, 158, 255]);
    let mid = Rgba([54, 116, 138, 255]);
    let deep = Rgba([40, 92, 116, 255]);
    let turbid = Rgba([96, 124, 116, 255]);
    let depth_mix = smoothstep(0.18, 0.92, state.depth);
    let water = lerp_rgba(lerp_rgba(shallow, mid, state.mass), deep, depth_mix);

    lerp_rgba(
        water,
        turbid,
        (state.turbidity * (0.46 + state.width * 0.16)).clamp(0.0, 0.70),
    )
}

fn regional_hydration_color(moisture: f32, temperature: f32) -> Rgba<u8> {
    let dry = (1.0 - moisture.clamp(0.0, 0.80) / 0.80).clamp(0.0, 1.0);
    let warm = temperature.clamp(0.0, 1.0);
    Rgba([
        (64.0 + dry * 22.0 + warm * 5.0) as u8,
        (108.0 + (1.0 - dry) * 20.0 + warm * 4.0) as u8,
        (76.0 + (1.0 - dry) * 16.0) as u8,
        255,
    ])
}

fn estuary_color(strength: f32, depth: f32) -> Rgba<u8> {
    let t = (strength * 0.44 + depth * 0.56).clamp(0.0, 1.0);
    lerp_rgba(Rgba([88, 122, 104, 255]), Rgba([58, 96, 90, 255]), t)
}

#[derive(Clone, Copy)]
struct RiverMouth {
    river_idx: usize,
    ocean_idx: usize,
    strength: f32,
    depth: f32,
    width: f32,
}

fn river_mouths(
    world: &World,
    states: &[ChannelRenderState],
    downstream: &[Option<usize>],
) -> Vec<RiverMouth> {
    downstream
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(idx, target)| {
            let target = target?;
            if states[idx].mass <= 0.0 || world.tiles[target].surface != Surface::Ocean {
                return None;
            }
            let state = states[idx];
            Some(RiverMouth {
                river_idx: idx,
                ocean_idx: target,
                strength: state.strength,
                depth: state.depth,
                width: state.width,
            })
        })
        .collect()
}

fn river_downstream_indices(world: &World, states: &[ChannelRenderState]) -> Vec<Option<usize>> {
    (0..world.tiles.len())
        .map(|idx| {
            if states[idx].mass <= 0.0 {
                return None;
            }
            downstream_neighbor(world, idx).map(|(x, y)| world.idx(x, y))
        })
        .collect()
}

fn downstream_neighbor(world: &World, idx: usize) -> Option<(usize, usize)> {
    let tile = &world.tiles[idx];
    let (x, y) = world.coords(idx);

    let (dx, dy) = flow_direction_offset(tile.flow_direction)?;
    let nx = x as isize + dx as isize;
    let ny = y as isize + dy as isize;
    if world.in_bounds(nx, ny) {
        Some((nx as usize, ny as usize))
    } else {
        None
    }
}

fn flow_direction_offset(direction: i8) -> Option<(i32, i32)> {
    match direction {
        0 => Some((0, -1)),
        1 => Some((1, -1)),
        2 => Some((1, 0)),
        3 => Some((1, 1)),
        4 => Some((0, 1)),
        5 => Some((-1, 1)),
        6 => Some((-1, 0)),
        7 => Some((-1, -1)),
        _ => None,
    }
}

fn bilerp_scalar(v00: f32, v10: f32, v01: f32, v11: f32, fx: f32, fy: f32) -> f32 {
    v00 * (1.0 - fx) * (1.0 - fy) + v10 * fx * (1.0 - fy) + v01 * (1.0 - fx) * fy + v11 * fx * fy
}

fn bilerp_rgba(
    v00: Rgba<u8>,
    v10: Rgba<u8>,
    v01: Rgba<u8>,
    v11: Rgba<u8>,
    fx: f32,
    fy: f32,
) -> Rgba<u8> {
    let channel = |idx: usize| -> u8 {
        bilerp_scalar(
            v00[idx] as f32,
            v10[idx] as f32,
            v01[idx] as f32,
            v11[idx] as f32,
            fx,
            fy,
        )
        .clamp(0.0, 255.0) as u8
    };

    Rgba([channel(0), channel(1), channel(2), 255])
}

fn blend_pixel(image: &mut RgbaImage, x: u32, y: u32, color: Rgba<u8>, alpha: f32) {
    if alpha <= 0.0 {
        return;
    }

    let base = image.get_pixel(x, y).0;
    let color = match_terrain_luminance(base, color);
    let alpha = alpha.clamp(0.0, 1.0);
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

fn match_terrain_luminance(base: [u8; 4], color: Rgba<u8>) -> Rgba<u8> {
    let base_luma = luma(base[0], base[1], base[2]);
    let color_luma = luma(color[0], color[1], color[2]).max(1.0);
    let shade_delta = (base_luma - color_luma) * 0.05;
    let adjust = |channel: u8| -> u8 { (channel as f32 + shade_delta).clamp(0.0, 255.0) as u8 };

    Rgba([adjust(color[0]), adjust(color[1]), adjust(color[2]), 255])
}

fn luma(r: u8, g: u8, b: u8) -> f32 {
    r as f32 * 0.299 + g as f32 * 0.587 + b as f32 * 0.114
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_density_tracks_simulated_width_and_depth() {
        let narrow = ChannelRenderState {
            strength: 0.70,
            depth: 0.22,
            width: 0.16,
            ..ChannelRenderState::default()
        };
        let broad = ChannelRenderState {
            strength: 0.70,
            depth: 0.72,
            width: 0.86,
            ..ChannelRenderState::default()
        };

        assert!(
            channel_tile_density(broad) > channel_tile_density(narrow) + 0.45,
            "tile density should come from simulated width/depth"
        );
    }

    #[test]
    fn channel_footprint_expands_with_simulated_width() {
        let narrow = ChannelRenderState {
            strength: 0.70,
            depth: 0.40,
            width: 0.24,
            ..ChannelRenderState::default()
        };
        let broad = ChannelRenderState {
            strength: 0.70,
            depth: 0.40,
            width: 0.92,
            ..ChannelRenderState::default()
        };

        assert!(
            channel_footprint_radius(broad) > channel_footprint_radius(narrow) + 0.95,
            "simulated width should create a multi-tile channel footprint"
        );
        assert!(
            channel_footprint_coverage(1.0, channel_footprint_radius(broad)) > 0.40,
            "broad simulated channels should visibly cover neighboring tiles"
        );
        assert!(
            channel_footprint_coverage(1.0, channel_footprint_radius(narrow)) <= 0.0,
            "narrow channels should not be widened into neighboring tiles"
        );
    }

    #[test]
    fn water_color_stays_muted_inland_not_ocean_blue() {
        let state = ChannelRenderState {
            strength: 0.92,
            depth: 0.90,
            width: 0.88,
            turbidity: 0.20,
            mass: 0.92,
            ..ChannelRenderState::default()
        };
        let color = channel_water_color(state);

        assert!(
            color[2] <= color[1] + 4,
            "river water should not shift into saturated ocean-blue: {color:?}"
        );
        assert!(
            color[1] as i16 - color[0] as i16 >= 18,
            "river water should keep muted green inland character: {color:?}"
        );
    }

    #[test]
    fn render_state_uses_local_simulated_hydraulics() {
        let tile = Tile {
            surface: Surface::Land,
            river: 0.64,
            river_depth: 0.48,
            river_width: 0.76,
            spill_discharge: 0.12,
            ..Tile::default()
        };
        let state = ChannelRenderState::new(&tile);

        assert_eq!(state.strength, tile.river);
        assert_eq!(state.depth, tile.river_depth);
        assert_eq!(state.width, tile.river_width);
        assert_eq!(state.spill, tile.spill_discharge);
    }
}
