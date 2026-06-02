use std::collections::VecDeque;

use crate::{WaterClass, World};

const COASTAL_CLEANUP_PASSES: usize = 2;
const MIN_SMALL_FEATURE_AREA: usize = 4;
const MAX_SMALL_FEATURE_AREA: usize = 96;
const CARDINAL_DIRS: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

pub(super) fn classify_ocean(world: &World) -> Vec<bool> {
    let mut ocean = vec![false; world.tile_count()];
    let mut queue = VecDeque::new();

    for x in 0..world.width {
        seed_ocean_boundary(world, &mut ocean, &mut queue, x, 0);
        seed_ocean_boundary(world, &mut ocean, &mut queue, x, world.height - 1);
    }
    for y in 0..world.height {
        seed_ocean_boundary(world, &mut ocean, &mut queue, 0, y);
        seed_ocean_boundary(world, &mut ocean, &mut queue, world.width - 1, y);
    }

    while let Some(idx) = queue.pop_front() {
        let (x, y) = world.coords(idx);
        for (nx, ny) in cardinal_neighbors(world, x, y) {
            let nidx = world.idx(nx, ny);
            if ocean[nidx] || world.tiles[nidx].elevation > world.sea_level {
                continue;
            }
            ocean[nidx] = true;
            queue.push_back(nidx);
        }
    }

    cleanup_coastal_mask(world, &mut ocean);
    ocean
}

pub(super) fn mark_ocean_and_coast(world: &mut World, ocean: &[bool]) {
    for (idx, is_ocean) in ocean.iter().copied().enumerate().take(world.tile_count()) {
        let tile = &mut world.tiles[idx];
        tile.water = if is_ocean {
            WaterClass::Ocean
        } else {
            WaterClass::Land
        };
        tile.coast = false;
    }

    for idx in 0..world.tile_count() {
        if !world.tiles[idx].is_land() {
            continue;
        }
        let (x, y) = world.coords(idx);
        if world
            .neighbors8(x, y)
            .any(|(nx, ny)| world.tiles[world.idx(nx, ny)].is_ocean())
        {
            world.tiles[idx].coast = true;
        }
    }
}

fn seed_ocean_boundary(
    world: &World,
    ocean: &mut [bool],
    queue: &mut VecDeque<usize>,
    x: usize,
    y: usize,
) {
    let idx = world.idx(x, y);
    if !ocean[idx] && world.tiles[idx].elevation <= world.sea_level {
        ocean[idx] = true;
        queue.push_back(idx);
    }
}

fn cleanup_coastal_mask(world: &World, ocean: &mut Vec<bool>) {
    for _ in 0..COASTAL_CLEANUP_PASSES {
        smooth_single_tile_artifacts(world, ocean);
        retain_boundary_connected_ocean(world, ocean);
    }
    remove_small_land_islands(world, ocean);
    smooth_single_tile_artifacts(world, ocean);
    retain_boundary_connected_ocean(world, ocean);
}

fn smooth_single_tile_artifacts(world: &World, ocean: &mut Vec<bool>) {
    let mut next = ocean.clone();

    for idx in 0..world.tile_count() {
        let (x, y) = world.coords(idx);
        if is_boundary(world, x, y) {
            continue;
        }

        let ocean4 = cardinal_neighbors(world, x, y)
            .filter(|(nx, ny)| ocean[world.idx(*nx, *ny)])
            .count();
        let ocean8 = world
            .neighbors8(x, y)
            .filter(|(nx, ny)| ocean[world.idx(*nx, *ny)])
            .count();
        let land4 = 4_usize.saturating_sub(ocean4);
        let land8 = 8_usize.saturating_sub(ocean8);

        if ocean[idx] {
            if land4 >= 3 && land8 >= 5 {
                next[idx] = false;
            }
        } else if ocean4 >= 3 && ocean8 >= 5 {
            next[idx] = true;
        }
    }

    *ocean = next;
}

fn remove_small_land_islands(world: &World, ocean: &mut [bool]) {
    let max_area = small_feature_area(world);
    let mut visited = vec![false; world.tile_count()];

    for start in 0..world.tile_count() {
        if visited[start] || ocean[start] {
            continue;
        }

        let mut component = Vec::new();
        let mut queue = VecDeque::from([start]);
        let mut touches_boundary = false;
        let mut touches_ocean_cardinal = false;
        visited[start] = true;

        while let Some(idx) = queue.pop_front() {
            component.push(idx);
            let (x, y) = world.coords(idx);
            touches_boundary |= is_boundary(world, x, y);

            for (nx, ny) in cardinal_neighbors(world, x, y) {
                let nidx = world.idx(nx, ny);
                if ocean[nidx] {
                    touches_ocean_cardinal = true;
                } else if !visited[nidx] {
                    visited[nidx] = true;
                    queue.push_back(nidx);
                }
            }
        }

        if !touches_boundary && touches_ocean_cardinal && component.len() <= max_area {
            for idx in component {
                ocean[idx] = true;
            }
        }
    }
}

fn retain_boundary_connected_ocean(world: &World, ocean: &mut Vec<bool>) {
    let mut connected = vec![false; world.tile_count()];
    let mut queue = VecDeque::new();

    for x in 0..world.width {
        seed_connected_boundary(world, ocean, &mut connected, &mut queue, x, 0);
        seed_connected_boundary(
            world,
            ocean,
            &mut connected,
            &mut queue,
            x,
            world.height - 1,
        );
    }
    for y in 0..world.height {
        seed_connected_boundary(world, ocean, &mut connected, &mut queue, 0, y);
        seed_connected_boundary(world, ocean, &mut connected, &mut queue, world.width - 1, y);
    }

    while let Some(idx) = queue.pop_front() {
        let (x, y) = world.coords(idx);
        for (nx, ny) in cardinal_neighbors(world, x, y) {
            let nidx = world.idx(nx, ny);
            if ocean[nidx] && !connected[nidx] {
                connected[nidx] = true;
                queue.push_back(nidx);
            }
        }
    }

    *ocean = connected;
}

fn seed_connected_boundary(
    world: &World,
    ocean: &[bool],
    connected: &mut [bool],
    queue: &mut VecDeque<usize>,
    x: usize,
    y: usize,
) {
    let idx = world.idx(x, y);
    if ocean[idx] && !connected[idx] {
        connected[idx] = true;
        queue.push_back(idx);
    }
}

fn cardinal_neighbors(
    world: &World,
    x: usize,
    y: usize,
) -> impl Iterator<Item = (usize, usize)> + '_ {
    CARDINAL_DIRS.into_iter().filter_map(move |(dx, dy)| {
        let nx = x as isize + dx;
        let ny = y as isize + dy;
        world
            .in_bounds(nx, ny)
            .then_some((nx as usize, ny as usize))
    })
}

fn is_boundary(world: &World, x: usize, y: usize) -> bool {
    x == 0 || y == 0 || x == world.width - 1 || y == world.height - 1
}

fn small_feature_area(world: &World) -> usize {
    let side = (world.effective_world_size() / 48.0).round() as usize;
    side.saturating_mul(side)
        .clamp(MIN_SMALL_FEATURE_AREA, MAX_SMALL_FEATURE_AREA)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Tile;

    fn test_world(width: usize, height: usize, elevation: f32) -> World {
        let mut world = World::new(1, width, height, 0.5, 0);
        for tile in &mut world.tiles {
            *tile = Tile {
                elevation,
                ..Tile::default()
            };
        }
        world
    }

    #[test]
    fn ocean_fill_does_not_cross_diagonal_corners() {
        let mut world = test_world(5, 5, 0.7);
        let boundary = world.idx(0, 0);
        let diagonal = world.idx(1, 1);
        world.tiles[boundary].elevation = 0.3;
        world.tiles[diagonal].elevation = 0.3;

        let ocean = classify_ocean(&world);

        assert!(ocean[boundary]);
        assert!(!ocean[diagonal]);
    }

    #[test]
    fn coastal_cleanup_removes_tiny_island_components() {
        let mut world = test_world(32, 32, 0.3);
        let island = world.idx(16, 16);
        world.tiles[island].elevation = 0.7;

        let ocean = classify_ocean(&world);

        assert!(ocean[island]);
    }

    #[test]
    fn coastal_cleanup_fills_single_tile_ocean_notches() {
        let mut world = test_world(7, 7, 0.7);
        for y in 0..world.height {
            let idx = world.idx(0, y);
            world.tiles[idx].elevation = 0.3;
        }
        let notch = world.idx(1, 3);
        world.tiles[notch].elevation = 0.3;

        let ocean = classify_ocean(&world);

        assert!(ocean[world.idx(0, 3)]);
        assert!(!ocean[notch]);
    }
}
