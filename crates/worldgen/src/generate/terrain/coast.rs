use crate::World;

const COASTAL_REGULARIZATION_PASSES: usize = 2;
const NEAR_SEA_BAND: f32 = 0.045;
const SHELF_BAND: f32 = 0.120;
const SEA_MARGIN: f32 = 0.004;
const CARDINAL_DIRS: [(isize, isize); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

pub(super) fn regularize_coastal_margins(world: &World, terrain: &mut Vec<f32>) {
    for _ in 0..COASTAL_REGULARIZATION_PASSES {
        let mut next = terrain.clone();

        for idx in 0..world.tile_count() {
            let current = terrain[idx];
            if (current - world.sea_level).abs() > NEAR_SEA_BAND {
                continue;
            }

            let (x, y) = world.coords(idx);
            let land4 = cardinal_neighbors(world, x, y)
                .filter(|nidx| terrain[*nidx] > world.sea_level)
                .count();
            let land8 = world
                .neighbor_indices8(idx)
                .filter(|nidx| terrain[*nidx] > world.sea_level)
                .count();
            let ocean4 = 4_usize.saturating_sub(land4);
            let ocean8 = 8_usize.saturating_sub(land8);

            if current > world.sea_level {
                if ocean4 >= 3 && ocean8 >= 5 {
                    next[idx] = world.sea_level - SEA_MARGIN;
                }
            } else if land4 >= 3 && land8 >= 5 {
                next[idx] = world.sea_level + SEA_MARGIN;
            }
        }

        *terrain = next;
    }
    shape_shallow_shelves(world, terrain);
}

fn cardinal_neighbors(world: &World, x: usize, y: usize) -> impl Iterator<Item = usize> + '_ {
    CARDINAL_DIRS.into_iter().filter_map(move |(dx, dy)| {
        let nx = x as isize + dx;
        let ny = y as isize + dy;
        world
            .in_bounds(nx, ny)
            .then(|| world.idx(nx as usize, ny as usize))
    })
}

fn shape_shallow_shelves(world: &World, terrain: &mut [f32]) {
    let previous = terrain.to_vec();

    for idx in 0..world.tile_count() {
        let current = previous[idx];
        let depth = world.sea_level - current;
        if !(0.0..=SHELF_BAND).contains(&depth) {
            continue;
        }

        let land8 = world
            .neighbor_indices8(idx)
            .filter(|nidx| previous[*nidx] > world.sea_level)
            .count();
        if land8 == 0 {
            continue;
        }

        let land_proximity = land8 as f32 / 8.0;
        let shelf_depth = 0.044 - land_proximity * 0.026;
        terrain[idx] = current.max(world.sea_level - shelf_depth.max(SEA_MARGIN * 2.0));
    }
}
