use crate::{Surface, World, WorldConfig};

use super::channel_thresholds;
use crate::generate::util::sample_seed_field;

pub(super) fn classify_surfaces(
    world: &World,
    config: &WorldConfig,
    ocean: &[bool],
    downstream: &[Option<usize>],
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
        }
    }

    let mut order: Vec<_> = (0..world.tiles.len()).collect();
    order.sort_by(|a, b| discharge[*b].total_cmp(&discharge[*a]));

    for idx in order {
        if surfaces[idx] != Surface::Land {
            continue;
        }
        if !is_channel_source(
            world,
            idx,
            thresholds.stream,
            density,
            contributing_area,
            discharge,
            stream_power,
        ) {
            continue;
        }
        trace_visible_channel(
            world,
            idx,
            &mut surfaces,
            downstream,
            thresholds.stream,
            density,
            discharge,
            stream_power,
        );
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

fn is_channel_source(
    world: &World,
    idx: usize,
    stream_threshold: f32,
    density: f32,
    contributing_area: &[f32],
    discharge: &[f32],
    stream_power: &[f32],
) -> bool {
    let (x, y) = world.coords(idx);
    let local_noise = sample_seed_field(world.seed, x, y, 24, 0xD1F1_0404);
    let dry = 1.0 - world.tiles[idx].precipitation.clamp(0.0, 1.0);
    let source_q =
        stream_threshold * (0.075 + dry * 0.055 + (1.0 - local_noise) * 0.045) / density.powf(0.62);
    let source_power = stream_threshold.powf(0.82) * 0.00155 / density.powf(0.55);

    discharge[idx] >= source_q
        && stream_power[idx] >= source_power
        && contributing_area[idx] >= (stream_threshold * 0.055).max(4.0)
}

#[allow(clippy::too_many_arguments)]
fn trace_visible_channel(
    world: &World,
    start: usize,
    surfaces: &mut [Surface],
    downstream: &[Option<usize>],
    stream_threshold: f32,
    density: f32,
    discharge: &[f32],
    stream_power: &[f32],
) {
    let continuation_q = stream_threshold * 0.055 / density.powf(0.35);
    let continuation_power = stream_threshold.powf(0.82) * 0.0011 / density.powf(0.45);
    let mut current = start;
    let mut guard = 0;

    while guard < world.tiles.len() {
        match surfaces[current] {
            Surface::Ocean | Surface::Lake => break,
            Surface::River => {}
            Surface::Land | Surface::Coast => {
                if current != start
                    && discharge[current] < continuation_q
                    && stream_power[current] < continuation_power
                {
                    break;
                }
                surfaces[current] = Surface::River;
            }
        }

        let Some(next) = downstream[current] else {
            break;
        };
        if matches!(surfaces[next], Surface::Ocean | Surface::Lake) {
            break;
        }
        current = next;
        guard += 1;
    }
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
