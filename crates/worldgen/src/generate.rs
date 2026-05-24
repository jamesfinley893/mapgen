use std::collections::VecDeque;

use noise::{NoiseFn, OpenSimplex};

use crate::{Biome, Surface, World, WorldConfig};

pub fn generate_world(config: &WorldConfig) -> Result<World, String> {
    config.validate()?;

    let mut world = World::new(config.seed, config.width, config.height, config.sea_level);

    let base = OpenSimplex::new(config.seed as u32);
    let ridge = OpenSimplex::new(config.seed.wrapping_add(1) as u32);
    let climate = OpenSimplex::new(config.seed.wrapping_add(2) as u32);

    let mut ocean = vec![false; world.tiles.len()];
    let mut distance_to_ocean = vec![u16::MAX; world.tiles.len()];
    let mut min_elevation = f32::MAX;
    let mut max_elevation = f32::MIN;

    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            let elevation = sample_elevation(&base, &ridge, x, y, world.width, world.height);
            world.tiles[idx].elevation = elevation;
            min_elevation = min_elevation.min(elevation);
            max_elevation = max_elevation.max(elevation);
        }
    }

    let span = (max_elevation - min_elevation).max(0.0001);
    for idx in 0..world.tiles.len() {
        let normalized = (world.tiles[idx].elevation - min_elevation) / span;
        world.tiles[idx].elevation = normalized;
        ocean[idx] = normalized <= config.sea_level;
    }

    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            world.tiles[idx].surface = classify_surface(&world, &ocean, x, y);
        }
    }

    fill_ocean_distance(&world, &ocean, &mut distance_to_ocean);

    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            let elevation = world.tiles[idx].elevation;
            let lat = latitude_factor(y, world.height);
            let climate_noise = octave_noise(&climate, x as f64 * 0.008, y as f64 * 0.008, 3, 0.5, 2.0);
            let temperature = (1.0 - lat * 1.15 + climate_noise * 0.12 - elevation * 0.35 + config.temperature_bias)
                .clamp(0.0, 1.0);
            let moisture = moisture_value(
                &world,
                &ocean,
                &distance_to_ocean,
                &climate,
                x,
                y,
                config.moisture_bias,
            );
            world.tiles[idx].temperature = temperature;
            world.tiles[idx].moisture = moisture;
        }
    }

    assign_drainage(&mut world, &ocean);
    assign_rivers_and_lakes(&mut world, &ocean);
    assign_biomes(&mut world);

    Ok(world)
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

fn classify_surface(world: &World, ocean: &[bool], x: usize, y: usize) -> Surface {
    let idx = world.idx(x, y);
    if ocean[idx] {
        return Surface::Ocean;
    }
    let near_ocean = world
        .neighbors8(x, y)
        .any(|(nx, ny)| ocean[world.idx(nx, ny)]);
    if near_ocean {
        Surface::Coast
    } else {
        Surface::Land
    }
}

fn fill_ocean_distance(world: &World, ocean: &[bool], out: &mut [u16]) {
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
}

fn moisture_value(
    world: &World,
    ocean: &[bool],
    distance_to_ocean: &[u16],
    climate: &OpenSimplex,
    x: usize,
    y: usize,
    bias: f32,
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

    (ocean_influence * 0.50 + zonal * 0.20 + noise * 0.20 + rain_shadow * 0.10 + bias).clamp(0.0, 1.0)
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
        height_barrier += (world.tiles[idx].elevation - world.sea_level).max(0.0) * 0.12;
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

fn assign_drainage(world: &mut World, ocean: &[bool]) {
    let mut order: Vec<usize> = (0..world.tiles.len()).collect();
    order.sort_by(|a, b| world.tiles[*b].elevation.total_cmp(&world.tiles[*a].elevation));

    for idx in 0..world.tiles.len() {
        if ocean[idx] {
            world.tiles[idx].downstream = None;
            continue;
        }
        let (x, y) = world.coords(idx);
        let mut best = None;
        let mut best_elevation = world.tiles[idx].elevation;
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            let candidate = world.tiles[nidx].elevation;
            if candidate < best_elevation {
                best_elevation = candidate;
                best = Some(nidx);
            }
        }
        world.tiles[idx].downstream = best;
    }

    for idx in order {
        if ocean[idx] {
            continue;
        }
        world.tiles[idx].flow_accumulation += 1.0;
        if let Some(downstream) = world.tiles[idx].downstream {
            let flow = world.tiles[idx].flow_accumulation;
            world.tiles[downstream].flow_accumulation += flow;
        }
    }
}

fn assign_rivers_and_lakes(world: &mut World, ocean: &[bool]) {
    let threshold = ((world.width * world.height) as f32 * 0.0016).max(28.0);
    let mut river = vec![false; world.tiles.len()];

    for idx in 0..world.tiles.len() {
        if ocean[idx] || world.tiles[idx].flow_accumulation < threshold {
            continue;
        }
        let mut current = idx;
        let mut guard = 0;
        while !ocean[current] && guard < world.tiles.len() {
            if river[current] {
                break;
            }
            river[current] = true;
            match world.tiles[current].downstream {
                Some(next) => current = next,
                None => break,
            }
            guard += 1;
        }
    }

    for idx in 0..world.tiles.len() {
        if river[idx] && !ocean[idx] {
            world.tiles[idx].surface = if world.tiles[idx].downstream.is_none() {
                Surface::Lake
            } else {
                Surface::River
            };
        }
    }

    for idx in 0..world.tiles.len() {
        if ocean[idx] || river[idx] {
            continue;
        }
        if world.tiles[idx].downstream.is_none() {
            world.tiles[idx].surface = Surface::Lake;
        }
    }
}

fn assign_biomes(world: &mut World) {
    for tile in &mut world.tiles {
        tile.biome = biome_for_tile(tile.surface, tile.elevation, world.sea_level, tile.temperature, tile.moisture);
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
