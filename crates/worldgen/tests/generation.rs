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
    assert!(foothill_tiles > 400, "too few foothill tiles: {foothill_tiles}");
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
        trunk < headwater,
        "trunk rivers should be less confined than headwaters: trunk={trunk}, headwater={headwater}"
    );
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
