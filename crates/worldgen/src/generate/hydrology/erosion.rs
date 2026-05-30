use crate::{Surface, World};

use super::{HydrologyState, channel_thresholds};
use crate::generate::util::{neighbor_distance, smoothstep};

pub(crate) const HYDROLOGY_EROSION_CYCLES: usize = 8;

const ACTIVITY_DECAY: f32 = 0.84;
const DISCHARGE_DECAY: f32 = 0.88;
const DEPTH_DECAY: f32 = 0.90;
const WIDTH_DECAY: f32 = 0.91;
const TRUNK_DECAY: f32 = 0.86;
const TRIBUTARY_DECAY: f32 = 0.82;
const TRUNK_WIDTH_RANGE_256: (f32, f32) = (10.0, 18.0);
const SECONDARY_WIDTH_RANGE_256: (f32, f32) = (5.0, 9.0);
const TRIBUTARY_WIDTH_RANGE_256: (f32, f32) = (2.0, 5.0);

#[derive(Clone, Debug)]
pub(crate) struct ValleyErosion {
    activity: Vec<f32>,
    long_term_discharge: Vec<f32>,
    valley_depth: Vec<f32>,
    valley_width: Vec<f32>,
    trunk_strength: Vec<f32>,
    tributary_strength: Vec<f32>,
}

impl ValleyErosion {
    pub(crate) fn new(len: usize) -> Self {
        Self {
            activity: vec![0.0; len],
            long_term_discharge: vec![0.0; len],
            valley_depth: vec![0.0; len],
            valley_width: vec![0.0; len],
            trunk_strength: vec![0.0; len],
            tributary_strength: vec![0.0; len],
        }
    }

    pub(crate) fn activity(&self, idx: usize) -> f32 {
        self.activity.get(idx).copied().unwrap_or(0.0)
    }

    pub(crate) fn long_term_discharge(&self, idx: usize) -> f32 {
        self.long_term_discharge.get(idx).copied().unwrap_or(0.0)
    }

    pub(crate) fn valley_depth(&self, idx: usize) -> f32 {
        self.valley_depth.get(idx).copied().unwrap_or(0.0)
    }

    pub(crate) fn valley_width(&self, idx: usize) -> f32 {
        self.valley_width.get(idx).copied().unwrap_or(0.0)
    }

    pub(crate) fn trunk_strength(&self, idx: usize) -> f32 {
        self.trunk_strength.get(idx).copied().unwrap_or(0.0)
    }

    pub(crate) fn tributary_strength(&self, idx: usize) -> f32 {
        self.tributary_strength.get(idx).copied().unwrap_or(0.0)
    }

    pub(crate) fn routing_bias(&self, idx: usize) -> f32 {
        let activity = self.activity(idx);
        let depth = self.valley_depth(idx);
        let width_bonus = (self.valley_width(idx) / 18.0).clamp(0.0, 1.0) * 0.020;
        (depth * (0.46 + activity * 0.34)
            + self.trunk_strength(idx) * 0.030
            + self.tributary_strength(idx) * 0.015
            + width_bonus)
            .clamp(0.0, 0.18)
    }

    fn resize(&mut self, len: usize) {
        self.activity.resize(len, 0.0);
        self.long_term_discharge.resize(len, 0.0);
        self.valley_depth.resize(len, 0.0);
        self.valley_width.resize(len, 0.0);
        self.trunk_strength.resize(len, 0.0);
        self.tributary_strength.resize(len, 0.0);
    }
}

pub(crate) fn apply_valley_erosion_cycle(
    world: &mut World,
    hydrology: &HydrologyState,
    valleys: &mut ValleyErosion,
    cycle: usize,
) {
    valleys.resize(world.tiles.len());
    let thresholds = channel_thresholds(world);
    let original_elevation: Vec<f32> = world.tiles.iter().map(|tile| tile.raw_elevation).collect();
    let mut terrain_carve = vec![0.0_f32; world.tiles.len()];
    let mut terrain_fill = vec![0.0_f32; world.tiles.len()];
    let mut activity_pulse = vec![0.0_f32; world.tiles.len()];
    let mut discharge_pulse = vec![0.0_f32; world.tiles.len()];
    let mut depth_pulse = vec![0.0_f32; world.tiles.len()];
    let mut width_pulse = vec![0.0_f32; world.tiles.len()];
    let mut trunk_pulse = vec![0.0_f32; world.tiles.len()];
    let mut tributary_pulse = vec![0.0_f32; world.tiles.len()];
    let width_scale = (world.effective_world_size() / 256.0).clamp(0.5, 3.0);
    let cycle_gain = (0.62 + cycle.min(HYDROLOGY_EROSION_CYCLES - 1) as f32 * 0.055).min(1.02);

    for idx in 0..world.tiles.len() {
        if matches!(hydrology.surfaces[idx], Surface::Ocean | Surface::Lake) {
            continue;
        }

        let discharge = hydrology.discharge[idx];
        let height_above_sea = (original_elevation[idx] - world.sea_level).max(0.0);
        let highland = smoothstep(0.16, 0.48, height_above_sea);
        let local_slope = local_hydro_slope(world, hydrology, idx);
        let stream_power = hydrology.stream_power[idx];
        let stream_signal = smoothstep(thresholds.stream * 0.18, thresholds.secondary, discharge);
        let secondary_signal = smoothstep(
            thresholds.secondary * 0.55,
            thresholds.trunk * 1.15,
            discharge,
        );
        let trunk_signal = smoothstep(thresholds.trunk * 0.65, thresholds.trunk * 2.25, discharge);
        let area_signal = smoothstep(
            thresholds.stream * 0.55,
            thresholds.trunk * 2.0,
            hydrology.contributing_area[idx],
        );
        let power_signal = smoothstep(0.12, 2.6, stream_power);
        let slope_signal = smoothstep(0.0, 0.055, local_slope);
        let activity_memory = valleys.activity(idx);
        let long_memory = valleys.long_term_discharge(idx);
        let depth_memory = valleys.valley_depth(idx);
        let width_memory = valleys.valley_width(idx);
        let trunk_memory = valleys.trunk_strength(idx);
        let tributary_memory = valleys.tributary_strength(idx);
        let supported_discharge = discharge.max(long_memory * activity_memory);

        let latent_mountain_channel = highland > 0.22
            && discharge >= thresholds.stream * 0.18
            && (stream_power > 0.16 || hydrology.contributing_area[idx] > thresholds.stream * 0.5);
        let visible_channel = hydrology.surfaces[idx] == Surface::River;
        let active_memory = activity_memory > 0.08 || depth_memory > 0.020;
        let reoccupied = discharge >= (long_memory * 0.22).max(thresholds.stream * 0.12)
            || visible_channel
            || latent_mountain_channel;
        if !visible_channel && !latent_mountain_channel && !active_memory {
            continue;
        }

        let visible_factor = if visible_channel { 1.0 } else { 0.62 };
        let valley_signal = ((stream_signal * 0.34
            + secondary_signal * 0.34
            + trunk_signal * 0.46
            + area_signal * 0.18)
            * (0.38 + power_signal * 0.42 + slope_signal * 0.20)
            * (0.55 + highland * 0.45)
            * visible_factor)
            .clamp(0.0, 1.0);
        let trunk_corridor = (trunk_signal * visible_factor).max(trunk_memory * 0.90);
        let secondary_corridor = secondary_signal.max(trunk_corridor * 0.58);
        let tributary_corridor = tributary_memory.max(
            (highland * stream_signal * (0.45 + power_signal * 0.45 + slope_signal * 0.10))
                .max(activity_memory * 0.58),
        );
        let flow_occupation = if reoccupied { 1.0 } else { 0.36 };
        let combined_valley = (valley_signal.max(activity_memory * 0.90) * flow_occupation)
            .max(if reoccupied { depth_memory * 2.2 } else { 0.0 })
            .clamp(0.0, 1.0);
        let q_factor = smoothstep(
            thresholds.stream * 0.20,
            thresholds.trunk * 2.40,
            supported_discharge,
        );
        let target_depth = (0.006
            + q_factor * 0.090
            + trunk_corridor * 0.105
            + secondary_corridor * 0.040
            + tributary_corridor * highland * 0.022)
            * (0.48 + highland * 0.52)
            * (0.66 + power_signal * 0.26 + slope_signal * 0.08);
        let target_width = valley_width_tiles(
            trunk_corridor,
            secondary_corridor,
            tributary_corridor,
            combined_valley,
            width_scale,
        );

        activity_pulse[idx] = combined_valley;
        discharge_pulse[idx] = if reoccupied {
            supported_discharge.max(discharge)
        } else {
            0.0
        };
        depth_pulse[idx] = if reoccupied {
            target_depth.max(depth_memory * 0.96)
        } else {
            0.0
        };
        width_pulse[idx] = if reoccupied {
            target_width.max(width_memory * 0.96)
        } else {
            0.0
        };
        trunk_pulse[idx] = trunk_corridor;
        tributary_pulse[idx] = tributary_corridor;

        if !reoccupied && depth_memory > 0.018 {
            let orphan = (1.0 - (discharge / (long_memory.max(thresholds.stream) + 0.0001)))
                .clamp(0.0, 1.0)
                * activity_memory.max(depth_memory * 4.0).clamp(0.0, 1.0);
            terrain_fill[idx] = terrain_fill[idx].max((depth_memory * orphan * 0.055).min(0.012));
        }

        let ratio = (discharge / thresholds.stream).max(1.0);
        let band_multiplier = if discharge >= thresholds.trunk {
            1.70
        } else if discharge >= thresholds.secondary {
            1.22
        } else {
            1.0
        };
        let slope_factor = if local_slope < 0.008 { 1.22 } else { 0.94 };
        let lowland_factor = 1.0 - smoothstep(0.06, 0.22, height_above_sea) * 0.32;
        let base_carve =
            (0.0055 + ratio.ln() * 0.017) * band_multiplier * slope_factor * lowland_factor;
        let mountain_dissection = highland
            * (trunk_corridor * 0.54 + secondary_corridor * 0.34 + activity_memory * 0.22)
            * (0.026 + ratio.ln_1p() * 0.018)
            * (0.72 + slope_signal * 0.50);
        let tributary_dissection =
            tributary_corridor * (0.010 + ratio.ln_1p() * 0.0055) * (0.82 + slope_signal * 0.32);
        let mut center_carve = ((base_carve + mountain_dissection + tributary_dissection)
            * cycle_gain
            * visible_factor)
            .clamp(0.0, 0.22);

        let target_profile = trunk_corridor > 0.10
            || secondary_corridor > 0.18
            || tributary_corridor > 0.34
            || activity_memory > 0.14
            || depth_memory > 0.025;
        if highland > 0.24 && target_profile {
            let floor_target = world.sea_level
                + 0.082
                + (1.0 - trunk_corridor).max(0.0) * 0.13
                + (1.0 - highland) * 0.052
                + (1.0 - secondary_corridor).max(0.0) * 0.025;
            let profile_strength =
                0.24 + trunk_corridor * 0.50 + secondary_corridor * 0.24 + activity_memory * 0.18;
            let center_grade =
                (original_elevation[idx] - floor_target).max(0.0) * profile_strength * cycle_gain;
            center_carve = center_carve.max(center_grade.min(0.41));
        }
        terrain_carve[idx] = terrain_carve[idx].max(center_carve);

        let Some(next) = hydrology.downstream[idx] else {
            spread_immediate_banks(
                world,
                hydrology,
                &original_elevation,
                &mut terrain_carve,
                idx,
                center_carve,
                highland,
                trunk_corridor,
                secondary_corridor,
            );
            continue;
        };

        let (x, y) = world.coords(idx);
        let (nx, ny) = world.coords(next);
        let step_x = (nx as isize - x as isize).signum();
        let step_y = (ny as isize - y as isize).signum();
        if step_x == 0 && step_y == 0 {
            continue;
        }

        let width_tiles = valley_width_tiles(
            trunk_corridor,
            secondary_corridor,
            tributary_corridor,
            combined_valley,
            width_scale,
        );
        let radius = ((width_tiles * 0.5).round() as isize).max(1);
        let side_a = (-step_y, step_x);
        let side_b = (step_y, -step_x);
        let channel_floor = original_elevation[idx] - center_carve;

        for distance in 1..=radius {
            let d = distance as f32;
            let norm = (d / radius as f32).clamp(0.0, 1.0);
            let wall_weight = (1.0 - norm).max(0.0).powf(1.25);
            let floor_weight = (1.0 - smoothstep(0.24, 0.72, norm)).clamp(0.0, 1.0);
            for side in [side_a, side_b] {
                let sx = x as isize + side.0 * distance;
                let sy = y as isize + side.1 * distance;
                if !world.in_bounds(sx, sy) {
                    continue;
                }
                let sidx = world.idx(sx as usize, sy as usize);
                if hydrology.surfaces[sidx] == Surface::Ocean {
                    continue;
                }

                let shoulder_rise = d * (0.012 + highland * 0.014 + (1.0 - trunk_corridor) * 0.006)
                    + norm.powf(1.7) * (0.035 + highland * 0.030);
                let side_target = channel_floor + shoulder_rise;
                let profile_strength = (trunk_corridor * 0.80
                    + secondary_corridor * 0.42
                    + tributary_corridor * 0.28
                    + activity_memory * 0.30
                    + depth_memory * 1.15)
                    * (wall_weight * 0.72 + floor_weight * 0.28)
                    * cycle_gain;
                let profile_carve =
                    (original_elevation[sidx] - side_target).max(0.0) * profile_strength;
                let wall_height = (original_elevation[sidx] - channel_floor).max(0.0);
                let floor_widening = center_carve
                    * (0.12
                        + highland * trunk_corridor * 0.28
                        + highland * secondary_corridor * 0.24
                        + tributary_corridor * 0.13
                        + activity_memory * 0.10)
                    * (wall_weight * 0.65 + floor_weight * 0.35);
                let wall_undercut = wall_height
                    * (0.045
                        + trunk_corridor * 0.19
                        + highland * trunk_corridor * 0.14
                        + secondary_corridor * 0.15
                        + highland * secondary_corridor * 0.10
                        + tributary_corridor * 0.08
                        + activity_memory * 0.08
                        + depth_memory * 0.45)
                    * wall_weight;
                let lateral_carve = (profile_carve + floor_widening + wall_undercut)
                    .min(0.086 + trunk_corridor * 0.045 + secondary_corridor * 0.026);
                terrain_carve[sidx] = terrain_carve[sidx].max(lateral_carve);
            }
        }

        spread_immediate_banks(
            world,
            hydrology,
            &original_elevation,
            &mut terrain_carve,
            idx,
            center_carve,
            highland,
            trunk_corridor,
            secondary_corridor,
        );
    }

    for idx in 0..world.tiles.len() {
        if matches!(hydrology.surfaces[idx], Surface::Ocean | Surface::Lake) {
            valleys.activity[idx] *= 0.68;
            valleys.long_term_discharge[idx] *= 0.72;
            valleys.valley_depth[idx] *= 0.70;
            valleys.valley_width[idx] *= 0.76;
            valleys.trunk_strength[idx] *= 0.68;
            valleys.tributary_strength[idx] *= 0.68;
        } else {
            let active = activity_pulse[idx] > 0.0 || discharge_pulse[idx] > 0.0;
            let depth_decay = if active {
                DEPTH_DECAY
            } else {
                DEPTH_DECAY * 0.82
            };
            let width_decay = if active {
                WIDTH_DECAY
            } else {
                WIDTH_DECAY * 0.86
            };
            valleys.activity[idx] =
                (valleys.activity[idx] * ACTIVITY_DECAY).max(activity_pulse[idx]);
            valleys.long_term_discharge[idx] =
                (valleys.long_term_discharge[idx] * DISCHARGE_DECAY).max(discharge_pulse[idx]);
            valleys.valley_depth[idx] =
                (valleys.valley_depth[idx] * depth_decay).max(depth_pulse[idx]);
            valleys.valley_width[idx] =
                (valleys.valley_width[idx] * width_decay).max(width_pulse[idx]);
            valleys.trunk_strength[idx] =
                (valleys.trunk_strength[idx] * TRUNK_DECAY).max(trunk_pulse[idx]);
            valleys.tributary_strength[idx] =
                (valleys.tributary_strength[idx] * TRIBUTARY_DECAY).max(tributary_pulse[idx]);
        }
    }

    for idx in 0..world.tiles.len() {
        let tile = &mut world.tiles[idx];
        tile.raw_elevation =
            (tile.raw_elevation - terrain_carve[idx] + terrain_fill[idx]).clamp(0.0, 1.0);
    }
}

fn local_hydro_slope(world: &World, hydrology: &HydrologyState, idx: usize) -> f32 {
    hydrology.downstream[idx]
        .map(|next| {
            let (x, y) = world.coords(idx);
            let (nx, ny) = world.coords(next);
            (hydrology.hydro_elevation[idx] - hydrology.hydro_elevation[next]).max(0.0)
                / neighbor_distance(x, y, nx, ny)
        })
        .unwrap_or(0.0)
}

fn valley_width_tiles(
    trunk: f32,
    secondary: f32,
    tributary: f32,
    memory: f32,
    width_scale: f32,
) -> f32 {
    let trunk_width =
        TRUNK_WIDTH_RANGE_256.0 + (TRUNK_WIDTH_RANGE_256.1 - TRUNK_WIDTH_RANGE_256.0) * trunk;
    let secondary_width = SECONDARY_WIDTH_RANGE_256.0
        + (SECONDARY_WIDTH_RANGE_256.1 - SECONDARY_WIDTH_RANGE_256.0) * secondary;
    let tributary_width = TRIBUTARY_WIDTH_RANGE_256.0
        + (TRIBUTARY_WIDTH_RANGE_256.1 - TRIBUTARY_WIDTH_RANGE_256.0)
            * tributary.max(memory * 0.45);

    let width = if trunk > 0.12 {
        trunk_width
    } else if secondary > 0.18 {
        secondary_width
    } else {
        tributary_width
    };
    (width * width_scale).clamp(1.0, 54.0)
}

fn spread_immediate_banks(
    world: &World,
    hydrology: &HydrologyState,
    original_elevation: &[f32],
    terrain_carve: &mut [f32],
    idx: usize,
    center_carve: f32,
    highland: f32,
    trunk_corridor: f32,
    secondary_corridor: f32,
) {
    let (x, y) = world.coords(idx);
    let neighbors: Vec<_> = world.neighbors8(x, y).collect();
    for (nx, ny) in neighbors {
        let nidx = world.idx(nx, ny);
        if hydrology.surfaces[nidx] == Surface::Ocean {
            continue;
        }
        let distance = neighbor_distance(x, y, nx, ny);
        let neighbor_relief = (original_elevation[nidx] - original_elevation[idx]).max(0.0);
        let side_factor = if hydrology.surfaces[nidx] == Surface::River {
            0.34
        } else if distance > 1.0 {
            0.08 + trunk_corridor * highland * 0.10 + secondary_corridor * highland * 0.10
        } else {
            0.14 + trunk_corridor * highland * 0.14 + secondary_corridor * highland * 0.14
        };
        let relief_factor = (0.5 + neighbor_relief * 1.8).clamp(0.5, 1.52);
        let lateral_carve = (center_carve * side_factor * relief_factor)
            .min(0.070 + trunk_corridor * 0.032 + secondary_corridor * 0.024);
        terrain_carve[nidx] = terrain_carve[nidx].max(lateral_carve);
    }
}
