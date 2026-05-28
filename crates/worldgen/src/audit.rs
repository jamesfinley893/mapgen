use std::collections::HashMap;

use crate::{Surface, World};

#[derive(Debug, Clone)]
pub struct RiverAudit {
    pub sources: usize,
    pub confluences: usize,
    pub mouths: usize,
    pub segment_min: usize,
    pub segment_median: usize,
    pub segment_p90: usize,
    pub segment_max: usize,
    pub cardinal_fraction: f32,
    pub diagonal_fraction: f32,
    pub dominant_direction: (isize, isize),
    pub dominant_direction_fraction: f32,
}

pub fn audit_rivers(world: &World) -> RiverAudit {
    let upstream = river_upstream_counts(world);
    let mut sources = 0;
    let mut confluences = 0;
    let mut mouths = 0;
    let mut segment_lengths = Vec::new();
    let mut direction_counts = HashMap::<(isize, isize), usize>::new();
    let mut cardinal = 0_usize;
    let mut diagonal = 0_usize;
    let mut steps = 0_usize;

    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface != Surface::River {
            continue;
        }
        if upstream[idx] == 0 {
            sources += 1;
            segment_lengths.push(path_len_to_junction_or_sink(world, &upstream, idx));
        } else if upstream[idx] > 1 {
            confluences += 1;
        }

        match tile.downstream {
            Some(next) if world.tiles[next].surface == Surface::River => {
                let dir = river_direction(world, idx, next);
                *direction_counts.entry(dir).or_default() += 1;
                if dir.0 == 0 || dir.1 == 0 {
                    cardinal += 1;
                } else {
                    diagonal += 1;
                }
                steps += 1;
            }
            _ => mouths += 1,
        }
    }

    segment_lengths.sort_unstable();
    let (dominant_direction, dominant_count) = direction_counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .unwrap_or(((0, 0), 0));

    RiverAudit {
        sources,
        confluences,
        mouths,
        segment_min: segment_lengths.first().copied().unwrap_or(0),
        segment_median: percentile(&segment_lengths, 0.50),
        segment_p90: percentile(&segment_lengths, 0.90),
        segment_max: segment_lengths.last().copied().unwrap_or(0),
        cardinal_fraction: cardinal as f32 / steps.max(1) as f32,
        diagonal_fraction: diagonal as f32 / steps.max(1) as f32,
        dominant_direction,
        dominant_direction_fraction: dominant_count as f32 / steps.max(1) as f32,
    }
}

pub(crate) fn river_discharge_percentiles(
    world: &World,
    lower_percentile: usize,
    upper_percentile: usize,
) -> (f32, f32) {
    let mut discharge: Vec<_> = world
        .tiles
        .iter()
        .filter_map(|tile| (tile.surface == Surface::River).then_some(tile.discharge))
        .collect();
    discharge.sort_by(f32::total_cmp);
    if discharge.is_empty() {
        return (f32::INFINITY, f32::INFINITY);
    }
    let last = discharge.len() - 1;
    (
        discharge[(discharge.len() * lower_percentile / 100).min(last)],
        discharge[(discharge.len() * upper_percentile / 100).min(last)],
    )
}

fn river_upstream_counts(world: &World) -> Vec<usize> {
    let mut upstream = vec![0_usize; world.tiles.len()];
    for tile in &world.tiles {
        if tile.surface != Surface::River {
            continue;
        }
        if let Some(next) = tile.downstream
            && world.tiles[next].surface == Surface::River
        {
            upstream[next] += 1;
        }
    }
    upstream
}

fn path_len_to_junction_or_sink(world: &World, upstream: &[usize], start: usize) -> usize {
    let mut len = 0;
    let mut current = start;
    let mut guard = 0;
    while guard < world.tiles.len() {
        len += 1;
        let Some(next) = world.tiles[current].downstream else {
            break;
        };
        if world.tiles[next].surface != Surface::River || upstream[next] > 1 {
            break;
        }
        current = next;
        guard += 1;
    }
    len
}

pub(crate) fn river_direction(world: &World, idx: usize, next: usize) -> (isize, isize) {
    let (x, y) = world.coords(idx);
    let (nx, ny) = world.coords(next);
    (
        (nx as isize - x as isize).signum(),
        (ny as isize - y as isize).signum(),
    )
}

fn percentile(values: &[usize], p: f32) -> usize {
    if values.is_empty() {
        return 0;
    }
    values[((values.len() - 1) as f32 * p).round() as usize]
}
