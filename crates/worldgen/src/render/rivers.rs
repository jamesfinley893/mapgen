use image::{Rgba, RgbaImage};

use crate::generate::{hash01, smoothstep};
use crate::{Surface, World};

const MINOR_STREAM_MIN: f32 = 0.28;
const MAJOR_RIVER_MIN: f32 = 0.55;

pub(super) fn draw_rivers(image: &mut RgbaImage, world: &World, scale: u32) {
    let downstream = river_downstream_indices(world);
    let upstream = river_upstream_summaries(world, &downstream);
    let anchors = river_anchors(world, scale, &downstream, &upstream);
    let floodplains = floodplain_strengths(world);
    let surface_mask = RenderSurfaceMask::new(world, scale);

    draw_floodplains(
        image,
        world,
        &surface_mask,
        &downstream,
        &anchors,
        &floodplains,
    );
    draw_confluence_pools(image, world, &surface_mask, &upstream, &anchors);
    draw_minor_streams(image, world, &surface_mask, &downstream, &anchors);

    for idx in 0..world.tiles.len() {
        let tile = &world.tiles[idx];
        if tile.surface == Surface::Ocean || tile.river <= MAJOR_RIVER_MIN {
            continue;
        }

        let Some(nidx) = downstream[idx] else {
            continue;
        };

        let start = anchors[idx].unwrap_or_else(|| tile_center_idx(world, idx, scale));
        let end = anchors[nidx].unwrap_or_else(|| tile_center_idx(world, nidx, scale));
        let strength = tile.river.clamp(0.0, 1.0);
        let downstream_tile = &world.tiles[nidx];
        let enters_ocean = downstream_tile.surface == Surface::Ocean;
        let downstream_strength = if downstream_tile.surface == Surface::Ocean {
            strength
        } else {
            downstream_tile.river.clamp(0.0, 1.0).max(strength * 0.72)
        };
        let start_width = river_width(scale, strength);
        let channel_end_width = river_width(scale, downstream_strength);
        let mouth_width = if enters_ocean {
            (start_width * (1.55 + strength * 0.38)).min(scale as f32 * 1.20)
        } else {
            channel_end_width
        };
        let control = river_curve_control(world, idx, nidx, start, end, scale, strength);

        if enters_ocean {
            let sediment = delta_sediment_strength(world, idx);
            if sediment > 0.0 {
                draw_water_tapered_segment(
                    image,
                    &surface_mask,
                    start,
                    end,
                    start_width * (1.10 + sediment * 0.38),
                    mouth_width * (1.18 + sediment * 0.36),
                    delta_plume_color(tile.moisture, tile.temperature),
                    0.045 + sediment * 0.070,
                );
            }
        }

        draw_curved_tapered_segment(
            image,
            start,
            end,
            start_width * 2.45,
            mouth_width * 2.20,
            river_bank_color(strength),
            0.075 + strength * 0.085,
            control,
        );
        if enters_ocean {
            draw_curved_tapered_segment(
                image,
                start,
                end,
                start_width * 1.18,
                mouth_width * 1.55,
                estuary_color(strength),
                0.14 + strength * 0.08,
                control,
            );
        }
        draw_curved_tapered_segment(
            image,
            start,
            end,
            start_width,
            mouth_width,
            river_color(strength),
            0.22 + strength * 0.14,
            control,
        );
    }
}

fn draw_confluence_pools(
    image: &mut RgbaImage,
    world: &World,
    surface_mask: &RenderSurfaceMask,
    upstream: &[RiverUpstreamSummary],
    anchors: &[Option<(f32, f32)>],
) {
    let scale = surface_mask.scale;
    for idx in 0..world.tiles.len() {
        let tile = &world.tiles[idx];
        if tile.surface == Surface::Ocean || tile.river <= MINOR_STREAM_MIN {
            continue;
        }

        let incoming = upstream[idx];
        if incoming.count < 2 {
            continue;
        }

        let confluence = smoothstep(0.80, 1.45, incoming.strength);
        if confluence <= 0.0 {
            continue;
        }

        let strongest = tile.river.clamp(0.0, 1.0).max(incoming.strongest);
        let extra_arms = incoming.count.saturating_sub(2) as f32;
        let radius = scale as f32 * (0.18 + strongest * 0.22 + (extra_arms * 0.055).min(0.16));
        let center = anchors[idx].unwrap_or_else(|| tile_center_idx(world, idx, scale));

        draw_land_disc(
            image,
            surface_mask,
            center,
            radius * 1.40,
            river_bank_color(strongest),
            0.040 + confluence * 0.075,
        );
        draw_land_disc(
            image,
            surface_mask,
            center,
            radius * 0.78,
            river_color(strongest),
            0.060 + confluence * 0.095,
        );
    }
}

fn draw_minor_streams(
    image: &mut RgbaImage,
    world: &World,
    surface_mask: &RenderSurfaceMask,
    downstream: &[Option<usize>],
    anchors: &[Option<(f32, f32)>],
) {
    let scale = surface_mask.scale;
    for idx in 0..world.tiles.len() {
        let tile = &world.tiles[idx];
        if tile.surface == Surface::Ocean
            || tile.river <= MINOR_STREAM_MIN
            || tile.river > MAJOR_RIVER_MIN
        {
            continue;
        }

        let Some(nidx) = downstream[idx] else {
            continue;
        };

        let start = anchors[idx].unwrap_or_else(|| tile_center_idx(world, idx, scale));
        let end = anchors[nidx].unwrap_or_else(|| tile_center_idx(world, nidx, scale));
        let strength = smoothstep(MINOR_STREAM_MIN, MAJOR_RIVER_MIN, tile.river);
        let enters_ocean = world.tiles[nidx].surface == Surface::Ocean;
        let downstream_strength = if enters_ocean {
            (strength * 0.82).max(0.22)
        } else {
            smoothstep(MINOR_STREAM_MIN, MAJOR_RIVER_MIN, world.tiles[nidx].river)
                .max(strength * 0.70)
        };
        let start_width = minor_stream_width(scale, strength);
        let end_width = if enters_ocean {
            (start_width * 1.38).min(scale as f32 * 0.34)
        } else {
            minor_stream_width(scale, downstream_strength)
        };
        let control = river_curve_control(world, idx, nidx, start, end, scale, strength * 0.62);

        if enters_ocean {
            draw_water_tapered_segment(
                image,
                surface_mask,
                start,
                end,
                start_width * 0.95,
                end_width * 1.55,
                estuary_color(tile.river),
                0.040 + strength * 0.045,
            );
        }

        draw_curved_tapered_segment(
            image,
            start,
            end,
            start_width,
            end_width,
            river_color(tile.river),
            0.10 + strength * 0.10,
            control,
        );
    }
}

fn draw_floodplains(
    image: &mut RgbaImage,
    world: &World,
    surface_mask: &RenderSurfaceMask,
    downstream: &[Option<usize>],
    anchors: &[Option<(f32, f32)>],
    floodplains: &[f32],
) {
    let scale = surface_mask.scale;
    for idx in 0..world.tiles.len() {
        let tile = &world.tiles[idx];
        let alluvial = floodplains[idx];
        if alluvial <= 0.0 {
            continue;
        }

        let Some(nidx) = downstream[idx] else {
            continue;
        };
        let start = anchors[idx].unwrap_or_else(|| tile_center_idx(world, idx, scale));
        let end = anchors[nidx].unwrap_or_else(|| tile_center_idx(world, nidx, scale));
        let downstream_strength = floodplains[nidx].max(alluvial * 0.62);
        let start_width = floodplain_width(scale, tile.river, alluvial);
        let end_width = floodplain_width(scale, world.tiles[nidx].river, downstream_strength);
        let control = river_curve_control(world, idx, nidx, start, end, scale, tile.river);

        draw_land_curved_tapered_segment(
            image,
            surface_mask,
            start,
            end,
            start_width,
            end_width,
            floodplain_color(tile.moisture, tile.temperature),
            0.045 + alluvial * 0.075,
            control,
        );
    }
}

fn floodplain_strengths(world: &World) -> Vec<f32> {
    (0..world.tiles.len())
        .map(|idx| floodplain_strength(world, idx))
        .collect()
}

fn floodplain_strength(world: &World, idx: usize) -> f32 {
    let tile = &world.tiles[idx];
    if tile.surface == Surface::Ocean || tile.river <= 0.32 {
        return 0.0;
    }

    let height_above_sea = (tile.raw_elevation - world.sea_level).max(0.0);
    let lowland = 1.0 - smoothstep(0.14, 0.44, height_above_sea);
    let low_gradient = 1.0 - smoothstep(0.020, 0.095, tile.slope + tile.relief * 0.55);
    let river = smoothstep(0.32, 0.88, tile.river);

    (river * lowland * low_gradient).clamp(0.0, 1.0)
}

fn delta_sediment_strength(world: &World, idx: usize) -> f32 {
    let tile = &world.tiles[idx];
    if tile.surface == Surface::Ocean || tile.river <= MAJOR_RIVER_MIN {
        return 0.0;
    }

    let height_above_sea = (tile.raw_elevation - world.sea_level).max(0.0);
    let lowland = 1.0 - smoothstep(0.07, 0.30, height_above_sea);
    let low_gradient = 1.0 - smoothstep(0.018, 0.088, tile.slope + tile.relief * 0.58);
    let discharge = smoothstep(0.52, 0.96, tile.river);
    let sediment_supply = smoothstep(0.08, 0.34, tile.runoff + tile.moisture * 0.30);

    (lowland * low_gradient * discharge * sediment_supply).clamp(0.0, 1.0)
}

fn floodplain_width(scale: u32, river: f32, strength: f32) -> f32 {
    let s = scale as f32;
    (s * (0.95 + river.clamp(0.0, 1.0) * 0.70 + strength * 0.95)).clamp(s * 0.75, s * 2.25)
}

fn floodplain_color(moisture: f32, temperature: f32) -> Rgba<u8> {
    let dry = (1.0 - moisture.clamp(0.0, 0.75) / 0.75).clamp(0.0, 1.0);
    let warm = temperature.clamp(0.0, 1.0);
    Rgba([
        (118.0 + dry * 34.0 + warm * 8.0) as u8,
        (142.0 + (1.0 - dry) * 22.0 + warm * 4.0) as u8,
        (88.0 + (1.0 - dry) * 14.0) as u8,
        255,
    ])
}

fn delta_plume_color(moisture: f32, temperature: f32) -> Rgba<u8> {
    let dry = (1.0 - moisture.clamp(0.0, 0.70) / 0.70).clamp(0.0, 1.0);
    let warm = temperature.clamp(0.0, 1.0);
    Rgba([
        (118.0 + dry * 22.0 + warm * 10.0) as u8,
        (142.0 + dry * 10.0 + warm * 4.0) as u8,
        (112.0 - dry * 8.0 + (1.0 - warm) * 8.0) as u8,
        255,
    ])
}

fn river_downstream_indices(world: &World) -> Vec<Option<usize>> {
    (0..world.tiles.len())
        .map(|idx| {
            let tile = &world.tiles[idx];
            if tile.surface == Surface::Ocean || tile.river <= MINOR_STREAM_MIN {
                return None;
            }

            downstream_neighbor(world, idx).map(|(x, y)| world.idx(x, y))
        })
        .collect()
}

#[derive(Clone, Copy, Default)]
struct RiverUpstreamSummary {
    count: usize,
    strength: f32,
    strongest: f32,
    strongest_idx: Option<usize>,
}

fn river_upstream_summaries(
    world: &World,
    downstream: &[Option<usize>],
) -> Vec<RiverUpstreamSummary> {
    let mut summaries = vec![RiverUpstreamSummary::default(); world.tiles.len()];

    for (idx, target) in downstream.iter().copied().enumerate() {
        let Some(target) = target else {
            continue;
        };

        let tile = &world.tiles[idx];
        if tile.surface == Surface::Ocean || tile.river <= MINOR_STREAM_MIN {
            continue;
        }

        let summary = &mut summaries[target];
        summary.count += 1;
        summary.strength +=
            smoothstep(MINOR_STREAM_MIN, MAJOR_RIVER_MIN, tile.river).clamp(0.0, 1.0);
        if summary.strongest_idx.is_none() || tile.river > summary.strongest {
            summary.strongest = tile.river.clamp(0.0, 1.0);
            summary.strongest_idx = Some(idx);
        }
    }

    summaries
}

fn river_anchors(
    world: &World,
    scale: u32,
    downstream: &[Option<usize>],
    upstream: &[RiverUpstreamSummary],
) -> Vec<Option<(f32, f32)>> {
    (0..world.tiles.len())
        .map(|idx| {
            let tile = &world.tiles[idx];
            if tile.surface == Surface::Ocean || tile.river <= MINOR_STREAM_MIN {
                return None;
            }

            let center = tile_center_idx(world, idx, scale);
            let mut sx = center.0 * 0.52;
            let mut sy = center.1 * 0.52;
            let mut weight = 0.52_f32;

            if let Some(target) = downstream[idx] {
                if world.tiles[target].surface != Surface::Ocean {
                    let point = tile_center_idx(world, target, scale);
                    sx += point.0 * 0.24;
                    sy += point.1 * 0.24;
                    weight += 0.24;
                }
            }

            if let Some(upstream) = upstream[idx].strongest_idx {
                let point = tile_center_idx(world, upstream, scale);
                sx += point.0 * 0.24;
                sy += point.1 * 0.24;
                weight += 0.24;
            }

            Some((sx / weight, sy / weight))
        })
        .collect()
}

fn downstream_neighbor(world: &World, idx: usize) -> Option<(usize, usize)> {
    let tile = &world.tiles[idx];
    let (x, y) = world.coords(idx);
    if let Some((dx, dy)) = flow_direction_offset(tile.flow_direction) {
        let nx = x as isize + dx;
        let ny = y as isize + dy;
        if world.in_bounds(nx, ny) {
            return Some((nx as usize, ny as usize));
        }
    }

    let mut best = None;
    let mut best_score = 0.0_f32;

    for (nx, ny) in world.neighbors8(x, y) {
        let nidx = world.idx(nx, ny);
        let neighbor = &world.tiles[nidx];
        let dx = x.abs_diff(nx);
        let dy = y.abs_diff(ny);
        let distance = if dx == 1 && dy == 1 {
            std::f32::consts::SQRT_2
        } else {
            1.0
        };

        let score = if neighbor.surface == Surface::Ocean {
            ((tile.raw_elevation - world.sea_level).max(0.0) + 0.006) / distance
        } else {
            let drop = tile.raw_elevation - neighbor.raw_elevation;
            if drop <= 0.0008 {
                continue;
            }
            drop / distance
        };

        if score > best_score {
            best_score = score;
            best = Some((nx, ny));
        }
    }

    best
}

fn flow_direction_offset(direction: i8) -> Option<(isize, isize)> {
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

fn tile_center(x: usize, y: usize, scale: u32) -> (f32, f32) {
    (
        x as f32 * scale as f32 + scale as f32 * 0.5,
        y as f32 * scale as f32 + scale as f32 * 0.5,
    )
}

fn tile_center_idx(world: &World, idx: usize, scale: u32) -> (f32, f32) {
    let (x, y) = world.coords(idx);
    tile_center(x, y, scale)
}

fn river_color(strength: f32) -> Rgba<u8> {
    let t = strength.clamp(0.0, 1.0);
    Rgba([
        (62.0 + (38.0 - 62.0) * t) as u8,
        (118.0 + (92.0 - 118.0) * t) as u8,
        (136.0 + (122.0 - 136.0) * t) as u8,
        255,
    ])
}

fn river_bank_color(strength: f32) -> Rgba<u8> {
    let t = strength.clamp(0.0, 1.0);
    Rgba([
        (82.0 + (58.0 - 82.0) * t) as u8,
        (126.0 + (100.0 - 126.0) * t) as u8,
        (112.0 + (104.0 - 112.0) * t) as u8,
        255,
    ])
}

fn estuary_color(strength: f32) -> Rgba<u8> {
    let t = strength.clamp(0.0, 1.0);
    Rgba([
        (88.0 + (64.0 - 88.0) * t) as u8,
        (144.0 + (126.0 - 144.0) * t) as u8,
        (150.0 + (142.0 - 150.0) * t) as u8,
        255,
    ])
}

fn river_width(scale: u32, strength: f32) -> f32 {
    (scale as f32 * (0.18 + strength.clamp(0.0, 1.0) * 0.38)).clamp(1.2, scale as f32 * 0.70)
}

fn minor_stream_width(scale: u32, strength: f32) -> f32 {
    (scale as f32 * (0.055 + strength.clamp(0.0, 1.0) * 0.14)).clamp(0.65, scale as f32 * 0.30)
}

fn river_curve_control(
    world: &World,
    idx: usize,
    nidx: usize,
    start: (f32, f32),
    end: (f32, f32),
    scale: u32,
    strength: f32,
) -> Option<(f32, f32)> {
    let vx = end.0 - start.0;
    let vy = end.1 - start.1;
    let length = (vx * vx + vy * vy).sqrt();
    if length < scale as f32 * 0.35 {
        return None;
    }

    let tile = &world.tiles[idx];
    let downstream = &world.tiles[nidx];
    if downstream.surface == Surface::Ocean {
        return None;
    }

    let mean_height =
        ((tile.raw_elevation + downstream.raw_elevation) * 0.5 - world.sea_level).max(0.0);
    let mean_roughness = ((tile.slope + downstream.slope) * 0.50
        + (tile.relief + downstream.relief) * 0.35)
        .clamp(0.0, 1.0);
    let lowland = 1.0 - smoothstep(0.16, 0.52, mean_height);
    let low_gradient = 1.0 - smoothstep(0.022, 0.105, mean_roughness);
    let river = smoothstep(0.20, 0.90, strength);
    let meander = (lowland * low_gradient * river).clamp(0.0, 1.0);
    if meander <= 0.05 {
        return None;
    }

    let noise = hash01(world.seed ^ 0xDA7A_B15E, idx, nidx) * 2.0 - 1.0;
    let side = if noise >= 0.0 { 1.0 } else { -1.0 };
    let magnitude = 0.45 + noise.abs() * 0.55;
    let max_offset = (length * 0.32).min(scale as f32 * (0.12 + meander * 0.42));
    let offset = side * magnitude * meander * max_offset;
    if offset.abs() < 0.18 {
        return None;
    }

    let inv_len = 1.0 / length;
    let px = -vy * inv_len;
    let py = vx * inv_len;
    Some((
        (start.0 + end.0) * 0.5 + px * offset,
        (start.1 + end.1) * 0.5 + py * offset,
    ))
}

fn draw_curved_tapered_segment(
    image: &mut RgbaImage,
    start: (f32, f32),
    end: (f32, f32),
    start_width: f32,
    end_width: f32,
    color: Rgba<u8>,
    alpha: f32,
    control: Option<(f32, f32)>,
) {
    let Some(control) = control else {
        draw_tapered_segment(image, start, end, start_width, end_width, color, alpha);
        return;
    };

    let mid_width = (start_width + end_width) * 0.52;
    draw_tapered_segment(image, start, control, start_width, mid_width, color, alpha);
    draw_tapered_segment(image, control, end, mid_width, end_width, color, alpha);
}

fn draw_tapered_segment(
    image: &mut RgbaImage,
    start: (f32, f32),
    end: (f32, f32),
    start_width: f32,
    end_width: f32,
    color: Rgba<u8>,
    alpha: f32,
) {
    let max_radius = start_width.max(end_width) * 0.5;
    let min_x = (start.0.min(end.0) - max_radius - 1.0).floor().max(0.0) as u32;
    let min_y = (start.1.min(end.1) - max_radius - 1.0).floor().max(0.0) as u32;
    let max_x = (start.0.max(end.0) + max_radius + 1.0)
        .ceil()
        .min(image.width().saturating_sub(1) as f32) as u32;
    let max_y = (start.1.max(end.1) + max_radius + 1.0)
        .ceil()
        .min(image.height().saturating_sub(1) as f32) as u32;

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let point = (px as f32 + 0.5, py as f32 + 0.5);
            let (distance, t) = distance_to_segment(point, start, end);
            let radius = (start_width + (end_width - start_width) * t) * 0.5;
            if distance > radius + 0.95 {
                continue;
            }
            let edge = 1.0 - smoothstep(radius * 0.62, radius + 0.95, distance);
            blend_pixel(image, px, py, color, alpha * edge);
        }
    }
}

fn draw_land_curved_tapered_segment(
    image: &mut RgbaImage,
    surface_mask: &RenderSurfaceMask,
    start: (f32, f32),
    end: (f32, f32),
    start_width: f32,
    end_width: f32,
    color: Rgba<u8>,
    alpha: f32,
    control: Option<(f32, f32)>,
) {
    let Some(control) = control else {
        draw_land_tapered_segment(
            image,
            surface_mask,
            start,
            end,
            start_width,
            end_width,
            color,
            alpha,
        );
        return;
    };

    let mid_width = (start_width + end_width) * 0.52;
    draw_land_tapered_segment(
        image,
        surface_mask,
        start,
        control,
        start_width,
        mid_width,
        color,
        alpha,
    );
    draw_land_tapered_segment(
        image,
        surface_mask,
        control,
        end,
        mid_width,
        end_width,
        color,
        alpha,
    );
}

fn draw_water_tapered_segment(
    image: &mut RgbaImage,
    surface_mask: &RenderSurfaceMask,
    start: (f32, f32),
    end: (f32, f32),
    start_width: f32,
    end_width: f32,
    color: Rgba<u8>,
    alpha: f32,
) {
    let max_radius = start_width.max(end_width) * 0.5;
    let min_x = (start.0.min(end.0) - max_radius - 1.0).floor().max(0.0) as u32;
    let min_y = (start.1.min(end.1) - max_radius - 1.0).floor().max(0.0) as u32;
    let max_x = (start.0.max(end.0) + max_radius + 1.0)
        .ceil()
        .min(image.width().saturating_sub(1) as f32) as u32;
    let max_y = (start.1.max(end.1) + max_radius + 1.0)
        .ceil()
        .min(image.height().saturating_sub(1) as f32) as u32;

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let point = (px as f32 + 0.5, py as f32 + 0.5);
            let (distance, t) = distance_to_segment(point, start, end);
            let radius = (start_width + (end_width - start_width) * t) * 0.5;
            if distance > radius + 1.35 {
                continue;
            }
            if !surface_mask.is_ocean_pixel(px, py) {
                continue;
            }

            let edge = 1.0 - smoothstep(radius * 0.28, radius + 1.35, distance);
            blend_pixel(image, px, py, color, alpha * edge);
        }
    }
}

fn draw_land_tapered_segment(
    image: &mut RgbaImage,
    surface_mask: &RenderSurfaceMask,
    start: (f32, f32),
    end: (f32, f32),
    start_width: f32,
    end_width: f32,
    color: Rgba<u8>,
    alpha: f32,
) {
    let max_radius = start_width.max(end_width) * 0.5;
    let min_x = (start.0.min(end.0) - max_radius - 1.0).floor().max(0.0) as u32;
    let min_y = (start.1.min(end.1) - max_radius - 1.0).floor().max(0.0) as u32;
    let max_x = (start.0.max(end.0) + max_radius + 1.0)
        .ceil()
        .min(image.width().saturating_sub(1) as f32) as u32;
    let max_y = (start.1.max(end.1) + max_radius + 1.0)
        .ceil()
        .min(image.height().saturating_sub(1) as f32) as u32;

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let point = (px as f32 + 0.5, py as f32 + 0.5);
            let (distance, t) = distance_to_segment(point, start, end);
            let radius = (start_width + (end_width - start_width) * t) * 0.5;
            if distance > radius + 1.25 {
                continue;
            }
            if surface_mask.is_ocean_pixel(px, py) {
                continue;
            }

            let edge = 1.0 - smoothstep(radius * 0.35, radius + 1.25, distance);
            blend_pixel(image, px, py, color, alpha * edge);
        }
    }
}

fn draw_land_disc(
    image: &mut RgbaImage,
    surface_mask: &RenderSurfaceMask,
    center: (f32, f32),
    radius: f32,
    color: Rgba<u8>,
    alpha: f32,
) {
    let min_x = (center.0 - radius - 1.0).floor().max(0.0) as u32;
    let min_y = (center.1 - radius - 1.0).floor().max(0.0) as u32;
    let max_x = (center.0 + radius + 1.0)
        .ceil()
        .min(image.width().saturating_sub(1) as f32) as u32;
    let max_y = (center.1 + radius + 1.0)
        .ceil()
        .min(image.height().saturating_sub(1) as f32) as u32;

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let dx = px as f32 + 0.5 - center.0;
            let dy = py as f32 + 0.5 - center.1;
            let distance = (dx * dx + dy * dy).sqrt();
            if distance > radius + 1.10 {
                continue;
            }
            if surface_mask.is_ocean_pixel(px, py) {
                continue;
            }

            let edge = 1.0 - smoothstep(radius * 0.45, radius + 1.10, distance);
            blend_pixel(image, px, py, color, alpha * edge);
        }
    }
}

struct RenderSurfaceMask {
    width: usize,
    height: usize,
    scale: u32,
    ocean: Vec<bool>,
}

impl RenderSurfaceMask {
    fn new(world: &World, scale: u32) -> Self {
        Self {
            width: world.width,
            height: world.height,
            scale,
            ocean: world
                .tiles
                .iter()
                .map(|tile| tile.surface == Surface::Ocean)
                .collect(),
        }
    }

    fn is_ocean_pixel(&self, px: u32, py: u32) -> bool {
        let x = (px / self.scale) as usize;
        let y = (py / self.scale) as usize;
        x < self.width && y < self.height && self.ocean[y * self.width + x]
    }
}

fn distance_to_segment(point: (f32, f32), start: (f32, f32), end: (f32, f32)) -> (f32, f32) {
    let vx = end.0 - start.0;
    let vy = end.1 - start.1;
    let wx = point.0 - start.0;
    let wy = point.1 - start.1;
    let len2 = (vx * vx + vy * vy).max(1e-6);
    let t = ((wx * vx + wy * vy) / len2).clamp(0.0, 1.0);
    let px = start.0 + vx * t;
    let py = start.1 + vy * t;
    let dx = point.0 - px;
    let dy = point.1 - py;
    ((dx * dx + dy * dy).sqrt(), t)
}

fn blend_pixel(image: &mut RgbaImage, x: u32, y: u32, color: Rgba<u8>, alpha: f32) {
    if alpha <= 0.0 || x >= image.width() || y >= image.height() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Tile;

    #[test]
    fn upstream_summaries_count_routed_stream_inputs_once() {
        let mut world = World::new(19, 3, 3, 0.50, 0);
        for tile in &mut world.tiles {
            *tile = Tile {
                surface: Surface::Land,
                raw_elevation: 0.56,
                river: 0.0,
                ..Tile::default()
            };
        }

        let center = world.idx(1, 1);
        let west = world.idx(0, 1);
        let north = world.idx(1, 0);
        let south = world.idx(1, 2);
        let east = world.idx(2, 1);
        world.tiles[center].river = 0.50;
        world.tiles[center].flow_direction = 2;
        world.tiles[west].river = 0.46;
        world.tiles[west].flow_direction = 2;
        world.tiles[north].river = 0.62;
        world.tiles[north].flow_direction = 4;
        world.tiles[south].river = 0.20;
        world.tiles[south].flow_direction = 0;
        world.tiles[east].river = 0.42;
        world.tiles[east].flow_direction = 2;

        let downstream = river_downstream_indices(&world);
        let summaries = river_upstream_summaries(&world, &downstream);
        let summary = summaries[center];

        assert_eq!(summary.count, 2);
        assert_eq!(summary.strongest_idx, Some(north));
        assert!(summary.strongest > 0.60);
        assert!(
            summary.strength > 1.65,
            "routed upstream strength was too weak: {}",
            summary.strength
        );
    }

    #[test]
    fn lowland_river_curve_offsets_straight_reaches() {
        let scale = 16;
        let lowland = river_curve_test_world(0.006, 0.010);
        let start = tile_center_idx(&lowland, 0, scale);
        let end = tile_center_idx(&lowland, 1, scale);
        let control = river_curve_control(&lowland, 0, 1, start, end, scale, 0.90)
            .expect("lowland river should receive a curve control point");
        let baseline_y = (start.1 + end.1) * 0.5;
        let lowland_offset = (control.1 - baseline_y).abs();

        let rugged = river_curve_test_world(0.16, 0.18);
        let rugged_control = river_curve_control(&rugged, 0, 1, start, end, scale, 0.90);
        let rugged_offset = rugged_control
            .map(|point| (point.1 - baseline_y).abs())
            .unwrap_or(0.0);

        assert!(
            lowland_offset > scale as f32 * 0.10,
            "lowland river curve is too weak: {lowland_offset}"
        );
        assert!(
            rugged_offset < lowland_offset * 0.35,
            "rugged river reaches should stay comparatively direct: lowland={lowland_offset} rugged={rugged_offset}"
        );
    }

    #[test]
    fn delta_sediment_is_limited_to_low_gradient_river_mouths() {
        let lowland = river_curve_test_world(0.004, 0.008);
        let rugged = river_curve_test_world(0.13, 0.16);

        let lowland_sediment = delta_sediment_strength(&lowland, 0);
        let rugged_sediment = delta_sediment_strength(&rugged, 0);

        assert!(
            lowland_sediment > 0.70,
            "lowland river mouth should carry visible sediment: {lowland_sediment}"
        );
        assert!(
            rugged_sediment < 0.12,
            "rugged river mouth should suppress delta sediment: {rugged_sediment}"
        );
    }

    fn river_curve_test_world(slope: f32, relief: f32) -> World {
        let mut world = World::new(19, 3, 1, 0.50, 0);
        for tile in &mut world.tiles {
            *tile = Tile {
                surface: Surface::Land,
                raw_elevation: 0.56,
                slope,
                relief,
                runoff: 0.32,
                river: 0.90,
                flow_direction: 2,
                temperature: 0.60,
                moisture: 0.48,
                ..Tile::default()
            };
        }
        world
    }
}
