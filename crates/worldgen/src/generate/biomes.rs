use noise::OpenSimplex;

use crate::{Biome, Surface, World};

use super::climate::ClimateFields;
use super::hydrology::HydrologyFields;
use super::terrain::TerrainFields;
use super::util::{octave_noise, smoothstep};

pub(super) struct BiomeFields {
    pub(super) biome: Vec<Biome>,
    pub(super) moisture: Vec<f32>,
}

#[derive(Clone, Copy)]
struct BiomeSample {
    biome: Biome,
    moisture: f32,
}

#[derive(Clone, Copy)]
struct BiomeContext {
    surface: Surface,
    elevation: f32,
    sea_level: f32,
    temperature: f32,
    moisture: f32,
    climate_moisture: f32,
    river_influence: f32,
    runoff: f32,
    flow_accumulation: f32,
    lake_depth: f32,
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

pub(super) fn assign_biomes(
    world: &World,
    terrain: &TerrainFields,
    climate: &ClimateFields,
    hydrology: &HydrologyFields,
    surfaces: &[Surface],
    ecotone_noise: &OpenSimplex,
) -> BiomeFields {
    let mut biome = Vec::with_capacity(world.tile_count());
    let mut moisture = Vec::with_capacity(world.tile_count());

    for idx in 0..world.tile_count() {
        let sample = biome_for_world_tile(
            world,
            terrain,
            climate,
            hydrology,
            surfaces,
            ecotone_noise,
            idx,
        );
        biome.push(sample.biome);
        moisture.push(sample.moisture);
    }

    BiomeFields { biome, moisture }
}

fn biome_for_world_tile(
    world: &World,
    terrain: &TerrainFields,
    climate: &ClimateFields,
    hydrology: &HydrologyFields,
    surfaces: &[Surface],
    ecotone_noise: &OpenSimplex,
    idx: usize,
) -> BiomeSample {
    let mut ctx = BiomeContext {
        surface: surfaces[idx],
        elevation: terrain.elevation[idx],
        sea_level: world.sea_level,
        temperature: climate.temperature[idx],
        moisture: climate.moisture[idx],
        climate_moisture: climate.moisture[idx],
        river_influence: riparian_influence(world, surfaces, hydrology, idx),
        runoff: hydrology.runoff[idx],
        flow_accumulation: hydrology.flow_accumulation[idx],
        lake_depth: hydrology.lake_depth[idx],
        support: mountain_support(world, terrain, idx),
        proximity: mountain_proximity(world, terrain, idx),
        relief: terrain.relief[idx],
    };
    apply_ecotone_variation(world, ecotone_noise, idx, &mut ctx);
    apply_riparian_moisture(&mut ctx);
    let moisture = ctx.moisture;
    BiomeSample {
        biome: biome_for_tile_with_support(ctx),
        moisture,
    }
}

fn riparian_influence(
    world: &World,
    surfaces: &[Surface],
    hydrology: &HydrologyFields,
    idx: usize,
) -> f32 {
    if surfaces[idx] != Surface::Land {
        return 0.0;
    }

    let (x, y) = world.coords(idx);
    let mut influence = riparian_water_signal(hydrology, idx);

    for dy in -3_isize..=3 {
        for dx in -3_isize..=3 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if !world.in_bounds(nx, ny) {
                continue;
            }
            let nidx = world.idx(nx as usize, ny as usize);
            if surfaces[nidx] == Surface::Ocean {
                continue;
            }
            let dist2 = (dx * dx + dy * dy) as f32;
            let falloff = (1.0 / (1.0 + dist2 * 0.72)).clamp(0.0, 0.72);
            influence = influence.max(riparian_water_signal(hydrology, nidx) * falloff);
        }
    }

    influence.clamp(0.0, 1.0)
}

fn riparian_water_signal(hydrology: &HydrologyFields, idx: usize) -> f32 {
    let river = smoothstep(0.08, 0.70, hydrology.river[idx]);
    let depth = smoothstep(0.08, 0.88, hydrology.river_depth[idx]);
    let width = smoothstep(0.10, 0.92, hydrology.river_width[idx]);
    let channel_capacity = (0.34 + depth * 0.46 + width * 0.20).clamp(0.0, 1.0);
    let channel = river * channel_capacity;
    let lake = hydrology.lake_depth[idx] * 0.90;
    let spill = hydrology.spill_discharge[idx] * (0.42 + depth * 0.28 + width * 0.14);

    channel.max(lake).max(spill).clamp(0.0, 1.0)
}

fn apply_riparian_moisture(ctx: &mut BiomeContext) {
    if ctx.surface != Surface::Land || ctx.river_influence <= 0.0 {
        return;
    }

    let height_above_sea = (ctx.elevation - ctx.sea_level).max(0.0);
    let lowland = 1.0 - smoothstep(0.20, 0.54, height_above_sea);
    let mountain_guard =
        (ctx.support * 0.44 + smoothstep(0.030, 0.080, ctx.relief) * 0.34).clamp(0.0, 0.72);
    let usable = (0.40 + lowland * 0.60) * (1.0 - mountain_guard);
    let dryland_response = 0.55 + (1.0 - ctx.moisture).clamp(0.0, 1.0) * 0.65;
    let boost = ctx.river_influence * usable * dryland_response * 0.34;

    ctx.moisture = (ctx.moisture + boost).clamp(0.0, 1.0);
}

fn apply_ecotone_variation(
    world: &World,
    ecotone_noise: &OpenSimplex,
    idx: usize,
    ctx: &mut BiomeContext,
) {
    if ctx.surface != Surface::Land {
        return;
    }

    let height_above_sea = (ctx.elevation - ctx.sea_level).max(0.0);
    let lowland = 1.0 - smoothstep(0.16, 0.44, height_above_sea);
    let rugged = smoothstep(0.030, 0.085, ctx.relief);
    let mountain_guard = (ctx.support * 0.42 + rugged * 0.34).clamp(0.0, 0.68);
    let strength = (0.42 + lowland * 0.58) * (1.0 - mountain_guard);
    if strength <= 0.05 {
        return;
    }

    let (x, y) = world.coords(idx);
    let ws = world.effective_world_size() as f64;
    let xf = x as f64 / ws;
    let yf = y as f64 / ws;
    let rxf = xf * 0.82 + yf * 0.57;
    let ryf = yf * 0.82 - xf * 0.57;
    let thermal_macro = octave_noise(
        ecotone_noise,
        rxf * 3.6 + 31.0,
        ryf * 3.6 - 17.0,
        3,
        0.56,
        2.0,
    ) - 0.5;
    let thermal_meso = octave_noise(
        ecotone_noise,
        (xf * 8.4 - yf * 1.9) - 73.0,
        (yf * 8.4 + xf * 1.9) + 41.0,
        2,
        0.54,
        2.0,
    ) - 0.5;
    let thermal_fine = octave_noise(
        ecotone_noise,
        (xf * 16.0 + yf * 3.5) + 107.0,
        (yf * 16.0 - xf * 3.5) - 83.0,
        2,
        0.52,
        2.0,
    ) - 0.5;
    let moisture_macro = octave_noise(
        ecotone_noise,
        (xf * 3.5 + yf * 1.4) - 151.0,
        (yf * 3.5 - xf * 1.4) + 97.0,
        3,
        0.56,
        2.0,
    ) - 0.5;
    let moisture_meso = octave_noise(
        ecotone_noise,
        xf * 11.0 + 211.0,
        yf * 11.0 - 61.0,
        2,
        0.54,
        2.0,
    ) - 0.5;
    let moisture_fine = octave_noise(
        ecotone_noise,
        xf * 22.0 - 19.0,
        yf * 22.0 + 173.0,
        2,
        0.52,
        2.0,
    ) - 0.5;

    let thermal_edge = transition_weight(ctx.temperature, &[0.12, 0.28, 0.48, 0.72], 0.13);
    let moisture_edge =
        transition_weight(ctx.moisture, &[0.16, 0.26, 0.36, 0.48, 0.62, 0.68], 0.12);
    let thermal_shift = (thermal_macro * 0.170 + thermal_meso * 0.074 + thermal_fine * 0.026)
        * strength
        * (0.66 + thermal_edge * 0.34);
    let moisture_shift = (moisture_macro * 0.160 + moisture_meso * 0.075 + moisture_fine * 0.028)
        * strength
        * (0.64 + moisture_edge * 0.36);

    ctx.temperature = (ctx.temperature + thermal_shift).clamp(0.0, 1.0);
    ctx.moisture = (ctx.moisture + moisture_shift).clamp(0.0, 1.0);
}

fn transition_weight(value: f32, thresholds: &[f32], width: f32) -> f32 {
    let nearest = thresholds
        .iter()
        .map(|threshold| (value - threshold).abs())
        .fold(f32::MAX, f32::min);
    1.0 - smoothstep(width * 0.25, width, nearest)
}

fn mountain_support(world: &World, terrain: &TerrainFields, idx: usize) -> f32 {
    let high_threshold = world.sea_level + 0.24;
    let alpine_threshold = world.sea_level + 0.34;
    weighted_neighbor_support(
        world,
        terrain,
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

fn mountain_proximity(world: &World, terrain: &TerrainFields, idx: usize) -> f32 {
    let alpine_threshold = world.sea_level + 0.38;
    let ridge_threshold = world.sea_level + 0.32;
    weighted_neighbor_support(
        world,
        terrain,
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
    terrain: &TerrainFields,
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
            let elev = terrain.elevation[nidx];
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
        climate_moisture: moisture,
        river_influence: 0.0,
        runoff: 0.0,
        flow_accumulation: 0.0,
        lake_depth: 0.0,
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
    let height_above_sea = (ctx.elevation - ctx.sea_level).max(0.0);
    if ctx.lake_depth > 0.0 {
        return Biome::Freshwater;
    }
    if ctx.elevation > ctx.sea_level + 0.36
        && ctx.support > 0.50
        && (ctx.relief > 0.034
            || (ctx.elevation > ctx.sea_level + 0.48 && ctx.support > 0.58 && ctx.proximity > 0.24))
    {
        return Biome::Alpine;
    }
    if ctx.elevation > ctx.sea_level + 0.27
        && ctx.support > 0.18
        && ctx.proximity > 0.16
        && ctx.relief > 0.024
    {
        return Biome::Foothills;
    }
    if ctx.temperature > 0.10
        && height_above_sea < 0.14
        && ctx.relief < 0.026
        && ctx.climate_moisture > 0.47
        && (ctx.climate_moisture + ctx.runoff * 0.28) > 0.52
        && ctx.flow_accumulation < 3.0
    {
        return Biome::Freshwater;
    }
    if ctx.temperature > 0.14
        && height_above_sea < 0.22
        && ctx.relief < 0.036
        && ctx.climate_moisture > 0.40
        && (ctx.climate_moisture + ctx.river_influence * 0.34) > 0.58
    {
        return Biome::Wetland;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generate::terrain::TerrainFields;

    fn empty_hydrology(world: &World) -> HydrologyFields {
        HydrologyFields {
            runoff: vec![0.0; world.tile_count()],
            flow_accumulation: vec![0.0; world.tile_count()],
            river: vec![0.0; world.tile_count()],
            river_depth: vec![0.0; world.tile_count()],
            river_width: vec![0.0; world.tile_count()],
            river_order: vec![0; world.tile_count()],
            lake_depth: vec![0.0; world.tile_count()],
            water_body_id: vec![0; world.tile_count()],
            lake_inflow: vec![0.0; world.tile_count()],
            lake_outlet: vec![false; world.tile_count()],
            spill_discharge: vec![0.0; world.tile_count()],
            erosion: vec![0.0; world.tile_count()],
            flow_direction: vec![-1; world.tile_count()],
        }
    }

    #[test]
    fn riparian_signal_scales_with_simulated_depth_and_width() {
        let world = World::new(7, 1, 1, 0.50, 0);
        let mut shallow = empty_hydrology(&world);
        shallow.river[0] = 0.82;
        shallow.river_depth[0] = 0.12;
        shallow.river_width[0] = 0.16;

        let mut deep = empty_hydrology(&world);
        deep.river[0] = 0.82;
        deep.river_depth[0] = 0.88;
        deep.river_width[0] = 0.64;

        let shallow_signal = riparian_water_signal(&shallow, 0);
        let deep_signal = riparian_water_signal(&deep, 0);

        assert!(
            deep_signal > shallow_signal + 0.34,
            "deep/wide sustained channel should hydrate more regionally: shallow={shallow_signal} deep={deep_signal}"
        );
    }

    #[test]
    fn assigned_biome_moisture_exports_depth_scaled_riparian_boost() {
        let world = World::new(7, 3, 3, 0.50, 0);
        let terrain = TerrainFields {
            elevation: vec![0.56; world.tile_count()],
            slope: vec![0.008; world.tile_count()],
            relief: vec![0.010; world.tile_count()],
        };
        let climate = ClimateFields {
            temperature: vec![0.55; world.tile_count()],
            moisture: vec![0.22; world.tile_count()],
            precipitation: vec![0.22; world.tile_count()],
            ocean_distance: vec![12; world.tile_count()],
            continentality: vec![0.35; world.tile_count()],
        };
        let surfaces = vec![Surface::Land; world.tile_count()];
        let center = world.idx(1, 1);

        let mut shallow = empty_hydrology(&world);
        shallow.river[center] = 0.82;
        shallow.river_depth[center] = 0.12;
        shallow.river_width[center] = 0.16;

        let mut deep = empty_hydrology(&world);
        deep.river[center] = 0.82;
        deep.river_depth[center] = 0.88;
        deep.river_width[center] = 0.64;

        let noise = OpenSimplex::new(17);
        let shallow_fields = assign_biomes(&world, &terrain, &climate, &shallow, &surfaces, &noise);
        let deep_fields = assign_biomes(&world, &terrain, &climate, &deep, &surfaces, &noise);

        assert!(
            deep_fields.moisture[center] > shallow_fields.moisture[center] + 0.06,
            "exported moisture should carry the depth-scaled riparian boost: shallow={} deep={}",
            shallow_fields.moisture[center],
            deep_fields.moisture[center]
        );
    }
}
