use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, VecDeque};

use noise::{NoiseFn, OpenSimplex};

use crate::{Biome, Surface, World, WorldConfig};

const HYDRO_EPSILON: f32 = 0.0001;

pub fn generate_world(config: &WorldConfig) -> Result<World, String> {
    config.validate()?;

    let mut world = World::new(config.seed, config.width, config.height, config.sea_level);

    let base = OpenSimplex::new(config.seed as u32);
    let ridge = OpenSimplex::new(config.seed.wrapping_add(1) as u32);
    let climate = OpenSimplex::new(config.seed.wrapping_add(2) as u32);

    populate_raw_elevation(&mut world, &base, &ridge);

    let mut ocean = classify_ocean(&world);
    let hydrology = simulate_hydrology(&world, &ocean);
    apply_channel_carving(&mut world, &hydrology);

    ocean = classify_ocean(&world);
    let hydrology = simulate_hydrology(&world, &ocean);
    apply_hydrology_to_world(&mut world, &ocean, &hydrology);

    let distance_to_ocean = fill_ocean_distance(&world, &ocean);
    populate_climate(&mut world, config, &ocean, &distance_to_ocean, &climate);
    assign_biomes(&mut world);

    Ok(world)
}

#[derive(Debug, Clone)]
struct HydrologyState {
    hydro_elevation: Vec<f32>,
    downstream: Vec<Option<usize>>,
    contributing_area: Vec<f32>,
    surfaces: Vec<Surface>,
    lake_id: Vec<Option<u32>>,
    water_level: Vec<Option<f32>>,
    basin_id: Vec<Option<u32>>,
}

#[derive(Clone, Copy, Debug)]
struct QueueCell {
    level: f32,
    idx: usize,
}

impl PartialEq for QueueCell {
    fn eq(&self, other: &Self) -> bool {
        self.idx == other.idx && self.level.to_bits() == other.level.to_bits()
    }
}

impl Eq for QueueCell {}

impl Ord for QueueCell {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .level
            .total_cmp(&self.level)
            .then_with(|| other.idx.cmp(&self.idx))
    }
}

impl PartialOrd for QueueCell {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn populate_raw_elevation(world: &mut World, base: &OpenSimplex, ridge: &OpenSimplex) {
    let mut min_elevation = f32::MAX;
    let mut max_elevation = f32::MIN;

    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            let elevation = sample_elevation(base, ridge, x, y, world.width, world.height);
            world.tiles[idx].raw_elevation = elevation;
            min_elevation = min_elevation.min(elevation);
            max_elevation = max_elevation.max(elevation);
        }
    }

    let span = (max_elevation - min_elevation).max(0.0001);
    for tile in &mut world.tiles {
        tile.raw_elevation = ((tile.raw_elevation - min_elevation) / span).clamp(0.0, 1.0);
    }
}

fn classify_ocean(world: &World) -> Vec<bool> {
    world.tiles.iter().map(|tile| tile.raw_elevation <= world.sea_level).collect()
}

fn populate_climate(
    world: &mut World,
    config: &WorldConfig,
    ocean: &[bool],
    distance_to_ocean: &[u16],
    climate: &OpenSimplex,
) {
    let nearby_water = compute_nearby_water(world);

    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            let elevation = world.tiles[idx].raw_elevation;
            let lat = latitude_factor(y, world.height);
            let climate_noise = octave_noise(climate, x as f64 * 0.008, y as f64 * 0.008, 3, 0.5, 2.0);
            let temperature =
                (1.0 - lat * 1.15 + climate_noise * 0.12 - elevation * 0.35 + config.temperature_bias)
                    .clamp(0.0, 1.0);
            let moisture = (moisture_value(world, ocean, distance_to_ocean, climate, &nearby_water, x, y)
                + config.moisture_bias)
                .clamp(0.0, 1.0);
            world.tiles[idx].temperature = temperature;
            world.tiles[idx].moisture = moisture;
        }
    }
}

fn compute_nearby_water(world: &World) -> Vec<f32> {
    let mut nearby = vec![0.0_f32; world.tiles.len()];
    for idx in 0..world.tiles.len() {
        if matches!(world.tiles[idx].surface, Surface::Ocean | Surface::Lake | Surface::River) {
            nearby[idx] = 1.0;
            let (x, y) = world.coords(idx);
            for (nx, ny) in world.neighbors8(x, y) {
                let nidx = world.idx(nx, ny);
                nearby[nidx] = nearby[nidx].max(0.35);
            }
        }
    }
    nearby
}

fn simulate_hydrology(world: &World, ocean: &[bool]) -> HydrologyState {
    let count = world.tiles.len();
    let mut hydro = vec![0.0; count];
    let mut fill_depth = vec![0.0; count];
    let mut downstream = vec![None; count];
    let mut visited = vec![false; count];
    let mut visit_order = Vec::with_capacity(count);
    let mut heap = BinaryHeap::new();

    for idx in 0..count {
        if ocean[idx] {
            visited[idx] = true;
            hydro[idx] = world.tiles[idx].raw_elevation;
            heap.push(QueueCell {
                level: hydro[idx],
                idx,
            });
            visit_order.push(idx);
        }
    }

    while let Some(cell) = heap.pop() {
        let (x, y) = world.coords(cell.idx);
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            if visited[nidx] {
                continue;
            }
            visited[nidx] = true;
            let raw = world.tiles[nidx].raw_elevation;
            let raised = raw.max(cell.level);
            hydro[nidx] = raised;
            fill_depth[nidx] = (raised - raw).max(0.0);
            downstream[nidx] = Some(cell.idx);
            heap.push(QueueCell {
                level: raised + HYDRO_EPSILON,
                idx: nidx,
            });
            visit_order.push(nidx);
        }
    }

    let provisional = identify_lakes(world, ocean, &hydro, &fill_depth, &downstream);
    let contributing_area = accumulate_contributing_area(&downstream, &visit_order, ocean);
    let basin_id = assign_basin_ids(world, ocean, &downstream, &provisional.lake_id, provisional.lake_count);
    let surfaces = classify_surfaces(world, ocean, &contributing_area, &provisional.lake_id);

    HydrologyState {
        hydro_elevation: hydro,
        downstream,
        contributing_area,
        surfaces,
        lake_id: provisional.lake_id,
        water_level: provisional.water_level,
        basin_id,
    }
}

struct LakeData {
    lake_id: Vec<Option<u32>>,
    water_level: Vec<Option<f32>>,
    lake_count: u32,
}

fn identify_lakes(
    world: &World,
    ocean: &[bool],
    hydro: &[f32],
    fill_depth: &[f32],
    downstream: &[Option<usize>],
) -> LakeData {
    let mut lake_id = vec![None; world.tiles.len()];
    let mut water_level = vec![None; world.tiles.len()];
    let mut visited = vec![false; world.tiles.len()];
    let mut next_lake_id = 0_u32;
    let area_threshold = ((world.width * world.height) as f32 * 0.00045).ceil() as usize;
    let area_threshold = area_threshold.max(4);
    let volume_threshold = ((world.width * world.height) as f32 * 0.00008).max(0.03);
    let depth_threshold = 0.012;

    for idx in 0..world.tiles.len() {
        if visited[idx] || ocean[idx] || fill_depth[idx] <= depth_threshold {
            continue;
        }
        let mut region = Vec::new();
        let mut queue = VecDeque::from([idx]);
        visited[idx] = true;

        while let Some(current) = queue.pop_front() {
            region.push(current);
            let (x, y) = world.coords(current);
            for (nx, ny) in world.neighbors8(x, y) {
                let nidx = world.idx(nx, ny);
                if visited[nidx] || ocean[nidx] || fill_depth[nidx] <= depth_threshold {
                    continue;
                }
                if (hydro[nidx] - hydro[current]).abs() > 0.02 {
                    continue;
                }
                visited[nidx] = true;
                queue.push_back(nidx);
            }
        }

        let volume: f32 = region.iter().map(|&cell| fill_depth[cell]).sum();
        if region.len() < area_threshold && volume < volume_threshold {
            continue;
        }

        let mut outlet = None;
        let mut outlet_level = f32::MAX;
        for &cell in &region {
            if let Some(next) = downstream[cell] {
                if !region.contains(&next) && hydro[cell] < outlet_level {
                    outlet_level = hydro[cell];
                    outlet = Some(next);
                }
            }
        }

        if outlet.is_none() {
            continue;
        }

        for &cell in &region {
            lake_id[cell] = Some(next_lake_id);
            water_level[cell] = Some(hydro[cell]);
        }
        next_lake_id += 1;
    }

    LakeData {
        lake_id,
        water_level,
        lake_count: next_lake_id,
    }
}

fn accumulate_contributing_area(downstream: &[Option<usize>], visit_order: &[usize], ocean: &[bool]) -> Vec<f32> {
    let mut contributing_area = vec![0.0; downstream.len()];
    for idx in visit_order.iter().rev().copied() {
        if ocean[idx] {
            continue;
        }
        contributing_area[idx] += 1.0;
        if let Some(next) = downstream[idx] {
            contributing_area[next] += contributing_area[idx];
        }
    }
    contributing_area
}

fn assign_basin_ids(
    world: &World,
    ocean: &[bool],
    downstream: &[Option<usize>],
    lake_id: &[Option<u32>],
    basin_offset: u32,
) -> Vec<Option<u32>> {
    let mut basin_id = vec![None; world.tiles.len()];
    let mut mouth_to_basin = HashMap::<usize, u32>::new();
    let mut next_basin = 0_u32;

    for idx in 0..world.tiles.len() {
        if ocean[idx] {
            continue;
        }
        let mut current = idx;
        let mut guard = 0;
        while guard < world.tiles.len() {
            if ocean[current] {
                break;
            }
            if let Some(lake) = lake_id[current] {
                basin_id[idx] = Some(lake);
                break;
            }
            match downstream[current] {
                Some(next) => {
                    if ocean[next] {
                        let basin = *mouth_to_basin.entry(current).or_insert_with(|| {
                            let id = basin_offset + next_basin;
                            next_basin += 1;
                            id
                        });
                        basin_id[idx] = Some(basin);
                        break;
                    }
                    current = next;
                }
                None => break,
            }
            guard += 1;
        }
    }

    basin_id
}

fn classify_surfaces(
    world: &World,
    ocean: &[bool],
    contributing_area: &[f32],
    lake_id: &[Option<u32>],
) -> Vec<Surface> {
    let mut surfaces = vec![Surface::Land; world.tiles.len()];
    let river_threshold = ((world.width * world.height) as f32 * 0.0012).max(18.0);

    for idx in 0..world.tiles.len() {
        if ocean[idx] {
            surfaces[idx] = Surface::Ocean;
        } else if lake_id[idx].is_some() {
            surfaces[idx] = Surface::Lake;
        } else if contributing_area[idx] >= river_threshold {
            surfaces[idx] = Surface::River;
        }
    }

    for idx in 0..world.tiles.len() {
        if surfaces[idx] != Surface::Land {
            continue;
        }
        let (x, y) = world.coords(idx);
        if world
            .neighbors8(x, y)
            .any(|(nx, ny)| surfaces[world.idx(nx, ny)] == Surface::Ocean)
        {
            surfaces[idx] = Surface::Coast;
        }
    }

    surfaces
}

fn apply_channel_carving(world: &mut World, hydrology: &HydrologyState) {
    let river_threshold = ((world.width * world.height) as f32 * 0.0012).max(18.0);

    for idx in 0..world.tiles.len() {
        if hydrology.surfaces[idx] != Surface::River {
            continue;
        }
        let discharge = hydrology.contributing_area[idx];
        let ratio = (discharge / river_threshold).max(1.0);
        let carve = (0.006 + ratio.ln() * 0.01).clamp(0.0, 0.05);
        world.tiles[idx].raw_elevation = (world.tiles[idx].raw_elevation - carve).max(0.0);

        let (x, y) = world.coords(idx);
        let neighbors: Vec<_> = world.neighbors8(x, y).collect();
        for (nx, ny) in neighbors {
            let nidx = world.idx(nx, ny);
            if hydrology.surfaces[nidx] == Surface::Ocean {
                continue;
            }
            world.tiles[nidx].raw_elevation = (world.tiles[nidx].raw_elevation - carve * 0.18).max(0.0);
        }
    }
}

fn apply_hydrology_to_world(world: &mut World, ocean: &[bool], hydrology: &HydrologyState) {
    for idx in 0..world.tiles.len() {
        world.tiles[idx].hydro_elevation = hydrology.hydro_elevation[idx];
        world.tiles[idx].contributing_area = hydrology.contributing_area[idx];
        world.tiles[idx].downstream = hydrology.downstream[idx];
        world.tiles[idx].surface = hydrology.surfaces[idx];
        world.tiles[idx].basin_id = hydrology.basin_id[idx];
        world.tiles[idx].lake_id = hydrology.lake_id[idx];
        world.tiles[idx].water_level = hydrology.water_level[idx];
        if ocean[idx] {
            world.tiles[idx].water_level = Some(world.sea_level);
        }
    }
}

fn sample_elevation(
    base: &OpenSimplex,
    ridge: &OpenSimplex,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
) -> f32 {
    let xf = x as f64 / width as f64;
    let yf = y as f64 / height as f64;

    let continent = octave_noise(base, xf * 1.6, yf * 1.6, 4, 0.5, 2.0);
    let detail = octave_noise(base, xf * 5.5 + 19.3, yf * 5.5 - 7.4, 5, 0.55, 2.1);
    let ridge_primary = ridge_noise(ridge, xf * 3.2 + 13.0, yf * 3.2 + 9.0, 4);
    let ridge_secondary = ridge_noise(ridge, xf * 7.1 - 4.0, yf * 7.1 + 17.0, 3);
    let basin = octave_noise(base, xf * 2.1 - 11.0, yf * 2.1 + 6.0, 3, 0.5, 2.0);

    let dx = xf - 0.5;
    let dy = yf - 0.5;
    let radial = (dx * dx + dy * dy).sqrt() as f32;
    let edge_falloff = ((radial - 0.36) * 2.2).clamp(0.0, 1.0);
    let macro_mask = (continent * 0.65 + 0.35) - edge_falloff * 0.35;

    let mountains = ridge_primary * 0.22 + ridge_secondary * 0.10;
    let lowlands = detail * 0.18 - basin.abs() * 0.10;

    (macro_mask + mountains + lowlands).clamp(0.0, 1.0)
}

fn octave_noise(
    noise: &OpenSimplex,
    x: f64,
    y: f64,
    octaves: usize,
    persistence: f64,
    lacunarity: f64,
) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut freq = 1.0;
    let mut norm = 0.0;
    for _ in 0..octaves {
        sum += noise.get([x * freq, y * freq]) * amp;
        norm += amp;
        amp *= persistence;
        freq *= lacunarity;
    }
    (((sum / norm) as f32) * 0.5 + 0.5).clamp(0.0, 1.0)
}

fn ridge_noise(noise: &OpenSimplex, x: f64, y: f64, octaves: usize) -> f32 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut freq = 1.0;
    let mut norm = 0.0;
    for _ in 0..octaves {
        let v = noise.get([x * freq, y * freq]).abs();
        sum += (1.0 - v) * amp;
        norm += amp;
        amp *= 0.5;
        freq *= 2.1;
    }
    (sum / norm) as f32
}

fn latitude_factor(y: usize, height: usize) -> f32 {
    let lat = y as f32 / (height.saturating_sub(1).max(1)) as f32;
    ((lat - 0.5).abs()) * 2.0
}

fn fill_ocean_distance(world: &World, ocean: &[bool]) -> Vec<u16> {
    let mut out = vec![u16::MAX; world.tiles.len()];
    let mut queue = VecDeque::new();
    for (idx, is_ocean) in ocean.iter().enumerate() {
        if *is_ocean {
            out[idx] = 0;
            queue.push_back(idx);
        }
    }
    while let Some(idx) = queue.pop_front() {
        let (x, y) = world.coords(idx);
        let next_dist = out[idx].saturating_add(1);
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            if out[nidx] > next_dist {
                out[nidx] = next_dist;
                queue.push_back(nidx);
            }
        }
    }
    out
}

fn moisture_value(
    world: &World,
    ocean: &[bool],
    distance_to_ocean: &[u16],
    climate: &OpenSimplex,
    nearby_water: &[f32],
    x: usize,
    y: usize,
) -> f32 {
    let idx = world.idx(x, y);
    if ocean[idx] {
        return 1.0;
    }

    let ocean_influence = 1.0 - (distance_to_ocean[idx] as f32 / (world.width.max(world.height) as f32 * 0.45))
        .clamp(0.0, 1.0);
    let rain_shadow = rain_shadow(world, ocean, x, y);
    let lat = latitude_factor(y, world.height);
    let zonal = (1.0 - (lat - 0.35).abs() * 1.4).clamp(0.0, 1.0);
    let noise = octave_noise(climate, x as f64 * 0.014 + 7.0, y as f64 * 0.014 - 9.0, 4, 0.55, 2.0);

    (ocean_influence * 0.44 + zonal * 0.20 + noise * 0.17 + rain_shadow * 0.11 + nearby_water[idx] * 0.18)
        .clamp(0.0, 1.0)
}

fn rain_shadow(world: &World, ocean: &[bool], x: usize, y: usize) -> f32 {
    let mut moisture = 0.0_f32;
    let mut height_barrier = 0.0_f32;

    for step in 1..=12 {
        let nx = x.saturating_sub(step);
        let idx = world.idx(nx, y);
        if ocean[idx] {
            moisture += 0.08;
            break;
        }
        height_barrier += (world.tiles[idx].raw_elevation - world.sea_level).max(0.0) * 0.12;
    }

    for step in 1..=8 {
        let nx = (x + step).min(world.width - 1);
        let idx = world.idx(nx, y);
        if ocean[idx] {
            moisture += 0.04;
            break;
        }
    }

    (moisture - height_barrier).clamp(0.0, 1.0)
}

fn assign_biomes(world: &mut World) {
    for tile in &mut world.tiles {
        tile.biome = biome_for_tile(
            tile.surface,
            tile.raw_elevation,
            world.sea_level,
            tile.temperature,
            tile.moisture,
        );
    }
}

pub fn biome_for_tile(
    surface: Surface,
    elevation: f32,
    sea_level: f32,
    temperature: f32,
    moisture: f32,
) -> Biome {
    match surface {
        Surface::Ocean => Biome::Ocean,
        Surface::Coast => Biome::Coast,
        Surface::Lake => Biome::Lake,
        Surface::River => {
            if elevation > sea_level + 0.28 {
                Biome::Alpine
            } else {
                land_biome(temperature, (moisture + 0.18).clamp(0.0, 1.0), elevation, sea_level)
            }
        }
        Surface::Land => land_biome(temperature, moisture, elevation, sea_level),
    }
}

fn land_biome(temperature: f32, moisture: f32, elevation: f32, sea_level: f32) -> Biome {
    if elevation > sea_level + 0.34 {
        return Biome::Alpine;
    }
    if temperature < 0.12 {
        return if moisture < 0.35 { Biome::PolarDesert } else { Biome::Tundra };
    }
    if temperature < 0.28 {
        return if moisture < 0.30 { Biome::Steppe } else { Biome::BorealForest };
    }
    if temperature < 0.48 {
        if moisture < 0.18 {
            Biome::Desert
        } else if moisture < 0.35 {
            Biome::TemperateGrassland
        } else if moisture < 0.62 {
            Biome::Woodland
        } else {
            Biome::TemperateForest
        }
    } else if temperature < 0.72 {
        if moisture < 0.16 {
            Biome::Desert
        } else if moisture < 0.38 {
            Biome::Savanna
        } else if moisture < 0.66 {
            Biome::Woodland
        } else {
            Biome::TropicalForest
        }
    } else if moisture < 0.18 {
        Biome::Desert
    } else if moisture < 0.42 {
        Biome::Savanna
    } else if moisture < 0.72 {
        Biome::TropicalForest
    } else {
        Biome::Rainforest
    }
}
