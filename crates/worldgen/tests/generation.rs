use std::sync::OnceLock;

use worldgen::{
    Biome, MountainFeature, RenderConfig, Surface, Tile, World, WorldConfig, build_metadata,
    generate_world, mountain_feature_for_tile, render_world,
};

fn config() -> WorldConfig {
    WorldConfig {
        seed: 99,
        width: 128,
        height: 128,
        render_scale: 2,
        ..WorldConfig::default()
    }
}

fn fixed_config(seed: u64) -> WorldConfig {
    WorldConfig {
        seed,
        width: 256,
        height: 256,
        render_scale: 2,
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
    let land = world
        .tiles
        .iter()
        .filter(|tile| tile.surface != Surface::Ocean)
        .count();
    let ocean = world
        .tiles
        .iter()
        .filter(|tile| tile.surface == Surface::Ocean)
        .count();

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
            if world.tiles[idx].surface == Surface::Ocean && !visited[idx] {
                visited[idx] = true;
                queue.push_back(idx);
            }
        }
    }
    for y in 0..world.height {
        for x in [0, world.width - 1] {
            let idx = world.idx(x, y);
            if world.tiles[idx].surface == Surface::Ocean && !visited[idx] {
                visited[idx] = true;
                queue.push_back(idx);
            }
        }
    }

    while let Some(idx) = queue.pop_front() {
        let (x, y) = world.coords(idx);
        for (nx, ny) in world.neighbors8(x, y) {
            let nidx = world.idx(nx, ny);
            if !visited[nidx] && world.tiles[nidx].surface == Surface::Ocean {
                visited[nidx] = true;
                queue.push_back(nidx);
            }
        }
    }

    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface == Surface::Ocean {
            assert!(visited[idx], "found inland ocean tile at index {idx}");
        }
    }
}

#[test]
fn generated_worlds_use_only_ocean_coast_and_land_surfaces() {
    for seed in [42_u64, 97, 3000, 7073116918442829777, 12302556654306610728] {
        let world = fixed_world(seed);
        for tile in &world.tiles {
            assert!(matches!(
                tile.surface,
                Surface::Ocean | Surface::Coast | Surface::Land
            ));
        }
    }
}

#[test]
fn coast_tiles_touch_ocean() {
    let world = generate_world(&config()).unwrap();
    let mut coasts = 0_usize;

    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface != Surface::Coast {
            continue;
        }
        coasts += 1;
        let (x, y) = world.coords(idx);
        assert!(
            world
                .neighbors8(x, y)
                .any(|(nx, ny)| world.tiles[world.idx(nx, ny)].surface == Surface::Ocean),
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
fn metadata_biome_counts_are_sorted_and_omit_empty_biomes() {
    let mut world = World::new(11, 4, 3, 0.50, 0);
    let biomes = [
        Biome::Ocean,
        Biome::Ocean,
        Biome::Ocean,
        Biome::Ocean,
        Biome::Desert,
        Biome::Desert,
        Biome::Desert,
        Biome::Rainforest,
        Biome::Rainforest,
        Biome::Alpine,
        Biome::Coast,
        Biome::Coast,
    ];
    for (tile, biome) in world.tiles.iter_mut().zip(biomes) {
        tile.biome = biome;
        tile.surface = match biome {
            Biome::Ocean => Surface::Ocean,
            Biome::Coast => Surface::Coast,
            _ => Surface::Land,
        };
    }

    let metadata = build_metadata(&world, &config());
    assert_eq!(
        metadata.biome_counts,
        vec![
            (Biome::Ocean, 4),
            (Biome::Desert, 3),
            (Biome::Coast, 2),
            (Biome::Rainforest, 2),
            (Biome::Alpine, 1),
        ]
    );
    assert!(metadata.biome_counts.iter().all(|(_, count)| *count > 0));
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
    assert!((0.0..=1.0).contains(&metadata.mean_land_runoff));
    assert!((0.0..=1.0).contains(&metadata.mean_land_continentality));
    assert!(metadata.river_tiles <= metadata.land_tiles);
    assert!((0.0..=1.0).contains(&metadata.strongest_river));
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
        assert!(tile.runoff.is_finite(), "tile {idx} runoff is not finite");
        assert!(
            (0.0..=1.0).contains(&tile.runoff),
            "tile {idx} runoff out of range: {}",
            tile.runoff
        );
        assert!(
            tile.flow_accumulation.is_finite() && tile.flow_accumulation >= 0.0,
            "tile {idx} flow accumulation out of range: {}",
            tile.flow_accumulation
        );
        assert!(
            (0.0..=1.0).contains(&tile.river),
            "tile {idx} river out of range: {}",
            tile.river
        );
        assert!(
            (-1..=7).contains(&tile.flow_direction),
            "tile {idx} flow direction out of range: {}",
            tile.flow_direction
        );
        assert!(
            (0.0..=1.0).contains(&tile.continentality),
            "tile {idx} continentality out of range: {}",
            tile.continentality
        );
        if tile.surface == Surface::Ocean {
            assert_eq!(tile.ocean_distance, 0);
            assert_eq!(tile.runoff, 0.0);
            assert_eq!(tile.flow_accumulation, 0.0);
            assert_eq!(tile.river, 0.0);
            assert_eq!(tile.flow_direction, -1);
        } else {
            assert!(tile.ocean_distance > 0);
            if tile.river > 0.08 {
                assert!(tile.flow_direction >= 0);
            }
        }
        assert_eq!(
            tile.mountain_feature,
            mountain_feature_for_tile(&world, idx)
        );
    }
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
fn fixed_seed_set_produces_ocean_draining_river_networks() {
    let mut river_worlds = 0_usize;

    for seed in [42_u64, 97, 3000, 7073116918442829777, 12302556654306610728] {
        let world = fixed_world(seed);
        let river_tiles = world
            .tiles
            .iter()
            .filter(|tile| tile.surface != Surface::Ocean && tile.river > 0.08)
            .count();
        let strongest = world
            .tiles
            .iter()
            .map(|tile| tile.river)
            .fold(0.0_f32, f32::max);

        if river_tiles > world.width / 2 && strongest > 0.55 {
            river_worlds += 1;
        }
    }

    assert!(
        river_worlds >= 4,
        "fixed seed set did not produce enough visible river networks: {river_worlds}"
    );
}

#[test]
fn major_rivers_are_incised_into_local_corridors() {
    let world = fixed_world(42);
    let mut major_rivers = 0_usize;
    let mut incised = 0_usize;
    let mut incision_sum = 0.0_f32;

    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface == Surface::Ocean || tile.river <= 0.55 {
            continue;
        }

        let (x, y) = world.coords(idx);
        let mut neighbor_sum = 0.0_f32;
        let mut neighbor_count = 0.0_f32;
        for (nx, ny) in world.neighbors8(x, y) {
            let neighbor = &world.tiles[world.idx(nx, ny)];
            if neighbor.surface == Surface::Ocean {
                continue;
            }
            neighbor_sum += neighbor.raw_elevation;
            neighbor_count += 1.0;
        }
        if neighbor_count <= 0.0 {
            continue;
        }

        major_rivers += 1;
        let local_incision = neighbor_sum / neighbor_count - tile.raw_elevation;
        incision_sum += local_incision;
        if local_incision > 0.0005 {
            incised += 1;
        }
    }

    assert!(major_rivers > world.width / 2);
    assert!(
        incised as f32 / major_rivers.max(1) as f32 > 0.72,
        "too few major river tiles are locally incised: {incised}/{major_rivers}"
    );
    assert!(
        incision_sum / major_rivers.max(1) as f32 > 0.0015,
        "mean local river incision is too weak"
    );
}

#[test]
fn interior_major_rivers_do_not_form_long_straight_runs() {
    let world = fixed_world(42);
    let longest = longest_major_river_straight_run(world, 8);

    assert!(
        longest < world.width / 9,
        "interior major river channel is too straight: {longest}"
    );
}

#[test]
fn warm_major_river_corridors_support_riparian_ecology() {
    let world = fixed_world(42);
    let mut warm_lowland_rivers = 0_usize;
    let mut riparian = 0_usize;
    let mut dry = 0_usize;

    for tile in &world.tiles {
        if tile.surface == Surface::Ocean
            || tile.river <= 0.55
            || tile.temperature < 0.28
            || tile.raw_elevation > world.sea_level + 0.32
        {
            continue;
        }

        warm_lowland_rivers += 1;
        if matches!(
            tile.biome,
            Biome::Woodland
                | Biome::Wetland
                | Biome::TemperateForest
                | Biome::BorealForest
                | Biome::TropicalForest
                | Biome::Rainforest
        ) {
            riparian += 1;
        }
        if matches!(tile.biome, Biome::Desert | Biome::Steppe) {
            dry += 1;
        }
    }

    assert!(warm_lowland_rivers > world.width / 3);
    assert!(
        riparian as f32 / warm_lowland_rivers.max(1) as f32 > 0.42,
        "too few warm lowland river tiles become riparian ecology: {riparian}/{warm_lowland_rivers}"
    );
    assert!(
        dry as f32 / (warm_lowland_rivers.max(1) as f32) < 0.08,
        "too many warm lowland river tiles remain dry steppe/desert: {dry}/{warm_lowland_rivers}"
    );
}

#[test]
fn fixed_seed_set_includes_wetland_lowlands() {
    let mut wetland_worlds = 0_usize;

    for seed in [42_u64, 97, 3000, 7073116918442829777, 12302556654306610728] {
        let world = fixed_world(seed);
        let wetlands = world
            .tiles
            .iter()
            .filter(|tile| tile.biome == Biome::Wetland)
            .count();
        if wetlands > world.width / 6 {
            wetland_worlds += 1;
        }
    }

    assert!(
        wetland_worlds >= 2,
        "fixed seed set did not produce enough wetland lowlands: {wetland_worlds}"
    );
}

#[test]
fn wetlands_stay_flat_low_and_saturated() {
    for seed in [42_u64, 97, 3000, 7073116918442829777, 12302556654306610728] {
        let world = fixed_world(seed);
        for (idx, tile) in world.tiles.iter().enumerate() {
            if tile.biome != Biome::Wetland {
                continue;
            }
            assert_eq!(tile.surface, Surface::Land, "wetland {idx} is not land");
            assert!(
                tile.raw_elevation <= world.sea_level + 0.24,
                "wetland {idx} is too high: {}",
                tile.raw_elevation
            );
            assert!(
                tile.relief < 0.052,
                "wetland {idx} is too rugged: {}",
                tile.relief
            );
            assert!(
                tile.moisture > 0.39,
                "wetland {idx} is too dry: moisture={}",
                tile.moisture
            );
        }
    }
}

#[test]
fn fixed_seed_set_includes_small_freshwater_bodies() {
    let mut freshwater_worlds = 0_usize;

    for seed in [42_u64, 97, 3000, 7073116918442829777, 12302556654306610728] {
        let world = fixed_world(seed);
        let freshwater = world
            .tiles
            .iter()
            .filter(|tile| tile.biome == Biome::Freshwater)
            .count();
        if freshwater > 0 {
            freshwater_worlds += 1;
        }
    }

    assert!(
        freshwater_worlds >= 2,
        "fixed seed set did not produce enough freshwater bodies: {freshwater_worlds}"
    );
}

#[test]
fn freshwater_bodies_stay_low_flat_and_non_oceanic() {
    for seed in [42_u64, 97, 3000, 7073116918442829777, 12302556654306610728] {
        let world = fixed_world(seed);
        for (idx, tile) in world.tiles.iter().enumerate() {
            if tile.biome != Biome::Freshwater {
                continue;
            }
            assert_eq!(
                tile.surface,
                Surface::Land,
                "freshwater {idx} is not inland land"
            );
            assert!(
                tile.raw_elevation <= world.sea_level + 0.14,
                "freshwater {idx} is too high: {}",
                tile.raw_elevation
            );
            assert!(
                tile.relief < 0.026,
                "freshwater {idx} is too rugged: {}",
                tile.relief
            );
            assert!(
                tile.moisture > 0.46,
                "freshwater {idx} is too dry: {}",
                tile.moisture
            );
            assert!(
                tile.flow_accumulation < 3.0,
                "freshwater {idx} follows a river channel: {}",
                tile.flow_accumulation
            );
        }
    }
}

#[test]
fn rainfall_scale_changes_precipitation() {
    let dry_config = WorldConfig {
        seed: 42,
        width: 128,
        height: 128,
        render_scale: 2,
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
    let land_tiles = world
        .tiles
        .iter()
        .filter(|tile| tile.surface != Surface::Ocean)
        .count();
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
fn mountain_capable_fixed_seeds_can_rise_above_normalized_range() {
    let mut strong_ranges = 0_usize;

    for seed in [42_u64, 97, 3000, 7073116918442829777] {
        let world = fixed_world(seed);
        let highest = world
            .tiles
            .iter()
            .map(|tile| tile.raw_elevation)
            .fold(f32::MIN, f32::max);
        let alpine_tiles = world
            .tiles
            .iter()
            .filter(|tile| tile.biome == Biome::Alpine)
            .count();

        if highest > 1.0 && alpine_tiles > 700 {
            strong_ranges += 1;
        }
    }

    assert!(
        strong_ranges >= 2,
        "fixed seed set did not produce enough unbounded alpine ranges: {strong_ranges}"
    );
}

#[test]
fn lowlands_are_not_overwhelmingly_woodland_and_tundra() {
    for seed in [42_u64, 97, 12302556654306610728] {
        let world = fixed_world(seed);
        let mut lowland = 0_usize;
        let mut dominant = 0_usize;
        for tile in &world.tiles {
            if tile.surface == Surface::Ocean {
                continue;
            }
            if tile.raw_elevation > world.sea_level + 0.18
                || matches!(tile.biome, Biome::Alpine | Biome::Foothills)
            {
                continue;
            }
            lowland += 1;
            if matches!(tile.biome, Biome::Woodland | Biome::Tundra) {
                dominant += 1;
            }
        }
        let fraction = dominant as f32 / lowland.max(1) as f32;
        assert!(
            fraction < 0.78,
            "lowland biome mix too narrow for seed {seed}: {fraction}"
        );
    }
}

#[test]
fn lowland_biome_edges_are_not_long_straight_climate_bands() {
    for seed in [42_u64, 97, 7073116918442829777, 12302556654306610728] {
        let world = fixed_world(seed);
        let longest = longest_lowland_biome_boundary_run(world);
        assert!(
            longest < world.width / 3,
            "lowland biome boundary has an implausibly straight run for seed {seed}: {longest}"
        );
    }
}

#[test]
fn highland_massifs_are_fragmented_into_subranges() {
    let mut fragmented = 0_usize;
    for seed in [42_u64, 97, 3000, 7073116918442829777] {
        let world = fixed_world(seed);
        let components = mountain_component_count(world, 120);
        fragmented += (components >= 2) as usize;
    }

    assert!(
        fragmented >= 3,
        "fixed seed set did not produce enough fragmented mountain ranges: {fragmented}"
    );
}

#[test]
fn landmass_shape_is_not_strongly_center_biased() {
    for seed in [42_u64, 97, 7073116918442829777] {
        let world = fixed_world(seed);
        let (center, outer) = center_vs_outer_land_fraction(world);
        assert!(
            center <= outer * 2.6 + 0.12,
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
    let image = render_world(&world, RenderConfig { scale: 6 });
    assert_eq!(image.width(), 30);
    assert_eq!(image.height(), 30);
}

#[test]
fn ocean_depth_rendering_interpolates_with_neighbor_depth() {
    let scale = 8;
    let mut world = World::new(7, 2, 2, 0.50, 0);
    for y in 0..world.height {
        for x in 0..world.width {
            let raw_elevation = if x == 0 { 0.49 } else { 0.20 };
            let idx = world.idx(x, y);
            world.tiles[idx] = warm_ocean_tile(raw_elevation);
        }
    }

    let image = render_world(&world, RenderConfig { scale });
    let shallow_side = image.get_pixel(1, scale / 2).0;
    let deep_side = image.get_pixel(scale - 1, scale / 2).0;
    assert!(
        luma(shallow_side) > luma(deep_side) + 14.0,
        "ocean depth stayed too flat inside a tile: shallow={shallow_side:?} deep={deep_side:?}"
    );
}

#[test]
fn diagonal_land_contact_softens_adjacent_ocean_corner() {
    let scale = 18;
    let mut base_world = World::new(7, 3, 3, 0.50, 0);
    for tile in &mut base_world.tiles {
        *tile = warm_ocean_tile(0.34);
    }

    let mut shore_world = base_world.clone();
    let diagonal_land = shore_world.idx(0, 0);
    shore_world.tiles[diagonal_land] = Tile {
        surface: Surface::Coast,
        biome: Biome::Coast,
        raw_elevation: 0.525,
        temperature: 0.52,
        moisture: 0.40,
        ..Tile::default()
    };

    let base = render_world(&base_world, RenderConfig { scale });
    let shore = render_world(&shore_world, RenderConfig { scale });
    let diagonal_corner = (scale, scale);
    let tile_center = (scale + scale / 2, scale + scale / 2);
    let corner_delta = luma(shore.get_pixel(diagonal_corner.0, diagonal_corner.1).0)
        - luma(base.get_pixel(diagonal_corner.0, diagonal_corner.1).0);
    let center_delta = luma(shore.get_pixel(tile_center.0, tile_center.1).0)
        - luma(base.get_pixel(tile_center.0, tile_center.1).0);

    assert!(
        corner_delta > 4.0,
        "diagonal land contact did not brighten the adjacent ocean corner: delta={corner_delta}"
    );
    assert!(
        corner_delta > center_delta + 3.0,
        "diagonal shelf influence should stay strongest near the shared corner: corner={corner_delta} center={center_delta}"
    );
}

#[test]
fn shallow_ocean_texture_is_coherent_not_block_filled() {
    let scale = 12;
    let mut world = World::new(7, 2, 2, 0.50, 0);
    for tile in &mut world.tiles {
        *tile = warm_ocean_tile(0.47);
    }

    let image = render_world(&world, RenderConfig { scale });
    let mut colors = std::collections::HashSet::new();
    for py in 3..7 {
        for px in 3..7 {
            let pixel = image.get_pixel(px, py).0;
            colors.insert((pixel[0], pixel[1], pixel[2]));
        }
    }

    assert!(
        colors.len() > 1,
        "shallow ocean texture still forms block-filled patches: {} colors",
        colors.len()
    );
}

#[test]
fn cold_shallow_ocean_renders_broken_ice_tint() {
    let scale = 12;
    let mut warm_world = World::new(7, 2, 2, 0.50, 0);
    for tile in &mut warm_world.tiles {
        *tile = warm_ocean_tile(0.47);
    }

    let mut cold_world = warm_world.clone();
    for tile in &mut cold_world.tiles {
        tile.temperature = 0.0;
    }

    let warm = render_world(&warm_world, RenderConfig { scale });
    let cold = render_world(&cold_world, RenderConfig { scale });
    let mut warm_luma = 0.0_f32;
    let mut cold_luma = 0.0_f32;
    let mut samples = 0.0_f32;
    for py in 2..10 {
        for px in 2..10 {
            warm_luma += luma(warm.get_pixel(px, py).0);
            cold_luma += luma(cold.get_pixel(px, py).0);
            samples += 1.0;
        }
    }
    warm_luma /= samples;
    cold_luma /= samples;

    assert!(
        cold_luma > warm_luma + 10.0,
        "cold shallow ocean did not pick up a visible ice tint: warm={warm_luma} cold={cold_luma}"
    );
}

#[test]
fn tropical_shallow_ocean_renders_lagoon_tint() {
    let scale = 12;
    let mut temperate_world = World::new(7, 2, 2, 0.50, 0);
    for tile in &mut temperate_world.tiles {
        *tile = warm_ocean_tile(0.47);
        tile.temperature = 0.50;
    }

    let mut tropical_world = temperate_world.clone();
    for tile in &mut tropical_world.tiles {
        tile.temperature = 0.90;
    }

    let temperate = render_world(&temperate_world, RenderConfig { scale });
    let tropical = render_world(&tropical_world, RenderConfig { scale });
    let mut temperate_green = 0.0_f32;
    let mut tropical_green = 0.0_f32;
    let mut temperate_chroma = 0.0_f32;
    let mut tropical_chroma = 0.0_f32;
    let mut samples = 0.0_f32;
    for py in 2..10 {
        for px in 2..10 {
            let temperate_pixel = temperate.get_pixel(px, py).0;
            let tropical_pixel = tropical.get_pixel(px, py).0;
            temperate_green += temperate_pixel[1] as f32;
            tropical_green += tropical_pixel[1] as f32;
            temperate_chroma += green_minus_red(temperate_pixel);
            tropical_chroma += green_minus_red(tropical_pixel);
            samples += 1.0;
        }
    }
    temperate_green /= samples;
    tropical_green /= samples;
    temperate_chroma /= samples;
    tropical_chroma /= samples;

    assert!(
        tropical_green > temperate_green + 9.0,
        "tropical shallow ocean did not brighten toward lagoon water: temperate={temperate_green} tropical={tropical_green}"
    );
    assert!(
        tropical_chroma > temperate_chroma + 6.0,
        "tropical shallow ocean did not shift green/cyan enough: temperate={temperate_chroma} tropical={tropical_chroma}"
    );
}

#[test]
fn land_adjacent_tropical_shelf_keeps_lagoon_tint() {
    let scale = 16;
    let mut temperate_world = World::new(7, 3, 2, 0.50, 0);
    for y in 0..temperate_world.height {
        for x in 0..temperate_world.width {
            let idx = temperate_world.idx(x, y);
            temperate_world.tiles[idx] = if x == 2 {
                Tile {
                    surface: Surface::Land,
                    biome: Biome::TemperateGrassland,
                    raw_elevation: 0.56,
                    temperature: 0.50,
                    moisture: 0.35,
                    ..Tile::default()
                }
            } else {
                let mut tile = warm_ocean_tile(0.47);
                tile.temperature = 0.50;
                tile
            };
        }
    }

    let mut tropical_world = temperate_world.clone();
    for tile in &mut tropical_world.tiles {
        tile.temperature = 0.90;
    }

    let temperate = render_world(&temperate_world, RenderConfig { scale });
    let tropical = render_world(&tropical_world, RenderConfig { scale });
    let sample = (2 * scale - 2, scale / 2);
    let temperate_pixel = temperate.get_pixel(sample.0, sample.1).0;
    let tropical_pixel = tropical.get_pixel(sample.0, sample.1).0;

    assert!(
        tropical_pixel[1] as f32 > temperate_pixel[1] as f32 + 6.0
            && green_minus_red(tropical_pixel) > green_minus_red(temperate_pixel) + 3.5,
        "land-adjacent tropical shelf lost lagoon tint: temperate={temperate_pixel:?} tropical={tropical_pixel:?}"
    );
}

#[test]
fn land_color_rendering_interpolates_with_neighbor_biome_color() {
    let scale = 8;
    let mut world = render_test_world(4, 2);
    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            let biome = if x < 2 {
                Biome::Desert
            } else {
                Biome::Rainforest
            };
            world.tiles[idx] = Tile {
                surface: Surface::Land,
                biome,
                raw_elevation: 0.56,
                temperature: if x < 2 { 0.82 } else { 0.76 },
                moisture: if x < 2 { 0.10 } else { 0.86 },
                ..Tile::default()
            };
        }
    }

    let image = render_world(&world, RenderConfig { scale });
    let desert_side = image.get_pixel(scale + 1, scale / 2).0;
    let rainforest_side = image.get_pixel(2 * scale - 1, scale / 2).0;
    assert!(
        desert_side[0] > rainforest_side[0] + 28,
        "land color stayed too flat inside a tile: desert={desert_side:?} rainforest={rainforest_side:?}"
    );
}

#[test]
fn diagonal_land_color_influences_shared_render_vertex() {
    let scale = 18;
    let mut base_world = render_test_world(3, 3);
    for tile in &mut base_world.tiles {
        *tile = Tile {
            surface: Surface::Land,
            biome: Biome::TemperateGrassland,
            raw_elevation: 0.56,
            temperature: 0.54,
            moisture: 0.34,
            ..Tile::default()
        };
    }

    let mut mixed_world = base_world.clone();
    let diagonal = mixed_world.idx(0, 0);
    mixed_world.tiles[diagonal] = Tile {
        surface: Surface::Land,
        biome: Biome::Rainforest,
        raw_elevation: 0.56,
        temperature: 0.82,
        moisture: 0.86,
        ..Tile::default()
    };

    let base = render_world(&base_world, RenderConfig { scale });
    let mixed = render_world(&mixed_world, RenderConfig { scale });
    let corner = (scale, scale);
    let center = (scale + scale / 2, scale + scale / 2);
    let corner_delta = color_delta(
        base.get_pixel(corner.0, corner.1).0,
        mixed.get_pixel(corner.0, corner.1).0,
    );
    let center_delta = color_delta(
        base.get_pixel(center.0, center.1).0,
        mixed.get_pixel(center.0, center.1).0,
    );
    let base_corner = base.get_pixel(corner.0, corner.1).0;
    let mixed_corner = mixed.get_pixel(corner.0, corner.1).0;

    assert!(
        corner_delta > 9.0,
        "diagonal biome did not influence the shared land render vertex: delta={corner_delta}"
    );
    assert!(
        corner_delta > center_delta + 5.0,
        "diagonal vertex influence should stay strongest near the corner: corner={corner_delta} center={center_delta}"
    );
    assert!(
        green_minus_red(mixed_corner) > green_minus_red(base_corner) + 4.0,
        "diagonal rainforest did not pull the corner toward canopy color: base={base_corner:?} mixed={mixed_corner:?}"
    );
}

#[test]
fn biome_edge_rendering_blends_land_cover_texture_before_boundary() {
    let scale = 18;
    let mut world = render_test_world(5, 3);
    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            let biome = if x < 2 {
                Biome::TemperateGrassland
            } else {
                Biome::Rainforest
            };
            world.tiles[idx] = Tile {
                surface: Surface::Land,
                biome,
                raw_elevation: 0.56,
                temperature: if x < 2 { 0.54 } else { 0.78 },
                moisture: if x < 2 { 0.34 } else { 0.82 },
                ..Tile::default()
            };
        }
    }

    let image = render_world(&world, RenderConfig { scale });
    let y = scale + scale / 2;
    let deep_grass = image.get_pixel(scale + 2, y).0;
    let edge_grass = image.get_pixel(2 * scale - 2, y).0;
    let edge_forest = image.get_pixel(2 * scale + 1, y).0;
    let deep_forest = image.get_pixel(3 * scale + scale / 2, y).0;
    let interior_delta = color_delta(deep_grass, deep_forest);
    let edge_delta = color_delta(edge_grass, edge_forest);

    assert!(
        edge_delta < interior_delta * 0.72,
        "land-cover texture still flips too abruptly at the biome edge: edge={edge_delta} interior={interior_delta}"
    );
    assert!(
        green_minus_red(edge_grass) > green_minus_red(deep_grass) + 8.0,
        "neighbor canopy texture did not begin inside the grassland edge: deep={deep_grass:?} edge={edge_grass:?}"
    );
}

#[test]
fn forest_rendering_has_canopy_hue_texture() {
    let scale = 12;
    let mut world = render_test_world(3, 3);
    for tile in &mut world.tiles {
        *tile = Tile {
            surface: Surface::Land,
            biome: Biome::TemperateForest,
            raw_elevation: 0.56,
            temperature: 0.55,
            moisture: 0.58,
            ..Tile::default()
        };
    }

    let image = render_world(&world, RenderConfig { scale });
    let mut min_chroma = f32::MAX;
    let mut max_chroma = f32::MIN;
    for py in 0..scale {
        for px in 0..scale {
            let pixel = image.get_pixel(scale + px, scale + py).0;
            let chroma = green_minus_red(pixel);
            min_chroma = min_chroma.min(chroma);
            max_chroma = max_chroma.max(chroma);
        }
    }

    assert!(
        max_chroma - min_chroma > 4.0,
        "forest canopy lacks hue texture: min={min_chroma} max={max_chroma}"
    );
}

#[test]
fn boreal_forest_rendering_has_conifer_shadow_texture() {
    let scale = 16;
    let mut world = render_test_world(3, 3);
    for tile in &mut world.tiles {
        *tile = Tile {
            surface: Surface::Land,
            biome: Biome::BorealForest,
            raw_elevation: 0.58,
            temperature: 0.18,
            moisture: 0.50,
            ..Tile::default()
        };
    }

    let image = render_world(&world, RenderConfig { scale });
    let mut min_chroma = f32::MAX;
    let mut max_chroma = f32::MIN;
    let mut min_luma = f32::MAX;
    let mut max_luma = f32::MIN;
    for py in 0..scale {
        for px in 0..scale {
            let pixel = image.get_pixel(scale + px, scale + py).0;
            let chroma = green_minus_red(pixel);
            min_chroma = min_chroma.min(chroma);
            max_chroma = max_chroma.max(chroma);
            let luma = luma(pixel);
            min_luma = min_luma.min(luma);
            max_luma = max_luma.max(luma);
        }
    }

    assert!(
        max_chroma - min_chroma > 5.0,
        "boreal forest lacks conifer hue texture: min={min_chroma} max={max_chroma}"
    );
    assert!(
        max_luma - min_luma > 6.0,
        "boreal forest lacks cool canopy shadow contrast: min={min_luma} max={max_luma}"
    );
}

#[test]
fn rainforest_rendering_has_dense_wet_canopy_texture() {
    let scale = 16;
    let mut world = render_test_world(3, 3);
    for tile in &mut world.tiles {
        *tile = Tile {
            surface: Surface::Land,
            biome: Biome::Rainforest,
            raw_elevation: 0.56,
            temperature: 0.82,
            moisture: 0.82,
            ..Tile::default()
        };
    }

    let image = render_world(&world, RenderConfig { scale });
    let mut min_wet_green = f32::MAX;
    let mut max_wet_green = f32::MIN;
    let mut min_luma = f32::MAX;
    let mut max_luma = f32::MIN;
    for py in 0..scale {
        for px in 0..scale {
            let pixel = image.get_pixel(scale + px, scale + py).0;
            let wet_green = pixel[1] as f32 * 0.75 + pixel[2] as f32 * 0.20 - pixel[0] as f32;
            min_wet_green = min_wet_green.min(wet_green);
            max_wet_green = max_wet_green.max(wet_green);
            let luma = luma(pixel);
            min_luma = min_luma.min(luma);
            max_luma = max_luma.max(luma);
        }
    }

    assert!(
        max_wet_green - min_wet_green > 8.0,
        "rainforest lacks dense wet canopy chroma variation: min={min_wet_green} max={max_wet_green}"
    );
    assert!(
        max_luma - min_luma > 6.0,
        "rainforest lacks deep understory and wet highlight contrast: min={min_luma} max={max_luma}"
    );
}

#[test]
fn woodland_rendering_has_open_canopy_mosaic_texture() {
    let scale = 16;
    let mut world = render_test_world(3, 3);
    for tile in &mut world.tiles {
        *tile = Tile {
            surface: Surface::Land,
            biome: Biome::Woodland,
            raw_elevation: 0.56,
            temperature: 0.52,
            moisture: 0.42,
            ..Tile::default()
        };
    }

    let image = render_world(&world, RenderConfig { scale });
    let mut min_chroma = f32::MAX;
    let mut max_chroma = f32::MIN;
    let mut min_luma = f32::MAX;
    let mut max_luma = f32::MIN;
    for py in 0..scale {
        for px in 0..scale {
            let pixel = image.get_pixel(scale + px, scale + py).0;
            let chroma = green_minus_red(pixel);
            min_chroma = min_chroma.min(chroma);
            max_chroma = max_chroma.max(chroma);
            let luma = luma(pixel);
            min_luma = min_luma.min(luma);
            max_luma = max_luma.max(luma);
        }
    }

    assert!(
        max_chroma - min_chroma > 6.0,
        "woodland lacks broken canopy hue texture: min={min_chroma} max={max_chroma}"
    );
    assert!(
        max_luma - min_luma > 5.0,
        "woodland lacks open ground and canopy contrast: min={min_luma} max={max_luma}"
    );
}

#[test]
fn desert_rendering_has_wind_streak_texture() {
    let scale = 18;
    let mut world = render_test_world(3, 3);
    for tile in &mut world.tiles {
        *tile = Tile {
            surface: Surface::Land,
            biome: Biome::Desert,
            raw_elevation: 0.58,
            temperature: 0.86,
            moisture: 0.08,
            ..Tile::default()
        };
    }

    let image = render_world(&world, RenderConfig { scale });
    let mut min_warmth = f32::MAX;
    let mut max_warmth = f32::MIN;
    for py in 0..scale {
        for px in 0..scale {
            let pixel = image.get_pixel(scale + px, scale + py).0;
            let warmth = pixel[0] as f32 + pixel[1] as f32 * 0.65 - pixel[2] as f32 * 0.45;
            min_warmth = min_warmth.min(warmth);
            max_warmth = max_warmth.max(warmth);
        }
    }

    assert!(
        max_warmth - min_warmth > 11.0,
        "desert lacks wind-streak dune texture: min={min_warmth} max={max_warmth}"
    );
}

#[test]
fn steppe_rendering_has_wind_combed_tussock_texture() {
    let scale = 18;
    let mut world = render_test_world(3, 3);
    for tile in &mut world.tiles {
        *tile = Tile {
            surface: Surface::Land,
            biome: Biome::Steppe,
            raw_elevation: 0.56,
            temperature: 0.48,
            moisture: 0.22,
            ..Tile::default()
        };
    }

    let image = render_world(&world, RenderConfig { scale });
    let mut min_warmth = f32::MAX;
    let mut max_warmth = f32::MIN;
    let mut min_luma = f32::MAX;
    let mut max_luma = f32::MIN;
    for py in 0..scale {
        for px in 0..scale {
            let pixel = image.get_pixel(scale + px, scale + py).0;
            let warmth = pixel[0] as f32 + pixel[1] as f32 * 0.45 - pixel[2] as f32 * 0.65;
            min_warmth = min_warmth.min(warmth);
            max_warmth = max_warmth.max(warmth);
            let luma = luma(pixel);
            min_luma = min_luma.min(luma);
            max_luma = max_luma.max(luma);
        }
    }

    assert!(
        max_warmth - min_warmth > 8.0,
        "steppe lacks warm dry-grass and bare-soil variation: min={min_warmth} max={max_warmth}"
    );
    assert!(
        max_luma - min_luma > 6.0,
        "steppe lacks wind-combed tussock contrast: min={min_luma} max={max_luma}"
    );
}

#[test]
fn savanna_rendering_has_scattered_scrub_texture() {
    let scale = 18;
    let mut world = render_test_world(3, 3);
    for tile in &mut world.tiles {
        *tile = Tile {
            surface: Surface::Land,
            biome: Biome::Savanna,
            raw_elevation: 0.56,
            temperature: 0.72,
            moisture: 0.34,
            ..Tile::default()
        };
    }

    let image = render_world(&world, RenderConfig { scale });
    let mut min_chroma = f32::MAX;
    let mut max_chroma = f32::MIN;
    let mut min_luma = f32::MAX;
    let mut max_luma = f32::MIN;
    for py in 0..scale {
        for px in 0..scale {
            let pixel = image.get_pixel(scale + px, scale + py).0;
            let chroma = green_minus_red(pixel);
            min_chroma = min_chroma.min(chroma);
            max_chroma = max_chroma.max(chroma);
            let luma = luma(pixel);
            min_luma = min_luma.min(luma);
            max_luma = max_luma.max(luma);
        }
    }

    assert!(
        max_chroma - min_chroma > 5.5,
        "savanna lacks green scrub variation: min={min_chroma} max={max_chroma}"
    );
    assert!(
        max_luma - min_luma > 8.0,
        "savanna lacks open grass and scrub contrast: min={min_luma} max={max_luma}"
    );
}

#[test]
fn tundra_rendering_has_frost_and_lichen_texture() {
    let scale = 16;
    let mut world = render_test_world(3, 3);
    for tile in &mut world.tiles {
        *tile = Tile {
            surface: Surface::Land,
            biome: Biome::Tundra,
            raw_elevation: 0.58,
            temperature: 0.10,
            moisture: 0.54,
            ..Tile::default()
        };
    }

    let image = render_world(&world, RenderConfig { scale });
    let mut min_cold_lichen = f32::MAX;
    let mut max_cold_lichen = f32::MIN;
    let mut min_luma = f32::MAX;
    let mut max_luma = f32::MIN;
    for py in 0..scale {
        for px in 0..scale {
            let pixel = image.get_pixel(scale + px, scale + py).0;
            let cold_lichen = pixel[1] as f32 * 0.65 + pixel[2] as f32 * 0.45 - pixel[0] as f32;
            min_cold_lichen = min_cold_lichen.min(cold_lichen);
            max_cold_lichen = max_cold_lichen.max(cold_lichen);
            let luma = luma(pixel);
            min_luma = min_luma.min(luma);
            max_luma = max_luma.max(luma);
        }
    }

    assert!(
        max_cold_lichen - min_cold_lichen > 8.0,
        "tundra lacks frost and lichen chroma variation: min={min_cold_lichen} max={max_cold_lichen}"
    );
    assert!(
        max_luma - min_luma > 7.0,
        "tundra lacks wind-scoured frost contrast: min={min_luma} max={max_luma}"
    );
}

#[test]
fn wetland_rendering_has_pooled_water_texture() {
    let scale = 16;
    let mut world = render_test_world(3, 3);
    for tile in &mut world.tiles {
        *tile = Tile {
            surface: Surface::Land,
            biome: Biome::Wetland,
            raw_elevation: 0.54,
            slope: 0.006,
            relief: 0.012,
            temperature: 0.55,
            moisture: 0.72,
            ..Tile::default()
        };
    }

    let image = render_world(&world, RenderConfig { scale });
    let mut min_blue_green = f32::MAX;
    let mut max_blue_green = f32::MIN;
    for py in 0..scale {
        for px in 0..scale {
            let pixel = image.get_pixel(scale + px, scale + py).0;
            let blue_green = pixel[2] as f32 + pixel[1] as f32 * 0.4 - pixel[0] as f32;
            min_blue_green = min_blue_green.min(blue_green);
            max_blue_green = max_blue_green.max(blue_green);
        }
    }

    assert!(
        max_blue_green - min_blue_green > 5.0,
        "wetland lacks pooled water texture: min={min_blue_green} max={max_blue_green}"
    );
}

#[test]
fn freshwater_rendering_uses_inland_water_texture() {
    let scale = 16;
    let mut world = render_test_world(3, 3);
    for tile in &mut world.tiles {
        *tile = Tile {
            surface: Surface::Land,
            biome: Biome::Freshwater,
            raw_elevation: 0.53,
            slope: 0.004,
            relief: 0.008,
            temperature: 0.55,
            moisture: 0.72,
            ..Tile::default()
        };
    }

    let image = render_world(&world, RenderConfig { scale });
    let mut colors = std::collections::HashSet::new();
    let mut min_blue_red = f32::MAX;
    for py in 0..scale {
        for px in 0..scale {
            let pixel = image.get_pixel(scale + px, scale + py).0;
            colors.insert((pixel[0], pixel[1], pixel[2]));
            min_blue_red = min_blue_red.min(pixel[2] as f32 - pixel[0] as f32);
        }
    }

    assert!(
        colors.len() > 1,
        "freshwater texture is flat-filled: {} colors",
        colors.len()
    );
    assert!(
        min_blue_red > 45.0,
        "freshwater is not rendering as blue-green water: min={min_blue_red}"
    );
}

#[test]
fn isolated_freshwater_renders_with_soft_shoreline() {
    let scale = 20;
    let mut world = render_test_world(3, 3);
    let center = world.idx(1, 1);
    world.tiles[center] = Tile {
        surface: Surface::Land,
        biome: Biome::Freshwater,
        raw_elevation: 0.53,
        slope: 0.004,
        relief: 0.008,
        temperature: 0.55,
        moisture: 0.72,
        ..Tile::default()
    };

    let image = render_world(&world, RenderConfig { scale });
    let edge = image.get_pixel(scale + 1, scale + scale / 2).0;
    let center = image.get_pixel(scale + scale / 2, scale + scale / 2).0;
    let edge_water = edge[2] as f32 - edge[0] as f32;
    let center_water = center[2] as f32 - center[0] as f32;

    assert!(
        center_water > edge_water + 35.0,
        "isolated freshwater tile has a blocky shore: edge={edge_water} center={center_water}"
    );
}

#[test]
fn diagonal_freshwater_land_contact_feathers_corner() {
    let scale = 20;
    let mut world = render_test_world(3, 3);
    for tile in &mut world.tiles {
        *tile = Tile {
            surface: Surface::Land,
            biome: Biome::Freshwater,
            raw_elevation: 0.53,
            slope: 0.004,
            relief: 0.008,
            temperature: 0.55,
            moisture: 0.72,
            ..Tile::default()
        };
    }
    let diagonal_land = world.idx(0, 0);
    world.tiles[diagonal_land] = Tile {
        surface: Surface::Land,
        biome: Biome::TemperateGrassland,
        raw_elevation: 0.56,
        temperature: 0.55,
        moisture: 0.35,
        ..Tile::default()
    };

    let image = render_world(&world, RenderConfig { scale });
    let corner = image.get_pixel(scale, scale).0;
    let center = image.get_pixel(scale + scale / 2, scale + scale / 2).0;
    let corner_water = corner[2] as f32 - corner[0] as f32;
    let center_water = center[2] as f32 - center[0] as f32;

    assert!(
        center_water > corner_water + 28.0,
        "diagonal land contact did not feather the freshwater corner: corner={corner_water} center={center_water}"
    );
}

#[test]
fn freshwater_color_does_not_bleed_into_neighbor_land() {
    let scale = 18;
    let base_world = render_test_world(3, 3);
    let mut pond_world = base_world.clone();
    let pond = pond_world.idx(1, 1);
    pond_world.tiles[pond] = Tile {
        surface: Surface::Land,
        biome: Biome::Freshwater,
        raw_elevation: 0.53,
        slope: 0.004,
        relief: 0.008,
        temperature: 0.55,
        moisture: 0.72,
        ..Tile::default()
    };

    let base = render_world(&base_world, RenderConfig { scale });
    let pond_image = render_world(&pond_world, RenderConfig { scale });
    let land_edge = (scale - 1, scale + scale / 2);
    let pond_center = (scale + scale / 2, scale + scale / 2);
    let land_delta = color_delta(
        base.get_pixel(land_edge.0, land_edge.1).0,
        pond_image.get_pixel(land_edge.0, land_edge.1).0,
    );
    let pond_delta = color_delta(
        base.get_pixel(pond_center.0, pond_center.1).0,
        pond_image.get_pixel(pond_center.0, pond_center.1).0,
    );

    assert!(
        land_delta < 6.0,
        "freshwater color bled into adjacent land: delta={land_delta}"
    );
    assert!(
        pond_delta > 45.0,
        "freshwater center should remain visually distinct: delta={pond_delta}"
    );
}

#[test]
fn freshwater_elevation_does_not_cast_land_hillshade_shadow() {
    let scale = 18;
    let base_world = render_test_world(3, 3);
    let mut pond_world = base_world.clone();
    let pond = pond_world.idx(1, 1);
    pond_world.tiles[pond] = Tile {
        surface: Surface::Land,
        biome: Biome::Freshwater,
        raw_elevation: 0.18,
        slope: 0.004,
        relief: 0.008,
        temperature: 0.55,
        moisture: 0.72,
        ..Tile::default()
    };

    let base = render_world(&base_world, RenderConfig { scale });
    let pond_image = render_world(&pond_world, RenderConfig { scale });
    let adjacent_land = (scale - 1, scale + scale / 2);
    let land_delta = color_delta(
        base.get_pixel(adjacent_land.0, adjacent_land.1).0,
        pond_image.get_pixel(adjacent_land.0, adjacent_land.1).0,
    );

    assert!(
        land_delta < 6.0,
        "freshwater elevation cast an artificial land shadow: delta={land_delta}"
    );
}

#[test]
fn mountain_summit_rendering_adds_crest_highlight() {
    let scale = 12;
    let mut slope_world = alpine_render_test_world();
    let center = slope_world.idx(1, 1);
    slope_world.tiles[center].mountain_feature = MountainFeature::AlpineSlope;

    let mut summit_world = slope_world.clone();
    summit_world.tiles[center].mountain_feature = MountainFeature::Summit;

    let slope = render_world(&slope_world, RenderConfig { scale });
    let summit = render_world(&summit_world, RenderConfig { scale });
    let px = scale + scale / 2;
    let py = scale + scale / 2;
    let slope_luma = luma(slope.get_pixel(px, py).0);
    let summit_luma = luma(summit.get_pixel(px, py).0);

    assert!(
        summit_luma > slope_luma + 7.0,
        "summit feature did not add enough crest highlight: summit={summit_luma} slope={slope_luma}"
    );
}

#[test]
fn alpine_ridge_rendering_adds_craggy_band_contrast() {
    let scale = 18;
    let mut slope_world = render_test_world(3, 3);
    for y in 0..slope_world.height {
        for x in 0..slope_world.width {
            let idx = slope_world.idx(x, y);
            slope_world.tiles[idx] = Tile {
                surface: Surface::Land,
                biome: Biome::Alpine,
                raw_elevation: 0.88 - ((x.abs_diff(1) + y.abs_diff(1)) as f32 * 0.012),
                slope: 0.080,
                relief: 0.120,
                temperature: 0.34,
                moisture: 0.22,
                mountain_feature: MountainFeature::AlpineSlope,
                ..Tile::default()
            };
        }
    }

    let mut ridge_world = slope_world.clone();
    let center = ridge_world.idx(1, 1);
    ridge_world.tiles[center].mountain_feature = MountainFeature::Ridge;

    let slope = render_world(&slope_world, RenderConfig { scale });
    let ridge = render_world(&ridge_world, RenderConfig { scale });
    let mut slope_min = f32::MAX;
    let mut slope_max = f32::MIN;
    let mut ridge_min = f32::MAX;
    let mut ridge_max = f32::MIN;

    for py in 0..scale {
        for px in 0..scale {
            let x = scale + px;
            let y = scale + py;
            let slope_luma = luma(slope.get_pixel(x, y).0);
            let ridge_luma = luma(ridge.get_pixel(x, y).0);
            slope_min = slope_min.min(slope_luma);
            slope_max = slope_max.max(slope_luma);
            ridge_min = ridge_min.min(ridge_luma);
            ridge_max = ridge_max.max(ridge_luma);
        }
    }

    let slope_range = slope_max - slope_min;
    let ridge_range = ridge_max - ridge_min;
    assert!(
        ridge_range > slope_range + 7.0,
        "ridge feature did not add enough crag contrast: ridge={ridge_range} slope={slope_range}"
    );
    assert!(
        ridge_max > slope_max + 8.0,
        "ridge crest bands did not brighten alpine rock enough: ridge_max={ridge_max} slope_max={slope_max}"
    );
}

#[test]
fn alpine_ridge_rendering_adds_directional_spine_highlight() {
    let scale = 20;
    let mut slope_world = render_test_world(3, 3);
    for y in 0..slope_world.height {
        for x in 0..slope_world.width {
            let idx = slope_world.idx(x, y);
            let eastward_drop = (x as f32 - 1.0) * 0.060;
            let cross_slope = y.abs_diff(1) as f32 * 0.006;
            slope_world.tiles[idx] = Tile {
                surface: Surface::Land,
                biome: Biome::Alpine,
                raw_elevation: 0.90 - eastward_drop - cross_slope,
                slope: 0.085,
                relief: 0.130,
                temperature: 0.42,
                moisture: 0.22,
                mountain_feature: MountainFeature::AlpineSlope,
                ..Tile::default()
            };
        }
    }

    let mut ridge_world = slope_world.clone();
    let center = ridge_world.idx(1, 1);
    ridge_world.tiles[center].mountain_feature = MountainFeature::Ridge;

    let slope = render_world(&slope_world, RenderConfig { scale });
    let ridge = render_world(&ridge_world, RenderConfig { scale });
    let mut slope_spine = 0.0_f32;
    let mut slope_shoulder = 0.0_f32;
    let mut ridge_spine = 0.0_f32;
    let mut ridge_shoulder = 0.0_f32;
    let mut samples = 0.0_f32;

    for py in scale / 4..scale * 3 / 4 {
        for px in scale / 2 - 1..=scale / 2 + 1 {
            let x = scale + px;
            let y = scale + py;
            slope_spine += luma(slope.get_pixel(x, y).0);
            ridge_spine += luma(ridge.get_pixel(x, y).0);
            samples += 1.0;
        }
        for px in scale * 4 / 5 - 1..=scale * 4 / 5 + 1 {
            let x = scale + px;
            let y = scale + py;
            slope_shoulder += luma(slope.get_pixel(x, y).0);
            ridge_shoulder += luma(ridge.get_pixel(x, y).0);
        }
    }

    slope_spine /= samples;
    slope_shoulder /= samples;
    ridge_spine /= samples;
    ridge_shoulder /= samples;
    let slope_contrast = slope_spine - slope_shoulder;
    let ridge_contrast = ridge_spine - ridge_shoulder;
    assert!(
        ridge_contrast > slope_contrast + 6.0,
        "ridge spine did not separate from the shoulder: ridge={ridge_contrast} slope={slope_contrast}"
    );
    assert!(
        ridge_spine > slope_spine + 8.0,
        "ridge spine did not brighten over the same alpine slope pixels: ridge={ridge_spine} slope={slope_spine}"
    );
}

#[test]
fn foothill_rendering_adds_stony_talus_below_alpine() {
    let scale = 18;
    let mut base_world = render_test_world(3, 3);
    for tile in &mut base_world.tiles {
        *tile = Tile {
            surface: Surface::Land,
            biome: Biome::Foothills,
            raw_elevation: 0.70,
            slope: 0.050,
            relief: 0.075,
            temperature: 0.42,
            moisture: 0.34,
            mountain_feature: MountainFeature::Foothill,
            ..Tile::default()
        };
    }

    let mut talus_world = base_world.clone();
    let alpine = talus_world.idx(0, 1);
    talus_world.tiles[alpine] = Tile {
        surface: Surface::Land,
        biome: Biome::Alpine,
        raw_elevation: 0.92,
        slope: 0.110,
        relief: 0.150,
        temperature: 0.32,
        moisture: 0.22,
        mountain_feature: MountainFeature::Ridge,
        ..Tile::default()
    };

    let base = render_world(&base_world, RenderConfig { scale });
    let talus = render_world(&talus_world, RenderConfig { scale });
    let mut mean_delta = 0.0_f32;
    let mut base_saturation = 0.0_f32;
    let mut talus_saturation = 0.0_f32;
    let mut samples = 0.0_f32;

    for py in scale / 3..scale * 2 / 3 {
        for px in scale / 2..scale - 2 {
            let x = scale + px;
            let y = scale + py;
            let base_pixel = base.get_pixel(x, y).0;
            let talus_pixel = talus.get_pixel(x, y).0;
            mean_delta += color_delta(base_pixel, talus_pixel);
            base_saturation += color_saturation(base_pixel);
            talus_saturation += color_saturation(talus_pixel);
            samples += 1.0;
        }
    }

    mean_delta /= samples;
    base_saturation /= samples;
    talus_saturation /= samples;
    assert!(
        mean_delta > 8.0,
        "alpine-adjacent foothills did not gain a visible talus apron: delta={mean_delta}"
    );
    assert!(
        talus_saturation < base_saturation - 3.0,
        "talus apron did not mute foothill color toward stone: talus_sat={talus_saturation} base_sat={base_saturation}"
    );
}

#[test]
fn foothill_rendering_blends_grassy_slopes_and_exposed_stone() {
    let scale = 16;
    let mut world = render_test_world(3, 3);
    for tile in &mut world.tiles {
        *tile = Tile {
            surface: Surface::Land,
            biome: Biome::Foothills,
            raw_elevation: 0.68,
            slope: 0.035,
            relief: 0.050,
            temperature: 0.48,
            moisture: 0.38,
            mountain_feature: MountainFeature::None,
            ..Tile::default()
        };
    }

    let image = render_world(&world, RenderConfig { scale });
    let mut min_grassy = f32::MAX;
    let mut max_grassy = f32::MIN;
    let mut min_stony = f32::MAX;
    let mut max_stony = f32::MIN;
    for py in 0..scale {
        for px in 0..scale {
            let pixel = image.get_pixel(scale + px, scale + py).0;
            let grassy = pixel[1] as f32 - pixel[0] as f32 * 0.55 - pixel[2] as f32 * 0.20;
            let stony = pixel[2] as f32 + pixel[0] as f32 * 0.25 - pixel[1] as f32 * 0.55;
            min_grassy = min_grassy.min(grassy);
            max_grassy = max_grassy.max(grassy);
            min_stony = min_stony.min(stony);
            max_stony = max_stony.max(stony);
        }
    }

    assert!(
        max_grassy - min_grassy > 5.0,
        "foothills lack grassy slope variation: min={min_grassy} max={max_grassy}"
    );
    assert!(
        max_stony - min_stony > 4.5,
        "foothills lack exposed stone variation: min={min_stony} max={max_stony}"
    );
}

#[test]
fn cold_high_alpine_rendering_applies_snow_overlay() {
    let scale = 12;
    let cold_world = alpine_render_test_world();
    let mut warm_world = cold_world.clone();
    for tile in &mut warm_world.tiles {
        tile.temperature = 0.62;
    }

    let cold = render_world(&cold_world, RenderConfig { scale });
    let warm = render_world(&warm_world, RenderConfig { scale });
    let px = scale + scale / 2;
    let py = scale + scale / 2;
    let cold_pixel = cold.get_pixel(px, py).0;
    let warm_pixel = warm.get_pixel(px, py).0;

    assert!(
        luma(cold_pixel) > luma(warm_pixel) + 10.0 && cold_pixel[2] > warm_pixel[2] + 8,
        "cold alpine did not receive visible snow overlay: cold={cold_pixel:?} warm={warm_pixel:?}"
    );
}

#[test]
fn cold_alpine_summit_rendering_adds_broken_snowfields() {
    let scale = 18;
    let mut cold_world = alpine_render_test_world();
    for tile in &mut cold_world.tiles {
        tile.raw_elevation = 0.96;
        tile.temperature = 0.02;
        tile.slope = 0.090;
        tile.relief = 0.140;
        tile.mountain_feature = MountainFeature::Summit;
    }

    let mut warm_world = cold_world.clone();
    for tile in &mut warm_world.tiles {
        tile.temperature = 0.42;
    }

    let cold = render_world(&cold_world, RenderConfig { scale });
    let warm = render_world(&warm_world, RenderConfig { scale });
    let mut cold_min = f32::MAX;
    let mut cold_max = f32::MIN;
    let mut warm_min = f32::MAX;
    let mut warm_max = f32::MIN;

    for py in 0..scale {
        for px in 0..scale {
            let x = scale + px;
            let y = scale + py;
            let cold_luma = luma(cold.get_pixel(x, y).0);
            let warm_luma = luma(warm.get_pixel(x, y).0);
            cold_min = cold_min.min(cold_luma);
            cold_max = cold_max.max(cold_luma);
            warm_min = warm_min.min(warm_luma);
            warm_max = warm_max.max(warm_luma);
        }
    }

    let cold_range = cold_max - cold_min;
    let warm_range = warm_max - warm_min;
    assert!(
        cold_max > warm_max + 18.0,
        "cold summit did not gain bright broken snow caps: cold_max={cold_max} warm_max={warm_max}"
    );
    assert!(
        cold_range > warm_range + 6.0,
        "summit snowfields are too uniform: cold_range={cold_range} warm_range={warm_range}"
    );
}

#[test]
fn major_river_rendering_feathers_wet_banks_around_channel() {
    let scale = 10;
    let mut river_world = render_test_world(3, 3);
    let base_world = river_world.clone();

    for x in 0..2 {
        let idx = river_world.idx(x, 1);
        river_world.tiles[idx].river = 1.0;
        river_world.tiles[idx].flow_direction = 2;
    }
    let mouth = river_world.idx(2, 1);
    river_world.tiles[mouth].river = 1.0;

    let base = render_world(&base_world, RenderConfig { scale });
    let river = render_world(&river_world, RenderConfig { scale });
    let core = (scale + scale / 2, scale + scale / 2);
    let shoulder = (core.0, core.1 + 4);
    let core_delta = color_delta(
        base.get_pixel(core.0, core.1).0,
        river.get_pixel(core.0, core.1).0,
    );
    let shoulder_delta = color_delta(
        base.get_pixel(shoulder.0, shoulder.1).0,
        river.get_pixel(shoulder.0, shoulder.1).0,
    );

    assert!(
        shoulder_delta > 5.0,
        "river bank feather did not reach outside the water core: delta={shoulder_delta}"
    );
    assert!(
        core_delta > shoulder_delta + 8.0,
        "river water core should remain stronger than the bank: core={core_delta} shoulder={shoulder_delta}"
    );
}

#[test]
fn minor_stream_rendering_adds_thin_tributary_channels() {
    let scale = 18;
    let mut minor_world = render_test_world(4, 3);
    let base_world = minor_world.clone();

    for x in 0..3 {
        let idx = minor_world.idx(x, 1);
        minor_world.tiles[idx].river = 0.42;
        minor_world.tiles[idx].flow_direction = 2;
    }

    let mut major_world = base_world.clone();
    for x in 0..3 {
        let idx = major_world.idx(x, 1);
        major_world.tiles[idx].river = 0.82;
        major_world.tiles[idx].flow_direction = 2;
    }

    let base = render_world(&base_world, RenderConfig { scale });
    let minor = render_world(&minor_world, RenderConfig { scale });
    let major = render_world(&major_world, RenderConfig { scale });
    let core = (scale + scale / 2, scale + scale / 2);
    let minor_delta = color_delta(
        base.get_pixel(core.0, core.1).0,
        minor.get_pixel(core.0, core.1).0,
    );
    let major_delta = color_delta(
        base.get_pixel(core.0, core.1).0,
        major.get_pixel(core.0, core.1).0,
    );

    assert!(
        minor_delta > 4.0,
        "minor stream was not visible enough: delta={minor_delta}"
    );
    assert!(
        major_delta > minor_delta + 8.0,
        "minor stream should remain subtler than major river: minor={minor_delta} major={major_delta}"
    );
}

#[test]
fn minor_stream_rendering_reaches_coastal_ocean() {
    let scale = 16;
    let mut base_world = render_test_world(4, 3);
    let coast = base_world.idx(1, 1);
    base_world.tiles[coast] = Tile {
        surface: Surface::Coast,
        biome: Biome::Coast,
        raw_elevation: 0.525,
        ..Tile::default()
    };
    let ocean = base_world.idx(2, 1);
    base_world.tiles[ocean] = warm_ocean_tile(0.42);

    let mut minor_world = base_world.clone();
    minor_world.tiles[coast].river = 0.42;
    minor_world.tiles[coast].flow_direction = 2;

    let mut major_world = base_world.clone();
    major_world.tiles[coast].river = 0.86;
    major_world.tiles[coast].flow_direction = 2;

    let base = render_world(&base_world, RenderConfig { scale });
    let minor = render_world(&minor_world, RenderConfig { scale });
    let major = render_world(&major_world, RenderConfig { scale });
    let mouth = (2 * scale + scale / 2, scale + scale / 2);
    let minor_delta = color_delta(
        base.get_pixel(mouth.0, mouth.1).0,
        minor.get_pixel(mouth.0, mouth.1).0,
    );
    let major_delta = color_delta(
        base.get_pixel(mouth.0, mouth.1).0,
        major.get_pixel(mouth.0, mouth.1).0,
    );

    assert!(
        minor_delta > 3.0,
        "minor coastal stream did not reach adjacent ocean: delta={minor_delta}"
    );
    assert!(
        major_delta > minor_delta + 5.0,
        "minor coastal stream should stay subtler than a major estuary: minor={minor_delta} major={major_delta}"
    );
}

#[test]
fn lowland_river_rendering_adds_alluvial_floodplain() {
    let scale = 14;
    let mut river_world = render_test_world(4, 3);
    for tile in &mut river_world.tiles {
        tile.raw_elevation = 0.57;
        tile.slope = 0.01;
        tile.relief = 0.015;
        tile.moisture = 0.42;
        tile.temperature = 0.62;
    }
    let base_world = river_world.clone();

    for x in 0..3 {
        let idx = river_world.idx(x, 1);
        river_world.tiles[idx].river = 0.82;
        river_world.tiles[idx].flow_direction = 2;
    }

    let base = render_world(&base_world, RenderConfig { scale });
    let river = render_world(&river_world, RenderConfig { scale });
    let floodplain = (scale + scale / 2, scale + scale / 2 + 6);
    let far = (scale + scale / 2, scale * 2 + scale / 2);
    let floodplain_delta = color_delta(
        base.get_pixel(floodplain.0, floodplain.1).0,
        river.get_pixel(floodplain.0, floodplain.1).0,
    );
    let far_delta = color_delta(
        base.get_pixel(far.0, far.1).0,
        river.get_pixel(far.0, far.1).0,
    );

    assert!(
        floodplain_delta > 4.0,
        "lowland river did not add a visible alluvial floodplain: delta={floodplain_delta}"
    );
    assert!(
        floodplain_delta > far_delta + 2.0,
        "alluvial tint should concentrate near the channel: floodplain={floodplain_delta} far={far_delta}"
    );
}

#[test]
fn river_bend_rendering_favors_smoothed_inside_corner() {
    let scale = 20;
    let mut river_world = render_test_world(3, 4);
    let base_world = river_world.clone();

    let west = river_world.idx(0, 1);
    let bend = river_world.idx(1, 1);
    let south = river_world.idx(1, 2);
    river_world.tiles[west].river = 0.66;
    river_world.tiles[west].flow_direction = 2;
    river_world.tiles[bend].river = 0.72;
    river_world.tiles[bend].flow_direction = 4;
    river_world.tiles[south].river = 0.74;

    let base = render_world(&base_world, RenderConfig { scale });
    let river = render_world(&river_world, RenderConfig { scale });
    let inside = (scale + scale / 4, scale + scale * 3 / 4);
    let outside = (scale + scale * 3 / 4, scale + scale / 4);
    let inside_delta = color_delta(
        base.get_pixel(inside.0, inside.1).0,
        river.get_pixel(inside.0, inside.1).0,
    );
    let outside_delta = color_delta(
        base.get_pixel(outside.0, outside.1).0,
        river.get_pixel(outside.0, outside.1).0,
    );

    assert!(
        inside_delta > outside_delta + 8.0,
        "river bend did not round toward the inside corner: inside={inside_delta} outside={outside_delta}"
    );
}

#[test]
fn river_confluence_rendering_widens_join_pool() {
    let scale = 28;
    let mut base_world = render_test_world(3, 3);
    for tile in &mut base_world.tiles {
        tile.raw_elevation = 0.64;
        tile.slope = 0.07;
        tile.relief = 0.09;
        tile.moisture = 0.46;
        tile.temperature = 0.60;
    }

    let mut simple_world = base_world.clone();
    let west = simple_world.idx(0, 1);
    let center = simple_world.idx(1, 1);
    let east = simple_world.idx(2, 1);
    simple_world.tiles[west].river = 0.46;
    simple_world.tiles[west].flow_direction = 2;
    simple_world.tiles[center].river = 0.50;
    simple_world.tiles[center].flow_direction = 2;
    simple_world.tiles[east].river = 0.50;

    let mut confluence_world = simple_world.clone();
    let north = confluence_world.idx(1, 0);
    confluence_world.tiles[north].river = 0.46;
    confluence_world.tiles[north].flow_direction = 4;

    let base = render_world(&base_world, RenderConfig { scale });
    let simple = render_world(&simple_world, RenderConfig { scale });
    let confluence = render_world(&confluence_world, RenderConfig { scale });
    let shoulder = (scale + scale / 2 - 6, scale + scale / 2 - 6);
    let simple_delta = color_delta(
        base.get_pixel(shoulder.0, shoulder.1).0,
        simple.get_pixel(shoulder.0, shoulder.1).0,
    );
    let confluence_delta = color_delta(
        base.get_pixel(shoulder.0, shoulder.1).0,
        confluence.get_pixel(shoulder.0, shoulder.1).0,
    );

    assert!(
        confluence_delta > simple_delta + 4.0,
        "confluence did not widen the join shoulder: confluence={confluence_delta} simple={simple_delta}"
    );
}

#[test]
fn river_mouth_rendering_widens_into_estuary() {
    let scale = 16;
    let mut base_world = render_test_world(4, 3);
    let coast = base_world.idx(1, 1);
    base_world.tiles[coast] = Tile {
        surface: Surface::Coast,
        biome: Biome::Coast,
        raw_elevation: 0.525,
        ..Tile::default()
    };
    let ocean = base_world.idx(2, 1);
    base_world.tiles[ocean] = warm_ocean_tile(0.42);

    let mut river_world = base_world.clone();
    river_world.tiles[coast].river = 1.0;
    river_world.tiles[coast].flow_direction = 2;

    let base = render_world(&base_world, RenderConfig { scale });
    let river = render_world(&river_world, RenderConfig { scale });
    let mouth_center = (2 * scale + scale / 2, scale + scale / 2);
    let mouth_shoulder = (mouth_center.0, mouth_center.1 + 7);
    let center_delta = color_delta(
        base.get_pixel(mouth_center.0, mouth_center.1).0,
        river.get_pixel(mouth_center.0, mouth_center.1).0,
    );
    let shoulder_delta = color_delta(
        base.get_pixel(mouth_shoulder.0, mouth_shoulder.1).0,
        river.get_pixel(mouth_shoulder.0, mouth_shoulder.1).0,
    );

    assert!(
        shoulder_delta > 8.0,
        "river mouth did not widen into the adjacent ocean: shoulder={shoulder_delta}"
    );
    assert!(
        center_delta + 2.0 >= shoulder_delta,
        "estuary shoulder should not overpower the mouth center: center={center_delta} shoulder={shoulder_delta}"
    );
}

#[test]
fn tiny_coastal_islets_do_not_draw_checkerboard_coastline() {
    let scale = 6;
    let mut world = render_test_world(5, 5);
    for tile in &mut world.tiles {
        *tile = warm_ocean_tile(0.30);
    }
    let islet = world.idx(2, 2);
    world.tiles[islet] = Tile {
        surface: Surface::Coast,
        biome: Biome::Coast,
        raw_elevation: 0.53,
        ..Tile::default()
    };

    let image = render_world(&world, RenderConfig { scale });
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

#[test]
fn diagonal_coast_contact_draws_corner_waterline() {
    let scale = 8;
    let mut base_world = render_test_world(3, 3);
    let coast = base_world.idx(1, 1);
    base_world.tiles[coast] = Tile {
        surface: Surface::Coast,
        biome: Biome::Coast,
        raw_elevation: 0.53,
        ..Tile::default()
    };
    let mut diagonal_world = base_world.clone();
    let ocean = diagonal_world.idx(0, 0);
    diagonal_world.tiles[ocean] = warm_ocean_tile(0.30);

    let base = render_world(&base_world, RenderConfig { scale });
    let diagonal = render_world(&diagonal_world, RenderConfig { scale });
    let coast_corner = (scale, scale);
    let base_pixel = base.get_pixel(coast_corner.0, coast_corner.1).0;
    let diagonal_pixel = diagonal.get_pixel(coast_corner.0, coast_corner.1).0;
    let delta = color_delta(base_pixel, diagonal_pixel);
    let blue_shift = diagonal_pixel[2] as f32 - base_pixel[2] as f32;
    let green_shift = diagonal_pixel[1] as f32 - base_pixel[1] as f32;
    let water_chroma = diagonal_pixel[2] as f32 - diagonal_pixel[0] as f32;

    assert!(
        delta > 24.0,
        "diagonal ocean contact did not cut a visible waterline into the coast corner: delta={delta}"
    );
    assert!(
        blue_shift > 32.0 && green_shift > 12.0 && water_chroma > 20.0,
        "diagonal coast corner did not shift toward shallow water: base={base_pixel:?} diagonal={diagonal_pixel:?}"
    );
}

#[test]
fn cardinal_coastline_has_subtile_waterline_variation() {
    let scale = 12;
    let mut base_world = render_test_world(3, 4);
    for tile in &mut base_world.tiles {
        tile.raw_elevation = 0.56;
    }
    let coast = base_world.idx(1, 1);
    base_world.tiles[coast] = Tile {
        surface: Surface::Coast,
        biome: Biome::Coast,
        raw_elevation: 0.525,
        ..Tile::default()
    };

    let mut coast_world = base_world.clone();
    let ocean = coast_world.idx(0, 1);
    coast_world.tiles[ocean] = warm_ocean_tile(0.44);

    let base = render_world(&base_world, RenderConfig { scale });
    let coast_image = render_world(&coast_world, RenderConfig { scale });
    let sample_x = scale + 1;
    let mut min_delta = f32::MAX;
    let mut max_delta = 0.0_f32;
    for py in 0..scale {
        let y = scale + py;
        let delta = color_delta(
            base.get_pixel(sample_x, y).0,
            coast_image.get_pixel(sample_x, y).0,
        );
        min_delta = min_delta.min(delta);
        max_delta = max_delta.max(delta);
    }

    assert!(
        max_delta > 30.0,
        "coastline water intrusion is too weak: max_delta={max_delta}"
    );
    assert!(
        max_delta - min_delta > 14.0,
        "coastline still behaves like a uniform tile edge: min={min_delta} max={max_delta}"
    );
}

#[test]
fn rugged_coasts_render_rockier_than_flat_beaches() {
    let scale = 20;
    let mut flat_world = render_test_world(3, 3);
    let coast = flat_world.idx(1, 1);
    flat_world.tiles[coast] = Tile {
        surface: Surface::Coast,
        biome: Biome::Coast,
        raw_elevation: 0.525,
        slope: 0.004,
        relief: 0.010,
        ..Tile::default()
    };
    let ocean = flat_world.idx(0, 1);
    flat_world.tiles[ocean] = warm_ocean_tile(0.32);

    let mut rocky_world = flat_world.clone();
    rocky_world.tiles[coast].slope = 0.11;
    rocky_world.tiles[coast].relief = 0.13;

    let flat = render_world(&flat_world, RenderConfig { scale });
    let rocky = render_world(&rocky_world, RenderConfig { scale });
    let sample = (scale + 1, scale + scale / 2);
    let flat_pixel = flat.get_pixel(sample.0, sample.1).0;
    let rocky_pixel = rocky.get_pixel(sample.0, sample.1).0;
    let flat_sand = flat_pixel[0] as f32 + flat_pixel[1] as f32;
    let rocky_sand = rocky_pixel[0] as f32 + rocky_pixel[1] as f32;

    assert!(
        flat_sand > rocky_sand + 20.0 && luma(flat_pixel) > luma(rocky_pixel) + 8.0,
        "rugged coast stayed too sandy: flat={flat_pixel:?} rocky={rocky_pixel:?}"
    );
}

#[test]
fn vegetated_coast_keeps_climate_color_away_from_waterline() {
    let scale = 16;
    let mut world = World::new(7, 4, 3, 0.50, 0);
    for tile in &mut world.tiles {
        *tile = Tile {
            surface: Surface::Coast,
            biome: Biome::Coast,
            raw_elevation: 0.56,
            temperature: 0.44,
            moisture: 0.68,
            ..Tile::default()
        };
    }
    for y in 0..world.height {
        let idx = world.idx(0, y);
        world.tiles[idx] = warm_ocean_tile(0.34);
    }

    let image = render_world(&world, RenderConfig { scale });
    let waterline = image.get_pixel(scale + 1, scale + scale / 2).0;
    let inland_edge = image.get_pixel(2 * scale - 2, scale + scale / 2).0;

    assert!(
        green_minus_red(inland_edge) > 24.0,
        "vegetated coast interior stayed too sandy: inland={inland_edge:?}"
    );
    assert!(
        color_delta(waterline, inland_edge) > 12.0,
        "coastal waterline no longer separates beach/surf from vegetated hinterland: waterline={waterline:?} inland={inland_edge:?}"
    );
}

#[test]
fn forested_coast_interior_uses_hinterland_texture() {
    let scale = 12;
    let mut world = render_test_world(3, 3);
    for tile in &mut world.tiles {
        *tile = Tile {
            surface: Surface::Coast,
            biome: Biome::Coast,
            raw_elevation: 0.56,
            temperature: 0.44,
            moisture: 0.68,
            ..Tile::default()
        };
    }

    let image = render_world(&world, RenderConfig { scale });
    let mut min_chroma = f32::MAX;
    let mut max_chroma = f32::MIN;
    for py in 0..scale {
        for px in 0..scale {
            let pixel = image.get_pixel(scale + px, scale + py).0;
            let chroma = green_minus_red(pixel);
            min_chroma = min_chroma.min(chroma);
            max_chroma = max_chroma.max(chroma);
        }
    }

    assert!(
        max_chroma - min_chroma > 4.0,
        "forested coast interior lacks canopy texture: min={min_chroma} max={max_chroma}"
    );
}

#[test]
fn coastline_blends_surf_onto_adjacent_ocean_pixels() {
    let scale = 8;
    let mut world = render_test_world(5, 3);
    for tile in &mut world.tiles {
        *tile = warm_ocean_tile(0.30);
    }

    let coast = world.idx(2, 1);
    world.tiles[coast] = Tile {
        surface: Surface::Coast,
        biome: Biome::Coast,
        raw_elevation: 0.53,
        ..Tile::default()
    };
    let land = world.idx(3, 1);
    world.tiles[land] = Tile {
        surface: Surface::Land,
        biome: Biome::TemperateGrassland,
        raw_elevation: 0.56,
        moisture: 0.35,
        temperature: 0.55,
        ..Tile::default()
    };

    let image = render_world(&world, RenderConfig { scale });
    let far_ocean = image.get_pixel(scale / 2, scale + scale / 2).0;
    let surf = image.get_pixel(2 * scale - 1, scale + scale / 2).0;
    assert!(
        luma(surf) > luma(far_ocean) + 8.0,
        "coastal surf did not lighten adjacent ocean pixels enough: surf={surf:?} far={far_ocean:?}"
    );
    assert_ne!(
        image.get_pixel(2 * scale, scale + scale / 2).0,
        [218, 210, 158, 255],
        "coastal land edge fell back to the old hard coastline color"
    );
}

fn luma(pixel: [u8; 4]) -> f32 {
    pixel[0] as f32 * 0.2126 + pixel[1] as f32 * 0.7152 + pixel[2] as f32 * 0.0722
}

fn color_delta(a: [u8; 4], b: [u8; 4]) -> f32 {
    (a[0] as f32 - b[0] as f32).abs()
        + (a[1] as f32 - b[1] as f32).abs()
        + (a[2] as f32 - b[2] as f32).abs()
}

fn color_saturation(pixel: [u8; 4]) -> f32 {
    let max = pixel[0].max(pixel[1]).max(pixel[2]) as f32;
    let min = pixel[0].min(pixel[1]).min(pixel[2]) as f32;
    max - min
}

fn green_minus_red(pixel: [u8; 4]) -> f32 {
    pixel[1] as f32 - pixel[0] as f32
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
            surface: Surface::Land,
            biome: Biome::TemperateGrassland,
            raw_elevation: 0.56,
            temperature: 0.55,
            moisture: 0.35,
            ..Tile::default()
        };
    }
    world
}

fn warm_ocean_tile(raw_elevation: f32) -> Tile {
    Tile {
        surface: Surface::Ocean,
        biome: Biome::Ocean,
        raw_elevation,
        temperature: 0.52,
        moisture: 1.0,
        ..Tile::default()
    }
}

fn alpine_render_test_world() -> World {
    let mut world = World::new(7, 3, 3, 0.50, 0);
    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            world.tiles[idx] = Tile {
                surface: Surface::Land,
                biome: Biome::Alpine,
                raw_elevation: 0.91 - ((x.abs_diff(1) + y.abs_diff(1)) as f32 * 0.015),
                slope: 0.055,
                relief: 0.075,
                temperature: 0.04,
                mountain_feature: MountainFeature::AlpineSlope,
                ..Tile::default()
            };
        }
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
            let land = world.tiles[world.idx(x, y)].surface != Surface::Ocean;
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
            let is_land = world.tiles[world.idx(x, y)].surface != Surface::Ocean;
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

fn major_landmass_count(world: &World, min_area: usize) -> usize {
    component_count(world, min_area, |tile| tile.surface != Surface::Ocean)
}

fn mountain_component_count(world: &World, min_area: usize) -> usize {
    component_count(world, min_area, |tile| {
        matches!(tile.biome, Biome::Alpine | Biome::Foothills)
    })
}

fn longest_major_river_straight_run(world: &World, edge_margin: usize) -> usize {
    let mut longest = 0_usize;
    let directions = [
        (0_isize, -1_isize),
        (1, -1),
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
    ];

    for idx in 0..world.tiles.len() {
        let tile = &world.tiles[idx];
        if tile.surface == Surface::Ocean || tile.river <= 0.55 || tile.flow_direction < 0 {
            continue;
        }

        let (x, y) = world.coords(idx);
        if is_inside_margin(world, x, y, edge_margin) {
            continue;
        }

        let (dx, dy) = directions[tile.flow_direction as usize];
        let px = x as isize - dx;
        let py = y as isize - dy;
        if world.in_bounds(px, py) {
            let previous = &world.tiles[world.idx(px as usize, py as usize)];
            if previous.surface != Surface::Ocean
                && previous.river > 0.55
                && previous.flow_direction == tile.flow_direction
            {
                continue;
            }
        }

        let mut run = 0_usize;
        let mut cx = x as isize;
        let mut cy = y as isize;
        while world.in_bounds(cx, cy) {
            let current_x = cx as usize;
            let current_y = cy as usize;
            if is_inside_margin(world, current_x, current_y, edge_margin) {
                break;
            }

            let current = &world.tiles[world.idx(current_x, current_y)];
            if current.surface == Surface::Ocean
                || current.river <= 0.55
                || current.flow_direction != tile.flow_direction
            {
                break;
            }

            run += 1;
            cx += dx;
            cy += dy;
        }

        longest = longest.max(run);
    }

    longest
}

fn is_inside_margin(world: &World, x: usize, y: usize, margin: usize) -> bool {
    x < margin
        || y < margin
        || x >= world.width.saturating_sub(margin)
        || y >= world.height.saturating_sub(margin)
}

fn longest_lowland_biome_boundary_run(world: &World) -> usize {
    let mut longest = 0_usize;

    for y in 0..world.height.saturating_sub(1) {
        let mut run = 0_usize;
        for x in 0..world.width {
            let a = world.idx(x, y);
            let b = world.idx(x, y + 1);
            if is_lowland_biome_boundary(world, a, b) {
                run += 1;
                longest = longest.max(run);
            } else {
                run = 0;
            }
        }
    }

    for x in 0..world.width.saturating_sub(1) {
        let mut run = 0_usize;
        for y in 0..world.height {
            let a = world.idx(x, y);
            let b = world.idx(x + 1, y);
            if is_lowland_biome_boundary(world, a, b) {
                run += 1;
                longest = longest.max(run);
            } else {
                run = 0;
            }
        }
    }

    longest
}

fn is_lowland_biome_boundary(world: &World, a: usize, b: usize) -> bool {
    let left = &world.tiles[a];
    let right = &world.tiles[b];
    left.biome != right.biome && is_lowland_ecology(world, left) && is_lowland_ecology(world, right)
}

fn is_lowland_ecology(world: &World, tile: &Tile) -> bool {
    tile.surface == Surface::Land
        && tile.raw_elevation <= world.sea_level + 0.30
        && !matches!(tile.biome, Biome::Alpine | Biome::Foothills)
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
