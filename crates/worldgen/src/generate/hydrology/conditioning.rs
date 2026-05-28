use std::collections::BinaryHeap;

use crate::World;

use super::{ConditioningState, QueueCell};
use crate::generate::HYDRO_EPSILON;

pub(super) fn condition_terrain(world: &World, ocean: &[bool]) -> ConditioningState {
    let count = world.tiles.len();
    let mut hydro = vec![0.0; count];
    let mut fill_depth = vec![0.0; count];
    let mut parent = vec![None; count];
    let mut rank = vec![usize::MAX; count];
    let mut visited = vec![false; count];
    let mut heap = BinaryHeap::new();
    let mut next_rank = 0_usize;

    for idx in 0..count {
        if ocean[idx] {
            visited[idx] = true;
            hydro[idx] = world.tiles[idx].raw_elevation;
            rank[idx] = next_rank;
            next_rank += 1;
            heap.push(QueueCell {
                level: hydro[idx],
                idx,
            });
        }
    }

    while let Some(cell) = heap.pop() {
        let (x, y) = world.coords(cell.idx);
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            if visited[nidx] {
                continue;
            }
            visited[nidx] = true;
            let raw = world.tiles[nidx].raw_elevation;
            let raised = raw.max(cell.level);
            hydro[nidx] = raised;
            fill_depth[nidx] = (raised - raw).max(0.0);
            parent[nidx] = Some(cell.idx);
            rank[nidx] = next_rank;
            next_rank += 1;
            heap.push(QueueCell {
                level: raised + HYDRO_EPSILON,
                idx: nidx,
            });
        }
    }

    ConditioningState {
        hydro_elevation: hydro,
        fill_depth,
        parent,
        rank,
    }
}
