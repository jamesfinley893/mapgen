use worldgen::{Biome, Surface, WorldConfig, build_metadata, generate_world};

fn config() -> WorldConfig {
    WorldConfig {
        seed: 99,
        width: 128,
        height: 128,
        render_scale: 2,
        ..WorldConfig::default()
    }
}

#[test]
fn worlds_contain_land_and_ocean() {
    let world = generate_world(&config()).unwrap();
    let mut land = 0;
    let mut ocean = 0;
    for tile in &world.tiles {
        match tile.surface {
            Surface::Ocean => ocean += 1,
            _ => land += 1,
        }
    }
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
fn worlds_have_at_least_one_river() {
    let world = generate_world(&config()).unwrap();
    assert!(world.tiles.iter().any(|tile| tile.surface == Surface::River));
}

#[test]
fn rivers_reach_a_sink() {
    let world = generate_world(&config()).unwrap();
    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface != Surface::River {
            continue;
        }
        let mut current = idx;
        let mut guard = 0;
        loop {
            let tile = &world.tiles[current];
            if matches!(tile.surface, Surface::Ocean | Surface::Lake) {
                break;
            }
            match tile.downstream {
                Some(next) => current = next,
                None => panic!("river did not terminate in a sink"),
            }
            guard += 1;
            assert!(guard < world.tiles.len(), "river path looped");
        }
    }
}

#[test]
fn conditioned_hydrology_eliminates_uphill_flow() {
    let world = generate_world(&config()).unwrap();
    for tile in &world.tiles {
        if let Some(next) = tile.downstream {
            assert!(world.tiles[next].hydro_elevation <= tile.hydro_elevation + 0.0002);
        }
    }
}

#[test]
fn lakes_are_multi_tile_when_present() {
    let world = generate_world(&config()).unwrap();
    let mut counts = std::collections::HashMap::<u32, usize>::new();
    for tile in &world.tiles {
        if let Some(lake_id) = tile.lake_id {
            *counts.entry(lake_id).or_default() += 1;
        }
    }
    for size in counts.into_values() {
        assert!(size >= 4);
    }
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
fn river_network_avoids_extreme_straight_runs() {
    let world = generate_world(&WorldConfig {
        seed: 42,
        width: 128,
        height: 128,
        render_scale: 2,
        ..WorldConfig::default()
    })
    .unwrap();
    assert!(longest_same_direction_run(&world) <= 26);
}

#[test]
fn metadata_reports_multiple_river_bands() {
    let world = generate_world(&config()).unwrap();
    let metadata = build_metadata(&world, &config());
    assert!(metadata.river_band_counts.iter().sum::<usize>() >= metadata.river_tiles);
    assert!(metadata.longest_trunk_length > 0);
    assert!(metadata.largest_contiguous_foothill_region <= metadata.land_tiles);
    assert!((0.0..=1.0).contains(&metadata.trunk_straight_run_ratio));
    assert!(metadata.tributary_spacing_variance >= 0.0);
    assert!((0.0..=1.0).contains(&metadata.mountain_exit_irregularity_score));
    assert!((0.0..=1.0).contains(&metadata.confined_trunk_fraction));
    assert!((0.0..=1.0).contains(&metadata.average_trunk_confinement));
}

#[test]
fn seed_42_does_not_collapse_into_alpine_blanket() {
    let world = generate_world(&WorldConfig {
        seed: 42,
        width: 256,
        height: 256,
        render_scale: 2,
        ..WorldConfig::default()
    })
    .unwrap();
    let land_tiles = world
        .tiles
        .iter()
        .filter(|tile| tile.surface != Surface::Ocean)
        .count();
    let alpine_tiles = world.tiles.iter().filter(|tile| tile.biome == Biome::Alpine).count();
    let alpine_fraction = alpine_tiles as f32 / land_tiles.max(1) as f32;
    assert!(alpine_fraction < 0.42, "alpine fraction too high: {alpine_fraction}");
}

#[test]
fn seed_42_keeps_mountain_adjacent_terrain_below_blanket_scale() {
    let config = WorldConfig {
        seed: 42,
        width: 256,
        height: 256,
        render_scale: 2,
        ..WorldConfig::default()
    };
    let world = generate_world(&config).unwrap();
    let metadata = build_metadata(&world, &config);
    let mountain_adjacent = metadata.alpine_fraction + metadata.foothill_fraction;
    assert!(
        mountain_adjacent < 0.7,
        "mountain-adjacent coverage too high: {mountain_adjacent}"
    );
    assert!(
        metadata.largest_contiguous_foothill_region < 29000,
        "foothill region too large: {}",
        metadata.largest_contiguous_foothill_region
    );
}

#[test]
fn fixed_seeds_still_produce_meaningful_high_ranges() {
    let world = generate_world(&WorldConfig {
        seed: 97,
        width: 256,
        height: 256,
        render_scale: 2,
        ..WorldConfig::default()
    })
    .unwrap();
    let alpine_tiles = world.tiles.iter().filter(|tile| tile.biome == Biome::Alpine).count();
    assert!(alpine_tiles > 1200, "too little alpine terrain survived: {alpine_tiles}");
}

#[test]
fn lowlands_are_not_overwhelmingly_woodland_and_tundra() {
    for seed in [42_u64, 97, 12302556654306610728] {
        let world = generate_world(&WorldConfig {
            seed,
            width: 256,
            height: 256,
            render_scale: 2,
            ..WorldConfig::default()
        })
        .unwrap();
        let mut lowland = 0_usize;
        let mut dominant = 0_usize;
        for tile in &world.tiles {
            if matches!(tile.surface, Surface::Ocean | Surface::Lake) {
                continue;
            }
            if tile.raw_elevation > world.sea_level + 0.18 || matches!(tile.biome, Biome::Alpine | Biome::Foothills) {
                continue;
            }
            lowland += 1;
            if matches!(tile.biome, Biome::Woodland | Biome::Tundra) {
                dominant += 1;
            }
        }
        let fraction = dominant as f32 / lowland.max(1) as f32;
        assert!(fraction < 0.78, "lowland biome mix too narrow for seed {seed}: {fraction}");
    }
}

#[test]
fn fixed_seeds_produce_foothill_transitions() {
    let world = generate_world(&WorldConfig {
        seed: 42,
        width: 256,
        height: 256,
        render_scale: 2,
        ..WorldConfig::default()
    })
    .unwrap();
    let foothill_tiles = world.tiles.iter().filter(|tile| tile.biome == Biome::Foothills).count();
    assert!(foothill_tiles > 220, "too few foothill tiles: {foothill_tiles}");
}

#[test]
fn alpine_strips_are_not_overly_isolated() {
    let world = generate_world(&WorldConfig {
        seed: 42,
        width: 256,
        height: 256,
        render_scale: 2,
        ..WorldConfig::default()
    })
    .unwrap();
    let isolated = world
        .tiles
        .iter()
        .enumerate()
        .filter(|(idx, tile)| {
            if tile.biome != Biome::Alpine {
                return false;
            }
            let (x, y) = world.coords(*idx);
            let neighbors = world
                .neighbors8(x, y)
                .filter(|(nx, ny)| world.tiles[world.idx(*nx, *ny)].biome == Biome::Alpine)
                .count();
            neighbors <= 1
        })
        .count();
    assert!(isolated < 180, "too many isolated alpine tiles: {isolated}");
}

#[test]
fn trunk_rivers_are_less_mountain_confined_than_headwaters() {
    let world = generate_world(&WorldConfig {
        seed: 42,
        width: 256,
        height: 256,
        render_scale: 2,
        ..WorldConfig::default()
    })
    .unwrap();
    let stream_threshold = (((world.width * world.height) as f32 * 0.00075).max(12.0)) * 6.5;
    let trunk_threshold = (((world.width * world.height) as f32 * 0.00075).max(12.0)) * 18.0;
    let headwater = mountain_banked_fraction(&world, stream_threshold, trunk_threshold);
    let trunk = mountain_banked_fraction(&world, trunk_threshold, f32::INFINITY);
    assert!(trunk < 0.42, "trunk rivers still too mountain-confined: {trunk}");
    assert!(
        trunk <= headwater + 0.03,
        "trunk rivers became materially more confined than headwaters: trunk={trunk}, headwater={headwater}"
    );
}

#[test]
fn trunk_rivers_avoid_extreme_straight_runs() {
    let world = generate_world(&WorldConfig {
        seed: 42,
        width: 256,
        height: 256,
        render_scale: 2,
        ..WorldConfig::default()
    })
    .unwrap();
    let trunk_threshold = (((world.width * world.height) as f32 * 0.00075).max(12.0)) * 18.0;
    let run = longest_same_direction_run_for_threshold(&world, trunk_threshold);
    assert!(
        run <= 19,
        "trunk river run too straight: {run}"
    );
}

#[test]
fn trunk_rivers_reduce_grid_locked_alignment() {
    for seed in [42_u64, 97, 7073116918442829777] {
        let config = WorldConfig {
            seed,
            width: 256,
            height: 256,
            render_scale: 2,
            ..WorldConfig::default()
        };
        let world = generate_world(&config).unwrap();
        let metadata = build_metadata(&world, &config);
        assert!(
            metadata.trunk_straight_run_ratio < 0.62,
            "trunk straight-run ratio too high for seed {seed}: {}",
            metadata.trunk_straight_run_ratio
        );
    }
}

#[test]
fn tributary_spacing_is_not_overly_even() {
    for seed in [42_u64, 97, 7073116918442829777] {
        let config = WorldConfig {
            seed,
            width: 256,
            height: 256,
            render_scale: 2,
            ..WorldConfig::default()
        };
        let world = generate_world(&config).unwrap();
        let metadata = build_metadata(&world, &config);
        assert!(
            metadata.tributary_spacing_variance > 35.0,
            "tributary spacing variance too low for seed {seed}: {}",
            metadata.tributary_spacing_variance
        );
    }
}

#[test]
fn mountain_exits_are_not_too_clean() {
    for seed in [42_u64, 97] {
        let config = WorldConfig {
            seed,
            width: 256,
            height: 256,
            render_scale: 2,
            ..WorldConfig::default()
        };
        let world = generate_world(&config).unwrap();
        let metadata = build_metadata(&world, &config);
        assert!(
            metadata.mountain_exit_irregularity_score > 0.16,
            "mountain exits too clean for seed {seed}: {}",
            metadata.mountain_exit_irregularity_score
        );
    }
}

#[test]
fn landmass_shape_is_not_strongly_center_biased() {
    for seed in [42_u64, 97, 7073116918442829777] {
        let world = generate_world(&WorldConfig {
            seed,
            width: 256,
            height: 256,
            render_scale: 2,
            ..WorldConfig::default()
        })
        .unwrap();
        let (center, outer) = center_vs_outer_land_fraction(&world);
        assert!(
            center <= outer * 2.2 + 0.12,
            "land remains too center-biased for seed {seed}: center={center} outer={outer}"
        );
    }
}

#[test]
fn edge_land_distribution_varies_by_edge() {
    for seed in [42_u64, 97, 7073116918442829777] {
        let world = generate_world(&WorldConfig {
            seed,
            width: 256,
            height: 256,
            render_scale: 2,
            ..WorldConfig::default()
        })
        .unwrap();
        let fractions = edge_land_fractions(&world, 20);
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
    let seeds = [42_u64, 97, 7073116918442829777, 12302556654306610728];
    let mut found = false;
    for seed in seeds {
        let world = generate_world(&WorldConfig {
            seed,
            width: 256,
            height: 256,
            render_scale: 2,
            ..WorldConfig::default()
        })
        .unwrap();
        let masses = major_landmass_count(&world, 900);
        if masses >= 2 {
            found = true;
            break;
        }
    }
    assert!(found, "fixed seed set did not produce multiple major landmasses");
}

#[test]
fn lakes_avoid_filamentary_shapes() {
    let world = generate_world(&WorldConfig {
        seed: 7073116918442829777,
        width: 256,
        height: 256,
        render_scale: 2,
        ..WorldConfig::default()
    })
    .unwrap();
    let mut counts = std::collections::HashMap::<u32, (usize, usize)>::new();
    for (idx, tile) in world.tiles.iter().enumerate() {
        let Some(lake_id) = tile.lake_id else { continue };
        let (x, y) = world.coords(idx);
        let edge_neighbors = world
            .neighbors8(x, y)
            .filter(|(nx, ny)| world.tiles[world.idx(*nx, *ny)].lake_id == Some(lake_id))
            .count();
        let entry = counts.entry(lake_id).or_insert((0, 0));
        entry.0 += 1;
        if edge_neighbors <= 1 {
            entry.1 += 1;
        }
    }
    for (area, exposed) in counts.into_values() {
        if area < 6 {
            continue;
        }
        let fraction = exposed as f32 / area as f32;
        assert!(fraction < 0.28, "lake too filamentary: area={area} fraction={fraction}");
    }
}

fn longest_same_direction_run(world: &worldgen::World) -> usize {
    let mut longest = 0;
    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface != Surface::River {
            continue;
        }
        let mut current = idx;
        let mut current_dir = None;
        let mut streak = 0;
        let mut guard = 0;
        while guard < world.tiles.len() {
            let tile = &world.tiles[current];
            if tile.surface != Surface::River {
                break;
            }
            let next = match tile.downstream {
                Some(next) => next,
                None => break,
            };
            let (x, y) = world.coords(current);
            let (nx, ny) = world.coords(next);
            let dir = ((nx as isize - x as isize).signum(), (ny as isize - y as isize).signum());
            if Some(dir) == current_dir {
                streak += 1;
            } else {
                current_dir = Some(dir);
                streak = 1;
            }
            longest = longest.max(streak);
            current = next;
            guard += 1;
        }
    }
    longest
}

fn longest_same_direction_run_for_threshold(world: &worldgen::World, min_area: f32) -> usize {
    let mut longest = 0;
    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface != Surface::River || tile.contributing_area < min_area {
            continue;
        }
        let mut current = idx;
        let mut current_dir = None;
        let mut streak = 0;
        let mut guard = 0;
        while guard < world.tiles.len() {
            let tile = &world.tiles[current];
            if tile.surface != Surface::River || tile.contributing_area < min_area {
                break;
            }
            let next = match tile.downstream {
                Some(next) => next,
                None => break,
            };
            let (x, y) = world.coords(current);
            let (nx, ny) = world.coords(next);
            let dir = ((nx as isize - x as isize).signum(), (ny as isize - y as isize).signum());
            if Some(dir) == current_dir {
                streak += 1;
            } else {
                current_dir = Some(dir);
                streak = 1;
            }
            longest = longest.max(streak);
            current = next;
            guard += 1;
        }
    }
    longest
}

fn mountain_banked_fraction(world: &worldgen::World, min_area: f32, max_area: f32) -> f32 {
    let mut total = 0_usize;
    let mut mountain_banked = 0_usize;

    for (idx, tile) in world.tiles.iter().enumerate() {
        if tile.surface != Surface::River
            || tile.contributing_area < min_area
            || tile.contributing_area >= max_area
        {
            continue;
        }
        let Some(next) = tile.downstream else {
            continue;
        };
        let (x, y) = world.coords(idx);
        let (nx, ny) = world.coords(next);
        let dx = (nx as isize - x as isize).signum();
        let dy = (ny as isize - y as isize).signum();
        if dx == 0 && dy == 0 {
            continue;
        }
        let banks = [(-dy, dx), (dy, -dx)];
        let mut bank_count = 0_usize;
        for bank in banks {
            let bx = x as isize + bank.0;
            let by = y as isize + bank.1;
            if !world.in_bounds(bx, by) {
                continue;
            }
            let bidx = world.idx(bx as usize, by as usize);
            if matches!(world.tiles[bidx].biome, Biome::Foothills | Biome::Alpine) {
                bank_count += 1;
            }
        }
        total += 1;
        if bank_count == 2 {
            mountain_banked += 1;
        }
    }

    if total == 0 {
        0.0
    } else {
        mountain_banked as f32 / total as f32
    }
}

fn center_vs_outer_land_fraction(world: &worldgen::World) -> (f32, f32) {
    let cx = (world.width as f32 - 1.0) * 0.5;
    let cy = (world.height as f32 - 1.0) * 0.5;
    let inner_r2 = (world.width.min(world.height) as f32 * 0.22).powi(2);
    let outer_r2 = (world.width.min(world.height) as f32 * 0.40).powi(2);
    let mut inner_land = 0_usize;
    let mut inner_total = 0_usize;
    let mut outer_land = 0_usize;
    let mut outer_total = 0_usize;

    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist2 = dx * dx + dy * dy;
            let land = !matches!(world.tiles[idx].surface, Surface::Ocean);
            if dist2 <= inner_r2 {
                inner_total += 1;
                if land {
                    inner_land += 1;
                }
            } else if dist2 >= outer_r2 {
                outer_total += 1;
                if land {
                    outer_land += 1;
                }
            }
        }
    }

    (
        inner_land as f32 / inner_total.max(1) as f32,
        outer_land as f32 / outer_total.max(1) as f32,
    )
}

fn edge_land_fractions(world: &worldgen::World, band: usize) -> [f32; 4] {
    let band = band.min(world.width / 2).min(world.height / 2).max(1);
    let mut counts = [(0_usize, 0_usize); 4];
    for y in 0..world.height {
        for x in 0..world.width {
            let idx = world.idx(x, y);
            let land = !matches!(world.tiles[idx].surface, Surface::Ocean);
            if y < band {
                counts[0].1 += 1;
                if land {
                    counts[0].0 += 1;
                }
            }
            if y >= world.height - band {
                counts[1].1 += 1;
                if land {
                    counts[1].0 += 1;
                }
            }
            if x < band {
                counts[2].1 += 1;
                if land {
                    counts[2].0 += 1;
                }
            }
            if x >= world.width - band {
                counts[3].1 += 1;
                if land {
                    counts[3].0 += 1;
                }
            }
        }
    }
    counts.map(|(land, total)| land as f32 / total.max(1) as f32)
}

fn major_landmass_count(world: &worldgen::World, min_area: usize) -> usize {
    let mut visited = vec![false; world.tiles.len()];
    let mut count = 0_usize;

    for idx in 0..world.tiles.len() {
        if visited[idx] || matches!(world.tiles[idx].surface, Surface::Ocean | Surface::Lake) {
            continue;
        }
        visited[idx] = true;
        let mut queue = std::collections::VecDeque::from([idx]);
        let mut area = 0_usize;
        while let Some(current) = queue.pop_front() {
            area += 1;
            let (x, y) = world.coords(current);
            for (nx, ny) in world.neighbors8(x, y) {
                let nidx = world.idx(nx, ny);
                if visited[nidx] || matches!(world.tiles[nidx].surface, Surface::Ocean | Surface::Lake) {
                    continue;
                }
                visited[nidx] = true;
                queue.push_back(nidx);
            }
        }
        if area >= min_area {
            count += 1;
        }
    }

    count
}
