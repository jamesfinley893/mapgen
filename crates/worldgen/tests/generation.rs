use std::sync::OnceLock;

use worldgen::{
    Biome, Landform, MountainFeature, Surface, Tile, World, WorldConfig, build_metadata,
    generate_world, mountain_feature_for_tile, render_world,
};

fn config() -> WorldConfig {
    WorldConfig {
        seed: 99,
        width: 128,
        height: 128,
        world_size: 128,
        ..WorldConfig::default()
    }
}

fn fixed_config(seed: u64) -> WorldConfig {
    WorldConfig {
        seed,
        width: 256,
        height: 256,
        world_size: 256,
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
        assert!(
            tile.elevation_shade.is_finite() && (0.0..=1.0).contains(&tile.elevation_shade),
            "tile {idx} elevation shade out of range: {}",
            tile.elevation_shade
        );
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
            tile.river_depth.is_finite() && (0.0..=1.0).contains(&tile.river_depth),
            "tile {idx} river depth out of range: {}",
            tile.river_depth
        );
        assert!(
            tile.river_width.is_finite() && (0.0..=1.0).contains(&tile.river_width),
            "tile {idx} river width out of range: {}",
            tile.river_width
        );
        assert!(
            tile.river_order <= 16,
            "tile {idx} river order out of range: {}",
            tile.river_order
        );
        assert!(
            tile.lake_depth.is_finite() && (0.0..=1.0).contains(&tile.lake_depth),
            "tile {idx} lake depth out of range: {}",
            tile.lake_depth
        );
        assert!(
            tile.lake_inflow.is_finite() && (0.0..=1.0).contains(&tile.lake_inflow),
            "tile {idx} lake inflow out of range: {}",
            tile.lake_inflow
        );
        assert!(
            tile.spill_discharge.is_finite() && (0.0..=1.0).contains(&tile.spill_discharge),
            "tile {idx} spill discharge out of range: {}",
            tile.spill_discharge
        );
        assert!(
            tile.erosion.is_finite() && (0.0..=1.0).contains(&tile.erosion),
            "tile {idx} erosion out of range: {}",
            tile.erosion
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
            assert_eq!(tile.river_depth, 0.0);
            assert_eq!(tile.river_width, 0.0);
            assert_eq!(tile.river_order, 0);
            assert_eq!(tile.lake_depth, 0.0);
            assert_eq!(tile.water_body_id, 0);
            assert_eq!(tile.lake_inflow, 0.0);
            assert!(!tile.lake_outlet);
            assert_eq!(tile.spill_discharge, 0.0);
            assert_eq!(tile.flow_direction, -1);
        } else {
            assert!(tile.ocean_distance > 0);
            if tile.lake_depth > 0.0 {
                assert!(
                    tile.water_body_id > 0,
                    "lake tile {idx} has no water body id"
                );
            }
            if tile.river > 0.035 {
                assert!(tile.flow_direction >= 0);
                assert!(
                    tile.river_order > 0,
                    "visible river tile {idx} has no stream order"
                );
                assert!(
                    tile.river_depth > 0.0,
                    "visible river tile {idx} has no depth"
                );
                assert!(
                    tile.river_width > 0.0,
                    "visible river tile {idx} has no width"
                );
            }
        }
        assert_eq!(
            tile.mountain_feature,
            mountain_feature_for_tile(&world, idx)
        );
        assert!(
            tile.terrain_texture.is_finite() && (0.0..=1.0).contains(&tile.terrain_texture),
            "tile {idx} terrain texture out of range: {}",
            tile.terrain_texture
        );
        assert!(
            tile.ecotone_strength.is_finite() && (0.0..=1.0).contains(&tile.ecotone_strength),
            "tile {idx} ecotone strength out of range: {}",
            tile.ecotone_strength
        );
        assert!(
            tile.shore_influence.is_finite() && (0.0..=1.0).contains(&tile.shore_influence),
            "tile {idx} shore influence out of range: {}",
            tile.shore_influence
        );
        if tile.surface == Surface::Ocean {
            assert_eq!(tile.landform, Landform::Water);
        }
    }
}

#[test]
fn generated_worlds_export_nontrivial_presentation_fields() {
    let world = fixed_world(42);
    let land_tiles = world
        .tiles
        .iter()
        .filter(|tile| tile.surface != Surface::Ocean)
        .count();
    let textured = world
        .tiles
        .iter()
        .filter(|tile| tile.surface != Surface::Ocean && (tile.terrain_texture - 0.5).abs() > 0.01)
        .count();
    let ecotones = world
        .tiles
        .iter()
        .filter(|tile| tile.ecotone_strength > 0.05)
        .count();
    let shores = world
        .tiles
        .iter()
        .filter(|tile| tile.shore_influence > 0.15)
        .count();

    assert!(
        textured > land_tiles / 4,
        "too few land tiles carry texture: {textured}/{land_tiles}"
    );
    assert!(
        ecotones > land_tiles / 30,
        "too few tiles expose ecotone strength: {ecotones}/{land_tiles}"
    );
    assert!(shores > 0, "no shore influence was exported");
    assert!(
        world.tiles.iter().any(|tile| matches!(
            tile.landform,
            Landform::Hill | Landform::Valley | Landform::Ridge | Landform::Peak
        )),
        "generated world did not classify any readable landforms"
    );
}

#[test]
fn coast_tiles_export_stronger_shore_influence_than_inland_tiles() {
    let world = fixed_world(42);
    let mut coast_sum = 0.0_f32;
    let mut coast_count = 0_usize;
    let mut inland_sum = 0.0_f32;
    let mut inland_count = 0_usize;

    for tile in &world.tiles {
        match tile.surface {
            Surface::Coast => {
                coast_sum += tile.shore_influence;
                coast_count += 1;
            }
            Surface::Land
                if tile.ocean_distance > 12 && tile.lake_depth <= 0.0 && tile.river < 0.08 =>
            {
                inland_sum += tile.shore_influence;
                inland_count += 1;
            }
            _ => {}
        }
    }

    let coast_mean = coast_sum / coast_count.max(1) as f32;
    let inland_mean = inland_sum / inland_count.max(1) as f32;
    assert!(coast_count > 0);
    assert!(inland_count > 0);
    assert!(
        coast_mean > inland_mean + 0.35,
        "coast tiles should carry a much stronger shore signal: coast={coast_mean} inland={inland_mean}"
    );
}

#[test]
fn mountain_features_export_readable_landforms() {
    let world = fixed_world(42);
    let mut summits = 0_usize;
    let mut summit_peaks = 0_usize;
    let mut ridges = 0_usize;
    let mut ridge_landforms = 0_usize;

    for tile in &world.tiles {
        match tile.mountain_feature {
            MountainFeature::Summit => {
                summits += 1;
                summit_peaks += usize::from(tile.landform == Landform::Peak);
            }
            MountainFeature::Ridge => {
                ridges += 1;
                ridge_landforms +=
                    usize::from(matches!(tile.landform, Landform::Ridge | Landform::Peak));
            }
            _ => {}
        }
    }

    assert!(summits > 0, "fixed seed has no summit features");
    assert_eq!(summits, summit_peaks);
    assert!(ridges > 0, "fixed seed has no ridge features");
    assert_eq!(ridges, ridge_landforms);
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
fn fixed_seed_set_includes_hydrological_lake_basins() {
    let mut lake_worlds = 0_usize;

    for seed in [42_u64, 97, 3000, 7073116918442829777, 12302556654306610728] {
        let world = fixed_world(seed);
        let lake_tiles = world
            .tiles
            .iter()
            .filter(|tile| tile.surface != Surface::Ocean && tile.lake_depth > 0.0)
            .count();

        if lake_tiles > world.width / 8 {
            lake_worlds += 1;
        }

        for tile in world.tiles.iter().filter(|tile| tile.lake_depth > 0.0) {
            assert_eq!(tile.biome, Biome::Freshwater);
            assert!(tile.water_body_id > 0);
            assert_eq!(tile.river, 0.0);
        }
    }

    assert!(
        lake_worlds >= 3,
        "fixed seed set did not produce enough hydrological lake basins: {lake_worlds}"
    );
}

#[test]
fn major_river_paths_terminate_in_lake_or_ocean() {
    let world = fixed_world(42);
    let mut major_rivers = 0_usize;
    let mut terminated = 0_usize;

    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface == Surface::Ocean || tile.river <= 0.55 {
            continue;
        }

        major_rivers += 1;
        if river_path_reaches_water_sink(world, idx) {
            terminated += 1;
        }
    }

    assert!(major_rivers > world.width / 2);
    assert!(
        terminated as f32 / major_rivers.max(1) as f32 > 0.94,
        "too many major river paths fail to terminate in lake or ocean: {terminated}/{major_rivers}"
    );
}

#[test]
fn major_rivers_export_stream_order_and_erosive_power() {
    let world = fixed_world(42);
    let mut major_rivers = 0_usize;
    let mut ordered = 0_usize;
    let mut erosive = 0_usize;
    let mut erosion_sum = 0.0_f32;

    for tile in &world.tiles {
        if tile.surface == Surface::Ocean || tile.river <= 0.55 {
            continue;
        }

        major_rivers += 1;
        ordered += usize::from(tile.river_order > 0);
        erosive += usize::from(tile.erosion > 0.045);
        erosion_sum += tile.erosion;
    }

    assert!(major_rivers > world.width / 2);
    assert_eq!(ordered, major_rivers);
    assert!(
        erosive as f32 / major_rivers.max(1) as f32 > 0.70,
        "too few major river tiles carry erosive power: {erosive}/{major_rivers}"
    );
    assert!(
        erosion_sum / major_rivers.max(1) as f32 > 0.08,
        "mean major-river erosion is too weak"
    );
}

#[test]
fn fixed_seed_major_rivers_reach_deep_render_regime() {
    for seed in [42_u64, 97, 3000] {
        let world = fixed_world(seed);
        let mut depths = world
            .tiles
            .iter()
            .filter(|tile| tile.surface != Surface::Ocean && tile.river > 0.55)
            .map(|tile| tile.river_depth)
            .collect::<Vec<_>>();
        depths.sort_by(|a, b| a.total_cmp(b));

        let major_rivers = depths.len();
        assert!(
            major_rivers > world.width / 2,
            "seed {seed} did not produce enough major river depth samples: {major_rivers}"
        );

        let deep_reaches = depths.iter().filter(|depth| **depth >= 0.70).count();
        let p90 = depths[((major_rivers - 1) as f32 * 0.90) as usize];

        assert!(
            p90 > 0.68,
            "seed {seed} major-river depth distribution is too flat for deep-water rendering: p90={p90}"
        );
        assert!(
            deep_reaches as f32 / major_rivers.max(1) as f32 > 0.08,
            "seed {seed} has too few major reaches in the deep-water regime: {deep_reaches}/{major_rivers}"
        );
    }
}

#[test]
fn fixed_seed_major_rivers_export_broad_simulated_width() {
    for seed in [42_u64, 97, 3000] {
        let world = fixed_world(seed);
        let mut widths = world
            .tiles
            .iter()
            .filter(|tile| tile.surface != Surface::Ocean && tile.river > 0.55)
            .map(|tile| tile.river_width)
            .collect::<Vec<_>>();
        widths.sort_by(|a, b| a.total_cmp(b));

        let major_rivers = widths.len();
        assert!(
            major_rivers > world.width / 2,
            "seed {seed} did not produce enough major river width samples: {major_rivers}"
        );

        let p50 = widths[((major_rivers - 1) as f32 * 0.50) as usize];
        let p90 = widths[((major_rivers - 1) as f32 * 0.90) as usize];

        assert!(
            p50 > 0.70,
            "seed {seed} major-river median simulated width is too narrow: p50={p50}"
        );
        assert!(
            p90 > 0.90,
            "seed {seed} major-river high-flow simulated width is too narrow: p90={p90}"
        );
    }
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
            if tile.lake_depth > 0.0 {
                assert!(
                    tile.water_body_id > 0,
                    "freshwater lake {idx} has no water body id"
                );
                assert!(
                    tile.raw_elevation > world.sea_level,
                    "freshwater lake {idx} sank below sea level"
                );
                continue;
            }
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
        world_size: 128,
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
#[ignore = "expensive default high-detail smoke test for release profiling"]
fn default_high_detail_world_has_sane_surface_and_biome_balance() {
    let config = WorldConfig {
        seed: 42,
        ..WorldConfig::default()
    };
    let world = generate_world(&config).unwrap();
    let metadata = build_metadata(&world, &config);
    let tile_count = world.tile_count();

    assert_eq!(metadata.width, 768);
    assert_eq!(metadata.height, 768);
    assert_eq!(metadata.effective_world_size, 768.0);
    assert!(metadata.land_tiles > tile_count / 10);
    assert!(metadata.ocean_tiles > tile_count / 10);
    assert!(metadata.river_tiles > world.width / 2);

    let dominant_non_ocean = metadata
        .biome_counts
        .iter()
        .filter(|(biome, _)| *biome != Biome::Ocean)
        .map(|(_, count)| *count)
        .max()
        .unwrap_or(0);
    assert!(
        dominant_non_ocean < metadata.land_tiles * 3 / 4,
        "large map is dominated by a single non-ocean biome"
    );
}

#[test]
fn render_world_produces_one_pixel_per_tile() {
    let world = render_test_world(5, 4);
    let image = render_world(&world);

    assert_eq!(image.width(), 5);
    assert_eq!(image.height(), 4);
}

#[test]
fn render_world_maps_each_tile_to_exactly_one_pixel() {
    let base_world = render_test_world(3, 3);
    let mut changed_world = base_world.clone();
    let center = changed_world.idx(1, 1);
    changed_world.tiles[center] = Tile {
        surface: Surface::Land,
        biome: Biome::Rainforest,
        raw_elevation: 0.62,
        elevation_shade: 0.86,
        temperature: 0.82,
        moisture: 0.90,
        ..Tile::default()
    };

    let base = render_world(&base_world);
    let changed = render_world(&changed_world);
    let mut changed_pixels = 0;
    for y in 0..3 {
        for x in 0..3 {
            let delta = color_delta(base.get_pixel(x, y).0, changed.get_pixel(x, y).0);
            if delta > 0.0 {
                changed_pixels += 1;
                assert_eq!((x, y), (1, 1));
            }
        }
    }

    assert_eq!(changed_pixels, 1);
}

#[test]
fn elevation_shade_controls_land_luminance_per_tile() {
    let mut shaded = render_test_world(2, 1);
    for tile in &mut shaded.tiles {
        tile.raw_elevation = 0.72;
        tile.biome = Biome::Foothills;
        tile.elevation_shade = 0.20;
    }
    let lit_idx = shaded.idx(1, 0);
    shaded.tiles[lit_idx].elevation_shade = 0.88;

    let image = render_world(&shaded);
    let dark = image.get_pixel(0, 0).0;
    let lit = image.get_pixel(1, 0).0;

    assert!(
        luma(lit) > luma(dark) + 35.0,
        "elevation shade should materially alter tile luminance: dark={dark:?} lit={lit:?}"
    );
}

#[test]
fn ocean_depth_and_temperature_render_from_same_tile_only() {
    let mut world = World::new(7, 3, 1, 0.50, 0);
    world.tiles[0] = warm_ocean_tile(0.49);
    world.tiles[1] = warm_ocean_tile(0.20);
    world.tiles[2] = warm_ocean_tile(0.49);
    world.tiles[2].temperature = 0.90;

    let image = render_world(&world);
    let shallow = image.get_pixel(0, 0).0;
    let deep = image.get_pixel(1, 0).0;
    let tropical = image.get_pixel(2, 0).0;

    assert!(
        luma(shallow) > luma(deep) + 20.0,
        "shallow ocean should render lighter than deep ocean: shallow={shallow:?} deep={deep:?}"
    );
    assert!(
        tropical[1] > shallow[1] && tropical[2] >= shallow[2],
        "warm shallow ocean should shift toward lagoon cyan: shallow={shallow:?} tropical={tropical:?}"
    );
}

#[test]
fn lake_and_river_fields_change_only_their_own_tiles() {
    let base_world = render_test_world(3, 2);
    let mut water_world = base_world.clone();
    let lake = water_world.idx(0, 1);
    water_world.tiles[lake] = Tile {
        surface: Surface::Land,
        biome: Biome::Freshwater,
        raw_elevation: 0.54,
        lake_depth: 0.82,
        water_body_id: 1,
        temperature: 0.52,
        moisture: 1.0,
        ..Tile::default()
    };
    let river = water_world.idx(2, 0);
    water_world.tiles[river].river = 0.86;
    water_world.tiles[river].river_order = 3;
    water_world.tiles[river].river_depth = 0.72;
    water_world.tiles[river].river_width = 0.65;
    water_world.tiles[river].flow_direction = 2;

    let base = render_world(&base_world);
    let water = render_world(&water_world);
    let mut changed = Vec::new();
    for y in 0..2 {
        for x in 0..3 {
            if color_delta(base.get_pixel(x, y).0, water.get_pixel(x, y).0) > 0.0 {
                changed.push((x, y));
            }
        }
    }

    assert_eq!(changed, vec![(2, 0), (0, 1)]);
    assert!(water.get_pixel(0, 1).0[2] > base.get_pixel(0, 1).0[2]);
    assert!(luma(water.get_pixel(2, 0).0) + 10.0 < luma(base.get_pixel(2, 0).0));
}

#[test]
fn presentation_fields_change_only_their_own_tile() {
    let base_world = render_test_world(3, 3);
    let mut changed_world = base_world.clone();
    let center = changed_world.idx(1, 1);
    changed_world.tiles[center].landform = Landform::Hill;
    changed_world.tiles[center].terrain_texture = 0.94;
    changed_world.tiles[center].ecotone_strength = 0.82;
    changed_world.tiles[center].shore_influence = 0.58;

    let base = render_world(&base_world);
    let changed = render_world(&changed_world);
    let mut changed_pixels = 0;
    for y in 0..3 {
        for x in 0..3 {
            let delta = color_delta(base.get_pixel(x, y).0, changed.get_pixel(x, y).0);
            if delta > 0.0 {
                changed_pixels += 1;
                assert_eq!((x, y), (1, 1));
            }
        }
    }

    assert_eq!(changed_pixels, 1);
}

#[test]
fn landform_and_texture_materially_affect_luminance() {
    let mut world = render_test_world(3, 1);
    let plain = world.idx(0, 0);
    let bright = world.idx(1, 0);
    let dark = world.idx(2, 0);
    world.tiles[bright].landform = Landform::Ridge;
    world.tiles[bright].terrain_texture = 0.96;
    world.tiles[bright].relief = 0.08;
    world.tiles[dark].landform = Landform::Basin;
    world.tiles[dark].terrain_texture = 0.04;

    let image = render_world(&world);
    let plain_luma = luma(image.get_pixel(plain as u32, 0).0);
    let bright_luma = luma(image.get_pixel(bright as u32, 0).0);
    let dark_luma = luma(image.get_pixel(dark as u32, 0).0);

    assert!(
        bright_luma > plain_luma + 8.0,
        "ridge/bright texture should read lighter: plain={plain_luma} bright={bright_luma}"
    );
    assert!(
        dark_luma + 5.0 < plain_luma,
        "basin/dark texture should read darker: plain={plain_luma} dark={dark_luma}"
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

fn river_path_reaches_water_sink(world: &World, start_idx: usize) -> bool {
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
    let mut seen = vec![false; world.tiles.len()];
    let mut idx = start_idx;

    for _ in 0..world.tiles.len().min(2048) {
        if seen[idx] {
            return false;
        }
        seen[idx] = true;

        let tile = &world.tiles[idx];
        if tile.surface == Surface::Ocean || tile.lake_depth > 0.0 {
            return true;
        }
        if tile.flow_direction < 0 {
            return false;
        }

        let (x, y) = world.coords(idx);
        let (dx, dy) = directions[tile.flow_direction as usize];
        let nx = x as isize + dx;
        let ny = y as isize + dy;
        if !world.in_bounds(nx, ny) {
            return false;
        }

        idx = world.idx(nx as usize, ny as usize);
    }

    false
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
