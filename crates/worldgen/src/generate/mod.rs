mod biomes;
mod climate;
mod hydrology;
mod ocean;
mod terrain;
mod util;

use noise::OpenSimplex;
use std::time::{Duration, Instant};

use crate::features::mountain_feature_for_tile;
use crate::{Landform, MountainFeature, Surface, World, WorldConfig};

pub use biomes::biome_for_tile;
pub(crate) use util::smoothstep;
use util::value_noise;

#[derive(Debug, Clone, Default)]
pub struct GenerationProfile {
    stages: Vec<GenerationStageTiming>,
}

#[derive(Debug, Clone, Copy)]
pub struct GenerationStageTiming {
    pub name: &'static str,
    pub duration: Duration,
}

impl GenerationProfile {
    pub fn stages(&self) -> &[GenerationStageTiming] {
        &self.stages
    }

    pub fn total_duration(&self) -> Duration {
        self.stages.iter().map(|stage| stage.duration).sum()
    }

    pub(crate) fn record(&mut self, name: &'static str, duration: Duration) {
        self.stages.push(GenerationStageTiming { name, duration });
    }

    pub(crate) fn time<T>(&mut self, name: &'static str, f: impl FnOnce() -> T) -> T {
        let start = Instant::now();
        let value = f();
        self.record(name, start.elapsed());
        value
    }
}

pub fn generate_world(config: &WorldConfig) -> Result<World, String> {
    generate_world_with_profile(config).map(|(world, _)| world)
}

pub fn generate_world_with_profile(
    config: &WorldConfig,
) -> Result<(World, GenerationProfile), String> {
    config.validate()?;
    let mut profile = GenerationProfile::default();

    let (mut world, base, ridge, climate_noise) = profile.time("world setup", || {
        (
            World::new(
                config.seed,
                config.width,
                config.height,
                config.sea_level,
                config.world_size,
            ),
            OpenSimplex::new(config.seed as u32),
            OpenSimplex::new(config.seed.wrapping_add(1) as u32),
            OpenSimplex::new(config.seed.wrapping_add(2) as u32),
        )
    });

    let mut terrain_fields = terrain::generate_terrain_fields(&world, &base, &ridge, &mut profile);
    profile.time("sea level", || {
        terrain::finalize_sea_level(&mut world, &terrain_fields);
    });

    let (mut ocean, mut surfaces, mut distance_to_ocean) =
        profile.time("surface classification", || {
            let ocean = ocean::classify_ocean(&world, &terrain_fields.elevation);
            let surfaces = ocean::classify_surfaces(&world, &ocean);
            let distance_to_ocean = climate::fill_ocean_distance(&world, &ocean);
            (ocean, surfaces, distance_to_ocean)
        });
    let mut climate_fields = profile.time("climate", || {
        climate::generate_climate_fields(
            &world,
            config,
            &terrain_fields,
            &ocean,
            &distance_to_ocean,
            &climate_noise,
        )
    });

    let mutation_context = terrain::TerrainMutationContext {
        ocean: &ocean,
        surfaces: &surfaces,
        climate: &climate_fields,
    };
    let terrain_mutated = profile.time("terrain mutation", || {
        terrain::apply_terrain_mutators(&world, &mutation_context, &mut terrain_fields)
    });
    if terrain_mutated {
        terrain::refresh_derived_fields(&world, &mut terrain_fields);
        terrain::finalize_sea_level(&mut world, &terrain_fields);
        (ocean, surfaces, distance_to_ocean) = profile.time("surface reclassification", || {
            let ocean = ocean::classify_ocean(&world, &terrain_fields.elevation);
            let surfaces = ocean::classify_surfaces(&world, &ocean);
            let distance_to_ocean = climate::fill_ocean_distance(&world, &ocean);
            (ocean, surfaces, distance_to_ocean)
        });
        climate_fields = profile.time("climate refresh", || {
            climate::generate_climate_fields(
                &world,
                config,
                &terrain_fields,
                &ocean,
                &distance_to_ocean,
                &climate_noise,
            )
        });
    }

    let hydrology_fields = profile.time("hydrology", || {
        hydrology::generate_hydrology_fields(
            &world,
            &terrain_fields,
            &climate_fields,
            &ocean,
            &surfaces,
        )
    });
    let biome_fields = profile.time("biomes", || {
        biomes::assign_biomes(
            &world,
            &terrain_fields,
            &climate_fields,
            &hydrology_fields,
            &surfaces,
            &climate_noise,
        )
    });

    profile.time("tile commit", || {
        commit_tiles(
            &mut world,
            &terrain_fields,
            &surfaces,
            &climate_fields,
            &hydrology_fields,
            &biome_fields,
        );
    });

    Ok((world, profile))
}

fn commit_tiles(
    world: &mut World,
    terrain: &terrain::TerrainFields,
    surfaces: &[Surface],
    climate: &climate::ClimateFields,
    hydrology: &hydrology::HydrologyFields,
    biomes: &biomes::BiomeFields,
) {
    let elevation_shade = (0..world.tile_count())
        .map(|idx| elevation_shade_for_tile(world, terrain, idx))
        .collect::<Vec<_>>();

    for idx in 0..world.tile_count() {
        let tile = &mut world.tiles[idx];
        tile.raw_elevation = terrain.elevation[idx];
        tile.elevation_shade = elevation_shade[idx];
        tile.slope = terrain.slope[idx];
        tile.relief = terrain.relief[idx];
        tile.runoff = hydrology.runoff[idx];
        tile.flow_accumulation = hydrology.flow_accumulation[idx];
        tile.river = hydrology.river[idx];
        tile.river_depth = hydrology.river_depth[idx];
        tile.river_width = hydrology.river_width[idx];
        tile.river_order = hydrology.river_order[idx];
        tile.lake_depth = hydrology.lake_depth[idx];
        tile.water_body_id = hydrology.water_body_id[idx];
        tile.lake_inflow = hydrology.lake_inflow[idx];
        tile.lake_outlet = hydrology.lake_outlet[idx];
        tile.spill_discharge = hydrology.spill_discharge[idx];
        tile.erosion = hydrology.erosion[idx];
        tile.flow_direction = hydrology.flow_direction[idx];
        tile.temperature = climate.temperature[idx];
        tile.moisture = biomes.moisture[idx];
        tile.precipitation = climate.precipitation[idx];
        tile.continentality = climate.continentality[idx];
        tile.ocean_distance = climate.ocean_distance[idx];
        tile.surface = surfaces[idx];
        tile.biome = biomes.biome[idx];
        tile.mountain_feature = MountainFeature::None;
    }

    let mountain_features = (0..world.tile_count())
        .map(|idx| mountain_feature_for_tile(world, idx))
        .collect::<Vec<_>>();
    for (tile, feature) in world.tiles.iter_mut().zip(mountain_features.into_iter()) {
        tile.mountain_feature = feature;
    }

    derive_presentation_fields(world);
}

fn derive_presentation_fields(world: &mut World) {
    let presentation = (0..world.tile_count())
        .map(|idx| {
            let landform = landform_for_tile(world, idx);
            (
                landform,
                terrain_texture_for_tile(world, idx, landform),
                ecotone_strength_for_tile(&world.tiles[idx], landform),
                shore_influence_for_tile(world, idx),
            )
        })
        .collect::<Vec<_>>();

    for (tile, (landform, terrain_texture, ecotone_strength, shore_influence)) in
        world.tiles.iter_mut().zip(presentation.into_iter())
    {
        tile.landform = landform;
        tile.terrain_texture = terrain_texture.clamp(0.0, 1.0);
        tile.ecotone_strength = ecotone_strength.clamp(0.0, 1.0);
        tile.shore_influence = shore_influence.clamp(0.0, 1.0);
    }
}

fn landform_for_tile(world: &World, idx: usize) -> Landform {
    let tile = &world.tiles[idx];
    if tile.surface == Surface::Ocean
        || matches!(tile.biome, crate::Biome::Ocean | crate::Biome::Freshwater)
        || tile.lake_depth > 0.0
    {
        return Landform::Water;
    }
    if tile.surface == Surface::Coast || tile.biome == crate::Biome::Coast {
        return Landform::Shore;
    }

    let stats = committed_neighbor_stats(world, idx);
    let height_above_sea = (tile.raw_elevation - world.sea_level).max(0.0);
    let rugged = (tile.relief + tile.slope * 0.75).clamp(0.0, 1.0);
    let channel = tile
        .river
        .max(tile.spill_discharge * 0.9)
        .max(tile.erosion * 1.25)
        .clamp(0.0, 1.0);
    let local_low = stats.higher_neighbors >= 5 && stats.lower_neighbors <= 2;
    let local_high = stats.lower_neighbors >= 4 && stats.higher_neighbors <= 2;

    if matches!(tile.mountain_feature, MountainFeature::Summit)
        || (height_above_sea > 0.34 && local_high && rugged > 0.045)
    {
        Landform::Peak
    } else if matches!(tile.mountain_feature, MountainFeature::Ridge)
        || (height_above_sea > 0.22 && local_high && rugged > 0.034)
    {
        Landform::Ridge
    } else if channel > 0.10
        || (tile.flow_accumulation > 2.4 && local_low)
        || (tile.erosion > 0.055 && height_above_sea > 0.025)
    {
        Landform::Valley
    } else if height_above_sea < 0.17
        && local_low
        && tile.relief < 0.040
        && tile.continentality > 0.30
    {
        Landform::Basin
    } else if matches!(tile.mountain_feature, MountainFeature::Foothill)
        || rugged > 0.045
        || (height_above_sea > 0.17 && rugged > 0.024)
    {
        Landform::Hill
    } else {
        Landform::Plain
    }
}

#[derive(Clone, Copy)]
struct CommittedNeighborStats {
    lower_neighbors: u8,
    higher_neighbors: u8,
}

fn committed_neighbor_stats(world: &World, idx: usize) -> CommittedNeighborStats {
    let tile = &world.tiles[idx];
    let mut lower_neighbors = 0_u8;
    let mut higher_neighbors = 0_u8;

    for nidx in world.neighbor_indices8(idx) {
        let neighbor = &world.tiles[nidx];
        if neighbor.surface == Surface::Ocean {
            continue;
        }
        if tile.raw_elevation > neighbor.raw_elevation + 0.006 {
            lower_neighbors += 1;
        } else if neighbor.raw_elevation > tile.raw_elevation + 0.006 {
            higher_neighbors += 1;
        }
    }

    CommittedNeighborStats {
        lower_neighbors,
        higher_neighbors,
    }
}

fn terrain_texture_for_tile(world: &World, idx: usize, landform: Landform) -> f32 {
    let tile = &world.tiles[idx];

    let (x, y) = world.coords(idx);
    let coarse = value_noise(
        world.seed ^ 0xA171_5A11_D3C0_0001,
        x,
        y,
        scaled_texture_cell(world, 28),
    );
    let medium = value_noise(
        world.seed ^ 0xA171_5A11_D3C0_0002,
        x,
        y,
        scaled_texture_cell(world, 11),
    );
    let fine = value_noise(
        world.seed ^ 0xA171_5A11_D3C0_0003,
        x,
        y,
        scaled_texture_cell(world, 5),
    );
    let noise = coarse * 0.46 + medium * 0.36 + fine * 0.18;
    if matches!(landform, Landform::Water) {
        let depth = if tile.surface == Surface::Ocean {
            (world.sea_level - tile.raw_elevation).max(0.0)
        } else {
            tile.lake_depth.max(0.0)
        };
        let shore = shore_influence_for_tile(world, idx);
        let depth_detail = smoothstep(0.015, 0.18, depth);
        let gain = (0.12 + shore * 0.18 + depth_detail * 0.08).clamp(0.10, 0.30);
        return (0.5 + (noise - 0.5) * gain).clamp(0.0, 1.0);
    }

    let rugged = smoothstep(0.018, 0.115, tile.relief + tile.slope * 0.75);
    let wetness = smoothstep(0.45, 0.88, tile.moisture);
    let erosion = smoothstep(0.025, 0.145, tile.erosion);
    let landform_gain = match landform {
        Landform::Peak | Landform::Ridge => 0.34,
        Landform::Hill | Landform::Valley => 0.24,
        Landform::Basin | Landform::Shore => 0.16,
        Landform::Plain => 0.12,
        Landform::Water => 0.0,
    };
    let gain = (0.10 + rugged * 0.18 + erosion * 0.10 + landform_gain).clamp(0.08, 0.46);
    let bias = match landform {
        Landform::Valley => -0.035 * wetness,
        Landform::Basin => -0.025,
        Landform::Ridge | Landform::Peak => 0.030,
        Landform::Shore => 0.020,
        _ => 0.0,
    };

    (0.5 + (noise - 0.5) * gain + bias).clamp(0.0, 1.0)
}

fn ecotone_strength_for_tile(tile: &crate::Tile, landform: Landform) -> f32 {
    if tile.surface != Surface::Land || matches!(landform, Landform::Water | Landform::Peak) {
        return 0.0;
    }

    let thermal = transition_weight(tile.temperature, &[0.12, 0.28, 0.48, 0.72], 0.11);
    let moisture = transition_weight(tile.moisture, &[0.16, 0.26, 0.36, 0.48, 0.62, 0.68], 0.10);
    let relief_guard = 1.0 - smoothstep(0.050, 0.135, tile.relief + tile.slope * 0.55) * 0.62;
    let hydrology_edge =
        transition_weight(tile.river.max(tile.lake_depth), &[0.035, 0.18, 0.55], 0.09) * 0.18;

    ((thermal * 0.38 + moisture * 0.48 + hydrology_edge) * relief_guard).clamp(0.0, 1.0)
}

fn shore_influence_for_tile(world: &World, idx: usize) -> f32 {
    let tile = &world.tiles[idx];
    let height_above_sea = (tile.raw_elevation - world.sea_level).max(0.0);
    let depth = (world.sea_level - tile.raw_elevation).max(0.0);
    let physical_ocean_distance = if tile.ocean_distance == u16::MAX {
        f32::MAX
    } else {
        tile.ocean_distance as f32 / world.high_detail_scale()
    };
    let ocean_edge = 1.0 - smoothstep(0.0, 5.5, physical_ocean_distance);
    let shallow_ocean = (1.0 - smoothstep(0.010, 0.115, depth)).max(0.0);
    let low_shore = 1.0 - smoothstep(0.018, 0.115, height_above_sea);
    let coast = if tile.surface == Surface::Coast {
        0.88
    } else {
        ocean_edge * low_shore * 0.62
    };
    let lake_margin = if tile.lake_depth > 0.0 {
        1.0 - smoothstep(0.06, 0.56, tile.lake_depth)
    } else {
        0.0
    };
    let channel_margin = smoothstep(0.035, 0.72, tile.river.max(tile.spill_discharge * 0.85))
        * (0.24 + tile.river_width * 0.44 + tile.river_depth * 0.22);

    if tile.surface == Surface::Ocean {
        shallow_ocean
    } else {
        coast.max(lake_margin).max(channel_margin).clamp(0.0, 1.0)
    }
}

fn transition_weight(value: f32, thresholds: &[f32], width: f32) -> f32 {
    let nearest = thresholds
        .iter()
        .map(|threshold| (value - threshold).abs())
        .fold(f32::MAX, f32::min);
    1.0 - smoothstep(width * 0.25, width, nearest)
}

fn scaled_texture_cell(world: &World, base: usize) -> usize {
    (base as f32 * world.high_detail_scale())
        .round()
        .max(base as f32) as usize
}

fn elevation_shade_for_tile(world: &World, terrain: &terrain::TerrainFields, idx: usize) -> f32 {
    let center = terrain.elevation[idx];
    if center <= world.sea_level {
        return 0.5;
    }

    let (x, y) = world.coords(idx);
    let sample = |dx: isize, dy: isize| -> f32 {
        let nx = (x as isize + dx).clamp(0, world.width as isize - 1) as usize;
        let ny = (y as isize + dy).clamp(0, world.height as isize - 1) as usize;
        visual_elevation(world, terrain.elevation[world.idx(nx, ny)])
    };

    let dz_dx = sample(1, 0) - sample(-1, 0);
    let dz_dy = sample(0, 1) - sample(0, -1);
    let height_above_sea = (center - world.sea_level).max(0.0);
    let visual_height = height_above_sea / (height_above_sea + 0.82);
    let z_scale = 8.0 + visual_height * 30.0 + terrain.relief[idx] * 8.0;
    let nx = -dz_dx * z_scale;
    let ny = 1.0_f32;
    let nz = -dz_dy * z_scale;
    let len = (nx * nx + ny * ny + nz * nz).sqrt().max(1e-6);
    let inv_sqrt3 = 1.0_f32 / 3.0_f32.sqrt();

    ((nx * (-inv_sqrt3) + ny * inv_sqrt3 + nz * (-inv_sqrt3)) / len).clamp(0.0, 1.0)
}

fn visual_elevation(world: &World, elevation: f32) -> f32 {
    let height_above_sea = (elevation - world.sea_level).max(0.0);
    height_above_sea / (height_above_sea + 0.74)
}
