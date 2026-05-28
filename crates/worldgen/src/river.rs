use crate::{Surface, World};

#[derive(Clone, Copy)]
pub(crate) struct RiverBandThresholds {
    pub(crate) secondary: f32,
    pub(crate) trunk: f32,
}

pub(crate) fn river_band_thresholds(world: &World) -> RiverBandThresholds {
    let (secondary, trunk) = river_discharge_percentiles(world, 58, 84);
    RiverBandThresholds { secondary, trunk }
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

pub(crate) fn river_direction(world: &World, idx: usize, next: usize) -> (isize, isize) {
    let (x, y) = world.coords(idx);
    let (nx, ny) = world.coords(next);
    (
        (nx as isize - x as isize).signum(),
        (ny as isize - y as isize).signum(),
    )
}
