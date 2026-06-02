use crate::World;
use crate::generate::smoothstep;

use super::fields::OrogenFields;

pub(super) fn commit_terrain(world: &mut World, elevation: &[f32], fields: &OrogenFields) {
    let mut slopes = vec![0.0_f32; world.tile_count()];
    let mut relief = vec![0.0_f32; world.tile_count()];

    for idx in 0..world.tile_count() {
        let current = elevation[idx];
        let mut min_elev = current;
        let mut max_elev = current;
        let mut max_delta = 0.0_f32;

        for nidx in world.neighbor_indices8(idx) {
            let neighbor = elevation[nidx];
            min_elev = min_elev.min(neighbor);
            max_elev = max_elev.max(neighbor);
            max_delta = max_delta.max((current - neighbor).abs());
        }

        slopes[idx] = max_delta.clamp(0.0, 1.0);
        relief[idx] = (max_elev - min_elev).clamp(0.0, 1.0);
    }

    for idx in 0..world.tile_count() {
        let tile = &mut world.tiles[idx];
        tile.elevation = elevation[idx];
        tile.slope = slopes[idx];
        tile.relief = relief[idx];
        tile.uplift = orogenic_uplift_signal(fields, idx);
        tile.mountain_presence = mountain_presence(
            world.sea_level,
            elevation[idx],
            slopes[idx],
            relief[idx],
            tile.uplift,
        );
    }
}

pub(super) fn finalize_sea_level(world: &mut World, elevation: &[f32]) {
    const MIN_LAND_FRAC: f32 = 0.25;
    let mut elevs = elevation.to_vec();
    elevs.sort_by(|a, b| a.total_cmp(b));
    let threshold_idx =
        ((elevs.len() as f32 * (1.0 - MIN_LAND_FRAC)) as usize).min(elevs.len().saturating_sub(1));
    world.sea_level = world.sea_level.min(elevs[threshold_idx]);
}

fn orogenic_uplift_signal(fields: &OrogenFields, idx: usize) -> f32 {
    (fields.axial_uplift[idx] * 1.0
        + fields.shoulder_uplift[idx] * 0.45
        + fields.plateau_support[idx] * 0.30
        - fields.basin_bias[idx] * 0.18)
        .clamp(0.0, 1.0)
}

fn mountain_presence(sea_level: f32, elevation: f32, slope: f32, relief: f32, uplift: f32) -> f32 {
    let height = (elevation - sea_level).max(0.0);
    let highland = smoothstep(0.22, 0.42, height);
    let rugged = smoothstep(0.018, 0.070, relief + slope * 0.72);
    let tectonic = smoothstep(0.10, 0.62, uplift);
    (highland * 0.52 + rugged * 0.24 + tectonic * 0.42 + highland * tectonic * 0.24).clamp(0.0, 1.0)
}
