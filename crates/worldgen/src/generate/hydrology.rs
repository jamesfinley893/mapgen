use crate::{LEGACY_WORLD_SIZE, Surface, World};

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, VecDeque};

use super::climate::ClimateFields;
use super::terrain::TerrainFields;
use super::util::{hash01, smoothstep, value_noise};

pub(crate) struct HydrologyFields {
    pub(crate) runoff: Vec<f32>,
    pub(crate) flow_accumulation: Vec<f32>,
    pub(crate) river: Vec<f32>,
    pub(crate) river_depth: Vec<f32>,
    pub(crate) river_width: Vec<f32>,
    pub(crate) river_order: Vec<u8>,
    pub(crate) lake_depth: Vec<f32>,
    pub(crate) water_body_id: Vec<u32>,
    pub(crate) lake_inflow: Vec<f32>,
    pub(crate) lake_outlet: Vec<bool>,
    pub(crate) spill_discharge: Vec<f32>,
    pub(crate) erosion: Vec<f32>,
    pub(crate) flow_direction: Vec<i8>,
}

#[derive(Clone, Copy)]
enum FlowTarget {
    Land { idx: usize, direction: i8 },
    Ocean { direction: i8 },
    Sink,
}

#[derive(Clone, Copy)]
struct QueueCell {
    elevation: f32,
    idx: usize,
}

#[derive(Clone, Copy)]
struct LakeOutlet {
    lake_idx: usize,
    target_idx: Option<usize>,
    score: f32,
}

struct LakeOverflowFields {
    lake_depth: Vec<f32>,
    water_body_id: Vec<u32>,
    lake_inflow: Vec<f32>,
    lake_outlet: Vec<bool>,
    spill_discharge: Vec<f32>,
    raw_spill_discharge: Vec<f32>,
}

struct CapturedFlow {
    flow_accumulation: Vec<f32>,
    lake_inflow_by_body: Vec<f32>,
}

#[derive(Clone, Copy)]
struct SecondaryFlowTarget {
    idx: usize,
    share: f32,
}

struct RiverThresholds {
    stream: f32,
    tributary: f32,
    major: f32,
    bankfull: f32,
}

struct DrainageRegions {
    tile_region: Vec<usize>,
    region_count: usize,
}

#[derive(Clone)]
struct RegionalBudget {
    land_tiles: usize,
    sustained_supply: f32,
    mean_moisture: f32,
    mean_precipitation: f32,
    persistent_channel_cap: usize,
}

struct RegionalWaterBudget {
    runoff: Vec<f32>,
    regions: DrainageRegions,
    budgets: Vec<RegionalBudget>,
}

const PERSISTENT_DEPTH_MIN: f32 = 0.030;
const PERSISTENT_WIDTH_MIN: f32 = 0.040;
const PERSISTENT_AREA_MIN: f32 = 0.046;

pub(super) fn generate_hydrology_fields(
    world: &World,
    terrain: &TerrainFields,
    climate: &ClimateFields,
    ocean: &[bool],
    surfaces: &[Surface],
) -> HydrologyFields {
    let raw_runoff = compute_local_runoff(world, terrain, climate, ocean);
    let filled_elevation = compute_filled_elevation(world, terrain, ocean);
    let mut lake_capacity =
        compute_lake_depth(world, terrain, climate, ocean, surfaces, &filled_elevation);
    let potential_water_body_id = assign_lake_bodies(world, &mut lake_capacity);
    let flow_targets = compute_flow_targets(world, ocean, &filled_elevation);
    let secondary_flow_targets =
        compute_secondary_flow_targets(world, ocean, &filled_elevation, &flow_targets);
    let elevation_order = elevation_order(world, &filled_elevation);
    let drainage_regions = compute_drainage_regions(world, ocean, &flow_targets, &elevation_order);
    let regional_budget = compute_regional_water_budget(
        world,
        terrain,
        climate,
        ocean,
        &raw_runoff,
        drainage_regions,
    );
    let runoff = regional_budget.runoff.clone();
    let captured_flow = accumulate_flow_with_lake_capture(
        world,
        ocean,
        &elevation_order,
        &runoff,
        &flow_targets,
        &secondary_flow_targets,
        &potential_water_body_id,
    );
    let drains_to_ocean = compute_ocean_drainage(world, &elevation_order, &flow_targets);
    let lake_overflow = compute_lake_overflow(
        world,
        terrain,
        climate,
        ocean,
        surfaces,
        &filled_elevation,
        &captured_flow.lake_inflow_by_body,
        &flow_targets,
        &lake_capacity,
        &potential_water_body_id,
        &drains_to_ocean,
    );
    let flow_accumulation = apply_spill_to_flow_accumulation(
        ocean,
        &captured_flow.flow_accumulation,
        &lake_overflow.raw_spill_discharge,
    );
    let (mut river_depth, mut river_width) = compute_river_geometry(
        world,
        terrain,
        surfaces,
        &filled_elevation,
        &flow_targets,
        &flow_accumulation,
        &lake_overflow.lake_depth,
        &lake_overflow.spill_discharge,
    );
    let mut river = compute_river_signal(
        world,
        surfaces,
        &flow_accumulation,
        &river_depth,
        &river_width,
        &lake_overflow.lake_depth,
        &lake_overflow.spill_discharge,
    );
    apply_regional_channel_cap(
        world,
        surfaces,
        &regional_budget,
        &flow_accumulation,
        &lake_overflow.lake_depth,
        &lake_overflow.spill_discharge,
        &mut river,
        &mut river_depth,
        &mut river_width,
    );
    expand_lowland_channel_corridors(
        world,
        terrain,
        surfaces,
        &regional_budget,
        &flow_accumulation,
        &lake_overflow.lake_depth,
        &lake_overflow.spill_discharge,
        &mut river,
        &mut river_depth,
        &mut river_width,
    );
    let river_order = compute_stream_order(world, ocean, &elevation_order, &flow_targets, &river);
    let erosion = compute_erosion(
        world,
        terrain,
        surfaces,
        &flow_accumulation,
        &river,
        &river_depth,
        &river_width,
        &lake_overflow.lake_depth,
        &lake_overflow.lake_inflow,
        &lake_overflow.spill_discharge,
    );
    let flow_direction = compute_flow_directions(&flow_targets);

    HydrologyFields {
        runoff,
        flow_accumulation,
        river,
        river_depth,
        river_width,
        river_order,
        lake_depth: lake_overflow.lake_depth,
        water_body_id: lake_overflow.water_body_id,
        lake_inflow: lake_overflow.lake_inflow,
        lake_outlet: lake_overflow.lake_outlet,
        spill_discharge: lake_overflow.spill_discharge,
        erosion,
        flow_direction,
    }
}

#[cfg(test)]
fn compute_runoff(
    world: &World,
    terrain: &TerrainFields,
    climate: &ClimateFields,
    ocean: &[bool],
) -> Vec<f32> {
    compute_local_runoff(world, terrain, climate, ocean)
}

fn compute_local_runoff(
    world: &World,
    terrain: &TerrainFields,
    climate: &ClimateFields,
    ocean: &[bool],
) -> Vec<f32> {
    let mut runoff = vec![0.0_f32; world.tile_count()];
    let cell_area = world.high_detail_cell_area();

    for idx in 0..world.tile_count() {
        if ocean[idx] {
            continue;
        }

        let height_above_sea = (terrain.elevation[idx] - world.sea_level).max(0.0);
        let slope_wash = smoothstep(0.012, 0.075, terrain.slope[idx]);
        let relief_wash = smoothstep(0.018, 0.095, terrain.relief[idx]);
        let saturation =
            (climate.moisture[idx] * 0.52 + climate.precipitation[idx] * 0.48).clamp(0.0, 1.0);
        let cold_storage = if climate.temperature[idx] < 0.26 && height_above_sea > 0.18 {
            smoothstep(0.18, 0.62, height_above_sea) * (0.26 - climate.temperature[idx]) * 0.55
        } else {
            0.0
        };
        let runoff_efficiency =
            0.14 + saturation * 0.52 + slope_wash * 0.18 + relief_wash * 0.11 + cold_storage;

        runoff[idx] = (climate.precipitation[idx] * runoff_efficiency * cell_area).clamp(0.0, 1.0);
    }

    runoff
}

fn compute_drainage_regions(
    world: &World,
    ocean: &[bool],
    targets: &[FlowTarget],
    elevation_order: &[usize],
) -> DrainageRegions {
    let tile_count = world.tile_count();
    let mut tile_region = vec![usize::MAX; tile_count];
    let mut basin_root = vec![usize::MAX; tile_count];
    let region_size = regional_budget_tile_size(world);
    let mut region_ids = HashMap::<(usize, usize, usize, usize), usize>::new();

    for idx in elevation_order.iter().copied() {
        if ocean[idx] {
            continue;
        }

        basin_root[idx] = match targets[idx] {
            FlowTarget::Ocean { .. } | FlowTarget::Sink => idx,
            FlowTarget::Land { idx: target, .. } => {
                let root = basin_root[target];
                if root == usize::MAX { target } else { root }
            }
        };
    }

    for idx in 0..tile_count {
        if ocean[idx] {
            continue;
        }

        let root = if basin_root[idx] == usize::MAX {
            idx
        } else {
            basin_root[idx]
        };
        let (x, y) = world.coords(idx);
        let (rx, ry) = (x / region_size, y / region_size);
        let (root_x, root_y) = world.coords(root);
        let key = (rx, ry, root_x / region_size, root_y / region_size);
        let next_id = region_ids.len();
        let region = *region_ids.entry(key).or_insert(next_id);
        tile_region[idx] = region;
    }

    DrainageRegions {
        tile_region,
        region_count: region_ids.len(),
    }
}

fn regional_budget_tile_size(world: &World) -> usize {
    let density = world.high_detail_scale();
    let span = world
        .effective_world_size()
        .round()
        .max(world.width.min(world.height).max(1) as f32) as usize;
    let min_region = (6.0 * density).round().max(6.0) as usize;
    let max_region = (48.0 * density).round().max(48.0) as usize;
    (span / 8)
        .clamp(min_region, max_region)
        .min(world.width.max(world.height).max(1))
}

fn compute_regional_water_budget(
    world: &World,
    terrain: &TerrainFields,
    climate: &ClimateFields,
    ocean: &[bool],
    raw_runoff: &[f32],
    regions: DrainageRegions,
) -> RegionalWaterBudget {
    #[derive(Clone, Copy, Default)]
    struct Accum {
        land_tiles: usize,
        raw_runoff: f32,
        sustained_supply: f32,
        moisture: f32,
        precipitation: f32,
        temperature: f32,
        continentality: f32,
        exposure: f32,
    }

    let mut accum = vec![Accum::default(); regions.region_count];
    let cell_area = world.high_detail_cell_area();

    for idx in 0..world.tile_count() {
        if ocean[idx] {
            continue;
        }
        let region = regions.tile_region[idx];
        if region == usize::MAX {
            continue;
        }

        let moisture = climate.moisture[idx].clamp(0.0, 1.0);
        let precipitation = climate.precipitation[idx].clamp(0.0, 1.0);
        let temperature = climate.temperature[idx].clamp(0.0, 1.0);
        let continentality = climate.continentality[idx].clamp(0.0, 1.0);
        let slope_exposure = smoothstep(
            0.040,
            0.180,
            terrain.slope[idx] + terrain.relief[idx] * 0.55,
        );
        let terrain_capture = (0.54
            + smoothstep(0.010, 0.085, terrain.slope[idx]) * 0.14
            + smoothstep(0.020, 0.120, terrain.relief[idx]) * 0.10
            + (1.0 - slope_exposure) * 0.18)
            .clamp(0.36, 1.08);
        let evaporation = (temperature * (0.13 + (1.0 - moisture) * 0.38)
            + (1.0 - moisture) * 0.32
            + continentality * 0.15
            + slope_exposure * 0.10)
            .clamp(0.04, 0.86);
        let supply = precipitation * (0.30 + moisture * 0.72);
        let cold_release = if temperature < 0.25 && terrain.elevation[idx] > world.sea_level + 0.18
        {
            (0.25 - temperature) * 0.18
        } else {
            0.0
        };
        let sustained_supply =
            (supply * terrain_capture * (1.0 - evaporation) + cold_release).clamp(0.0, 1.0);

        let bucket = &mut accum[region];
        bucket.land_tiles += 1;
        bucket.raw_runoff += raw_runoff[idx].max(0.0);
        bucket.sustained_supply += sustained_supply * cell_area;
        bucket.moisture += moisture;
        bucket.precipitation += precipitation;
        bucket.temperature += temperature;
        bucket.continentality += continentality;
        bucket.exposure += slope_exposure;
    }

    let mut runoff = raw_runoff.to_vec();
    let mut runoff_multiplier_by_region = vec![0.0_f32; regions.region_count];
    let mut budgets = vec![
        RegionalBudget {
            land_tiles: 0,
            sustained_supply: 0.0,
            mean_moisture: 0.0,
            mean_precipitation: 0.0,
            persistent_channel_cap: 0,
        };
        regions.region_count
    ];

    for (region, bucket) in accum.iter().copied().enumerate() {
        if bucket.land_tiles == 0 {
            continue;
        }

        let area = bucket.land_tiles as f32;
        let mean_moisture = bucket.moisture / area;
        let mean_precipitation = bucket.precipitation / area;
        let mean_temperature = bucket.temperature / area;
        let mean_continentality = bucket.continentality / area;
        let mean_exposure = bucket.exposure / area;
        let wetness = (mean_moisture * 0.52 + mean_precipitation * 0.48).clamp(0.0, 1.0);
        let aridity_penalty = (1.0 - wetness).powf(1.35);
        let sustained_target = bucket.sustained_supply
            * (0.86 + wetness * 0.18
                - mean_temperature * aridity_penalty * 0.18
                - mean_continentality * aridity_penalty * 0.10
                - mean_exposure * aridity_penalty * 0.08)
                .clamp(0.28, 1.12);
        let runoff_multiplier = if bucket.raw_runoff > 0.0 {
            (sustained_target / bucket.raw_runoff).clamp(0.035, 1.24)
        } else {
            0.0
        };
        runoff_multiplier_by_region[region] = runoff_multiplier;

        let physical_area = (area * cell_area).max(1.0);
        let supply_per_tile = bucket.sustained_supply / physical_area;
        let persistence =
            smoothstep(0.10, 0.72, wetness).max(smoothstep(0.012, 0.24, supply_per_tile) * 0.86);
        let channel_fraction = (0.010 + persistence * 0.245 + wetness * 0.030).clamp(0.010, 0.31);
        let minimum_channels = if wetness >= 0.46 {
            area.sqrt().ceil() as usize + 3
        } else if wetness >= 0.28 {
            (area.sqrt() * 0.55).ceil() as usize + 1
        } else {
            (area.sqrt() * 0.22).ceil() as usize
        };
        let persistent_channel_cap =
            ((area * channel_fraction).round() as usize).max(minimum_channels);

        budgets[region] = RegionalBudget {
            land_tiles: bucket.land_tiles,
            sustained_supply: bucket.sustained_supply,
            mean_moisture,
            mean_precipitation,
            persistent_channel_cap,
        };
    }

    for idx in 0..world.tile_count() {
        if ocean[idx] {
            runoff[idx] = 0.0;
            continue;
        }
        let region = regions.tile_region[idx];
        if region == usize::MAX {
            runoff[idx] = 0.0;
            continue;
        }
        runoff[idx] = (runoff[idx] * runoff_multiplier_by_region[region]).clamp(0.0, 1.0);
    }

    RegionalWaterBudget {
        runoff,
        regions,
        budgets,
    }
}

fn compute_filled_elevation(world: &World, terrain: &TerrainFields, ocean: &[bool]) -> Vec<f32> {
    let mut filled = terrain.elevation.clone();
    let mut visited = vec![false; world.tile_count()];
    let mut queue = BinaryHeap::new();

    for (idx, is_ocean) in ocean.iter().copied().enumerate() {
        if !is_ocean {
            continue;
        }
        visited[idx] = true;
        filled[idx] = world.sea_level.min(terrain.elevation[idx]);
        queue.push(QueueCell {
            elevation: filled[idx],
            idx,
        });
    }

    if queue.is_empty() {
        return filled;
    }

    while let Some(cell) = queue.pop() {
        let (x, y) = world.coords(cell.idx);
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            if visited[nidx] {
                continue;
            }
            visited[nidx] = true;

            let spill = cell.elevation + 0.0001 / world.high_detail_scale();
            filled[nidx] = if ocean[nidx] {
                world.sea_level.min(terrain.elevation[nidx])
            } else {
                terrain.elevation[nidx].max(spill)
            };
            queue.push(QueueCell {
                elevation: filled[nidx],
                idx: nidx,
            });
        }
    }

    filled
}

fn compute_flow_targets(
    world: &World,
    ocean: &[bool],
    filled_elevation: &[f32],
) -> Vec<FlowTarget> {
    let mut targets = vec![FlowTarget::Sink; world.tile_count()];
    let gradient_scale = world.high_detail_scale();

    for idx in 0..world.tile_count() {
        if ocean[idx] {
            continue;
        }

        let current = filled_elevation[idx];
        let (x, y) = world.coords(idx);
        let mut best = FlowTarget::Sink;
        let mut best_score = 0.0_f32;
        let mut best_land_gradient = 0.0_f32;

        for (dx, dy, direction) in FLOW_NEIGHBORS {
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if !world.in_bounds(nx, ny) {
                continue;
            }

            let nidx = world.idx(nx as usize, ny as usize);
            let distance = if dx != 0 && dy != 0 {
                std::f32::consts::SQRT_2
            } else {
                1.0
            };

            if ocean[nidx] {
                let drop = (current - world.sea_level).max(0.0) + 0.006;
                let score = drop / distance * gradient_scale;
                if score > best_score {
                    best_score = score;
                    best = FlowTarget::Ocean { direction };
                }
                continue;
            }

            let neighbor = filled_elevation[nidx];
            let drop = current - neighbor;
            if drop <= 0.00001 {
                continue;
            }

            let gradient = drop / distance * gradient_scale;
            best_land_gradient = best_land_gradient.max(gradient);
        }

        for (dx, dy, direction) in FLOW_NEIGHBORS {
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if !world.in_bounds(nx, ny) {
                continue;
            }

            let nidx = world.idx(nx as usize, ny as usize);
            if ocean[nidx] {
                continue;
            }

            let distance = if dx != 0 && dy != 0 {
                std::f32::consts::SQRT_2
            } else {
                1.0
            };
            let neighbor = filled_elevation[nidx];
            let drop = current - neighbor;
            if drop <= 0.00001 {
                continue;
            }

            let gradient = drop / distance * gradient_scale;
            let low_gradient = 1.0 - smoothstep(0.00018, 0.006, gradient);
            let near_best = near_best_downhill_candidate(best_land_gradient, gradient);
            let route_bias = low_gradient_route_bias(world, x, y, dx, dy, direction, gradient);
            let score = gradient + route_bias * low_gradient * near_best;
            if score > best_score {
                best_score = score;
                best = FlowTarget::Land {
                    idx: nidx,
                    direction,
                };
            }
        }

        targets[idx] = best;
    }

    targets
}

fn near_best_downhill_candidate(best_gradient: f32, gradient: f32) -> f32 {
    if best_gradient <= 0.0 || gradient <= 0.0 {
        return 0.0;
    }

    let deficit = (best_gradient - gradient).max(0.0);
    1.0 - smoothstep(
        best_gradient * 0.08 + 0.000015,
        best_gradient * 0.44 + 0.000180,
        deficit,
    )
}

fn low_gradient_route_bias(
    world: &World,
    x: usize,
    y: usize,
    dx: isize,
    dy: isize,
    direction: i8,
    gradient: f32,
) -> f32 {
    let density = world.high_detail_scale();
    let base_scale = (world.effective_world_size() / 18.0).round().max(1.0) as usize;
    let min_scale = (7.0 * density).round().max(7.0) as usize;
    let max_scale = (31.0 * density).round().max(31.0) as usize;
    let scale = base_scale.clamp(min_scale, max_scale);
    let vx = value_noise(world.seed ^ 0x6CA1_33B5_7E91_22A7, x, y, scale) * 2.0 - 1.0;
    let vy = value_noise(world.seed ^ 0xB28D_F413_09AC_4E7B, x, y, scale) * 2.0 - 1.0;
    let meander_scale = ((scale as f32 * 0.42).round() as usize).clamp(
        (5.0 * density).round().max(5.0) as usize,
        (17.0 * density).round().max(17.0) as usize,
    );
    let mx = value_noise(
        world.seed ^ 0x11D3_5EED_A631_49C5,
        x.wrapping_add(direction.max(0) as usize * 3),
        y,
        meander_scale,
    ) * 2.0
        - 1.0;
    let my = value_noise(
        world.seed ^ 0xE6E0_3A7B_5C17_D91D,
        x,
        y.wrapping_add(direction.max(0) as usize * 5),
        meander_scale,
    ) * 2.0
        - 1.0;
    let turn_phase = value_noise(
        world.seed ^ 0x7A5A_93D2_6B12_C04F,
        x.wrapping_add(y / meander_scale.max(1)),
        y.wrapping_add(x / meander_scale.max(1)),
        meander_scale,
    ) * 2.0
        - 1.0;
    let distance = if dx != 0 && dy != 0 {
        std::f32::consts::SQRT_2
    } else {
        1.0
    };
    let dir_x = dx as f32 / distance;
    let dir_y = dy as f32 / distance;
    let vector_len = (vx * vx + vy * vy).sqrt().max(0.001);
    let planform_alignment = ((vx * dir_x + vy * dir_y) / vector_len).clamp(-1.0, 1.0) * 0.5 + 0.5;
    let meander_len = (mx * mx + my * my).sqrt().max(0.001);
    let meander_alignment = ((mx * dir_x + my * dir_y) / meander_len).clamp(-1.0, 1.0) * 0.5 + 0.5;
    let cross = (dir_x * vy - dir_y * vx).clamp(-1.0, 1.0);
    let turn_preference = (cross * turn_phase).clamp(-1.0, 1.0) * 0.5 + 0.5;
    let local_jitter = hash01(
        world.seed ^ 0xA5E1_F10D_7723_4B91,
        x.wrapping_mul(17).wrapping_add(direction.max(0) as usize),
        y.wrapping_mul(17).wrapping_add(direction.max(0) as usize),
    );
    let tie_breaker = planform_alignment * 0.50
        + meander_alignment * 0.22
        + turn_preference * 0.16
        + local_jitter * 0.12;
    let amplitude = (gradient * 0.56 + 0.000075).min(0.00105);

    tie_breaker * amplitude
}

fn compute_secondary_flow_targets(
    world: &World,
    ocean: &[bool],
    filled_elevation: &[f32],
    primary_targets: &[FlowTarget],
) -> Vec<Option<SecondaryFlowTarget>> {
    let mut secondary = vec![None; world.tile_count()];

    for idx in 0..world.tile_count() {
        if ocean[idx] {
            continue;
        }

        let FlowTarget::Land {
            idx: primary_idx, ..
        } = primary_targets[idx]
        else {
            continue;
        };

        let current = filled_elevation[idx];
        let primary_gradient = flow_target_gradient(world, filled_elevation, idx, primary_idx);
        let low_gradient = 1.0 - smoothstep(0.00018, 0.006, primary_gradient);
        if low_gradient <= 0.0 {
            continue;
        }

        let (x, y) = world.coords(idx);
        let mut best_idx = None;
        let mut best_score = 0.0_f32;

        for (dx, dy, direction) in FLOW_NEIGHBORS {
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if !world.in_bounds(nx, ny) {
                continue;
            }

            let nidx = world.idx(nx as usize, ny as usize);
            if nidx == primary_idx || ocean[nidx] {
                continue;
            }

            let distance = if dx != 0 && dy != 0 {
                std::f32::consts::SQRT_2
            } else {
                1.0
            };
            let drop = current - filled_elevation[nidx];
            if drop <= 0.00001 {
                continue;
            }

            let gradient = drop / distance * world.high_detail_scale();
            let near_best = near_best_downhill_candidate(primary_gradient, gradient);
            if near_best <= 0.0 {
                continue;
            }

            let planform =
                low_gradient_route_bias(world, x, y, dx, dy, direction, gradient) / 0.00078;
            let gradient_ratio = (gradient / primary_gradient.max(0.00001)).clamp(0.0, 1.0);
            let score = near_best * (gradient_ratio * 0.72 + planform * 0.28);
            if score > best_score {
                best_score = score;
                best_idx = Some(nidx);
            }
        }

        if let Some(target) = best_idx {
            let share = (low_gradient * best_score * 0.20).clamp(0.0, 0.22);
            if share >= 0.035 {
                secondary[idx] = Some(SecondaryFlowTarget { idx: target, share });
            }
        }
    }

    secondary
}

fn flow_target_gradient(world: &World, filled_elevation: &[f32], idx: usize, target: usize) -> f32 {
    let drop = (filled_elevation[idx] - filled_elevation[target]).max(0.00001);
    drop / distance_between_indices(world, idx, target) * world.high_detail_scale()
}

fn compute_lake_depth(
    world: &World,
    terrain: &TerrainFields,
    climate: &ClimateFields,
    ocean: &[bool],
    surfaces: &[Surface],
    filled_elevation: &[f32],
) -> Vec<f32> {
    let mut lake_depth = vec![0.0_f32; world.tile_count()];

    for idx in 0..world.tile_count() {
        if ocean[idx] || surfaces[idx] == Surface::Ocean {
            continue;
        }

        let depression_depth = filled_elevation[idx] - terrain.elevation[idx];
        if depression_depth <= 0.006 {
            continue;
        }

        let height_above_sea = (terrain.elevation[idx] - world.sea_level).max(0.0);
        if height_above_sea <= 0.002 {
            continue;
        }

        let roughness = (terrain.slope[idx] * 0.55 + terrain.relief[idx] * 0.55).clamp(0.0, 1.0);
        let basin_floor = 1.0 - smoothstep(0.030, 0.140, roughness);
        let water_supply =
            (climate.moisture[idx] * 0.48 + climate.precipitation[idx] * 0.52).clamp(0.0, 1.0);
        let storage = smoothstep(0.006, 0.060, depression_depth);
        let retained = storage * (0.52 + water_supply * 0.34 + basin_floor * 0.28);

        if retained > 0.22 {
            lake_depth[idx] =
                (retained * smoothstep(0.004, 0.080, depression_depth)).clamp(0.0, 1.0);
        }
    }

    lake_depth
}

fn assign_lake_bodies(world: &World, lake_depth: &mut [f32]) -> Vec<u32> {
    let mut water_body_id = vec![0_u32; world.tile_count()];
    let mut visited = vec![false; world.tile_count()];
    let mut next_id = 1_u32;
    let min_body_tiles = scaled_area_tiles(world, 3);

    for idx in 0..world.tile_count() {
        if visited[idx] || lake_depth[idx] <= 0.0 {
            continue;
        }

        let mut queue = VecDeque::new();
        let mut body = Vec::new();
        visited[idx] = true;
        queue.push_back(idx);

        while let Some(current) = queue.pop_front() {
            body.push(current);

            for nidx in world.neighbor_indices8(current) {
                if visited[nidx] || lake_depth[nidx] <= 0.0 {
                    continue;
                }
                visited[nidx] = true;
                queue.push_back(nidx);
            }
        }

        // Drop sub-minimum depressions outright: at the current tile density they
        // are terrain-noise dimples, not lakes. Their runoff routes over the
        // filled elevation instead of ponding as stray pixels of water.
        if body.len() < min_body_tiles {
            for body_idx in body {
                lake_depth[body_idx] = 0.0;
            }
            continue;
        }

        for body_idx in body {
            water_body_id[body_idx] = next_id;
        }
        next_id = next_id.saturating_add(1);
    }

    water_body_id
}

fn compute_lake_overflow(
    world: &World,
    terrain: &TerrainFields,
    climate: &ClimateFields,
    ocean: &[bool],
    surfaces: &[Surface],
    filled_elevation: &[f32],
    lake_inflow_by_body: &[f32],
    targets: &[FlowTarget],
    lake_capacity: &[f32],
    potential_water_body_id: &[u32],
    drains_to_ocean: &[bool],
) -> LakeOverflowFields {
    let tile_count = world.tile_count();
    let mut fields = LakeOverflowFields {
        lake_depth: vec![0.0; tile_count],
        water_body_id: vec![0; tile_count],
        lake_inflow: vec![0.0; tile_count],
        lake_outlet: vec![false; tile_count],
        spill_discharge: vec![0.0; tile_count],
        raw_spill_discharge: vec![0.0; tile_count],
    };
    let max_body_id = potential_water_body_id.iter().copied().max().unwrap_or(0) as usize;
    if max_body_id == 0 {
        return fields;
    }

    let mut body_tiles = vec![Vec::<usize>::new(); max_body_id + 1];
    let mut body_capacity = vec![0.0_f32; max_body_id + 1];
    let mut body_max_capacity = vec![0.0_f32; max_body_id + 1];
    let mut body_temperature = vec![0.0_f32; max_body_id + 1];
    let mut body_moisture = vec![0.0_f32; max_body_id + 1];
    let mut body_outlet = vec![None; max_body_id + 1];

    for idx in 0..tile_count {
        let body_id = potential_water_body_id[idx] as usize;
        if body_id == 0 {
            continue;
        }
        body_tiles[body_id].push(idx);
        body_capacity[body_id] += lake_capacity[idx];
        body_max_capacity[body_id] = body_max_capacity[body_id].max(lake_capacity[idx]);
        body_temperature[body_id] += climate.temperature[idx];
        body_moisture[body_id] += climate.moisture[idx];
    }

    for body_id in 1..=max_body_id {
        for lake_idx in body_tiles[body_id].iter().copied() {
            let (x, y) = world.coords(lake_idx);
            for (nx, ny) in world.neighbors8(x, y) {
                let nidx = world.idx(nx, ny);
                if potential_water_body_id[nidx] as usize == body_id {
                    continue;
                }

                let target_idx = if ocean[nidx] || surfaces[nidx] == Surface::Ocean {
                    None
                } else {
                    if potential_water_body_id[nidx] != 0 || !drains_to_ocean[nidx] {
                        continue;
                    }
                    Some(nidx)
                };

                let dx = x.abs_diff(nx);
                let dy = y.abs_diff(ny);
                let distance = if dx == 1 && dy == 1 {
                    std::f32::consts::SQRT_2
                } else {
                    1.0
                };
                let neighbor_level = target_idx
                    .map(|idx| filled_elevation[idx])
                    .unwrap_or(world.sea_level);
                let spill_drop = (filled_elevation[lake_idx] - neighbor_level).max(0.0);
                let saddle = terrain.elevation[lake_idx].max(neighbor_level);
                let tie_breaker =
                    hash01(world.seed ^ 0xC01D_A11E_51A2_9037, lake_idx, nidx) * 0.00008;
                let score = saddle + distance * 0.00022 - spill_drop * 0.018 + tie_breaker;

                if body_outlet[body_id]
                    .map(|outlet: LakeOutlet| score < outlet.score)
                    .unwrap_or(true)
                {
                    body_outlet[body_id] = Some(LakeOutlet {
                        lake_idx,
                        target_idx,
                        score,
                    });
                }
            }
        }
    }

    let thresholds = river_thresholds(world);
    for body_id in 1..=max_body_id {
        if body_tiles[body_id].is_empty() {
            continue;
        }

        let area = body_tiles[body_id].len() as f32;
        let physical_area = area * world.high_detail_cell_area();
        let inflow = lake_inflow_by_body.get(body_id).copied().unwrap_or(0.0);
        let temperature = body_temperature[body_id] / area.max(1.0);
        let moisture = body_moisture[body_id] / area.max(1.0);
        let evaporative_loss = physical_area
            * (0.0025 + temperature * 0.0060 + (1.0 - moisture).clamp(0.0, 1.0) * 0.0075);
        let storage_ratio = if evaporative_loss <= 0.0 {
            1.0
        } else {
            (inflow / evaporative_loss).clamp(0.0, 1.0).powf(0.72)
        };
        let wet_stage = if inflow > evaporative_loss {
            1.0
        } else {
            storage_ratio
        };
        if wet_stage <= 0.10 && body_max_capacity[body_id] < 0.50 {
            continue;
        }

        let inflow_signal = normalize_discharge(inflow, &thresholds);
        for idx in body_tiles[body_id].iter().copied() {
            let staged_depth = lake_capacity[idx] * wet_stage;
            let basin_signal = smoothstep(0.018, 0.74, staged_depth);
            if basin_signal <= 0.0 {
                continue;
            }
            fields.lake_depth[idx] = (basin_signal * (0.38 + wet_stage * 0.62)).clamp(0.0, 1.0);
            fields.water_body_id[idx] = body_id as u32;
            fields.lake_inflow[idx] = inflow_signal;
        }

        let Some(outlet) = body_outlet[body_id] else {
            continue;
        };

        let spill = (inflow - evaporative_loss).max(0.0);
        if spill <= 0.0 {
            continue;
        }

        fields.lake_outlet[outlet.lake_idx] = true;
        fields.raw_spill_discharge[outlet.lake_idx] += spill;
        if let Some(target_idx) = outlet.target_idx {
            propagate_spill_discharge(
                world,
                ocean,
                targets,
                &fields.lake_depth,
                target_idx,
                spill,
                &mut fields.raw_spill_discharge,
                &mut fields.lake_outlet,
            );
        }
    }

    for idx in 0..tile_count {
        fields.spill_discharge[idx] =
            normalize_discharge(fields.raw_spill_discharge[idx], &thresholds);
    }

    fields
}

fn propagate_spill_discharge(
    world: &World,
    ocean: &[bool],
    targets: &[FlowTarget],
    lake_depth: &[f32],
    start_idx: usize,
    spill: f32,
    raw_spill_discharge: &mut [f32],
    lake_outlet: &mut [bool],
) {
    let mut seen = vec![false; world.tile_count()];
    let mut idx = start_idx;

    let limit = world
        .tile_count()
        .min((2048.0 * world.high_detail_scale()).round() as usize);
    for _ in 0..limit {
        if seen[idx] || ocean[idx] {
            break;
        }
        seen[idx] = true;

        raw_spill_discharge[idx] += spill;
        lake_outlet[idx] = true;
        if lake_depth[idx] > 0.0 {
            break;
        }

        match targets[idx] {
            FlowTarget::Land { idx: target, .. } => idx = target,
            FlowTarget::Ocean { .. } | FlowTarget::Sink => break,
        }
    }
}

fn accumulate_flow_with_lake_capture(
    world: &World,
    ocean: &[bool],
    elevation_order: &[usize],
    runoff: &[f32],
    targets: &[FlowTarget],
    secondary_targets: &[Option<SecondaryFlowTarget>],
    water_body_id: &[u32],
) -> CapturedFlow {
    let mut flow = runoff.to_vec();
    let max_body_id = water_body_id.iter().copied().max().unwrap_or(0) as usize;
    let mut lake_inflow_by_body = vec![0.0_f32; max_body_id + 1];

    for idx in elevation_order.iter().rev().copied() {
        if ocean[idx] {
            continue;
        }

        let body_id = water_body_id[idx] as usize;
        if body_id > 0 {
            lake_inflow_by_body[body_id] += runoff[idx];
            flow[idx] = 0.0;
            continue;
        }

        if let FlowTarget::Land { idx: target, .. } = targets[idx] {
            let captured = flow[idx];
            let secondary = secondary_targets[idx]
                .map(|target| (target.idx, captured * target.share))
                .filter(|(_, amount)| *amount > 0.0);
            let primary_amount = captured - secondary.map(|(_, amount)| amount).unwrap_or(0.0);

            route_captured_flow(
                &mut flow,
                &mut lake_inflow_by_body,
                water_body_id,
                target,
                primary_amount,
            );
            if let Some((target, amount)) = secondary {
                route_captured_flow(
                    &mut flow,
                    &mut lake_inflow_by_body,
                    water_body_id,
                    target,
                    amount,
                );
            }
        }
    }

    for (idx, is_ocean) in ocean.iter().copied().enumerate() {
        if is_ocean || water_body_id[idx] > 0 {
            flow[idx] = 0.0;
        }
    }

    debug_assert_eq!(flow.len(), world.tile_count());

    CapturedFlow {
        flow_accumulation: flow,
        lake_inflow_by_body,
    }
}

fn route_captured_flow(
    flow: &mut [f32],
    lake_inflow_by_body: &mut [f32],
    water_body_id: &[u32],
    target: usize,
    amount: f32,
) {
    if amount <= 0.0 {
        return;
    }

    let target_body = water_body_id[target] as usize;
    if target_body > 0 {
        lake_inflow_by_body[target_body] += amount;
    } else {
        flow[target] += amount;
    }
}

fn apply_spill_to_flow_accumulation(
    ocean: &[bool],
    base_flow_accumulation: &[f32],
    raw_spill_discharge: &[f32],
) -> Vec<f32> {
    base_flow_accumulation
        .iter()
        .zip(raw_spill_discharge.iter())
        .zip(ocean.iter())
        .map(
            |((flow, spill), is_ocean)| {
                if *is_ocean { 0.0 } else { flow + spill }
            },
        )
        .collect()
}

fn compute_ocean_drainage(
    world: &World,
    elevation_order: &[usize],
    targets: &[FlowTarget],
) -> Vec<bool> {
    let mut drains_to_ocean = vec![false; world.tile_count()];

    for idx in elevation_order.iter().copied() {
        drains_to_ocean[idx] = match targets[idx] {
            FlowTarget::Ocean { .. } => true,
            FlowTarget::Land { idx: target, .. } => drains_to_ocean[target],
            FlowTarget::Sink => false,
        };
    }

    drains_to_ocean
}

fn compute_river_signal(
    world: &World,
    surfaces: &[Surface],
    flow_accumulation: &[f32],
    river_depth: &[f32],
    river_width: &[f32],
    lake_depth: &[f32],
    spill_discharge: &[f32],
) -> Vec<f32> {
    let mut river = vec![0.0_f32; world.tile_count()];
    let thresholds = river_thresholds(world);

    for idx in 0..world.tile_count() {
        if surfaces[idx] == Surface::Ocean || lake_depth[idx] > 0.0 {
            continue;
        }

        let discharge_signal = normalize_discharge(flow_accumulation[idx], &thresholds);
        let channel_area = (river_width[idx] * 0.62 + river_depth[idx] * 0.38).clamp(0.0, 1.0);
        let persistent_channel = river_depth[idx] >= PERSISTENT_DEPTH_MIN
            && river_width[idx] >= PERSISTENT_WIDTH_MIN
            && channel_area >= PERSISTENT_AREA_MIN;
        let spill_channel = spill_discharge[idx] >= 0.018 && channel_area >= 0.026;
        if !persistent_channel && !spill_channel {
            continue;
        }

        river[idx] = channel_area
            .max(if persistent_channel {
                discharge_signal * 0.34
            } else {
                0.0
            })
            .max(spill_discharge[idx] * 0.72)
            .clamp(0.0, 1.0);
    }

    river
}

fn apply_regional_channel_cap(
    world: &World,
    surfaces: &[Surface],
    regional_budget: &RegionalWaterBudget,
    flow_accumulation: &[f32],
    lake_depth: &[f32],
    spill_discharge: &[f32],
    river: &mut [f32],
    river_depth: &mut [f32],
    river_width: &mut [f32],
) {
    let mut candidates_by_region = vec![Vec::<usize>::new(); regional_budget.regions.region_count];

    for idx in 0..world.tile_count() {
        if surfaces[idx] == Surface::Ocean || lake_depth[idx] > 0.0 || river[idx] <= 0.0 {
            continue;
        }

        let region = regional_budget.regions.tile_region[idx];
        if region == usize::MAX {
            suppress_channel(idx, river, river_depth, river_width);
            continue;
        }

        let budget = &regional_budget.budgets[region];
        let wetness =
            (budget.mean_moisture * 0.52 + budget.mean_precipitation * 0.48).clamp(0.0, 1.0);
        let dryness = 1.0 - smoothstep(0.22, 0.56, wetness);
        let supply_per_tile = if budget.land_tiles > 0 {
            budget.sustained_supply / budget.land_tiles as f32
        } else {
            0.0
        };
        let dry_region_signal_floor = (0.032 + dryness * 0.090
            - smoothstep(0.08, 0.30, supply_per_tile) * 0.026)
            .clamp(0.030, 0.122);
        let lake_spill = spill_discharge[idx] > 0.018;
        if !lake_spill && river[idx] < dry_region_signal_floor {
            suppress_channel(idx, river, river_depth, river_width);
            continue;
        }

        if !lake_spill {
            candidates_by_region[region].push(idx);
        }
    }

    for (region, candidates) in candidates_by_region.iter_mut().enumerate() {
        let cap = regional_budget
            .budgets
            .get(region)
            .map(|budget| budget.persistent_channel_cap)
            .unwrap_or(0);
        if candidates.len() <= cap {
            continue;
        }

        candidates.sort_unstable_by(|left, right| {
            channel_priority(*right, flow_accumulation, river, river_depth, river_width)
                .total_cmp(&channel_priority(
                    *left,
                    flow_accumulation,
                    river,
                    river_depth,
                    river_width,
                ))
                .then_with(|| left.cmp(right))
        });

        for idx in candidates.iter().skip(cap).copied() {
            suppress_channel(idx, river, river_depth, river_width);
        }
    }
}

fn channel_priority(
    idx: usize,
    flow_accumulation: &[f32],
    river: &[f32],
    river_depth: &[f32],
    river_width: &[f32],
) -> f32 {
    flow_accumulation[idx].max(0.0) * (0.58 + river_depth[idx] * 0.24 + river_width[idx] * 0.18)
        + river[idx] * 0.34
}

fn suppress_channel(
    idx: usize,
    river: &mut [f32],
    river_depth: &mut [f32],
    river_width: &mut [f32],
) {
    river[idx] = 0.0;
    river_depth[idx] = 0.0;
    river_width[idx] = 0.0;
}

fn expand_lowland_channel_corridors(
    world: &World,
    terrain: &TerrainFields,
    surfaces: &[Surface],
    regional_budget: &RegionalWaterBudget,
    flow_accumulation: &[f32],
    lake_depth: &[f32],
    spill_discharge: &[f32],
    river: &mut [f32],
    river_depth: &mut [f32],
    river_width: &mut [f32],
) {
    #[derive(Clone, Copy, Default)]
    struct CorridorPaint {
        river: f32,
        depth: f32,
        width: f32,
    }

    let thresholds = river_thresholds(world);
    let mut paints = vec![CorridorPaint::default(); world.tile_count()];

    for idx in 0..world.tile_count() {
        if surfaces[idx] == Surface::Ocean
            || lake_depth[idx] > 0.0
            || river[idx] <= 0.0
            || river_width[idx] < 0.46
            || river_depth[idx] < 0.12
        {
            continue;
        }

        let discharge = flow_accumulation[idx].max(0.0);
        if discharge < thresholds.major * 0.46 && spill_discharge[idx] < 0.10 {
            continue;
        }

        let channel_slope = (terrain.slope[idx] + terrain.relief[idx] * 0.42).clamp(0.0, 1.0);
        let low_gradient = 1.0 - smoothstep(0.016, 0.105, channel_slope);
        let valley_open = 1.0
            - smoothstep(
                0.040,
                0.180,
                terrain.relief[idx] + terrain.slope[idx] * 0.42,
            );
        let region = regional_budget.regions.tile_region[idx];
        let wetness = regional_budget
            .budgets
            .get(region)
            .map(|budget| {
                (budget.mean_moisture * 0.52 + budget.mean_precipitation * 0.48).clamp(0.0, 1.0)
            })
            .unwrap_or(0.0);
        let corridor_signal = low_gradient * valley_open * smoothstep(0.28, 0.62, wetness);
        if corridor_signal <= 0.0 {
            continue;
        }

        let radius = ((0.84
            + smoothstep(0.46, 1.0, river_width[idx]) * 1.34
            + smoothstep(0.14, 0.80, river_depth[idx]) * 0.42
            + corridor_signal * 0.52)
            .clamp(0.84, 2.65)
            * world.high_detail_scale())
        .clamp(0.84, 2.65 * world.high_detail_scale());
        let reach = radius.ceil() as isize;
        let (x, y) = world.coords(idx);

        for dy in -reach..=reach {
            for dx in -reach..=reach {
                if dx == 0 && dy == 0 {
                    continue;
                }

                let tx = x as isize + dx;
                let ty = y as isize + dy;
                if !world.in_bounds(tx, ty) {
                    continue;
                }

                let target_idx = world.idx(tx as usize, ty as usize);
                if surfaces[target_idx] == Surface::Ocean || lake_depth[target_idx] > 0.0 {
                    continue;
                }
                if regional_budget.regions.tile_region[target_idx] != region {
                    continue;
                }

                let distance = ((dx * dx + dy * dy) as f32).sqrt();
                if distance > radius {
                    continue;
                }

                let target_rugged =
                    (terrain.relief[target_idx] + terrain.slope[target_idx] * 0.44).clamp(0.0, 1.0);
                let valley_acceptance = 1.0 - smoothstep(0.055, 0.210, target_rugged);
                if valley_acceptance <= 0.0 {
                    continue;
                }

                let elevation_delta = terrain.elevation[target_idx] - terrain.elevation[idx];
                let bankfull_tolerance = 0.012 + river_width[idx] * 0.028 + low_gradient * 0.020;
                let bank_acceptance = 1.0
                    - smoothstep(
                        bankfull_tolerance * 0.45,
                        bankfull_tolerance,
                        elevation_delta.max(0.0),
                    );
                if bank_acceptance <= 0.0 {
                    continue;
                }

                let coverage = 1.0 - smoothstep(radius * 0.34, radius, distance);
                if coverage <= 0.0 {
                    continue;
                }

                let strength = (coverage * valley_acceptance * bank_acceptance * corridor_signal)
                    .clamp(0.0, 1.0);
                if strength <= 0.045 {
                    continue;
                }

                let side_width = (river_width[idx] * (0.50 + strength * 0.54)).clamp(0.0, 1.0);
                let side_depth = (river_depth[idx] * (0.26 + strength * 0.38)).clamp(0.0, 1.0);
                let side_area = (side_width * 0.62 + side_depth * 0.38).clamp(0.0, 1.0);
                if side_area < PERSISTENT_AREA_MIN * 0.88 {
                    continue;
                }

                let paint = &mut paints[target_idx];
                paint.width = paint.width.max(side_width);
                paint.depth = paint.depth.max(side_depth);
                paint.river = paint.river.max(side_area.max(river[idx] * strength * 0.72));
            }
        }
    }

    for (idx, paint) in paints.into_iter().enumerate() {
        if paint.river <= 0.0 {
            continue;
        }

        river_width[idx] = river_width[idx].max(paint.width);
        river_depth[idx] = river_depth[idx].max(paint.depth);
        river[idx] = river[idx].max(paint.river).clamp(0.0, 1.0);
    }
}

fn compute_stream_order(
    world: &World,
    ocean: &[bool],
    elevation_order: &[usize],
    targets: &[FlowTarget],
    river: &[f32],
) -> Vec<u8> {
    let mut order = vec![0_u8; world.tile_count()];
    let mut incoming_max = vec![0_u8; world.tile_count()];
    let mut incoming_count = vec![0_u8; world.tile_count()];

    for idx in elevation_order.iter().rev().copied() {
        if ocean[idx] {
            continue;
        }

        let mut current = incoming_max[idx];
        if river[idx] > 0.035 {
            current = current.max(1);
            order[idx] = current;
        }

        if current == 0 {
            continue;
        }

        if let FlowTarget::Land { idx: target, .. } = targets[idx] {
            match current.cmp(&incoming_max[target]) {
                Ordering::Greater => {
                    incoming_max[target] = current;
                    incoming_count[target] = 1;
                }
                Ordering::Equal => {
                    incoming_count[target] = incoming_count[target].saturating_add(1);
                    if incoming_count[target] >= 2 {
                        incoming_max[target] = incoming_max[target].saturating_add(1);
                        incoming_count[target] = 1;
                    }
                }
                Ordering::Less => {}
            }
        }
    }

    order
}

fn compute_erosion(
    world: &World,
    terrain: &TerrainFields,
    surfaces: &[Surface],
    flow_accumulation: &[f32],
    river: &[f32],
    river_depth: &[f32],
    river_width: &[f32],
    lake_depth: &[f32],
    lake_inflow: &[f32],
    spill_discharge: &[f32],
) -> Vec<f32> {
    let mut erosion = vec![0.0_f32; world.tile_count()];
    let thresholds = river_thresholds(world);

    for idx in 0..world.tile_count() {
        if surfaces[idx] == Surface::Ocean {
            continue;
        }

        if lake_depth[idx] > 0.0 {
            erosion[idx] =
                (lake_depth[idx] * 0.30 + lake_inflow[idx] * 0.22 + spill_discharge[idx] * 0.24)
                    .clamp(0.0, 1.0);
            continue;
        }

        if river[idx] <= 0.025 && spill_discharge[idx] <= 0.0 {
            continue;
        }

        let discharge = (flow_accumulation[idx] / thresholds.bankfull)
            .max(0.0)
            .sqrt()
            .clamp(0.0, 2.6);
        let gradient = (terrain.slope[idx] + terrain.relief[idx] * 0.55).clamp(0.0, 1.0);
        let slope_power = smoothstep(0.004, 0.135, gradient);
        let low_gradient = 1.0 - smoothstep(0.020, 0.120, gradient);
        let stream_power = (flow_accumulation[idx] / thresholds.major.max(1.0))
            .max(0.0)
            .powf(0.52)
            * (0.36 + slope_power * 0.76)
            * (river_depth[idx] * 0.62 + river_width[idx] * 0.38);
        let lowland_meander =
            low_gradient * river_width[idx] * (0.10 + discharge * 0.34).clamp(0.0, 0.46);
        let outlet_scour = spill_discharge[idx] * (0.16 + slope_power * 0.18);

        erosion[idx] = (stream_power * 0.68 + lowland_meander + outlet_scour).clamp(0.0, 1.0);
    }

    erosion
}

fn compute_river_geometry(
    world: &World,
    terrain: &TerrainFields,
    surfaces: &[Surface],
    filled_elevation: &[f32],
    targets: &[FlowTarget],
    flow_accumulation: &[f32],
    lake_depth: &[f32],
    spill_discharge: &[f32],
) -> (Vec<f32>, Vec<f32>) {
    let mut depth = vec![0.0_f32; world.tile_count()];
    let mut width = vec![0.0_f32; world.tile_count()];
    let thresholds = river_thresholds(world);
    let (incoming_count, incoming_flow) = incoming_flow_stats(world, targets, flow_accumulation);

    for idx in 0..world.tile_count() {
        if surfaces[idx] == Surface::Ocean || lake_depth[idx] > 0.0 {
            continue;
        }

        let discharge = flow_accumulation[idx].max(0.0);
        if discharge <= thresholds.stream * 0.08 && spill_discharge[idx] <= 0.0 {
            continue;
        }
        let active_channel = smoothstep(
            thresholds.stream * 0.28,
            thresholds.stream * 1.15,
            discharge,
        )
        .max(smoothstep(0.015, 0.24, spill_discharge[idx]));
        let channel_slope = flow_slope(world, terrain, filled_elevation, targets, idx);
        let slope_factor = smoothstep(0.0012, 0.035, channel_slope);
        let low_gradient =
            1.0 - smoothstep(0.0025, 0.030, channel_slope + terrain.slope[idx] * 0.18);
        let valley_open = 1.0
            - smoothstep(
                0.035,
                0.160,
                terrain.relief[idx] + terrain.slope[idx] * 0.38,
            );
        let confinement = (1.0 - valley_open).clamp(0.0, 1.0);
        let q_major = (discharge / thresholds.major.max(1.0)).max(0.0);
        let q_bankfull = (discharge / thresholds.bankfull.max(1.0)).max(0.0);

        let raw_width = 0.90
            + discharge.powf(0.55)
                * (0.34 + low_gradient * 0.42 + valley_open * 0.26).clamp(0.32, 1.20);
        width[idx] = (active_channel
            * (0.070
                + q_major.powf(0.50).min(1.4) * 0.34
                + q_bankfull.powf(0.42).min(1.2) * 0.31
                + low_gradient * 0.15
                + spill_discharge[idx] * 0.12))
            .clamp(0.0, 1.0);

        let hydraulic_depth = discharge / raw_width.max(0.1) * (0.76 + low_gradient * 0.58)
            / (0.92 + slope_factor * 0.52);
        let confluence = if incoming_count[idx] >= 2 {
            smoothstep(
                discharge * 0.20,
                discharge * 0.74 + 0.01,
                incoming_flow[idx],
            )
        } else {
            0.0
        };
        let mouth = matches!(targets[idx], FlowTarget::Ocean { .. }) as u8 as f32;
        let sustained_depth = smoothstep(
            thresholds.major * 0.72,
            thresholds.bankfull * 0.78,
            discharge,
        );
        let confined_depth = confinement
            * smoothstep(thresholds.tributary, thresholds.major * 1.35, discharge)
            * (1.0 - width[idx] * 0.34).clamp(0.46, 1.0);
        let depth_ratio = (hydraulic_depth / (thresholds.major * 0.30).max(0.20))
            .max(0.0)
            .powf(0.45);
        depth[idx] = (active_channel
            * (0.072
                + depth_ratio.min(1.42) * 0.52
                + low_gradient * 0.075
                + slope_factor * 0.055
                + sustained_depth * 0.125
                + confined_depth * 0.085
                + confluence * 0.15
                + mouth * 0.105
                + spill_discharge[idx] * 0.13))
            .clamp(0.0, 1.0);
    }

    (depth, width)
}

#[cfg(test)]
fn compute_river_width(
    world: &World,
    terrain: &TerrainFields,
    surfaces: &[Surface],
    flow_accumulation: &[f32],
) -> Vec<f32> {
    let filled_elevation = terrain.elevation.clone();
    let targets = vec![FlowTarget::Sink; world.tile_count()];
    let lake_depth = vec![0.0; world.tile_count()];
    let spill_discharge = vec![0.0; world.tile_count()];
    compute_river_geometry(
        world,
        terrain,
        surfaces,
        &filled_elevation,
        &targets,
        flow_accumulation,
        &lake_depth,
        &spill_discharge,
    )
    .1
}

fn incoming_flow_stats(
    world: &World,
    targets: &[FlowTarget],
    flow_accumulation: &[f32],
) -> (Vec<u8>, Vec<f32>) {
    let mut count = vec![0_u8; world.tile_count()];
    let mut flow = vec![0.0_f32; world.tile_count()];
    let thresholds = river_thresholds(world);

    for idx in 0..world.tile_count() {
        if flow_accumulation[idx] <= thresholds.stream * 0.35 {
            continue;
        }

        if let FlowTarget::Land { idx: target, .. } = targets[idx] {
            count[target] = count[target].saturating_add(1);
            flow[target] += flow_accumulation[idx];
        }
    }

    (count, flow)
}

fn flow_slope(
    world: &World,
    terrain: &TerrainFields,
    filled_elevation: &[f32],
    targets: &[FlowTarget],
    idx: usize,
) -> f32 {
    match targets[idx] {
        FlowTarget::Land { idx: target, .. } => {
            let drop = (filled_elevation[idx] - filled_elevation[target]).max(0.00004);
            drop / distance_between_indices(world, idx, target) * world.high_detail_scale()
        }
        FlowTarget::Ocean { .. } => {
            (filled_elevation[idx] - world.sea_level).max(0.00004) * world.high_detail_scale()
        }
        FlowTarget::Sink => terrain.slope[idx].max(0.00004),
    }
}

fn distance_between_indices(world: &World, a: usize, b: usize) -> f32 {
    let (ax, ay) = world.coords(a);
    let (bx, by) = world.coords(b);
    if ax != bx && ay != by {
        std::f32::consts::SQRT_2
    } else {
        1.0
    }
}

fn river_thresholds(world: &World) -> RiverThresholds {
    let ws = world.effective_world_size().max(1.0);
    let world_units_x = world.width as f32 / ws;
    let world_units_y = world.height as f32 / ws;
    let world_area = (world_units_x * world_units_y).max(1.0 / ws.max(1.0));
    let reference_density = ws.min(LEGACY_WORLD_SIZE as f32);
    let scale = (reference_density * world_area.sqrt()).max(1.0);
    let stream = (scale * 0.045).max(0.35);
    let tributary = (scale * 0.120).max(stream * 1.9);
    let major = (scale * 0.360).max(tributary * 2.3);
    let bankfull = (scale * 1.550).max(major * 3.2);

    RiverThresholds {
        stream,
        tributary,
        major,
        bankfull,
    }
}

fn scaled_area_tiles(world: &World, base_tiles: usize) -> usize {
    (base_tiles as f32 * world.high_detail_scale().powi(2))
        .round()
        .max(base_tiles as f32) as usize
}

fn normalize_discharge(discharge: f32, thresholds: &RiverThresholds) -> f32 {
    if discharge <= thresholds.stream * 0.25 {
        return 0.0;
    }

    let stream = smoothstep(thresholds.stream * 0.25, thresholds.tributary, discharge) * 0.24;
    let tributary = smoothstep(thresholds.tributary, thresholds.major, discharge) * 0.34;
    let major = smoothstep(thresholds.major, thresholds.bankfull, discharge) * 0.42;
    (stream + tributary + major).clamp(0.0, 1.0)
}

fn compute_flow_directions(targets: &[FlowTarget]) -> Vec<i8> {
    targets
        .iter()
        .map(|target| match *target {
            FlowTarget::Land { direction, .. } | FlowTarget::Ocean { direction, .. } => direction,
            FlowTarget::Sink => -1,
        })
        .collect()
}

fn elevation_order(world: &World, elevation: &[f32]) -> Vec<usize> {
    let mut order = (0..world.tile_count()).collect::<Vec<_>>();
    order.sort_unstable_by(|a, b| elevation[*a].total_cmp(&elevation[*b]));
    order
}

const FLOW_NEIGHBORS: [(isize, isize, i8); 8] = [
    (-1, -1, 7),
    (0, -1, 0),
    (1, -1, 1),
    (-1, 0, 6),
    (1, 0, 2),
    (-1, 1, 5),
    (0, 1, 4),
    (1, 1, 3),
];

impl Eq for QueueCell {}

impl PartialEq for QueueCell {
    fn eq(&self, other: &Self) -> bool {
        self.idx == other.idx && self.elevation.to_bits() == other.elevation.to_bits()
    }
}

impl Ord for QueueCell {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .elevation
            .total_cmp(&self.elevation)
            .then_with(|| other.idx.cmp(&self.idx))
    }
}

impl PartialOrd for QueueCell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::World;

    #[test]
    fn flow_targets_store_direction_for_land_and_ocean_outlets() {
        let world = World::new(11, 3, 3, 0.50, 0);
        let center = world.idx(1, 1);
        let east = world.idx(2, 1);
        let west = world.idx(0, 1);
        let mut elevation = vec![0.90_f32; world.tile_count()];
        elevation[center] = 0.82;
        elevation[east] = 0.30;
        elevation[west] = 0.70;

        let land_ocean = vec![false; world.tile_count()];
        let land_targets = compute_flow_targets(&world, &land_ocean, &elevation);
        let land_directions = compute_flow_directions(&land_targets);
        assert!(matches!(
            land_targets[center],
            FlowTarget::Land {
                idx,
                direction: 2
            } if idx == east
        ));
        assert_eq!(land_directions[center], 2);

        let mut ocean = vec![false; world.tile_count()];
        ocean[west] = true;
        let mut outlet_elevation = elevation;
        outlet_elevation[east] = 0.78;
        let ocean_targets = compute_flow_targets(&world, &ocean, &outlet_elevation);
        let ocean_directions = compute_flow_directions(&ocean_targets);
        assert!(matches!(
            ocean_targets[center],
            FlowTarget::Ocean { direction: 6 }
        ));
        assert_eq!(ocean_directions[center], 6);
    }

    #[test]
    fn river_width_follows_bankfull_discharge() {
        let world = World::new(11, 5, 1, 0.50, 0);
        let terrain = TerrainFields {
            elevation: vec![0.62; world.tile_count()],
            slope: vec![0.012; world.tile_count()],
            relief: vec![0.016; world.tile_count()],
        };
        let surfaces = vec![Surface::Land; world.tile_count()];
        let flow_accumulation = vec![0.4, 1.2, 3.0, 7.0, 12.0];
        let width = compute_river_width(&world, &terrain, &surfaces, &flow_accumulation);

        assert!(
            width[3] > width[1] + 0.34,
            "bankfull channel did not widen enough: small={} large={}",
            width[1],
            width[3]
        );
        assert!(
            width[3] > 0.78,
            "bankfull discharge should export a broad channel width: {:?}",
            width
        );
        assert!(
            width[4] >= width[3],
            "width should not shrink as discharge increases: {:?}",
            width
        );
    }

    #[test]
    fn sustained_major_discharge_exports_clearer_depth_range() {
        let world = World::new(11, 5, 1, 0.50, 0);
        let terrain = TerrainFields {
            elevation: vec![0.68, 0.64, 0.60, 0.56, 0.40],
            slope: vec![0.012; world.tile_count()],
            relief: vec![0.016; world.tile_count()],
        };
        let surfaces = vec![
            Surface::Land,
            Surface::Land,
            Surface::Land,
            Surface::Coast,
            Surface::Ocean,
        ];
        let ocean = vec![false, false, false, false, true];
        let filled = terrain.elevation.clone();
        let targets = compute_flow_targets(&world, &ocean, &filled);
        let thresholds = river_thresholds(&world);
        let lake_depth = vec![0.0; world.tile_count()];
        let spill_discharge = vec![0.0; world.tile_count()];

        let tributary_flow = vec![thresholds.tributary * 1.08; world.tile_count()];
        let major_flow = vec![thresholds.bankfull * 0.72; world.tile_count()];
        let (tributary_depth, _) = compute_river_geometry(
            &world,
            &terrain,
            &surfaces,
            &filled,
            &targets,
            &tributary_flow,
            &lake_depth,
            &spill_discharge,
        );
        let (major_depth, _) = compute_river_geometry(
            &world,
            &terrain,
            &surfaces,
            &filled,
            &targets,
            &major_flow,
            &lake_depth,
            &spill_discharge,
        );

        assert!(
            major_depth[2] > tributary_depth[2] + 0.24,
            "major sustained discharge should export visibly deeper normalized channels: tributary={} major={}",
            tributary_depth[2],
            major_depth[2]
        );
        assert!(
            major_depth[2] > 0.62,
            "major sustained discharge should reach the deep-water render regime: {:?}",
            major_depth
        );
    }

    #[test]
    fn priority_flood_depressions_become_numbered_lakes() {
        let world = World::new(11, 7, 7, 0.50, 0);
        let mut elevation = vec![0.82_f32; world.tile_count()];
        let mut ocean = vec![false; world.tile_count()];
        let mut surfaces = vec![Surface::Land; world.tile_count()];

        for y in 0..world.height {
            for x in 0..world.width {
                let idx = world.idx(x, y);
                if x == 0 || y == 0 || x == world.width - 1 || y == world.height - 1 {
                    elevation[idx] = 0.32;
                    ocean[idx] = true;
                    surfaces[idx] = Surface::Ocean;
                }
            }
        }
        for (x, y) in [(3, 3), (3, 2), (2, 3), (4, 3)] {
            elevation[world.idx(x, y)] = 0.58;
        }

        let terrain = TerrainFields {
            elevation,
            slope: vec![0.010; world.tile_count()],
            relief: vec![0.014; world.tile_count()],
        };
        let climate = ClimateFields {
            temperature: vec![0.55; world.tile_count()],
            moisture: vec![0.78; world.tile_count()],
            precipitation: vec![0.82; world.tile_count()],
            ocean_distance: vec![1; world.tile_count()],
            continentality: vec![0.45; world.tile_count()],
        };

        let filled = compute_filled_elevation(&world, &terrain, &ocean);
        let mut lake_depth =
            compute_lake_depth(&world, &terrain, &climate, &ocean, &surfaces, &filled);
        let water_body_id = assign_lake_bodies(&world, &mut lake_depth);
        let center = world.idx(3, 3);

        assert!(
            lake_depth[center] > 0.35,
            "depression did not retain enough lake water: {}",
            lake_depth[center]
        );
        assert!(
            water_body_id[center] > 0,
            "lake depression was not assigned a water body id"
        );
        assert_eq!(water_body_id[world.idx(3, 2)], water_body_id[center]);
    }

    #[test]
    fn high_inflow_lakes_spill_into_downstream_channels() {
        let (world, terrain, climate, ocean, surfaces) = lake_overflow_fixture();
        let runoff = compute_runoff(&world, &terrain, &climate, &ocean);
        let filled = compute_filled_elevation(&world, &terrain, &ocean);
        let mut lake_capacity =
            compute_lake_depth(&world, &terrain, &climate, &ocean, &surfaces, &filled);
        let water_body_id = assign_lake_bodies(&world, &mut lake_capacity);
        let targets = compute_flow_targets(&world, &ocean, &filled);
        let secondary_targets = compute_secondary_flow_targets(&world, &ocean, &filled, &targets);
        let order = elevation_order(&world, &filled);
        let captured_flow = accumulate_flow_with_lake_capture(
            &world,
            &ocean,
            &order,
            &runoff,
            &targets,
            &secondary_targets,
            &water_body_id,
        );
        let base_flow = captured_flow.flow_accumulation;
        let drains_to_ocean = compute_ocean_drainage(&world, &order, &targets);
        let overflow = compute_lake_overflow(
            &world,
            &terrain,
            &climate,
            &ocean,
            &surfaces,
            &filled,
            &captured_flow.lake_inflow_by_body,
            &targets,
            &lake_capacity,
            &water_body_id,
            &drains_to_ocean,
        );
        let final_flow =
            apply_spill_to_flow_accumulation(&ocean, &base_flow, &overflow.raw_spill_discharge);

        let lake_outlet = (0..world.tile_count())
            .find(|idx| overflow.lake_depth[*idx] > 0.0 && overflow.lake_outlet[*idx])
            .expect("high-inflow lake did not mark an outlet edge");
        let spill_channel = (0..world.tile_count())
            .find(|idx| {
                overflow.lake_depth[*idx] <= 0.0
                    && overflow.raw_spill_discharge[*idx] > 0.0
                    && !ocean[*idx]
            })
            .expect("lake spill did not enter a downstream land channel");

        assert!(overflow.spill_discharge[lake_outlet] > 0.0);
        assert!(
            final_flow[spill_channel] > base_flow[spill_channel] + 0.05,
            "spill channel discharge did not increase: base={} final={}",
            base_flow[spill_channel],
            final_flow[spill_channel]
        );
    }

    #[test]
    fn tributaries_below_major_threshold_keep_order_depth_and_width() {
        let world = World::new(11, 5, 1, 0.50, 0);
        let terrain = TerrainFields {
            elevation: vec![0.64, 0.62, 0.60, 0.58, 0.40],
            slope: vec![0.010; world.tile_count()],
            relief: vec![0.014; world.tile_count()],
        };
        let surfaces = vec![
            Surface::Land,
            Surface::Land,
            Surface::Land,
            Surface::Coast,
            Surface::Ocean,
        ];
        let ocean = vec![false, false, false, false, true];
        let filled = terrain.elevation.clone();
        let targets = compute_flow_targets(&world, &ocean, &filled);
        let order = elevation_order(&world, &filled);
        let thresholds = river_thresholds(&world);
        let mut flow_accumulation = vec![0.0; world.tile_count()];
        for idx in 0..4 {
            flow_accumulation[idx] = thresholds.tributary * 1.12;
        }
        let lake_depth = vec![0.0; world.tile_count()];
        let spill_discharge = vec![0.0; world.tile_count()];
        let (depth, width) = compute_river_geometry(
            &world,
            &terrain,
            &surfaces,
            &filled,
            &targets,
            &flow_accumulation,
            &lake_depth,
            &spill_discharge,
        );
        let river = compute_river_signal(
            &world,
            &surfaces,
            &flow_accumulation,
            &depth,
            &width,
            &lake_depth,
            &spill_discharge,
        );
        let stream_order = compute_stream_order(&world, &ocean, &order, &targets, &river);

        assert!(
            river[1] > 0.035 && river[1] < 0.70,
            "tributary should be visible but not major: {}",
            river[1]
        );
        assert!(stream_order[1] > 0, "tributary lost stream order");
        assert!(depth[1] > 0.0, "tributary lost hydraulic depth");
        assert!(width[1] > 0.0, "tributary lost bankfull width");
    }

    #[test]
    fn regional_budget_makes_wet_regions_sustain_more_channels_than_dry_regions() {
        let (world, terrain, ocean, surfaces) = sloped_channel_fixture();
        let dry_climate = ClimateFields {
            temperature: vec![0.86; world.tile_count()],
            moisture: vec![0.07; world.tile_count()],
            precipitation: vec![0.07; world.tile_count()],
            ocean_distance: vec![4; world.tile_count()],
            continentality: vec![0.88; world.tile_count()],
        };
        let wet_climate = ClimateFields {
            temperature: vec![0.44; world.tile_count()],
            moisture: vec![0.88; world.tile_count()],
            precipitation: vec![0.92; world.tile_count()],
            ocean_distance: vec![4; world.tile_count()],
            continentality: vec![0.28; world.tile_count()],
        };

        let dry = generate_hydrology_fields(&world, &terrain, &dry_climate, &ocean, &surfaces);
        let wet = generate_hydrology_fields(&world, &terrain, &wet_climate, &ocean, &surfaces);
        let dry_channels = dry.river.iter().filter(|river| **river > 0.035).count();
        let wet_channels = wet.river.iter().filter(|river| **river > 0.035).count();
        let dry_max_discharge = dry
            .flow_accumulation
            .iter()
            .copied()
            .fold(0.0_f32, f32::max);
        let wet_max_discharge = wet
            .flow_accumulation
            .iter()
            .copied()
            .fold(0.0_f32, f32::max);

        assert!(
            wet_channels > dry_channels + world.height,
            "wet region should sustain more visible channels: wet={wet_channels} dry={dry_channels}"
        );
        assert!(
            wet_max_discharge > dry_max_discharge * 4.0,
            "regional budget did not materially increase wet sustained discharge: wet={wet_max_discharge} dry={dry_max_discharge}"
        );
    }

    #[test]
    fn lakes_only_spill_after_inflow_exceeds_evaporation_and_storage_loss() {
        let (world, terrain, climate, ocean, surfaces) = lake_overflow_fixture();
        let filled = compute_filled_elevation(&world, &terrain, &ocean);
        let mut lake_capacity =
            compute_lake_depth(&world, &terrain, &climate, &ocean, &surfaces, &filled);
        let water_body_id = assign_lake_bodies(&world, &mut lake_capacity);
        let targets = compute_flow_targets(&world, &ocean, &filled);
        let order = elevation_order(&world, &filled);
        let drains_to_ocean = compute_ocean_drainage(&world, &order, &targets);
        let max_body_id = water_body_id.iter().copied().max().unwrap_or(0) as usize;
        assert!(max_body_id > 0, "fixture did not produce a lake body");

        let mut low_inflow = vec![0.0_f32; max_body_id + 1];
        for inflow in low_inflow.iter_mut().skip(1) {
            *inflow = 0.001;
        }
        let low = compute_lake_overflow(
            &world,
            &terrain,
            &climate,
            &ocean,
            &surfaces,
            &filled,
            &low_inflow,
            &targets,
            &lake_capacity,
            &water_body_id,
            &drains_to_ocean,
        );
        assert!(
            low.raw_spill_discharge.iter().all(|spill| *spill == 0.0),
            "lake spilled despite sub-evaporative inflow"
        );

        let mut high_inflow = vec![0.0_f32; max_body_id + 1];
        for inflow in high_inflow.iter_mut().skip(1) {
            *inflow = 3.0;
        }
        let high = compute_lake_overflow(
            &world,
            &terrain,
            &climate,
            &ocean,
            &surfaces,
            &filled,
            &high_inflow,
            &targets,
            &lake_capacity,
            &water_body_id,
            &drains_to_ocean,
        );
        assert!(
            high.raw_spill_discharge.iter().any(|spill| *spill > 0.0),
            "lake did not spill after surplus inflow exceeded losses"
        );
    }

    #[test]
    fn river_export_is_zero_below_persistent_depth_and_width() {
        let world = World::new(11, 3, 1, 0.50, 0);
        let surfaces = vec![Surface::Land; world.tile_count()];
        let flow_accumulation = vec![10.0; world.tile_count()];
        let river_depth = vec![PERSISTENT_DEPTH_MIN * 0.50; world.tile_count()];
        let river_width = vec![PERSISTENT_WIDTH_MIN * 0.50; world.tile_count()];
        let lake_depth = vec![0.0; world.tile_count()];
        let spill_discharge = vec![0.0; world.tile_count()];

        let river = compute_river_signal(
            &world,
            &surfaces,
            &flow_accumulation,
            &river_depth,
            &river_width,
            &lake_depth,
            &spill_discharge,
        );

        assert!(
            river.iter().all(|value| *value == 0.0),
            "classification thresholds leaked into non-persistent river export: {:?}",
            river
        );
    }

    #[test]
    fn stream_order_increases_after_equal_order_confluences() {
        let world = World::new(11, 3, 3, 0.50, 0);
        let west = world.idx(0, 1);
        let north = world.idx(1, 0);
        let center = world.idx(1, 1);
        let east = world.idx(2, 1);
        let mut elevation = vec![0.80_f32; world.tile_count()];
        elevation[west] = 0.72;
        elevation[north] = 0.72;
        elevation[center] = 0.62;
        elevation[east] = 0.30;
        let mut ocean = vec![false; world.tile_count()];
        ocean[east] = true;
        let mut targets = vec![FlowTarget::Sink; world.tile_count()];
        targets[west] = FlowTarget::Land {
            idx: center,
            direction: 2,
        };
        targets[north] = FlowTarget::Land {
            idx: center,
            direction: 4,
        };
        targets[center] = FlowTarget::Ocean { direction: 2 };
        let order = elevation_order(&world, &elevation);
        let mut river = vec![0.0; world.tile_count()];
        river[west] = 0.24;
        river[north] = 0.24;
        river[center] = 0.30;

        let stream_order = compute_stream_order(&world, &ocean, &order, &targets, &river);

        assert_eq!(stream_order[west], 1);
        assert_eq!(stream_order[north], 1);
        assert!(
            stream_order[center] >= 2,
            "confluence did not increase stream order: {:?}",
            stream_order
        );
    }

    fn lake_overflow_fixture() -> (World, TerrainFields, ClimateFields, Vec<bool>, Vec<Surface>) {
        let world = World::new(11, 9, 7, 0.50, 0);
        let mut elevation = vec![0.76_f32; world.tile_count()];
        let mut ocean = vec![false; world.tile_count()];
        let mut surfaces = vec![Surface::Land; world.tile_count()];

        for y in 0..world.height {
            let idx = world.idx(8, y);
            elevation[idx] = 0.34;
            ocean[idx] = true;
            surfaces[idx] = Surface::Ocean;
        }

        for (x, y, h) in [
            (2, 3, 0.58),
            (3, 2, 0.58),
            (3, 3, 0.56),
            (3, 4, 0.58),
            (4, 3, 0.58),
            (5, 3, 0.63),
            (6, 3, 0.56),
            (7, 3, 0.52),
        ] {
            elevation[world.idx(x, y)] = h;
        }

        let terrain = TerrainFields {
            elevation,
            slope: vec![0.010; world.tile_count()],
            relief: vec![0.014; world.tile_count()],
        };
        let climate = ClimateFields {
            temperature: vec![0.58; world.tile_count()],
            moisture: vec![0.92; world.tile_count()],
            precipitation: vec![0.95; world.tile_count()],
            ocean_distance: vec![1; world.tile_count()],
            continentality: vec![0.42; world.tile_count()],
        };

        (world, terrain, climate, ocean, surfaces)
    }

    fn sloped_channel_fixture() -> (World, TerrainFields, Vec<bool>, Vec<Surface>) {
        let world = World::new(11, 16, 6, 0.50, 0);
        let mut elevation = vec![0.68_f32; world.tile_count()];
        let mut ocean = vec![false; world.tile_count()];
        let mut surfaces = vec![Surface::Land; world.tile_count()];

        for y in 0..world.height {
            for x in 0..world.width {
                let idx = world.idx(x, y);
                elevation[idx] = 0.76 - x as f32 * 0.018 + (y as f32 - 2.5).abs() * 0.004;
                if x == world.width - 1 {
                    elevation[idx] = 0.34;
                    ocean[idx] = true;
                    surfaces[idx] = Surface::Ocean;
                } else if x == world.width - 2 {
                    surfaces[idx] = Surface::Coast;
                }
            }
        }

        let terrain = TerrainFields {
            elevation,
            slope: vec![0.018; world.tile_count()],
            relief: vec![0.024; world.tile_count()],
        };

        (world, terrain, ocean, surfaces)
    }
}
