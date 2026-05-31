use std::collections::VecDeque;

use crate::{Surface, World};

pub(super) fn classify_ocean(world: &World, elevation: &[f32]) -> Vec<bool> {
    let mut ocean = vec![false; world.tile_count()];
    let mut queue = VecDeque::new();

    for x in 0..world.width {
        seed_ocean_boundary(world, elevation, &mut ocean, &mut queue, x, 0);
        seed_ocean_boundary(
            world,
            elevation,
            &mut ocean,
            &mut queue,
            x,
            world.height - 1,
        );
    }
    for y in 0..world.height {
        seed_ocean_boundary(world, elevation, &mut ocean, &mut queue, 0, y);
        seed_ocean_boundary(world, elevation, &mut ocean, &mut queue, world.width - 1, y);
    }

    while let Some(idx) = queue.pop_front() {
        let (x, y) = world.coords(idx);
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            if ocean[nidx] || elevation[nidx] > world.sea_level {
                continue;
            }
            ocean[nidx] = true;
            queue.push_back(nidx);
        }
    }

    ocean
}

pub(super) fn classify_surfaces(world: &World, ocean: &[bool]) -> Vec<Surface> {
    let mut surfaces = vec![Surface::Land; world.tile_count()];

    for (idx, is_ocean) in ocean.iter().copied().enumerate().take(world.tile_count()) {
        if is_ocean {
            surfaces[idx] = Surface::Ocean;
        }
    }

    for idx in 0..world.tile_count() {
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

fn seed_ocean_boundary(
    world: &World,
    elevation: &[f32],
    ocean: &mut [bool],
    queue: &mut VecDeque<usize>,
    x: usize,
    y: usize,
) {
    let idx = world.idx(x, y);
    if !ocean[idx] && elevation[idx] <= world.sea_level {
        ocean[idx] = true;
        queue.push_back(idx);
    }
}
