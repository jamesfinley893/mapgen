mod biomes;
mod climate;
mod hydrology;
mod terrain;
mod util;

use noise::OpenSimplex;

use crate::{World, WorldConfig};

pub use biomes::biome_for_tile;

pub(super) const HYDRO_EPSILON: f32 = 0.0001;
pub(super) const DIAGONAL_COST: f32 = std::f32::consts::SQRT_2;
pub(super) const EROSION_STEPS: usize = 18;

pub fn generate_world(config: &WorldConfig) -> Result<World, String> {
    config.validate()?;

    let mut world = World::new(config.seed, config.width, config.height, config.sea_level, config.world_size);

    let base = OpenSimplex::new(config.seed as u32);
    let ridge = OpenSimplex::new(config.seed.wrapping_add(1) as u32);
    let climate_noise = OpenSimplex::new(config.seed.wrapping_add(2) as u32);

    terrain::populate_raw_elevation(&mut world, &base, &ridge);

    let mut ocean = hydrology::classify_ocean(&world);
    let hydrology = hydrology::simulate_hydrology(&world, &ocean);
    hydrology::apply_channel_carving(&mut world, &hydrology);

    ocean = hydrology::classify_ocean(&world);
    let hydrology = hydrology::simulate_hydrology(&world, &ocean);
    hydrology::apply_hydrology_to_world(&mut world, &ocean, &hydrology);

    let distance_to_ocean = climate::fill_ocean_distance(&world, &ocean);
    climate::populate_climate(&mut world, config, &ocean, &distance_to_ocean, &climate_noise);
    biomes::assign_biomes(&mut world);

    Ok(world)
}
