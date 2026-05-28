use std::cmp::Ordering;

mod conditioning;
mod flow;
mod lakes;
mod ocean;
mod routing;
mod surfaces;

use crate::{Surface, World, WorldConfig};

use super::util::neighbor_distance;

pub(super) use ocean::classify_ocean;

use conditioning::condition_terrain;
use flow::{
    accumulate_contributing_area, accumulate_discharge, assign_basin_ids, compute_runoff,
    compute_stream_power, flow_accumulation_order,
};
use lakes::identify_lakes;
use routing::{break_downstream_cycles, build_downstream};
use surfaces::{assign_channel_order, classify_surfaces, suppress_short_weak_channels};

pub(super) struct HydrologyState {
    pub(super) hydro_elevation: Vec<f32>,
    pub(super) downstream: Vec<Option<usize>>,
    pub(super) contributing_area: Vec<f32>,
    pub(super) runoff: Vec<f32>,
    pub(super) discharge: Vec<f32>,
    pub(super) stream_power: Vec<f32>,
    pub(super) channel_order: Vec<u8>,
    pub(super) surfaces: Vec<Surface>,
    pub(super) lake_id: Vec<Option<u32>>,
    pub(super) water_level: Vec<Option<f32>>,
    pub(super) basin_id: Vec<Option<u32>>,
}

struct RiverThresholds {
    stream: f32,
    secondary: f32,
    trunk: f32,
}

#[derive(Clone, Copy, Debug)]
struct QueueCell {
    level: f32,
    idx: usize,
}

struct ConditioningState {
    hydro_elevation: Vec<f32>,
    fill_depth: Vec<f32>,
    parent: Vec<Option<usize>>,
    rank: Vec<usize>,
}

struct LakeData {
    lake_id: Vec<Option<u32>>,
    water_level: Vec<Option<f32>>,
    lake_count: u32,
}

#[derive(Clone, Copy)]
struct RoutingCandidate {
    next: usize,
    score: f32,
    slope: f32,
    direction: (isize, isize),
    mountain_front: f32,
}

impl PartialEq for QueueCell {
    fn eq(&self, other: &Self) -> bool {
        self.idx == other.idx && self.level.to_bits() == other.level.to_bits()
    }
}

impl Eq for QueueCell {}

impl Ord for QueueCell {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .level
            .total_cmp(&self.level)
            .then_with(|| other.idx.cmp(&self.idx))
    }
}

impl PartialOrd for QueueCell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub(super) fn simulate_hydrology(
    world: &World,
    config: &WorldConfig,
    ocean: &[bool],
) -> HydrologyState {
    let conditioning = condition_terrain(world, ocean);
    let provisional = identify_lakes(
        world,
        ocean,
        &conditioning.hydro_elevation,
        &conditioning.fill_depth,
        &conditioning.parent,
    );
    let mut downstream = build_downstream(world, ocean, &conditioning, &provisional.lake_id);
    break_downstream_cycles(&mut downstream, &conditioning.parent, ocean);
    let accumulation_order = flow_accumulation_order(&conditioning);
    let contributing_area = accumulate_contributing_area(&accumulation_order, &downstream, ocean);
    let runoff = compute_runoff(world, config, ocean, &conditioning, &downstream);
    let discharge = accumulate_discharge(&accumulation_order, &downstream, ocean, &runoff);
    let stream_power = compute_stream_power(world, &conditioning, &downstream, &discharge);
    let basin_id = assign_basin_ids(
        world,
        ocean,
        &downstream,
        &provisional.lake_id,
        provisional.lake_count,
    );
    let mut surfaces = classify_surfaces(
        world,
        config,
        ocean,
        &contributing_area,
        &discharge,
        &stream_power,
        &provisional.lake_id,
    );
    suppress_short_weak_channels(world, &mut surfaces, &downstream, &stream_power);
    let channel_order = assign_channel_order(world, &surfaces, &discharge);

    HydrologyState {
        hydro_elevation: conditioning.hydro_elevation,
        downstream,
        contributing_area,
        runoff,
        discharge,
        stream_power,
        channel_order,
        surfaces,
        lake_id: provisional.lake_id,
        water_level: provisional.water_level,
        basin_id,
    }
}

pub(super) fn apply_channel_carving(world: &mut World, hydrology: &HydrologyState) {
    let thresholds = river_thresholds(world);

    for idx in 0..world.tiles.len() {
        if hydrology.surfaces[idx] != Surface::River {
            continue;
        }
        let discharge = hydrology.discharge[idx];
        let ratio = (discharge / thresholds.stream).max(1.0);
        let band_multiplier = if discharge >= thresholds.trunk {
            1.75
        } else if discharge >= thresholds.secondary {
            1.25
        } else {
            1.0
        };
        let local_slope = hydrology.downstream[idx]
            .map(|next| {
                let (x, y) = world.coords(idx);
                let (nx, ny) = world.coords(next);
                (hydrology.hydro_elevation[idx] - hydrology.hydro_elevation[next]).max(0.0)
                    / neighbor_distance(x, y, nx, ny)
            })
            .unwrap_or(0.0);
        let slope_factor = if local_slope < 0.008 { 1.25 } else { 0.95 };
        let carve = (0.0045 + ratio.ln() * 0.0135) * band_multiplier * slope_factor;
        let carve = carve.clamp(0.0, 0.085);
        world.tiles[idx].raw_elevation = (world.tiles[idx].raw_elevation - carve).max(0.0);

        let (x, y) = world.coords(idx);
        let neighbors: Vec<_> = world.neighbors8(x, y).collect();
        for (nx, ny) in neighbors {
            let nidx = world.idx(nx, ny);
            if hydrology.surfaces[nidx] == Surface::Ocean {
                continue;
            }
            let distance = neighbor_distance(x, y, nx, ny);
            let neighbor_relief =
                (world.tiles[nidx].raw_elevation - world.tiles[idx].raw_elevation).max(0.0);
            let side_factor = if hydrology.surfaces[nidx] == Surface::River {
                0.38
            } else if distance > 1.0 {
                0.12
            } else {
                0.22
            };
            let relief_factor = (0.5 + neighbor_relief * 1.8).clamp(0.5, 1.4);
            let lateral_carve = carve * side_factor * relief_factor;
            world.tiles[nidx].raw_elevation =
                (world.tiles[nidx].raw_elevation - lateral_carve).max(0.0);
        }
    }
}

pub(super) fn apply_hydrology_to_world(
    world: &mut World,
    ocean: &[bool],
    hydrology: &HydrologyState,
) {
    for (idx, is_ocean) in ocean.iter().copied().enumerate().take(world.tiles.len()) {
        world.tiles[idx].hydro_elevation = hydrology.hydro_elevation[idx];
        world.tiles[idx].contributing_area = hydrology.contributing_area[idx];
        world.tiles[idx].runoff = hydrology.runoff[idx];
        world.tiles[idx].discharge = hydrology.discharge[idx];
        world.tiles[idx].stream_power = hydrology.stream_power[idx];
        world.tiles[idx].channel_order = hydrology.channel_order[idx];
        world.tiles[idx].downstream = hydrology.downstream[idx];
        world.tiles[idx].surface = hydrology.surfaces[idx];
        world.tiles[idx].basin_id = hydrology.basin_id[idx];
        world.tiles[idx].lake_id = hydrology.lake_id[idx];
        world.tiles[idx].water_level = hydrology.water_level[idx];
        if is_ocean {
            world.tiles[idx].water_level = Some(world.sea_level);
        }
    }
}

fn river_thresholds(world: &World) -> RiverThresholds {
    let ws = world.effective_world_size();
    let stream = (ws * ws * 0.00075).max(12.0);
    RiverThresholds {
        stream,
        secondary: stream * 6.5,
        trunk: stream * 18.0,
    }
}
