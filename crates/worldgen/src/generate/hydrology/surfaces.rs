use crate::{Surface, World, WorldConfig};

use super::channel_thresholds;
use crate::generate::util::{sample_seed_field, smoothstep};

pub(super) fn classify_surfaces(
    world: &World,
    config: &WorldConfig,
    ocean: &[bool],
    contributing_area: &[f32],
    discharge: &[f32],
    stream_power: &[f32],
    lake_id: &[Option<u32>],
) -> Vec<Surface> {
    let mut surfaces = vec![Surface::Land; world.tiles.len()];
    let thresholds = channel_thresholds(world);
    let density = config.channel_density.clamp(0.25, 4.0);

    for idx in 0..world.tiles.len() {
        if ocean[idx] {
            surfaces[idx] = Surface::Ocean;
        } else if lake_id[idx].is_some() {
            surfaces[idx] = Surface::Lake;
        } else {
            let (x, y) = world.coords(idx);
            let local_noise = sample_seed_field(world.seed, x, y, 22, 0xD1F1_0404);
            let height_above_sea = (world.tiles[idx].raw_elevation - world.sea_level).max(0.0);
            let highland = smoothstep(
                world.sea_level + 0.12,
                world.sea_level + 0.42,
                world.tiles[idx].raw_elevation,
            );
            let dry = 1.0 - world.tiles[idx].precipitation.clamp(0.0, 1.0);
            let erodibility =
                (0.92 + dry * 0.58 - highland * 0.18 + (1.0 - local_noise) * 0.22).clamp(0.52, 1.8);
            let discharge_threshold = thresholds.stream
                * (0.08 + dry * 0.08 + (1.0 - local_noise) * 0.06)
                / density.sqrt();
            let power_threshold =
                thresholds.stream.powf(0.82) * 0.0028 * erodibility / density.powf(0.72);
            let lowland_relief_bonus = if height_above_sea < 0.12 { 0.86 } else { 1.0 };
            if discharge[idx] >= discharge_threshold
                && stream_power[idx] >= power_threshold * lowland_relief_bonus
                && contributing_area[idx] >= (thresholds.stream * 0.07).max(4.0)
            {
                surfaces[idx] = Surface::River;
            }
        }
    }

    for idx in 0..world.tiles.len() {
        if surfaces[idx] != Surface::Land {
            continue;
        }
        let (x, y) = world.coords(idx);
        if world
            .neighbors8(x, y)
            .any(|(nx, ny)| surfaces[world.idx(nx, ny)] == Surface::Ocean)
        {
            surfaces[idx] = Surface::Coast;
        }
    }

    surfaces
}

pub(super) fn suppress_short_weak_channels(
    world: &World,
    surfaces: &mut [Surface],
    downstream: &[Option<usize>],
    stream_power: &[f32],
) {
    let mut upstream = vec![0_usize; world.tiles.len()];
    for (idx, surface) in surfaces.iter().enumerate() {
        if *surface != Surface::River {
            continue;
        }
        if let Some(next) = downstream[idx]
            && surfaces[next] == Surface::River
        {
            upstream[next] += 1;
        }
    }

    let powers: Vec<_> = stream_power
        .iter()
        .enumerate()
        .filter_map(|(idx, power)| (surfaces[idx] == Surface::River).then_some(*power))
        .collect();
    if powers.is_empty() {
        return;
    }
    let mean_power = powers.iter().sum::<f32>() / powers.len() as f32;
    let mut remove = Vec::new();
    for idx in 0..world.tiles.len() {
        if surfaces[idx] != Surface::River || upstream[idx] > 0 {
            continue;
        }
        let len = path_len_to_junction_or_sink(world, surfaces, downstream, &upstream, idx);
        if len <= 1 && stream_power[idx] < mean_power * 0.55 {
            remove.push(idx);
        }
    }
    for idx in remove {
        surfaces[idx] = Surface::Land;
    }
}

pub(super) fn assign_channel_order(
    world: &World,
    surfaces: &[Surface],
    discharge: &[f32],
) -> Vec<u8> {
    let mut order = vec![0_u8; world.tiles.len()];
    let mut river_discharge: Vec<_> = discharge
        .iter()
        .enumerate()
        .filter_map(|(idx, value)| (surfaces[idx] == Surface::River).then_some(*value))
        .collect();
    river_discharge.sort_by(f32::total_cmp);
    if river_discharge.is_empty() {
        return order;
    }
    let last = river_discharge.len() - 1;
    let q55 = river_discharge[(river_discharge.len() * 55 / 100).min(last)];
    let q82 = river_discharge[(river_discharge.len() * 82 / 100).min(last)];
    let q94 = river_discharge[(river_discharge.len() * 94 / 100).min(last)];

    for idx in 0..world.tiles.len() {
        if surfaces[idx] != Surface::River {
            continue;
        }
        order[idx] = if discharge[idx] >= q94 {
            4
        } else if discharge[idx] >= q82 {
            3
        } else if discharge[idx] >= q55 {
            2
        } else {
            1
        };
    }

    order
}

fn path_len_to_junction_or_sink(
    world: &World,
    surfaces: &[Surface],
    downstream: &[Option<usize>],
    upstream: &[usize],
    start: usize,
) -> usize {
    let mut len = 0;
    let mut current = start;
    let mut guard = 0;
    while guard < world.tiles.len() {
        len += 1;
        let Some(next) = downstream[current] else {
            break;
        };
        if surfaces[next] != Surface::River || upstream[next] > 1 {
            break;
        }
        current = next;
        guard += 1;
    }
    len
}
