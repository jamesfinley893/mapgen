mod biomes;
mod climate;
mod ocean;
mod terrain;
mod util;

use noise::OpenSimplex;

use crate::{World, WorldConfig};

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

    terrain::populate_raw_elevation(&mut world, &base, &ridge);

    let ocean = ocean::classify_ocean(&world);
    ocean::apply_ocean_surfaces(&mut world, &ocean);
    let distance_to_ocean = climate::fill_ocean_distance(&world, &ocean);
    climate::populate_climate(
        &mut world,
        config,
        &ocean,
        &distance_to_ocean,
        &climate_noise,
    );
    biomes::assign_biomes(&mut world);

    Ok(world)
}
