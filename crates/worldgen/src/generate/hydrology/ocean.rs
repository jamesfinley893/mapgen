use std::collections::VecDeque;

use crate::World;

pub(in crate::generate) fn classify_ocean(world: &World) -> Vec<bool> {
    let mut ocean = vec![false; world.tiles.len()];
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
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            if ocean[nidx] || world.tiles[nidx].raw_elevation > world.sea_level {
                continue;
            }
            ocean[nidx] = true;
            queue.push_back(nidx);
        }
    }

    ocean
}

fn seed_ocean_boundary(
    world: &World,
    ocean: &mut [bool],
    queue: &mut VecDeque<usize>,
    x: usize,
    y: usize,
) {
    let idx = world.idx(x, y);
    if !ocean[idx] && world.tiles[idx].raw_elevation <= world.sea_level {
        ocean[idx] = true;
        queue.push_back(idx);
    }
}
