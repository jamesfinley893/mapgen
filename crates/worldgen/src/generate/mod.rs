mod biomes;
mod climate;
mod ocean;
mod terrain;
mod util;

use noise::OpenSimplex;

use crate::features::mountain_feature_for_tile;
use crate::{MountainFeature, World, WorldConfig};

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

    terrain::generate_terrain(&mut world, &base, &ridge);
    let ocean = ocean::classify_ocean(&world);
    ocean::mark_ocean_and_coast(&mut world, &ocean);
    climate::generate_climate(&mut world, config, &ocean, &climate_noise);
    biomes::assign_biomes(&mut world);
    assign_mountain_features(&mut world);

    Ok(world)
}

fn assign_mountain_features(world: &mut World) {
    for idx in 0..world.tile_count() {
        world.tiles[idx].mountain_feature = MountainFeature::None;
    }

    let mountain_features = (0..world.tile_count())
        .map(|idx| mountain_feature_for_tile(world, idx))
        .collect::<Vec<_>>();
    for (tile, feature) in world.tiles.iter_mut().zip(mountain_features.into_iter()) {
        tile.mountain_feature = feature;
    }
}
