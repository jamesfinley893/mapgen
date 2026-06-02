use crate::{Biome, MountainFeature, WaterClass, World};

#[derive(Clone, Copy)]
pub struct BiomeInputs {
    pub water: WaterClass,
    pub elevation: f32,
    pub sea_level: f32,
    pub temperature: f32,
    pub moisture: f32,
    pub mountain_feature: MountainFeature,
    pub mountain_presence: f32,
    pub mountain_proximity: f32,
    pub relief: f32,
}

impl Default for BiomeInputs {
    fn default() -> Self {
        Self {
            water: WaterClass::Land,
            elevation: 0.0,
            sea_level: 0.5,
            temperature: 0.0,
            moisture: 0.0,
            mountain_feature: MountainFeature::None,
            mountain_presence: 0.0,
            mountain_proximity: 0.0,
            relief: 0.08,
        }
    }
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
    biome_for_tile(BiomeInputs {
        water: tile.water,
        elevation: tile.elevation,
        sea_level: world.sea_level,
        temperature: tile.temperature,
        moisture: tile.moisture,
        mountain_feature: tile.mountain_feature,
        mountain_presence: tile.mountain_presence,
        mountain_proximity: mountain_proximity(world, idx),
        relief: tile.relief,
    })
}

fn mountain_proximity(world: &World, idx: usize) -> f32 {
    let (x, y) = world.coords(idx);
    let mut support = 0.0_f32;
    let mut total = 0.0_f32;

    for dy in -4_isize..=4 {
        for dx in -4_isize..=4 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if !world.in_bounds(nx, ny) {
                continue;
            }
            let nidx = world.idx(nx as usize, ny as usize);
            if !world.tiles[nidx].is_land() {
                continue;
            }
            let dist = ((dx * dx + dy * dy) as f32).sqrt();
            let weight = (1.0 / (1.0 + dist)).clamp(0.12, 0.7);
            total += weight;
            support += world.tiles[nidx].mountain_presence * weight;
        }
    }

    if total <= f32::EPSILON {
        0.0
    } else {
        (support / total).clamp(0.0, 1.0)
    }
}

pub fn biome_for_tile(ctx: BiomeInputs) -> Biome {
    match ctx.water {
        WaterClass::Ocean => Biome::Ocean,
        WaterClass::Lake => Biome::Freshwater,
        WaterClass::Land => land_biome_with_support(ctx),
    }
}

fn land_biome_with_support(ctx: BiomeInputs) -> Biome {
    if matches!(
        ctx.mountain_feature,
        MountainFeature::AlpineSlope | MountainFeature::Ridge | MountainFeature::Summit
    ) && ctx.mountain_presence > 0.40
    {
        return Biome::Alpine;
    }
    if ctx.mountain_feature == MountainFeature::Foothill
        || (ctx.elevation > ctx.sea_level + 0.27
            && ctx.mountain_presence > 0.22
            && ctx.mountain_proximity > 0.14
            && ctx.relief > 0.018)
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
