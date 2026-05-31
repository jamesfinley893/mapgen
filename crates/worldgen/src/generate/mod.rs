mod biomes;
mod climate;
mod ocean;
mod terrain;
mod util;

use noise::OpenSimplex;

use crate::features::mountain_feature_for_tile;
use crate::{Biome, MountainFeature, Surface, World, WorldConfig};

pub use biomes::biome_for_tile;
pub(crate) use util::{hash01, smoothstep};

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
    terrain::apply_terrain_mutators(&world, &mut terrain_fields);
    terrain::refresh_derived_fields(&world, &mut terrain_fields);
    terrain::finalize_sea_level(&mut world, &terrain_fields);

    let ocean = ocean::classify_ocean(&world, &terrain_fields.elevation);
    let surfaces = ocean::classify_surfaces(&world, &ocean);
    let distance_to_ocean = climate::fill_ocean_distance(&world, &ocean);
    let climate_fields = climate::generate_climate_fields(
        &world,
        config,
        &terrain_fields,
        &ocean,
        &distance_to_ocean,
        &climate_noise,
    );
    let biomes = biomes::assign_biomes(&world, &terrain_fields, &climate_fields, &surfaces);

    commit_tiles(
        &mut world,
        &terrain_fields,
        &surfaces,
        &climate_fields,
        &biomes,
    );

    Ok(world)
}

fn commit_tiles(
    world: &mut World,
    terrain: &terrain::TerrainFields,
    surfaces: &[Surface],
    climate: &climate::ClimateFields,
    biomes: &[Biome],
) {
    for idx in 0..world.tile_count() {
        let tile = &mut world.tiles[idx];
        tile.raw_elevation = terrain.elevation[idx];
        tile.slope = terrain.slope[idx];
        tile.relief = terrain.relief[idx];
        tile.temperature = climate.temperature[idx];
        tile.moisture = climate.moisture[idx];
        tile.precipitation = climate.precipitation[idx];
        tile.continentality = climate.continentality[idx];
        tile.ocean_distance = climate.ocean_distance[idx];
        tile.surface = surfaces[idx];
        tile.biome = biomes[idx];
        tile.mountain_feature = MountainFeature::None;
    }

    let mountain_features = (0..world.tile_count())
        .map(|idx| mountain_feature_for_tile(world, idx))
        .collect::<Vec<_>>();
    for (tile, feature) in world.tiles.iter_mut().zip(mountain_features.into_iter()) {
        tile.mountain_feature = feature;
    }
}
