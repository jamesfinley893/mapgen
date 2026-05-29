use std::collections::VecDeque;

use crate::{Surface, World, WorldConfig};

use super::channel_thresholds;
use crate::generate::util::{sample_seed_field, smoothstep};

pub(super) fn classify_surfaces(
    world: &World,
    config: &WorldConfig,
    ocean: &[bool],
    downstream: &[Option<usize>],
    discharge: &[f32],
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

    // Mark every tile whose accumulated discharge exceeds the local threshold as a river.
    // This is the correct implementation of flow-accumulation-based channel networks:
    // discharge is monotonically non-decreasing downstream, so qualifying tiles always
    // form connected paths from headwater to sink — no tracing required.
    // Tributaries emerge automatically because they share discharge accumulation with the
    // trunk; no independent source-trace means no spurious parallel rivers either.
    for idx in 0..world.tiles.len() {
        if surfaces[idx] != Surface::Land {
            continue;
        }
        if is_river_tile(world, idx, thresholds.stream, density, discharge) {
            surfaces[idx] = Surface::River;
        }
    }

    // Where many independent catchments reach the ocean within a short coastal stretch,
    // keep only the highest-discharge outlet in each cluster. Upstream fragments made
    // stranded by this suppression are cleaned up by remove_stranded_rivers.
    suppress_parallel_mouths(world, &mut surfaces, downstream, discharge, 30, thresholds.secondary);

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

fn is_river_tile(
    world: &World,
    idx: usize,
    stream_threshold: f32,
    density: f32,
    discharge: &[f32],
) -> bool {
    let (x, y) = world.coords(idx);
    // Two-scale noise: coarse sets regional drainage density, fine adds local texture.
    let coarse = sample_seed_field(world.seed, x, y, 64, 0xD1F1_0404);
    let fine = sample_seed_field(world.seed, x, y, 16, 0xD1F1_0405);
    let local_noise = coarse * 0.62 + fine * 0.38;

    let wet = world.tiles[idx].precipitation.clamp(0.0, 1.0);
    // Dry areas need substantially more discharge to sustain surface channels.
    // Climate is already encoded in discharge (via runoff coefficients), but an explicit
    // threshold shift ensures that very dry regions don't show ephemeral channels.
    let climate_factor = 0.75 + (1.0 - wet) * 0.75; // wet=0.75 to dry=1.50
    // Regional noise creates density variation: favorable zones have dense headwaters,
    // unfavorable inter-basin areas have none.
    let noise_factor = 0.80 + (1.0 - local_noise) * 0.40; // 0.80 to 1.20

    let threshold = stream_threshold * 0.55 * climate_factor * noise_factor / density.powf(0.62);
    discharge[idx] >= threshold
}

fn suppress_parallel_mouths(
    world: &World,
    surfaces: &mut [Surface],
    downstream: &[Option<usize>],
    discharge: &[f32],
    radius: usize,
    secondary_threshold: f32,
) {
    // Collect all tiles where a river exits to ocean.
    // Lake inflows are left unsuppressed — lakes naturally accept multiple tributaries.
    let mut outlets: Vec<usize> = (0..world.tiles.len())
        .filter(|&idx| {
            surfaces[idx] == Surface::River
                && downstream[idx]
                    .map(|next| surfaces[next] == Surface::Ocean)
                    .unwrap_or(false)
        })
        .collect();

    // Process in descending discharge order: dominant outlets claim their exclusion zone first.
    outlets.sort_by(|a, b| discharge[*b].total_cmp(&discharge[*a]));

    let r = radius as isize;
    let r_sq = r * r;
    let mut suppressed = vec![false; world.tiles.len()];

    for &dominant in &outlets {
        if suppressed[dominant] {
            continue;
        }
        let (dx, dy) = world.coords(dominant);
        for &candidate in &outlets {
            if candidate == dominant || suppressed[candidate] {
                continue;
            }
            let (cx, cy) = world.coords(candidate);
            let ddx = dx as isize - cx as isize;
            let ddy = dy as isize - cy as isize;
            if ddx * ddx + ddy * ddy > r_sq {
                continue;
            }
            // Two-tier suppression:
            // Small rivers (below secondary threshold): the highest-discharge outlet
            // in each coastal zone wins outright. Equal or near-equal discharge rivers
            // (common when many catchments have similar size) are all suppressed by the
            // zone's dominant. This prevents the comb of parallel gully outlets.
            // Large rivers (above secondary threshold): only suppress if dominant has
            // 2.5× more discharge, preserving genuine multi-river coastlines.
            if discharge[candidate] < secondary_threshold
                || discharge[dominant] >= discharge[candidate] * 2.5
            {
                suppressed[candidate] = true;
            }
        }
    }

    for idx in 0..world.tiles.len() {
        if suppressed[idx] {
            surfaces[idx] = Surface::Land;
        }
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
        let height_above_sea = (world.tiles[idx].raw_elevation - world.sea_level).max(0.0);
        // Relax suppression threshold for mountain headwaters: steep first-order streams
        // have high stream power relative to discharge and should survive pruning.
        let suppress_t = mean_power * (0.55 - smoothstep(0.08, 0.24, height_above_sea) * 0.35);
        if len <= 2 && stream_power[idx] < suppress_t {
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

// Remove river tiles that have no downstream path reaching ocean or a lake.
//
// This is a safety pass after the main surface classification. It handles edge
// cases where the accumulation order or routing produces an isolated fragment —
// tiles classified as River whose downstream chain terminates at a land sink.
// The BFS runs backward from all connected sinks (ocean, lake) through the
// river graph; any River tile not reached is converted back to Land.
pub(super) fn remove_stranded_rivers(
    world: &World,
    surfaces: &mut [Surface],
    downstream: &[Option<usize>],
) {
    let n = world.tiles.len();

    // Reverse graph: for each tile, which River tiles drain into it?
    let mut upstream_of: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (idx, &next_opt) in downstream.iter().enumerate() {
        if surfaces[idx] != Surface::River {
            continue;
        }
        if let Some(next) = next_opt {
            upstream_of[next].push(idx);
        }
    }

    // BFS backward from all connected sinks.
    let mut connected = vec![false; n];
    let mut queue = VecDeque::new();

    for idx in 0..n {
        if matches!(surfaces[idx], Surface::Ocean | Surface::Lake) {
            connected[idx] = true;
            for &up in &upstream_of[idx] {
                if !connected[up] {
                    connected[up] = true;
                    queue.push_back(up);
                }
            }
        }
    }

    while let Some(idx) = queue.pop_front() {
        for &up in &upstream_of[idx] {
            if !connected[up] {
                connected[up] = true;
                queue.push_back(up);
            }
        }
    }

    // Demote every River tile that never reaches a valid terminus.
    for idx in 0..n {
        if surfaces[idx] == Surface::River && !connected[idx] {
            surfaces[idx] = Surface::Land;
        }
    }
}
