use crate::{Biome, World};

#[derive(Clone, Copy)]
struct BiomeContext {
    is_ocean: bool,
    is_coast: bool,
    elevation: f32,
    sea_level: f32,
    temperature: f32,
    moisture: f32,
    support: f32,
    proximity: f32,
    relief: f32,
}

#[derive(Clone, Copy)]
struct NeighborSupportSpec {
    radius: isize,
    full_threshold: f32,
    partial_threshold: f32,
    partial_weight: f32,
}

pub(super) fn assign_biomes(world: &mut World) {
    let biomes = (0..world.tile_count())
        .map(|idx| biome_for_world_tile(world, idx))
        .collect::<Vec<_>>();

    for (tile, biome) in world.tiles.iter_mut().zip(biomes.into_iter()) {
        tile.biome = biome;
    }
}

fn biome_for_world_tile(world: &World, idx: usize) -> Biome {
    let tile = &world.tiles[idx];
    biome_for_tile_with_support(BiomeContext {
        is_ocean: tile.is_ocean(),
        is_coast: tile.is_coast(),
        elevation: tile.elevation,
        sea_level: world.sea_level,
        temperature: tile.temperature,
        moisture: tile.moisture,
        support: mountain_support(world, idx),
        proximity: mountain_proximity(world, idx),
        relief: tile.relief,
    })
}

fn mountain_support(world: &World, idx: usize) -> f32 {
    let high_threshold = world.sea_level + 0.24;
    let alpine_threshold = world.sea_level + 0.34;
    weighted_neighbor_support(
        world,
        idx,
        NeighborSupportSpec {
            radius: 2,
            full_threshold: alpine_threshold,
            partial_threshold: high_threshold,
            partial_weight: 0.55,
        },
        |dx, dy| {
            let dist = dx.abs().max(dy.abs()) as f32;
            if dist <= 1.0 { 1.0 } else { 0.45 }
        },
    )
}

fn mountain_proximity(world: &World, idx: usize) -> f32 {
    let alpine_threshold = world.sea_level + 0.38;
    let ridge_threshold = world.sea_level + 0.32;
    weighted_neighbor_support(
        world,
        idx,
        NeighborSupportSpec {
            radius: 4,
            full_threshold: alpine_threshold,
            partial_threshold: ridge_threshold,
            partial_weight: 0.45,
        },
        |dx, dy| {
            let dist = ((dx * dx + dy * dy) as f32).sqrt();
            (1.0 / (1.0 + dist)).clamp(0.12, 0.7)
        },
    )
}

fn weighted_neighbor_support(
    world: &World,
    idx: usize,
    spec: NeighborSupportSpec,
    weight_for: impl Fn(isize, isize) -> f32,
) -> f32 {
    let (x, y) = world.coords(idx);
    let mut support = 0.0_f32;
    let mut total = 0.0_f32;

    for dy in -spec.radius..=spec.radius {
        for dx in -spec.radius..=spec.radius {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if !world.in_bounds(nx, ny) {
                continue;
            }
            let nidx = world.idx(nx as usize, ny as usize);
            let weight = weight_for(dx, dy);
            let elev = world.tiles[nidx].elevation;
            total += weight;
            if elev > spec.full_threshold {
                support += weight;
            } else if elev > spec.partial_threshold {
                support += weight * spec.partial_weight;
            }
        }
    }

    if total <= f32::EPSILON {
        0.0
    } else {
        (support / total).clamp(0.0, 1.0)
    }
}

pub fn biome_for_tile(
    is_ocean: bool,
    is_coast: bool,
    elevation: f32,
    sea_level: f32,
    temperature: f32,
    moisture: f32,
) -> Biome {
    biome_for_tile_with_support(BiomeContext {
        is_ocean,
        is_coast,
        elevation,
        sea_level,
        temperature,
        moisture,
        support: 1.0,
        proximity: 1.0,
        relief: 0.08,
    })
}

fn biome_for_tile_with_support(ctx: BiomeContext) -> Biome {
    if ctx.is_ocean {
        Biome::Ocean
    } else if ctx.is_coast {
        Biome::Coast
    } else {
        land_biome_with_support(ctx)
    }
}

fn land_biome_with_support(ctx: BiomeContext) -> Biome {
    if ctx.elevation > ctx.sea_level + 0.36
        && ctx.support > 0.46
        && (ctx.relief > 0.022 || ctx.elevation > ctx.sea_level + 0.42)
    {
        return Biome::Alpine;
    }
    if ctx.elevation > ctx.sea_level + 0.27
        && ctx.support > 0.18
        && ctx.proximity > 0.16
        && ctx.relief > 0.020
    {
        return Biome::Foothills;
    }
    if ctx.temperature < 0.12 {
        return if ctx.moisture < 0.35 {
            Biome::PolarDesert
        } else {
            Biome::Tundra
        };
    }
    if ctx.temperature < 0.28 {
        return if ctx.moisture < 0.30 {
            Biome::Steppe
        } else {
            Biome::BorealForest
        };
    }
    if ctx.temperature < 0.48 {
        if ctx.moisture < 0.18 {
            Biome::Desert
        } else if ctx.moisture < 0.29 {
            Biome::Steppe
        } else if ctx.moisture < 0.43 {
            Biome::TemperateGrassland
        } else if ctx.moisture < 0.58 {
            Biome::Woodland
        } else {
            Biome::TemperateForest
        }
    } else if ctx.temperature < 0.72 {
        if ctx.moisture < 0.16 {
            Biome::Desert
        } else if ctx.moisture < 0.26 {
            Biome::Steppe
        } else if ctx.moisture < 0.48 {
            Biome::Savanna
        } else if ctx.moisture < 0.62 {
            Biome::Woodland
        } else {
            Biome::TropicalForest
        }
    } else if ctx.moisture < 0.16 {
        Biome::Desert
    } else if ctx.moisture < 0.46 {
        Biome::Savanna
    } else if ctx.moisture < 0.68 {
        Biome::TropicalForest
    } else {
        Biome::Rainforest
    }
}
