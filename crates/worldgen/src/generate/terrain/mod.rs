use noise::OpenSimplex;

mod commit;
mod continents;
mod detail;
mod evolution;
mod fields;
mod normalize;
mod tectonics;

use crate::World;

use commit::{commit_terrain, finalize_sea_level};
use detail::apply_mountain_crag_detail;
use evolution::{apply_tectonic_equilibrium, relax_terrain};
use fields::sample_orogen_fields;
use normalize::normalize_terrain;

pub(super) fn generate_terrain(world: &mut World, base: &OpenSimplex, ridge: &OpenSimplex) {
    let fields = sample_orogen_fields(world, base, ridge);
    let mut elevation = fields.initial_terrain();

    relax_terrain(world, &fields, &mut elevation);
    normalize_terrain(&mut elevation, 0.02, 0.98);
    apply_tectonic_equilibrium(world, ridge, &fields, &mut elevation);
    apply_mountain_crag_detail(world, ridge, &fields, &mut elevation);

    finalize_sea_level(world, &elevation);
    commit_terrain(world, &elevation, &fields);
}
