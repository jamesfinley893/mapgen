use std::collections::HashSet;

use crate::World;

use super::{ConditioningState, RoutingCandidate, ValleyErosion};
use crate::generate::HYDRO_EPSILON;
use crate::generate::util::{
    direction_vector, local_aspect, local_aspect_on_values, neighbor_distance, normalize,
    sample_seed_field, smoothstep,
};

struct RoutingScoring {
    preferred: (f32, f32),
    persistence: Option<(f32, f32)>,
    tributary_opportunity: f32,
    mountain_front: f32,
    mountain_exit_bias: f32,
    lowland_opening: f32,
    meander_bias: f32,
}

struct CandidateGeometry {
    next: usize,
    distance: f32,
    direction: (isize, isize),
    unit_direction: (f32, f32),
    hydro_drop: f32,
    raw_slope: f32,
    current_valley: f32,
    next_valley: f32,
    current_depth: f32,
    next_depth: f32,
    next_width: f32,
    next_discharge_memory: f32,
    next_trunk: f32,
    next_tributary: f32,
    effective_drop: f32,
}

pub(super) fn build_downstream(
    world: &World,
    ocean: &[bool],
    conditioning: &ConditioningState,
    lake_id: &[Option<u32>],
    valleys: &ValleyErosion,
) -> Vec<Option<usize>> {
    let mut downstream = vec![None; world.tiles.len()];
    let mut order: Vec<_> = (0..world.tiles.len()).collect();
    order.sort_by(|a, b| {
        conditioning.hydro_elevation[*b]
            .total_cmp(&conditioning.hydro_elevation[*a])
            .then_with(|| {
                world.tiles[*b]
                    .raw_elevation
                    .total_cmp(&world.tiles[*a].raw_elevation)
            })
            .then_with(|| conditioning.rank[*b].cmp(&conditioning.rank[*a]))
    });

    for idx in order {
        if ocean[idx] {
            continue;
        }
        downstream[idx] = routing_candidates(world, idx, conditioning, lake_id, valleys)
            .first()
            .map(|candidate| candidate.next);
    }

    refine_lowland_bends(
        world,
        ocean,
        conditioning,
        lake_id,
        valleys,
        &mut downstream,
    );

    downstream
}

fn routing_candidates(
    world: &World,
    idx: usize,
    conditioning: &ConditioningState,
    lake_id: &[Option<u32>],
    valleys: &ValleyErosion,
) -> Vec<RoutingCandidate> {
    let (x, y) = world.coords(idx);
    let current_hydro = conditioning.hydro_elevation[idx];
    let current_raw = world.tiles[idx].raw_elevation;
    let scoring = routing_scoring(world, conditioning, idx);
    let mut candidates = Vec::with_capacity(8);

    for (nx, ny) in world.neighbors8(x, y) {
        let next = world.idx(nx, ny);
        if lake_id[idx].is_some() && lake_id[idx] == lake_id[next] {
            continue;
        }
        let neighbor_hydro = conditioning.hydro_elevation[next];
        if neighbor_hydro > current_hydro + HYDRO_EPSILON {
            continue;
        }
        let is_flat_or_equal = (neighbor_hydro - current_hydro).abs() <= HYDRO_EPSILON;
        if is_flat_or_equal && conditioning.rank[next] >= conditioning.rank[idx] {
            continue;
        }

        let geometry = candidate_geometry(
            world,
            conditioning,
            (x, y),
            current_hydro,
            current_raw,
            nx,
            ny,
            valleys,
        );
        let slope = geometry.hydro_drop / geometry.distance;
        let score = score_routing_candidate(&scoring, &geometry, slope, conditioning.parent[idx]);
        candidates.push(RoutingCandidate {
            next: geometry.next,
            score,
            direction: geometry.direction,
        });
    }

    candidates.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.next.cmp(&b.next))
    });
    candidates
}

fn routing_scoring(world: &World, conditioning: &ConditioningState, idx: usize) -> RoutingScoring {
    let (x, y) = world.coords(idx);
    let raw_aspect = local_aspect(world, x, y);
    let hydro_aspect = local_aspect_on_values(
        &conditioning.hydro_elevation,
        world.width,
        world.height,
        x,
        y,
    );
    let perturb_angle = (sample_seed_field(world.seed, x, y, 18, 0xD1F1_0101) - 0.5) * 0.9;
    let rotated = rotate_vector(hydro_aspect, perturb_angle);
    let preferred = normalize((
        hydro_aspect.0 * 0.72 + raw_aspect.0 * 0.20 + rotated.0 * 0.08,
        hydro_aspect.1 * 0.72 + raw_aspect.1 * 0.20 + rotated.1 * 0.08,
    ));
    let persistence = conditioning.parent[idx].and_then(|parent| {
        let (px, py) = world.coords(parent);
        direction_vector((x, y), (px, py))
    });
    let coarse_meander = sample_seed_field(world.seed, x, y, 14, 0xD1F1_0505);
    let fine_meander = sample_seed_field(world.seed, x, y, 6, 0xD1F1_0506);

    RoutingScoring {
        preferred,
        persistence,
        tributary_opportunity: sample_seed_field(world.seed, x, y, 22, 0xD1F1_0202),
        mountain_front: mountain_front_factor(world, idx),
        mountain_exit_bias: sample_seed_field(world.seed, x, y, 14, 0xD1F1_0303) * 2.0 - 1.0,
        lowland_opening: lowland_opening_factor(world, idx),
        // Per-tile (not per-pair) smooth bias for consistent lateral preference along a path.
        // Two scales preserve tributary spacing while reducing per-step erratic zigs.
        meander_bias: (coarse_meander * 0.55 + fine_meander * 0.45) * 2.0 - 1.0,
    }
}

fn candidate_geometry(
    world: &World,
    conditioning: &ConditioningState,
    from: (usize, usize),
    current_hydro: f32,
    current_raw: f32,
    nx: usize,
    ny: usize,
    valleys: &ValleyErosion,
) -> CandidateGeometry {
    let (x, y) = from;
    let next = world.idx(nx, ny);
    let distance = neighbor_distance(x, y, nx, ny);
    let unit_direction = direction_vector((x, y), (nx, ny)).unwrap_or((0.0, 0.0));
    let hydro_drop = (current_hydro - conditioning.hydro_elevation[next]).max(0.0);
    let raw_drop = (current_raw - world.tiles[next].raw_elevation).max(-0.08);
    let current_bias = valleys.routing_bias(world.idx(x, y));
    let next_bias = valleys.routing_bias(next);
    let effective_drop =
        (current_hydro - current_bias - (conditioning.hydro_elevation[next] - next_bias))
            .max(-0.02);

    CandidateGeometry {
        next,
        distance,
        direction: (
            (nx as isize - x as isize).signum(),
            (ny as isize - y as isize).signum(),
        ),
        unit_direction,
        hydro_drop,
        raw_slope: raw_drop / distance,
        current_valley: valleys.activity(world.idx(x, y)),
        next_valley: valleys.activity(next),
        current_depth: valleys.valley_depth(world.idx(x, y)),
        next_depth: valleys.valley_depth(next),
        next_width: valleys.valley_width(next),
        next_discharge_memory: valleys.long_term_discharge(next),
        next_trunk: valleys.trunk_strength(next),
        next_tributary: valleys.tributary_strength(next),
        effective_drop: effective_drop / distance,
    }
}

fn score_routing_candidate(
    scoring: &RoutingScoring,
    geometry: &CandidateGeometry,
    slope: f32,
    parent: Option<usize>,
) -> f32 {
    let dir = geometry.unit_direction;
    let alignment = dir.0 * scoring.preferred.0 + dir.1 * scoring.preferred.1;
    let aspect_cross = dir.0 * -scoring.preferred.1 + dir.1 * scoring.preferred.0;
    let persistence_bonus = scoring.persistence.map_or(0.0, |prev| {
        let dot = dir.0 * prev.0 + dir.1 * prev.1;
        if scoring.lowland_opening > 0.35 {
            -dot.max(0.0) * 0.10
        } else {
            dot.max(-0.5) * 0.06
        }
    });
    let flat_bonus = if geometry.hydro_drop <= HYDRO_EPSILON {
        geometry.raw_slope.max(0.0) * 2.1 + alignment * 0.15
    } else {
        0.0
    };
    let meander_bonus = if slope < 0.034 {
        aspect_cross * scoring.meander_bias * (0.18 * (1.0 - (slope / 0.034).clamp(0.0, 1.0)))
    } else {
        0.0
    };
    let anisotropy_penalty = if slope < 0.022 {
        let cardinal = if dir.0.abs() > 0.92 || dir.1.abs() > 0.92 {
            1.0
        } else {
            0.0
        };
        let diagonal = if dir.0.abs() > 0.65 && dir.1.abs() > 0.65 {
            1.0
        } else {
            0.0
        };
        cardinal * (0.06 + scoring.lowland_opening * 0.03) + diagonal * 0.035
    } else {
        0.0
    };
    let spacing_bonus =
        (scoring.tributary_opportunity - 0.5) * (0.30 + scoring.lowland_opening * 0.14);
    let mountain_exit_bonus = if scoring.mountain_front > 0.0 {
        aspect_cross * scoring.mountain_exit_bias * 0.28 * scoring.mountain_front
    } else {
        0.0
    };
    let parent_bonus = if Some(geometry.next) == parent {
        0.02
    } else {
        0.0
    };
    let valley_gradient = geometry.next_valley - geometry.current_valley;
    let valley_capture_bonus = (geometry.next_valley * 0.34
        + geometry.next_trunk * 0.26
        + geometry.next_tributary * 0.12
        + geometry.next_depth * 2.20
        + (geometry.next_width / 18.0).clamp(0.0, 1.0) * 0.18
        + valley_gradient.max(0.0) * 0.42
        - (-valley_gradient).max(0.0) * 0.08)
        * (0.48 + scoring.lowland_opening * 0.28 + scoring.mountain_front * 0.24);
    let discharge_memory_bonus =
        geometry.next_discharge_memory.max(1.0).ln_1p() * 0.010 * geometry.next_valley;
    let valley_abandonment_penalty = (geometry.current_depth - geometry.next_depth).max(0.0)
        * (0.95 + scoring.mountain_front * 0.65)
        + (geometry.current_valley - geometry.next_valley).max(0.0) * 0.10;

    slope * 8.3
        + geometry.effective_drop.max(-0.004) * 5.5
        + geometry.raw_slope.max(0.0) * 2.8
        + alignment * 0.92
        + persistence_bonus
        + flat_bonus
        + meander_bonus
        + spacing_bonus
        + mountain_exit_bonus
        + parent_bonus
        + valley_capture_bonus
        + discharge_memory_bonus
        - anisotropy_penalty
        - valley_abandonment_penalty
}

fn rotate_vector(v: (f32, f32), angle: f32) -> (f32, f32) {
    let (s, c) = angle.sin_cos();
    (v.0 * c - v.1 * s, v.0 * s + v.1 * c)
}

fn refine_lowland_bends(
    world: &World,
    ocean: &[bool],
    conditioning: &ConditioningState,
    lake_id: &[Option<u32>],
    valleys: &ValleyErosion,
    downstream: &mut [Option<usize>],
) {
    let mut order: Vec<_> = (0..world.tiles.len()).collect();
    order.sort_by(|a, b| {
        conditioning.hydro_elevation[*b]
            .total_cmp(&conditioning.hydro_elevation[*a])
            .then_with(|| conditioning.rank[*b].cmp(&conditioning.rank[*a]))
    });

    for _ in 0..2 {
        for idx in order.iter().copied() {
            if ocean[idx] || downstream[idx].is_none() || !is_run_start(world, downstream, idx) {
                continue;
            }
            let slope = local_downstream_slope(world, conditioning, downstream, idx);
            let segment = same_direction_segment(world, downstream, idx);
            let limit = if slope < 0.014 {
                3
            } else if slope < 0.032 {
                4
            } else {
                6
            };
            if segment.len() < limit {
                continue;
            }

            let target = segment[segment.len() / 2];
            let Some(current) = downstream[target] else {
                continue;
            };
            let current_dir = direction_for(world, target, current);
            let target_slope = local_downstream_slope(world, conditioning, downstream, target);
            let candidates = routing_candidates(world, target, conditioning, lake_id, valleys);
            let current_score = candidates
                .iter()
                .find(|candidate| candidate.next == current)
                .map(|candidate| candidate.score)
                .unwrap_or(f32::MIN);
            let tolerance = if target_slope < 0.02 {
                0.42
            } else if target_slope < 0.05 {
                0.28
            } else {
                0.14
            };
            if let Some(alternative) = candidates.into_iter().find(|candidate| {
                candidate.next != current
                    && candidate.direction != current_dir
                    && candidate.score >= current_score - tolerance
            }) {
                downstream[target] = Some(alternative.next);
            }
        }
    }
}

fn direction_for(world: &World, idx: usize, next: usize) -> (isize, isize) {
    let (x, y) = world.coords(idx);
    let (nx, ny) = world.coords(next);
    (
        (nx as isize - x as isize).signum(),
        (ny as isize - y as isize).signum(),
    )
}

fn same_direction_segment(world: &World, downstream: &[Option<usize>], start: usize) -> Vec<usize> {
    let mut current = start;
    let mut direction = None;
    let mut segment = Vec::new();
    let mut guard = 0;

    while guard < world.tiles.len() {
        let Some(next) = downstream[current] else {
            break;
        };
        let dir = direction_for(world, current, next);
        if Some(dir) == direction {
            segment.push(current);
        } else if direction.is_none() {
            direction = Some(dir);
            segment.push(current);
        } else {
            break;
        }
        current = next;
        guard += 1;
    }

    segment
}

fn is_run_start(world: &World, downstream: &[Option<usize>], idx: usize) -> bool {
    let Some(next) = downstream[idx] else {
        return false;
    };
    let (x, y) = world.coords(idx);
    let current_dir = direction_for(world, idx, next);
    for (nx, ny) in world.neighbors8(x, y) {
        let nidx = world.idx(nx, ny);
        if downstream[nidx] == Some(idx) && direction_for(world, nidx, idx) == current_dir {
            return false;
        }
    }
    true
}

fn local_downstream_slope(
    world: &World,
    conditioning: &ConditioningState,
    downstream: &[Option<usize>],
    idx: usize,
) -> f32 {
    let Some(next) = downstream[idx] else {
        return 0.0;
    };
    let (x, y) = world.coords(idx);
    let (nx, ny) = world.coords(next);
    (conditioning.hydro_elevation[idx] - conditioning.hydro_elevation[next]).max(0.0)
        / neighbor_distance(x, y, nx, ny)
}

pub(super) fn local_relief(world: &World, idx: usize, radius: isize) -> f32 {
    let (x, y) = world.coords(idx);
    let current = world.tiles[idx].raw_elevation;
    let mut rise = 0.0_f32;
    let mut count = 0.0_f32;

    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if !world.in_bounds(nx, ny) {
                continue;
            }
            let nidx = world.idx(nx as usize, ny as usize);
            rise += (world.tiles[nidx].raw_elevation - current).abs();
            count += 1.0;
        }
    }

    if count <= f32::EPSILON {
        0.0
    } else {
        rise / count
    }
}

fn mountain_front_factor(world: &World, idx: usize) -> f32 {
    let current = world.tiles[idx].raw_elevation;
    let relief = local_relief(world, idx, 2);
    let highland = smoothstep(world.sea_level + 0.12, world.sea_level + 0.34, current);
    let relief_factor = smoothstep(0.02, 0.11, relief);
    let mut downstream_opening = 0.0_f32;
    let (x, y) = world.coords(idx);
    for (nx, ny) in world.neighbors8(x, y) {
        let nidx = world.idx(nx, ny);
        let drop = (current - world.tiles[nidx].raw_elevation).max(0.0);
        let relief_gap = (relief - local_relief(world, nidx, 2)).max(0.0);
        downstream_opening = downstream_opening
            .max(smoothstep(0.02, 0.12, drop) * smoothstep(0.0, 0.05, relief_gap));
    }
    (highland * relief_factor * downstream_opening).clamp(0.0, 1.0)
}

fn lowland_opening_factor(world: &World, idx: usize) -> f32 {
    let current = world.tiles[idx].raw_elevation;
    let relief = local_relief(world, idx, 2);
    let lowland = 1.0 - smoothstep(world.sea_level + 0.10, world.sea_level + 0.26, current);
    let relief_soft = 1.0 - smoothstep(0.025, 0.09, relief);
    (lowland * relief_soft).clamp(0.0, 1.0)
}

pub(super) fn break_downstream_cycles(
    downstream: &mut [Option<usize>],
    parent: &[Option<usize>],
    ocean: &[bool],
) {
    for _ in 0..4 {
        let mut changed = false;
        for start in 0..downstream.len() {
            if ocean[start] {
                continue;
            }
            let mut path: Vec<usize> = Vec::new();
            let mut path_set: HashSet<usize> = HashSet::new();
            let mut current = start;
            loop {
                if ocean[current] {
                    break;
                }
                if path_set.contains(&current) {
                    if let Some(pos) = path.iter().position(|&idx| idx == current) {
                        for &cycle_idx in &path[pos..] {
                            downstream[cycle_idx] = parent[cycle_idx];
                        }
                        changed = true;
                    }
                    break;
                }
                path_set.insert(current);
                path.push(current);
                match downstream[current] {
                    Some(next) => current = next,
                    None => break,
                }
            }
        }
        if !changed {
            break;
        }
    }
}
