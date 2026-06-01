mod biomes;
mod climate;
mod hydrology;
mod ocean;
mod terrain;
mod util;

use noise::OpenSimplex;

use crate::features::mountain_feature_for_tile;
use crate::{MountainFeature, Surface, World, WorldConfig};

pub use biomes::biome_for_tile;
pub(crate) use util::{hash01, smoothstep, value_noise};

pub fn generate_world(config: &WorldConfig) -> Result<World, String> {
    config.validate()?;

    let mut world = World::new(
        config.seed,
        config.width,
        config.height,
        config.sea_level,
        config.world_size,
    );

    let base = OpenSimplex::new(config.seed as u32);
    let ridge = OpenSimplex::new(config.seed.wrapping_add(1) as u32);
    let climate_noise = OpenSimplex::new(config.seed.wrapping_add(2) as u32);

    let mut terrain_fields = terrain::generate_terrain_fields(&world, &base, &ridge);
    terrain::finalize_sea_level(&mut world, &terrain_fields);

    let mut ocean = ocean::classify_ocean(&world, &terrain_fields.elevation);
    let mut surfaces = ocean::classify_surfaces(&world, &ocean);
    let mut distance_to_ocean = climate::fill_ocean_distance(&world, &ocean);
    let mut climate_fields = climate::generate_climate_fields(
        &world,
        config,
        &terrain_fields,
        &ocean,
        &distance_to_ocean,
        &climate_noise,
    );

    let mutation_context = terrain::TerrainMutationContext {
        ocean: &ocean,
        surfaces: &surfaces,
        climate: &climate_fields,
    };
    if terrain::apply_terrain_mutators(&world, &mutation_context, &mut terrain_fields) {
        terrain::refresh_derived_fields(&world, &mut terrain_fields);
        terrain::finalize_sea_level(&mut world, &terrain_fields);
        ocean = ocean::classify_ocean(&world, &terrain_fields.elevation);
        surfaces = ocean::classify_surfaces(&world, &ocean);
        distance_to_ocean = climate::fill_ocean_distance(&world, &ocean);
        climate_fields = climate::generate_climate_fields(
            &world,
            config,
            &terrain_fields,
            &ocean,
            &distance_to_ocean,
            &climate_noise,
        );
    }

    let hydrology_fields = hydrology::generate_hydrology_fields(
        &world,
        &terrain_fields,
        &climate_fields,
        &ocean,
        &surfaces,
    );
    let biome_fields = biomes::assign_biomes(
        &world,
        &terrain_fields,
        &climate_fields,
        &hydrology_fields,
        &surfaces,
        &climate_noise,
    );

    commit_tiles(
        &mut world,
        &terrain_fields,
        &surfaces,
        &climate_fields,
        &hydrology_fields,
        &biome_fields,
    );

    Ok(world)
}

fn commit_tiles(
    world: &mut World,
    terrain: &terrain::TerrainFields,
    surfaces: &[Surface],
    climate: &climate::ClimateFields,
    hydrology: &hydrology::HydrologyFields,
    biomes: &biomes::BiomeFields,
) {
    for idx in 0..world.tile_count() {
        let tile = &mut world.tiles[idx];
        tile.raw_elevation = terrain.elevation[idx];
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
}
