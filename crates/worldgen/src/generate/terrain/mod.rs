use noise::OpenSimplex;

mod continents;
mod tectonics;

use crate::{Surface, World};

use super::climate::ClimateFields;
use super::hydrology::{self, HydrologyFields};
use super::util::{latitude_factor, octave_noise, ridge_noise, smoothstep};
use continents::build_continental_config;
use tectonics::{generate_plates, sample_tectonic_elevation};

const TERRAIN_RELAX_STEPS: usize = 18;
const TECTONIC_EQUILIBRIUM_STEPS: usize = 16;

#[derive(Clone, Copy)]
struct Plate {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
}

#[derive(Clone, Copy)]
struct OrogenSample {
    basement: f32,
    axial_uplift: f32,
    shoulder_uplift: f32,
    plateau_support: f32,
    foreland_loading: f32,
    backarc_loading: f32,
    craton_stability: f32,
    basin_bias: f32,
}

#[derive(Clone, Copy)]
struct ContinentalFields {
    support: f32,
    interior: f32,
    seaway_cut: f32,
    ocean_basin: f32,
    major_secondary_balance: f32,
}

// Precomputed per-lobe parameters (seed-only, not tile-dependent).
struct PreparedLobe {
    cx: f32,
    cy: f32,
    sin_a: f32,
    cos_a: f32,
    rx: f32,
    ry: f32,
    strength: f32,
}

struct PreparedCut {
    cx: f32,
    cy: f32,
    sin_a: f32,
    cos_a: f32,
    width: f32,
    extent: f32,
    strength: f32,
}

struct ContinentalConfig {
    land_lobes: Vec<PreparedLobe>,
    basins: Vec<PreparedLobe>,
    seaways: Vec<PreparedCut>,
}

struct OrogenFields {
    basement: Vec<f32>,
    axial_uplift: Vec<f32>,
    shoulder_uplift: Vec<f32>,
    plateau_support: Vec<f32>,
    foreland_loading: Vec<f32>,
    backarc_loading: Vec<f32>,
    craton_stability: Vec<f32>,
    basin_bias: Vec<f32>,
}

pub(crate) struct TerrainFields {
    pub(crate) elevation: Vec<f32>,
    pub(crate) slope: Vec<f32>,
    pub(crate) relief: Vec<f32>,
}

pub(super) struct TerrainMutationContext<'a> {
    pub(super) ocean: &'a [bool],
    pub(super) surfaces: &'a [Surface],
    pub(super) climate: &'a ClimateFields,
}

struct NeighborStats {
    avg_neighbor: f32,
    count: f32,
    max_neighbor_drop: f32,
    ocean_neighbors: f32,
}

pub(super) fn generate_terrain_fields(
    world: &World,
    base: &OpenSimplex,
    ridge: &OpenSimplex,
) -> TerrainFields {
    let fields = sample_orogen_fields(world, base, ridge);
    let mut elevation = fields.initial_terrain();

    relax_terrain(world, &fields, &mut elevation);
    apply_tectonic_equilibrium(world, ridge, &fields, &mut elevation);
    apply_landform_detail(world, ridge, &fields, &mut elevation);
    apply_orographic_denudation(world, &mut elevation);
    apply_talus_relaxation(world, &mut elevation);

    let mut terrain = TerrainFields::from_elevation(elevation);
    refresh_derived_fields(world, &mut terrain);
    terrain
}

pub(super) fn apply_terrain_mutators(
    world: &World,
    context: &TerrainMutationContext<'_>,
    terrain: &mut TerrainFields,
) -> bool {
    if context.tile_count() != world.tile_count() {
        return false;
    }

    let hydrology = hydrology::generate_hydrology_fields(
        world,
        terrain,
        context.climate,
        context.ocean,
        context.surfaces,
    );
    apply_fluvial_incision(world, context, terrain, &hydrology)
}

fn apply_fluvial_incision(
    world: &World,
    context: &TerrainMutationContext<'_>,
    terrain: &mut TerrainFields,
    hydrology: &HydrologyFields,
) -> bool {
    let mut delta = vec![0.0_f32; world.tile_count()];
    let mut changed = false;

    for idx in 0..world.tile_count() {
        if context.ocean[idx] || hydrology.river[idx] <= 0.06 {
            continue;
        }

        let current = terrain.elevation[idx];
        let height_above_sea = (current - world.sea_level).max(0.0);
        if height_above_sea <= 0.018 {
            continue;
        }

        let river = hydrology.river[idx];
        let channel = smoothstep(0.08, 0.78, river);
        let elevated = smoothstep(0.035, 0.34, height_above_sea);
        let wet = (context.climate.precipitation[idx] * 0.55
            + context.climate.moisture[idx] * 0.45)
            .clamp(0.0, 1.0);
        let incision = channel * elevated * (0.0025 + wet * 0.0065 + river * 0.0050);
        if incision <= 0.00008 {
            continue;
        }

        delta[idx] -= incision;
        let (x, y) = world.coords(idx);
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            if context.ocean[nidx] {
                continue;
            }
            let dx = x.abs_diff(nx);
            let dy = y.abs_diff(ny);
            let flank_weight = if dx == 1 && dy == 1 { 0.10 } else { 0.18 };
            let neighbor_height = (terrain.elevation[nidx] - world.sea_level).max(0.0);
            let neighbor_emerged = smoothstep(0.025, 0.26, neighbor_height);
            delta[nidx] -= incision * flank_weight * neighbor_emerged;
        }
    }

    for (idx, change) in delta.into_iter().enumerate() {
        if change >= -0.00001 {
            continue;
        }

        let floor = if matches!(context.surfaces[idx], Surface::Coast) {
            world.sea_level + 0.003
        } else {
            world.sea_level + 0.001
        };
        let next = (terrain.elevation[idx] + change).max(floor);
        changed |= (terrain.elevation[idx] - next).abs() > 0.00005;
        terrain.elevation[idx] = next;
    }

    changed
}

pub(super) fn refresh_derived_fields(world: &World, terrain: &mut TerrainFields) {
    for idx in 0..world.tile_count() {
        let current = terrain.elevation[idx];
        let mut min_elev = current;
        let mut max_elev = current;
        let mut max_delta = 0.0_f32;

        for nidx in world.neighbor_indices8(idx) {
            let neighbor = terrain.elevation[nidx];
            min_elev = min_elev.min(neighbor);
            max_elev = max_elev.max(neighbor);
            max_delta = max_delta.max((current - neighbor).abs());
        }

        terrain.slope[idx] = max_delta.clamp(0.0, 1.0);
        terrain.relief[idx] = (max_elev - min_elev).clamp(0.0, 1.0);
    }
}

pub(super) fn finalize_sea_level(_world: &mut World, _terrain: &TerrainFields) {
    // Sea level is an environmental datum from config. The terrain model is
    // responsible for producing land and ocean around it instead of moving the
    // datum to satisfy a target land fraction.
}

impl TerrainFields {
    fn from_elevation(elevation: Vec<f32>) -> Self {
        let tile_count = elevation.len();
        Self {
            elevation,
            slope: vec![0.0; tile_count],
            relief: vec![0.0; tile_count],
        }
    }
}

impl TerrainMutationContext<'_> {
    fn tile_count(&self) -> usize {
        self.ocean
            .len()
            .min(self.surfaces.len())
            .min(self.climate.temperature.len())
    }
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

    fn initial_terrain(&self) -> Vec<f32> {
        self.basement
            .iter()
            .zip(self.foreland_loading.iter())
            .zip(self.backarc_loading.iter())
            .map(|((base, foreland), backarc)| (base - foreland * 0.16 - backarc * 0.10).max(0.02))
            .collect()
    }

    fn smooth_deformation_belts(&mut self, world: &World) {
        smooth_field(world, &mut self.axial_uplift, 2, 3.5);
        smooth_field(world, &mut self.shoulder_uplift, 2, 3.0);
        smooth_field(world, &mut self.plateau_support, 2, 3.0);
        smooth_field(world, &mut self.foreland_loading, 1, 3.0);
        smooth_field(world, &mut self.backarc_loading, 1, 3.0);
    }
}

fn smooth_field(world: &World, values: &mut Vec<f32>, passes: usize, center_weight: f32) {
    let mut next = vec![0.0_f32; values.len()];

    for _ in 0..passes {
        for idx in 0..values.len() {
            let mut sum = values[idx] * center_weight;
            let mut weight = center_weight;

            for nidx in world.neighbor_indices8(idx) {
                sum += values[nidx];
                weight += 1.0;
            }

            next[idx] = sum / weight;
        }
        std::mem::swap(values, &mut next);
    }
}

fn sample_orogen_fields(world: &World, base: &OpenSimplex, ridge: &OpenSimplex) -> OrogenFields {
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

    fields.smooth_deformation_belts(world);
    fields
}

fn relax_terrain(world: &World, fields: &OrogenFields, terrain: &mut Vec<f32>) {
    let uplift_per_step = 0.54 / TERRAIN_RELAX_STEPS as f32;
    let mut next = vec![0.0_f32; terrain.len()];

    for step in 0..TERRAIN_RELAX_STEPS {
        let progress = (step + 1) as f32 / TERRAIN_RELAX_STEPS as f32;
        apply_orogenic_uplift(fields, terrain, progress, uplift_per_step);

        for (idx, value) in next.iter_mut().enumerate() {
            *value = relaxed_tile_elevation(world, fields, terrain, idx, progress);
        }

        std::mem::swap(terrain, &mut next);
    }
}

fn apply_landform_detail(
    world: &World,
    ridge: &OpenSimplex,
    fields: &OrogenFields,
    terrain: &mut [f32],
) {
    let ws = world.effective_world_size();
    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            let current = terrain[idx];
            if current <= world.sea_level {
                continue;
            }

            let height_above_sea = (current - world.sea_level).max(0.0);
            let uplift_signal = fields.axial_uplift[idx] + fields.shoulder_uplift[idx] * 0.65;
            let uplift = smoothstep(0.10, 0.58, uplift_signal);
            let highland = smoothstep(0.16, 0.48, height_above_sea);
            let crag_mask = highland * (0.35 + uplift * 0.65);
            let rolling_mask = smoothstep(0.02, 0.18, height_above_sea)
                * (1.0 - smoothstep(0.34, 0.68, height_above_sea));
            let upland_mask = smoothstep(0.12, 0.34, height_above_sea)
                * (1.0 - smoothstep(0.52, 0.92, height_above_sea));
            let shoulder_fabric = smoothstep(
                0.08,
                0.48,
                fields.shoulder_uplift[idx] + fields.plateau_support[idx],
            );

            let xf = x as f64 / ws as f64;
            let yf = y as f64 / ws as f64;
            let broad_warp = octave_noise(ridge, xf * 1.55 + 83.0, yf * 1.55 - 47.0, 4, 0.55, 2.0);
            let basin_load = (fields.basin_bias[idx] * 0.50
                + fields.foreland_loading[idx] * 0.25
                + fields.backarc_loading[idx] * 0.20)
                .clamp(0.0, 1.0);
            let continental_warp = smoothstep(0.10, 0.26, height_above_sea)
                * (1.0 - smoothstep(0.36, 0.64, height_above_sea))
                * (1.0 - basin_load * 0.62)
                * (0.078
                    + broad_warp * 0.155
                    + shoulder_fabric * 0.058
                    + fields.craton_stability[idx] * 0.030
                    + uplift * 0.020);
            let basin_subsidence = smoothstep(0.04, 0.18, height_above_sea)
                * (1.0 - smoothstep(0.24, 0.44, height_above_sea))
                * basin_load
                * (0.035 + (1.0 - broad_warp) * 0.065);
            let swells = octave_noise(ridge, xf * 3.2 - 29.0, yf * 3.2 + 17.0, 3, 0.54, 1.95);
            let wrinkles = ridge_noise(ridge, xf * 7.4 + 37.0, yf * 7.4 - 41.0, 3);
            let benches = ridge_noise(ridge, xf * 12.5 - 53.0, yf * 12.5 + 11.0, 2);
            let grain = octave_noise(ridge, xf * 22.0 + 61.0, yf * 22.0 - 71.0, 2, 0.52, 2.0);
            let ribs = ridge_noise(ridge, xf * 18.0 + 7.0, yf * 18.0 - 11.0, 3);
            let fracture = octave_noise(ridge, xf * 31.0 - 19.0, yf * 31.0 + 23.0, 2, 0.52, 2.1);
            let crag_detail = (ribs - 0.40) * 0.070 + (fracture - 0.5) * 0.032;
            let rolling_detail = (broad_warp - 0.5) * 0.120
                + (swells - 0.5) * 0.150
                + (wrinkles - 0.48) * 0.110
                + (benches - 0.50) * 0.070
                + (grain - 0.50) * 0.036;
            let upland_detail = (wrinkles - 0.50) * 0.060 + (ribs - 0.52) * 0.048;
            let rolling_strength = rolling_mask * (1.20 + fields.craton_stability[idx] * 0.30)
                + upland_mask * (0.48 + shoulder_fabric * 0.58);
            terrain[idx] = (current + continental_warp - basin_subsidence
                + crag_detail * crag_mask
                + rolling_detail * rolling_strength
                + upland_detail * shoulder_fabric * upland_mask)
                .max(world.sea_level + 0.001);
        }
    }
}

fn apply_talus_relaxation(world: &World, terrain: &mut [f32]) {
    let mut delta = vec![0.0_f32; terrain.len()];

    for _ in 0..10 {
        delta.fill(0.0);

        for idx in 0..terrain.len() {
            let current = terrain[idx];
            if current <= world.sea_level {
                continue;
            }

            for nidx in world.neighbor_indices8(idx) {
                if nidx <= idx {
                    continue;
                }

                let neighbor = terrain[nidx];
                if neighbor <= world.sea_level {
                    continue;
                }

                let drop = current - neighbor;
                let abs_drop = drop.abs();
                let high = current.max(neighbor);
                let highland = smoothstep(0.10, 0.52, high - world.sea_level);
                let talus_angle = 0.020 + highland * 0.028;
                if abs_drop <= talus_angle {
                    continue;
                }

                let transfer = (abs_drop - talus_angle) * (0.18 + highland * 0.12);
                if drop > 0.0 {
                    delta[idx] -= transfer;
                    delta[nidx] += transfer;
                } else {
                    delta[idx] += transfer;
                    delta[nidx] -= transfer;
                }
            }
        }

        for (elevation, change) in terrain.iter_mut().zip(delta.iter().copied()) {
            if *elevation > world.sea_level {
                *elevation = (*elevation + change).max(world.sea_level + 0.001);
            }
        }
    }
}

fn apply_orographic_denudation(world: &World, terrain: &mut [f32]) {
    let previous = terrain.to_vec();

    for idx in 0..terrain.len() {
        let current = previous[idx];
        if current <= world.sea_level {
            continue;
        }

        let (x, y) = world.coords(idx);
        let stats = terrain_neighbor_stats(world, &previous, x, y, current);
        let height_above_sea = (current - world.sea_level).max(0.0);
        let exposed_height = smoothstep(0.20, 0.82, height_above_sea);
        if exposed_height <= 0.0 {
            continue;
        }

        let relief_load = smoothstep(0.020, 0.120, stats.max_neighbor_drop);
        let extreme_exposure = smoothstep(0.58, 0.86, height_above_sea);
        let weathering = height_above_sea.powf(1.88)
            * exposed_height
            * (0.040 + relief_load * 0.060 + stats.ocean_neighbors * 0.0020)
            * (1.0 + extreme_exposure * 0.70);
        terrain[idx] = (current - weathering).max(world.sea_level + 0.001);
    }
}

fn apply_tectonic_equilibrium(
    world: &World,
    ridge: &OpenSimplex,
    fields: &OrogenFields,
    terrain: &mut Vec<f32>,
) {
    let ws = world.effective_world_size();
    let mut next = vec![0.0_f32; terrain.len()];

    for step in 0..TECTONIC_EQUILIBRIUM_STEPS {
        let progress = (step + 1) as f32 / TECTONIC_EQUILIBRIUM_STEPS as f32;

        for (idx, value) in next.iter_mut().enumerate() {
            let current = terrain[idx];
            if current <= world.sea_level {
                *value = current;
                continue;
            }

            let (x, y) = world.coords(idx);
            let xf = x as f64 / ws as f64;
            let yf = y as f64 / ws as f64;
            let stats = terrain_neighbor_stats(world, terrain, x, y, current);
            let height_above_sea = (current - world.sea_level).max(0.0);
            let uplift_signal = fields.axial_uplift[idx] + fields.shoulder_uplift[idx] * 0.42;
            let active_core = smoothstep(0.14, 0.66, uplift_signal);
            let emerged = smoothstep(0.00, 0.18, height_above_sea);
            let highland = smoothstep(0.10, 0.36, height_above_sea);
            let shoulder_zone = smoothstep(
                0.16,
                0.58,
                fields.shoulder_uplift[idx] + fields.plateau_support[idx] * 0.48,
            );
            let basin_load = (fields.foreland_loading[idx] * 0.55
                + fields.backarc_loading[idx] * 0.45)
                .clamp(0.0, 1.0);
            let ridge_segmentation = ridge_noise(ridge, xf * 9.5 + 17.0, yf * 9.5 - 23.0, 3);
            let ridge_focus = smoothstep(0.42, 0.84, ridge_segmentation);
            let inherited_root = smoothstep(0.20, 0.42, height_above_sea)
                * ridge_focus
                * (0.34
                    + fields.plateau_support[idx] * 0.38
                    + fields.shoulder_uplift[idx] * 0.24
                    + active_core * 0.18)
                * (1.0 - basin_load * 0.35)
                * (1.0 - fields.craton_stability[idx] * 0.38);
            let crest_focus = active_core * highland * (0.45 + ridge_focus * 0.55)
                + inherited_root * (0.42 + ridge_focus * 0.38);
            let root_support = (fields.axial_uplift[idx] * 0.36
                + fields.shoulder_uplift[idx] * 0.58
                + fields.plateau_support[idx] * 0.78)
                * (0.30 + emerged * 0.70)
                * (1.0 - basin_load * 0.28)
                + inherited_root * 0.82;
            let tectonic_uplift = root_support
                * (0.016
                    + active_core * 0.012
                    + crest_focus * 0.020
                    + fields.axial_uplift[idx] * 0.008
                    + fields.plateau_support[idx] * 0.006
                    + progress * 0.004);

            let local_drop = stats.max_neighbor_drop;
            let above_neighbors = (current - stats.avg_neighbor).max(0.0);
            let active_relief_protection = (1.0 - active_core * 0.42).max(0.54);
            let slope_failure = local_drop.powf(1.18)
                * (0.018 + height_above_sea * 0.026 + (1.0 - active_core) * 0.014)
                * active_relief_protection;
            let height_denudation = height_above_sea.powf(1.72)
                * (0.0014 + shoulder_zone * 0.0018 + (1.0 - active_core) * 0.0032)
                * active_relief_protection;
            let extreme_height = (height_above_sea - 0.34).max(0.0);
            let orographic_collapse = extreme_height.powf(1.72)
                * (0.230 + local_drop * 0.170 + (1.0 - shoulder_zone) * 0.040);
            let relief_denudation = above_neighbors
                * (0.0038 + local_drop * 0.023 + height_above_sea * 0.0045)
                * (1.0 - crest_focus * 0.36).max(0.52);
            let diffusion = (stats.avg_neighbor - current)
                * (0.006
                    + local_drop * 0.020
                    + shoulder_zone * 0.011
                    + (1.0 - active_core) * 0.013)
                * (1.0 - crest_focus * 0.48).max(0.42);
            let glacial_wear = smoothstep(0.52, 0.84, height_above_sea)
                * (0.0015 + local_drop * 0.012 + above_neighbors * 0.006);

            *value = (current + tectonic_uplift + diffusion
                - slope_failure
                - height_denudation
                - orographic_collapse
                - relief_denudation
                - glacial_wear)
                .max(world.sea_level + 0.001);
        }

        std::mem::swap(terrain, &mut next);
    }
}

fn apply_orogenic_uplift(
    fields: &OrogenFields,
    terrain: &mut [f32],
    progress: f32,
    uplift_per_step: f32,
) {
    for (idx, current) in terrain.iter_mut().enumerate() {
        let uplift_core = smoothstep(
            0.48,
            0.92,
            fields.axial_uplift[idx] + fields.shoulder_uplift[idx] * 0.24,
        );
        let orogen_margin = smoothstep(
            0.16,
            0.54,
            fields.shoulder_uplift[idx] + fields.plateau_support[idx] * 0.55,
        );
        let uplift_add = fields.axial_uplift[idx] * uplift_per_step * (0.24 + progress * 0.10)
            + fields.shoulder_uplift[idx] * uplift_per_step * 0.72 * (0.84 + progress * 0.30)
            + fields.plateau_support[idx] * uplift_per_step * 0.66;
        let core_boost =
            0.82 + uplift_core * 0.28 + fields.axial_uplift[idx] * 0.06 + orogen_margin * 0.24
                - fields.basin_bias[idx] * 0.12;
        *current += uplift_add * core_boost.max(0.18);
    }
}

fn relaxed_tile_elevation(
    world: &World,
    fields: &OrogenFields,
    terrain: &[f32],
    idx: usize,
    progress: f32,
) -> f32 {
    let current = terrain[idx];
    if current <= world.sea_level {
        // The only water influence left in terrain shaping is ocean adjacency for land tiles.
        return current;
    }

    let (x, y) = world.coords(idx);
    let stats = terrain_neighbor_stats(world, terrain, x, y, current);
    let height_above_sea = (current - world.sea_level).max(0.0);
    let relief = (current - fields.basement[idx]).max(0.0);
    let highland = smoothstep(0.10, 0.34, height_above_sea);
    let alpine = smoothstep(0.22, 0.52, height_above_sea);
    let super_alpine = smoothstep(0.42, 0.78, height_above_sea);
    let uplift_core = smoothstep(
        0.48,
        0.92,
        fields.axial_uplift[idx] + fields.shoulder_uplift[idx] * 0.24,
    );
    let interior_high = highland * (1.0 - uplift_core);
    let shoulder_zone = smoothstep(
        0.12,
        0.48,
        fields.shoulder_uplift[idx] + fields.plateau_support[idx] * 0.4,
    );
    let plain_zone = smoothstep(0.38, 0.82, fields.craton_stability[idx]) * (1.0 - uplift_core);
    let basin_zone = smoothstep(
        0.24,
        0.72,
        fields.basin_bias[idx]
            + fields.foreland_loading[idx] * 0.55
            + fields.backarc_loading[idx] * 0.45,
    );
    let lat = latitude_factor(y, world.height);
    let snowline =
        world.sea_level + (0.34 - lat * 0.16 - fields.plateau_support[idx] * 0.04).max(0.16);
    let glacial_band = smoothstep(snowline, snowline + 0.16, current);
    let ridge_crest = highland * (1.0 - smoothstep(0.012, 0.075, stats.max_neighbor_drop));
    let coastal = stats.ocean_neighbors / stats.count.max(1.0);
    let diffusion = (stats.avg_neighbor - current)
        * (0.019 * (1.0 - ridge_crest * 0.78)
            + stats.max_neighbor_drop * 0.034
            + interior_high * 0.046 * (1.0 - ridge_crest * 0.72)
            + shoulder_zone * 0.022
            + plain_zone * 0.03
            + basin_zone * 0.028
            + coastal * 0.020
            + alpine * 0.012 * (1.0 - uplift_core * 0.45)
            + glacial_band * 0.02);
    let slope_failure = stats.max_neighbor_drop
        * (0.011
            + interior_high * 0.03
            + shoulder_zone * 0.012
            + alpine * 0.011 * (1.0 - uplift_core * 0.24)
            + glacial_band * 0.012);
    let ridge_decay = relief
        * (0.013
            + interior_high * 0.044
            + shoulder_zone * 0.022
            + alpine * 0.011 * (1.0 - uplift_core * 0.38))
        * (1.0 - fields.axial_uplift[idx] * 0.22).max(0.48);
    let extreme_height = (height_above_sea - 0.40).max(0.0);
    let alpine_relax = interior_high * 0.018
        + shoulder_zone * 0.008
        + alpine * (0.012 + (1.0 - uplift_core) * 0.016)
        + super_alpine * (0.016 + (1.0 - uplift_core) * 0.02)
        + (height_above_sea - 0.46).max(0.0) * (0.024 + (1.0 - uplift_core) * 0.026)
        + extreme_height.powf(1.34) * (0.220 + (1.0 - uplift_core) * 0.084);
    let glacial_relief_softening = glacial_band
        * (0.004 + stats.max_neighbor_drop * 0.012 + relief * 0.008)
        * (0.5 + fields.axial_uplift[idx] * 0.62 + fields.shoulder_uplift[idx] * 0.18);
    let plain_planation =
        relief * (0.008 + plain_zone * 0.020 + basin_zone * 0.018) * (1.0 - uplift_core * 0.75);
    let shoulder_denudation = relief
        * (0.006 + shoulder_zone * 0.020 + interior_high * 0.014)
        * (1.0 - fields.axial_uplift[idx] * 0.65);
    let coastal_planation =
        coastal * (0.0015 + plain_zone * 0.0035 + basin_zone * 0.003) * (0.7 + progress * 0.3);
    let basin_subsidence = (fields.foreland_loading[idx] * 0.0105
        + fields.backarc_loading[idx] * 0.007
        + basin_zone * 0.004
        + shoulder_zone * 0.0025 * (1.0 - uplift_core))
        * (0.62 + progress * 0.48);

    let land_delta = diffusion
        - ridge_decay
        - alpine_relax
        - slope_failure
        - glacial_relief_softening
        - plain_planation
        - shoulder_denudation
        - coastal_planation
        - basin_subsidence;

    (current + land_delta).max(world.sea_level + 0.001)
}

fn terrain_neighbor_stats(
    world: &World,
    terrain: &[f32],
    x: usize,
    y: usize,
    current: f32,
) -> NeighborStats {
    let mut avg = 0.0;
    let mut count = 0.0;
    let mut max_neighbor_drop = 0.0_f32;
    let mut ocean_neighbors = 0.0_f32;

    for (nx, ny) in world.neighbors8(x, y) {
        let nidx = world.idx(nx, ny);
        let neighbor = terrain[nidx];
        avg += neighbor;
        count += 1.0;
        max_neighbor_drop = max_neighbor_drop.max((current - neighbor).max(0.0));
        if neighbor <= world.sea_level {
            ocean_neighbors += 1.0;
        }
    }

    NeighborStats {
        avg_neighbor: if count > 0.0 { avg / count } else { current },
        count,
        max_neighbor_drop,
        ocean_neighbors,
    }
}
