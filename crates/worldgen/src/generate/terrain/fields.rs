use noise::OpenSimplex;

use crate::World;

use super::continents::build_continental_config;
use super::tectonics::{OrogenSample, generate_plates, sample_tectonic_elevation};

pub(super) struct OrogenFields {
    pub(super) basement: Vec<f32>,
    pub(super) axial_uplift: Vec<f32>,
    pub(super) shoulder_uplift: Vec<f32>,
    pub(super) plateau_support: Vec<f32>,
    pub(super) foreland_loading: Vec<f32>,
    pub(super) backarc_loading: Vec<f32>,
    pub(super) craton_stability: Vec<f32>,
    pub(super) basin_bias: Vec<f32>,
}

impl OrogenFields {
    fn new(tile_count: usize) -> Self {
        Self {
            basement: vec![0.0_f32; tile_count],
            axial_uplift: vec![0.0_f32; tile_count],
            shoulder_uplift: vec![0.0_f32; tile_count],
            plateau_support: vec![0.0_f32; tile_count],
            foreland_loading: vec![0.0_f32; tile_count],
            backarc_loading: vec![0.0_f32; tile_count],
            craton_stability: vec![0.0_f32; tile_count],
            basin_bias: vec![0.0_f32; tile_count],
        }
    }

    fn set(&mut self, idx: usize, sample: OrogenSample) {
        self.basement[idx] = sample.basement;
        self.axial_uplift[idx] = sample.axial_uplift;
        self.shoulder_uplift[idx] = sample.shoulder_uplift;
        self.plateau_support[idx] = sample.plateau_support;
        self.foreland_loading[idx] = sample.foreland_loading;
        self.backarc_loading[idx] = sample.backarc_loading;
        self.craton_stability[idx] = sample.craton_stability;
        self.basin_bias[idx] = sample.basin_bias;
    }

    pub(super) fn initial_terrain(&self) -> Vec<f32> {
        self.basement
            .iter()
            .zip(self.foreland_loading.iter())
            .zip(self.backarc_loading.iter())
            .map(|((base, foreland), backarc)| (base - foreland * 0.18 - backarc * 0.11).max(0.0))
            .collect()
    }
}

pub(super) fn sample_orogen_fields(
    world: &World,
    base: &OpenSimplex,
    ridge: &OpenSimplex,
) -> OrogenFields {
    let ws = world.effective_world_size();
    let world_units_x = world.width as f32 / ws;
    let world_units_y = world.height as f32 / ws;
    let plates = generate_plates(world);
    let continental_config = build_continental_config(world.seed, world_units_x, world_units_y);
    let mut fields = OrogenFields::new(world.tiles.len());

    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            let sample =
                sample_tectonic_elevation(world, base, ridge, &plates, &continental_config, x, y);
            fields.set(idx, sample);
        }
    }

    fields
}
