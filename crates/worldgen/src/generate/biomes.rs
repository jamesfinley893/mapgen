use crate::{Biome, Surface, World};

#[derive(Clone, Copy)]
struct BiomeContext {
    surface: Surface,
    elevation: f32,
    sea_level: f32,
    temperature: f32,
    moisture: f32,
    support: f32,
    proximity: f32,
    relief: f32,
}

pub(super) fn assign_biomes(world: &mut World) {
    let mut biomes = Vec::with_capacity(world.tiles.len());
    for idx in 0..world.tiles.len() {
        biomes.push(biome_for_world_tile(world, idx));
    }
    for (tile, biome) in world.tiles.iter_mut().zip(biomes.into_iter()) {
        tile.biome = biome;
    }
}

fn biome_for_world_tile(world: &World, idx: usize) -> Biome {
    let tile = &world.tiles[idx];
    biome_for_tile_with_support(BiomeContext {
        surface: tile.surface,
        elevation: tile.raw_elevation,
        sea_level: world.sea_level,
        temperature: tile.temperature,
        moisture: tile.moisture,
        support: mountain_support(world, idx),
        proximity: mountain_proximity(world, idx),
        relief: local_relief(world, idx),
    })
}

fn mountain_support(world: &World, idx: usize) -> f32 {
    let (x, y) = world.coords(idx);
    let high_threshold = world.sea_level + 0.24;
    let alpine_threshold = world.sea_level + 0.34;
    let mut support = 0.0_f32;
    let mut total = 0.0_f32;

    for dy in -2..=2 {
        for dx in -2..=2 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if !world.in_bounds(nx, ny) {
                continue;
            }
            let nidx = world.idx(nx as usize, ny as usize);
            let dist = dx.abs().max(dy.abs()) as f32;
            let weight = if dist <= 1.0 { 1.0 } else { 0.45 };
            let elev = world.tiles[nidx].raw_elevation;
            total += weight;
            if elev > alpine_threshold {
                support += weight;
            } else if elev > high_threshold {
                support += weight * 0.55;
            }
        }
    }

    if total <= f32::EPSILON {
        0.0
    } else {
        (support / total).clamp(0.0, 1.0)
    }
}

fn mountain_proximity(world: &World, idx: usize) -> f32 {
    let (x, y) = world.coords(idx);
    let alpine_threshold = world.sea_level + 0.38;
    let ridge_threshold = world.sea_level + 0.32;
    let mut support = 0.0_f32;
    let mut total = 0.0_f32;

    for dy in -4..=4 {
        for dx in -4..=4 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if !world.in_bounds(nx, ny) {
                continue;
            }
            let nidx = world.idx(nx as usize, ny as usize);
            let dist = ((dx * dx + dy * dy) as f32).sqrt();
            let weight = (1.0 / (1.0 + dist)).clamp(0.12, 0.7);
            let elev = world.tiles[nidx].raw_elevation;
            total += weight;
            if elev > alpine_threshold {
                support += weight;
            } else if elev > ridge_threshold {
                support += weight * 0.45;
            }
        }
    }

    if total <= f32::EPSILON {
        0.0
    } else {
        (support / total).clamp(0.0, 1.0)
    }
}

fn local_relief(world: &World, idx: usize) -> f32 {
    let (x, y) = world.coords(idx);
    let current = world.tiles[idx].raw_elevation;
    let mut max_drop = 0.0_f32;
    let mut max_rise = 0.0_f32;
    for (nx, ny) in world.neighbors8(x, y) {
        let elev = world.tiles[world.idx(nx, ny)].raw_elevation;
        max_drop = max_drop.max((current - elev).max(0.0));
        max_rise = max_rise.max((elev - current).max(0.0));
    }
    (max_drop + max_rise * 0.5).clamp(0.0, 1.0)
}

pub fn biome_for_tile(
    surface: Surface,
    elevation: f32,
    sea_level: f32,
    temperature: f32,
    moisture: f32,
) -> Biome {
    biome_for_tile_with_support(BiomeContext {
        surface,
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
    match ctx.surface {
        Surface::Ocean => Biome::Ocean,
        Surface::Coast => Biome::Coast,
        Surface::Land => land_biome_with_support(ctx),
    }
}

fn land_biome_with_support(ctx: BiomeContext) -> Biome {
    if ctx.elevation > ctx.sea_level + 0.38 && ctx.support > 0.5 {
        return Biome::Alpine;
    }
    if ctx.elevation > ctx.sea_level + 0.31
        && ctx.support > 0.24
        && ctx.proximity > 0.2
        && ctx.relief > 0.04
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
