use crate::World;

use super::ErosionFlow;
use crate::generate::util::{
    direction_vector, local_aspect_on_values, neighbor_distance, normalize, sample_seed_field,
    smoothstep,
};

pub(super) fn simulate_erosion_flow(
    world: &World,
    terrain: &[f32],
    routing_noise_field: &[(f32, f32)],
    flow_opportunity: &[f32],
    trib_opportunity: &[f32],
    meander_field: &[f32],
) -> ErosionFlow {
    let mut is_ocean = vec![false; terrain.len()];
    for idx in 0..terrain.len() {
        is_ocean[idx] = terrain[idx] <= world.sea_level;
    }

    let mut downstream = vec![None; terrain.len()];
    let mut local_slope = vec![0.0_f32; terrain.len()];
    let mut order: Vec<_> = (0..terrain.len()).collect();
    order.sort_by(|a, b| terrain[*b].total_cmp(&terrain[*a]));

    for idx in 0..terrain.len() {
        if is_ocean[idx] {
            continue;
        }
        let (x, y) = world.coords(idx);
        let current = terrain[idx];
        let aspect = local_aspect_on_surface(world, terrain, x, y);
        let routing_noise = routing_noise_field[idx];
        let routing_noise_strength = 0.09;
        let preferred = normalize((
            aspect.0 * (1.0 - routing_noise_strength) + routing_noise.0 * routing_noise_strength,
            aspect.1 * (1.0 - routing_noise_strength) + routing_noise.1 * routing_noise_strength,
        ));
        let opportunity = flow_opportunity[idx];
        let meander_signed = meander_field[idx];
        let mut best = None;
        let mut best_score = f32::MIN;
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            let distance = neighbor_distance(x, y, nx, ny);
            let drop = current - terrain[nidx];
            if drop <= 0.0 {
                continue;
            }
            let dir = direction_vector((x, y), (nx, ny)).unwrap_or((0.0, 0.0));
            let alignment = dir.0 * preferred.0 + dir.1 * preferred.1;
            let cross_flow = dir.0 * -preferred.1 + dir.1 * preferred.0;
            let slope = drop / distance;
            let meander = if slope < 0.04 {
                cross_flow * meander_signed * (0.045 * (1.0 - slope / 0.04))
            } else {
                0.0
            };
            let clustering = (opportunity - 0.5) * (0.018 + slope.min(0.03));
            let score = slope + alignment * 0.045 + meander + clustering
                - if distance > 1.0 { 0.0015 } else { 0.0 };
            if score > best_score {
                best_score = score;
                best = Some(nidx);
                local_slope[idx] = slope;
            }
        }
        downstream[idx] = best;
    }

    let mut contributing_area = vec![0.0_f32; terrain.len()];
    for idx in order.iter().copied() {
        if is_ocean[idx] {
            continue;
        }
        contributing_area[idx] += 1.0;
        if let Some(next) = downstream[idx] {
            contributing_area[next] += contributing_area[idx];
        }
    }

    let mut confinement = vec![0.0_f32; terrain.len()];
    let mut valley_scale = vec![0.0_f32; terrain.len()];
    let mut transport_capacity = vec![0.0_f32; terrain.len()];
    let mut sediment_flux = vec![0.0_f32; terrain.len()];
    let mut deposition = vec![0.0_f32; terrain.len()];
    let mut floodplain_scale = vec![0.0_f32; terrain.len()];
    for idx in 0..terrain.len() {
        if is_ocean[idx] {
            continue;
        }
        let Some(next) = downstream[idx] else {
            continue;
        };
        let conf = flow_confinement(world, terrain, idx, next);
        let discharge = contributing_area[idx].max(1.0).ln();
        let slope = local_slope[idx];
        let tributary_opportunity = trib_opportunity[idx];
        confinement[idx] = conf;
        valley_scale[idx] = smoothstep(3.1, 5.7, discharge)
            * (1.0 - smoothstep(0.045, 0.17, slope))
            * (0.54 + (1.0 - conf) * 0.46)
            * (0.84 + tributary_opportunity * 0.22);
        transport_capacity[idx] =
            (contributing_area[idx].max(1.0).ln().powf(1.28) * slope.max(0.0008).sqrt()) * 0.9;
    }

    for idx in order.iter().copied() {
        if is_ocean[idx] {
            continue;
        }
        let slope = local_slope[idx];
        let discharge = contributing_area[idx].max(1.0).ln();
        let tributary_opportunity = trib_opportunity[idx];
        let local_supply = (0.015 + slope * 0.22 + valley_scale[idx] * 0.06)
            * (0.40 + discharge * 0.13 + (tributary_opportunity - 0.5) * 0.08)
            * (0.75 + confinement[idx] * 0.35);
        let incoming = sediment_flux[idx] + local_supply;
        let capacity = transport_capacity[idx];
        let deposited = (incoming - capacity).max(0.0);
        deposition[idx] = deposited.min(0.18);
        let carried = (incoming - deposition[idx] * 0.72).max(0.0);
        if let Some(next) = downstream[idx] {
            sediment_flux[next] += carried;
        }
        let low_slope = 1.0 - smoothstep(0.03, 0.12, slope);
        let overcapacity = if incoming <= f32::EPSILON {
            0.0
        } else {
            (deposition[idx] / incoming).clamp(0.0, 1.0)
        };
        floodplain_scale[idx] = smoothstep(2.7, 5.6, discharge)
            * low_slope
            * (0.45 + overcapacity * 0.55)
            * (0.6 + (1.0 - confinement[idx]) * 0.4);
    }

    ErosionFlow {
        downstream,
        contributing_area,
        local_slope,
        confinement,
        valley_scale,
        transport_capacity,
        sediment_flux,
        deposition,
        floodplain_scale,
        is_ocean,
    }
}

fn flow_confinement(world: &World, terrain: &[f32], idx: usize, next: usize) -> f32 {
    let (x, y) = world.coords(idx);
    let (nx, ny) = world.coords(next);
    let dx = (nx as isize - x as isize).signum();
    let dy = (ny as isize - y as isize).signum();
    if dx == 0 && dy == 0 {
        return 0.0;
    }
    let current = terrain[idx];
    let side_a = (-dy, dx);
    let side_b = (dy, -dx);
    let mut rise = 0.0_f32;
    let mut weight_sum = 0.0_f32;

    for distance in 1..=2 {
        let weight = if distance == 1 { 1.0 } else { 0.55 };
        for side in [side_a, side_b] {
            let sx = x as isize + side.0 * distance;
            let sy = y as isize + side.1 * distance;
            if !world.in_bounds(sx, sy) {
                continue;
            }
            let sidx = world.idx(sx as usize, sy as usize);
            rise += (terrain[sidx] - current).max(0.0) * weight;
            weight_sum += weight;
        }
    }

    if weight_sum <= f32::EPSILON {
        0.0
    } else {
        smoothstep(0.015, 0.13, rise / weight_sum)
    }
}

fn local_aspect_on_surface(world: &World, terrain: &[f32], x: usize, y: usize) -> (f32, f32) {
    local_aspect_on_values(terrain, world.width, world.height, x, y)
}

pub(super) fn routing_field_vector(
    seed: u64,
    x: usize,
    y: usize,
    cell_size: usize,
    channel: u64,
) -> (f32, f32) {
    let angle = sample_seed_field(seed, x, y, cell_size, channel) * std::f32::consts::TAU;
    (angle.cos(), angle.sin())
}

pub(super) fn normalize_terrain(terrain: &mut [f32], low_q: f32, high_q: f32) {
    let mut sorted = terrain.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let last = sorted.len().saturating_sub(1);
    let lo_idx = ((last as f32) * low_q).round() as usize;
    let hi_idx = ((last as f32) * high_q).round() as usize;
    let lo = sorted[lo_idx.min(last)];
    let hi = sorted[hi_idx.min(last)].max(lo + 0.0001);

    for value in terrain.iter_mut() {
        let mapped = ((*value - lo) / (hi - lo)).clamp(0.0, 1.0);
        let compressed = smoothstep(0.0, 1.0, mapped).powf(1.04);
        // Start compression later and use a smaller cap, so peaks reach ~0.95
        // instead of 0.91. This gives the Alpine biome real elevation range for
        // hillshade relief and color variation (was 0.01-wide band at 0.91).
        let top_tail = smoothstep(0.80, 1.0, compressed);
        *value = (compressed - top_tail * 0.05).clamp(0.0, 1.0);
    }
}
