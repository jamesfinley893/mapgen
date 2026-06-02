use std::sync::OnceLock;

use worldgen::{
    Biome, MountainFeature, Tile, WaterClass, World, WorldConfig, build_metadata, generate_world,
    mountain_feature_for_tile, render_world,
};

fn config() -> WorldConfig {
    WorldConfig {
        seed: 99,
        width: 128,
        height: 128,
        ..WorldConfig::default()
    }
}

fn fixed_config(seed: u64) -> WorldConfig {
    WorldConfig {
        seed,
        width: 256,
        height: 256,
        ..WorldConfig::default()
    }
}

fn fixed_world(seed: u64) -> &'static World {
    match seed {
        42 => {
            static WORLD: OnceLock<World> = OnceLock::new();
            WORLD.get_or_init(|| generate_world(&fixed_config(42)).unwrap())
        }
        97 => {
            static WORLD: OnceLock<World> = OnceLock::new();
            WORLD.get_or_init(|| generate_world(&fixed_config(97)).unwrap())
        }
        3000 => {
            static WORLD: OnceLock<World> = OnceLock::new();
            WORLD.get_or_init(|| generate_world(&fixed_config(3000)).unwrap())
        }
        7073116918442829777 => {
            static WORLD: OnceLock<World> = OnceLock::new();
            WORLD.get_or_init(|| generate_world(&fixed_config(7073116918442829777)).unwrap())
        }
        12302556654306610728 => {
            static WORLD: OnceLock<World> = OnceLock::new();
            WORLD.get_or_init(|| generate_world(&fixed_config(12302556654306610728)).unwrap())
        }
        _ => panic!("fixed test world is not cached for seed {seed}"),
    }
}

#[test]
fn worlds_contain_land_and_ocean() {
    let world = generate_world(&config()).unwrap();
    let land = world.tiles.iter().filter(|tile| tile.is_land()).count();
    let ocean = world.tiles.iter().filter(|tile| tile.is_ocean()).count();

    assert!(land > 0);
    assert!(ocean > 0);
}

#[test]
fn ocean_tiles_are_boundary_connected() {
    let world = generate_world(&config()).unwrap();
    let mut visited = vec![false; world.tiles.len()];
    let mut queue = std::collections::VecDeque::new();

    for x in 0..world.width {
        for y in [0, world.height - 1] {
            let idx = world.idx(x, y);
            if world.tiles[idx].is_ocean() && !visited[idx] {
                visited[idx] = true;
                queue.push_back(idx);
            }
        }
    }
    for y in 0..world.height {
        for x in [0, world.width - 1] {
            let idx = world.idx(x, y);
            if world.tiles[idx].is_ocean() && !visited[idx] {
                visited[idx] = true;
                queue.push_back(idx);
            }
        }
    }

    while let Some(idx) = queue.pop_front() {
        let (x, y) = world.coords(idx);
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            if !visited[nidx] && world.tiles[nidx].is_ocean() {
                visited[nidx] = true;
                queue.push_back(nidx);
            }
        }
    }

    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.is_ocean() {
            assert!(visited[idx], "found inland ocean tile at index {idx}");
        }
    }
}

#[test]
fn generated_worlds_have_ocean_coast_and_land_tiles() {
    for seed in [42_u64, 97, 3000, 7073116918442829777, 12302556654306610728] {
        let world = fixed_world(seed);
        assert!(world.tiles.iter().any(|tile| tile.is_ocean()));
        assert!(world.tiles.iter().any(|tile| tile.is_coast()));
        assert!(
            world
                .tiles
                .iter()
                .any(|tile| tile.is_land() && !tile.is_coast())
        );
    }
}

#[test]
fn coast_tiles_touch_ocean() {
    let world = generate_world(&config()).unwrap();
    let mut coasts = 0_usize;

    for (idx, tile) in world.tiles.iter().enumerate() {
        if !tile.is_coast() {
            continue;
        }
        coasts += 1;
        let (x, y) = world.coords(idx);
        assert!(
            world
                .neighbors8(x, y)
                .any(|(nx, ny)| world.tiles[world.idx(nx, ny)].is_ocean()),
            "coast tile {idx} is not adjacent to ocean"
        );
    }

    assert!(coasts > 0);
}

#[test]
fn every_tile_is_classified() {
    let world = generate_world(&config()).unwrap();
    assert_eq!(world.tiles.len(), config().width * config().height);
    let metadata = build_metadata(&world, &config());
    let classified: usize = metadata.biome_counts.iter().map(|(_, count)| *count).sum();
    assert_eq!(classified, world.tiles.len());
}

#[test]
fn metadata_counts_land_and_ocean_only() {
    let world = generate_world(&config()).unwrap();
    let metadata = build_metadata(&world, &config());

    assert_eq!(
        metadata.land_tiles + metadata.ocean_tiles,
        world.tiles.len()
    );
    assert!(metadata.highest_elevation >= world.sea_level);
    assert!((0.0..=1.0).contains(&metadata.mean_land_slope));
    assert!((0.0..=1.0).contains(&metadata.mean_land_relief));
    assert!((0.0..=1.0).contains(&metadata.mean_land_continentality));
    assert!((0.0..=1.0).contains(&metadata.alpine_fraction));
    assert!((0.0..=1.0).contains(&metadata.foothill_fraction));
}

#[test]
fn exported_tiles_include_bounded_geology_context() {
    let world = generate_world(&config()).unwrap();

    for (idx, tile) in world.tiles.iter().enumerate() {
        assert!(tile.slope.is_finite(), "tile {idx} slope is not finite");
        assert!(tile.relief.is_finite(), "tile {idx} relief is not finite");
        assert!(
            (0.0..=1.0).contains(&tile.slope),
            "tile {idx} slope out of range: {}",
            tile.slope
        );
        assert!(
            (0.0..=1.0).contains(&tile.relief),
            "tile {idx} relief out of range: {}",
            tile.relief
        );
        assert!(tile.uplift.is_finite(), "tile {idx} uplift is not finite");
        assert!(
            (0.0..=1.0).contains(&tile.uplift),
            "tile {idx} uplift out of range: {}",
            tile.uplift
        );
        assert!(
            tile.mountain_presence.is_finite(),
            "tile {idx} mountain_presence is not finite"
        );
        assert!(
            (0.0..=1.0).contains(&tile.mountain_presence),
            "tile {idx} mountain_presence out of range: {}",
            tile.mountain_presence
        );
        assert!(
            (0.0..=1.0).contains(&tile.continentality),
            "tile {idx} continentality out of range: {}",
            tile.continentality
        );
        for (field, value) in [
            ("temperature", tile.temperature),
            ("moisture", tile.moisture),
            ("precipitation", tile.precipitation),
        ] {
            assert!(value.is_finite(), "tile {idx} {field} is not finite");
            assert!(
                (0.0..=1.0).contains(&value),
                "tile {idx} {field} out of range: {value}"
            );
        }
        assert_eq!(tile.moisture, tile.precipitation);
        if tile.is_ocean() {
            assert_eq!(tile.ocean_distance, 0);
        } else {
            assert!(tile.ocean_distance > 0);
            assert_ne!(tile.biome, Biome::Ocean);
        }
        assert!(!tile.is_coast() || tile.is_land());
        assert_eq!(
            tile.mountain_feature,
            mountain_feature_for_tile(&world, idx)
        );
    }
}

#[test]
fn generator_does_not_emit_freshwater_without_hydrology() {
    for seed in [42_u64, 97, 3000, 7073116918442829777, 12302556654306610728] {
        let world = fixed_world(seed);
        assert!(
            world
                .tiles
                .iter()
                .all(|tile| tile.water != WaterClass::Lake),
            "generated lake tiles before lake hydrology for seed {seed}"
        );
        assert!(
            world
                .tiles
                .iter()
                .all(|tile| tile.biome != Biome::Freshwater),
            "generated freshwater biomes before lake hydrology for seed {seed}"
        );
    }
}

#[test]
fn tile_helpers_use_physical_water_fields() {
    let ocean_biome_land = Tile {
        water: WaterClass::Land,
        coast: true,
        biome: Biome::Ocean,
        ..Tile::default()
    };
    assert!(ocean_biome_land.is_land());
    assert!(!ocean_biome_land.is_ocean());
    assert!(ocean_biome_land.is_coast());

    let forest_biome_ocean = Tile {
        water: WaterClass::Ocean,
        biome: Biome::TemperateForest,
        ..Tile::default()
    };
    assert!(forest_biome_ocean.is_ocean());
    assert!(!forest_biome_ocean.is_land());
}

#[test]
fn exported_mountain_features_track_mountain_biomes() {
    let world = fixed_world(42);
    let mut mountains = 0_usize;

    for tile in &world.tiles {
        match tile.biome {
            Biome::Foothills => {
                mountains += 1;
                assert_eq!(tile.mountain_feature, MountainFeature::Foothill);
            }
            Biome::Alpine => {
                mountains += 1;
                assert!(matches!(
                    tile.mountain_feature,
                    MountainFeature::AlpineSlope | MountainFeature::Ridge | MountainFeature::Summit
                ));
            }
            _ => assert_eq!(tile.mountain_feature, MountainFeature::None),
        }
    }

    assert!(mountains > 0);
}

#[test]
fn rainfall_scale_changes_precipitation() {
    let dry_config = WorldConfig {
        seed: 42,
        width: 128,
        height: 128,
        rainfall_scale: 0.65,
        ..WorldConfig::default()
    };
    let wet_config = WorldConfig {
        rainfall_scale: 1.45,
        ..dry_config.clone()
    };
    let dry_world = generate_world(&dry_config).unwrap();
    let wet_world = generate_world(&wet_config).unwrap();

    assert!(
        mean_precipitation(&wet_world) > mean_precipitation(&dry_world),
        "rainfall scale did not increase tile precipitation"
    );
}

#[test]
fn seed_42_does_not_collapse_into_alpine_blanket() {
    let world = fixed_world(42);
    let land_tiles = world.tiles.iter().filter(|tile| tile.is_land()).count();
    let alpine_tiles = world
        .tiles
        .iter()
        .filter(|tile| tile.biome == Biome::Alpine)
        .count();
    let alpine_fraction = alpine_tiles as f32 / land_tiles.max(1) as f32;

    assert!(
        alpine_fraction < 0.42,
        "alpine fraction too high: {alpine_fraction}"
    );
}

#[test]
fn fixed_seeds_still_produce_meaningful_high_ranges() {
    for seed in [42_u64, 97, 3000] {
        let world = fixed_world(seed);
        let alpine_tiles = world
            .tiles
            .iter()
            .filter(|tile| tile.biome == Biome::Alpine)
            .count();
        assert!(
            alpine_tiles > 800,
            "too little alpine terrain survived for seed {seed}: {alpine_tiles}"
        );
    }
}

#[test]
fn lowlands_are_not_overwhelmingly_woodland_and_tundra() {
    for seed in [42_u64, 97, 12302556654306610728] {
        let world = fixed_world(seed);
        let mut lowland = 0_usize;
        let mut dominant = 0_usize;
        let mut biomes = Vec::new();
        for tile in &world.tiles {
            if tile.is_ocean() {
                continue;
            }
            if tile.elevation > world.sea_level + 0.18
                || matches!(tile.biome, Biome::Alpine | Biome::Foothills)
            {
                continue;
            }
            lowland += 1;
            if !biomes.contains(&tile.biome) {
                biomes.push(tile.biome);
            }
            if matches!(tile.biome, Biome::Woodland | Biome::Tundra) {
                dominant += 1;
            }
        }
        let fraction = dominant as f32 / lowland.max(1) as f32;
        assert!(
            fraction < 0.78,
            "lowland biome mix too narrow for seed {seed}: {fraction}"
        );
        assert!(
            biomes.len() >= 4,
            "lowland biome variety too narrow for seed {seed}: {:?}",
            biomes
        );
    }
}

#[test]
fn low_uplift_land_does_not_form_extreme_cliffs() {
    for seed in [
        42_u64,
        97,
        3000,
        7073116918442829777,
        12302556654306610728,
        2113193422894607504,
        16973655909001791133,
    ] {
        let world = generate_world(&fixed_config(seed)).unwrap();
        let worst = max_low_uplift_land_slope(&world);
        assert!(
            worst < 0.09,
            "low-uplift land has excessive slope for seed {seed}: {worst}"
        );
    }
}

#[test]
fn highland_massifs_are_fragmented_into_subranges() {
    for seed in [42_u64, 97, 3000] {
        let world = fixed_world(seed);
        let components = mountain_component_count(world, 120);
        assert!(
            components >= 2,
            "mountain terrain remains too monolithic for seed {seed}: components={components}"
        );
    }
}

#[test]
fn landmass_shape_is_not_strongly_center_biased() {
    for seed in [42_u64, 97, 7073116918442829777] {
        let world = fixed_world(seed);
        let (center, outer) = center_vs_outer_land_fraction(world);
        assert!(
            center <= outer * 2.2 + 0.12,
            "land remains too center-biased for seed {seed}: center={center} outer={outer}"
        );
    }
}

#[test]
fn edge_land_distribution_varies_by_edge() {
    for seed in [42_u64, 97, 7073116918442829777] {
        let world = fixed_world(seed);
        let fractions = edge_land_fractions(world, 20);
        let min = fractions.iter().copied().fold(1.0_f32, f32::min);
        let max = fractions.iter().copied().fold(0.0_f32, f32::max);
        assert!(
            max - min > 0.08,
            "edge land fractions are too uniform for seed {seed}: {:?}",
            fractions
        );
    }
}

#[test]
fn fixed_seed_set_includes_multiple_major_landmasses() {
    let seeds = [42_u64, 97, 7073116918442829777, 12302556654306610728, 3000];
    let mut found = false;
    for seed in seeds {
        let world = fixed_world(seed);
        let masses = major_landmass_count(world, 900);
        if masses >= 2 {
            found = true;
            break;
        }
    }
    assert!(
        found,
        "fixed seed set did not produce multiple major landmasses"
    );
}

#[test]
fn render_world_produces_expected_dimensions() {
    let world = render_test_world(5, 5);
    let image = render_world(&world, 6);
    assert_eq!(image.width(), 30);
    assert_eq!(image.height(), 30);
}

#[test]
fn tiny_coastal_islets_do_not_draw_checkerboard_coastline() {
    let scale = 6;
    let mut world = render_test_world(5, 5);
    for tile in &mut world.tiles {
        *tile = Tile {
            water: WaterClass::Ocean,
            biome: Biome::Ocean,
            elevation: 0.30,
            ..Tile::default()
        };
    }
    let islet = world.idx(2, 2);
    world.tiles[islet] = Tile {
        water: WaterClass::Land,
        coast: true,
        biome: Biome::TemperateGrassland,
        elevation: 0.53,
        ..Tile::default()
    };

    let image = render_world(&world, scale);
    let coastline = [218, 210, 158, 255];
    for py in 0..scale {
        for px in 0..scale {
            let pixel = image.get_pixel(2 * scale + px, 2 * scale + py);
            assert_ne!(
                pixel.0, coastline,
                "tiny coastal islet retained a hard coastline pixel at ({px},{py})"
            );
        }
    }
}

fn mean_precipitation(world: &World) -> f32 {
    world
        .tiles
        .iter()
        .map(|tile| tile.precipitation)
        .sum::<f32>()
        / world.tiles.len().max(1) as f32
}

fn render_test_world(width: usize, height: usize) -> World {
    let mut world = World::new(7, width, height, 0.50, 0);
    for tile in &mut world.tiles {
        *tile = Tile {
            water: WaterClass::Land,
            biome: Biome::TemperateGrassland,
            elevation: 0.56,
            temperature: 0.55,
            moisture: 0.35,
            ..Tile::default()
        };
    }
    world
}

fn center_vs_outer_land_fraction(world: &World) -> (f32, f32) {
    let x0 = world.width / 4;
    let x1 = world.width * 3 / 4;
    let y0 = world.height / 4;
    let y1 = world.height * 3 / 4;
    let mut center_land = 0_usize;
    let mut center_total = 0_usize;
    let mut outer_land = 0_usize;
    let mut outer_total = 0_usize;

    for y in 0..world.height {
        for x in 0..world.width {
            let land = world.tiles[world.idx(x, y)].is_land();
            if (x0..x1).contains(&x) && (y0..y1).contains(&y) {
                center_total += 1;
                center_land += land as usize;
            } else {
                outer_total += 1;
                outer_land += land as usize;
            }
        }
    }

    (
        center_land as f32 / center_total.max(1) as f32,
        outer_land as f32 / outer_total.max(1) as f32,
    )
}

fn edge_land_fractions(world: &World, band: usize) -> [f32; 4] {
    let band = band.min(world.width / 2).min(world.height / 2).max(1);
    let mut land = [0_usize; 4];
    let mut total = [0_usize; 4];

    for y in 0..world.height {
        for x in 0..world.width {
            let is_land = world.tiles[world.idx(x, y)].is_land();
            if y < band {
                total[0] += 1;
                land[0] += is_land as usize;
            }
            if x >= world.width - band {
                total[1] += 1;
                land[1] += is_land as usize;
            }
            if y >= world.height - band {
                total[2] += 1;
                land[2] += is_land as usize;
            }
            if x < band {
                total[3] += 1;
                land[3] += is_land as usize;
            }
        }
    }

    [
        land[0] as f32 / total[0].max(1) as f32,
        land[1] as f32 / total[1].max(1) as f32,
        land[2] as f32 / total[2].max(1) as f32,
        land[3] as f32 / total[3].max(1) as f32,
    ]
}

fn max_low_uplift_land_slope(world: &World) -> f32 {
    world
        .tiles
        .iter()
        .filter(|tile| tile.is_land() && tile.uplift < 0.08)
        .map(|tile| tile.slope)
        .fold(0.0_f32, f32::max)
}

fn major_landmass_count(world: &World, min_area: usize) -> usize {
    component_count(world, min_area, |tile| tile.is_land())
}

fn mountain_component_count(world: &World, min_area: usize) -> usize {
    component_count(world, min_area, |tile| {
        matches!(tile.biome, Biome::Alpine | Biome::Foothills)
    })
}

fn component_count<F>(world: &World, min_area: usize, accept: F) -> usize
where
    F: Fn(&Tile) -> bool,
{
    let mut visited = vec![false; world.tiles.len()];
    let mut count = 0_usize;

    for start in 0..world.tiles.len() {
        if visited[start] || !accept(&world.tiles[start]) {
            continue;
        }
        let mut area = 0_usize;
        let mut queue = std::collections::VecDeque::from([start]);
        visited[start] = true;

        while let Some(idx) = queue.pop_front() {
            area += 1;
            let (x, y) = world.coords(idx);
            for (nx, ny) in world.neighbors8(x, y) {
                let nidx = world.idx(nx, ny);
                if !visited[nidx] && accept(&world.tiles[nidx]) {
                    visited[nidx] = true;
                    queue.push_back(nidx);
                }
            }
        }

        if area >= min_area {
            count += 1;
        }
    }

    count
}
