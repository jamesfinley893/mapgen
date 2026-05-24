use worldgen::{Surface, WorldConfig, build_metadata, generate_world};

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
fn every_tile_is_classified() {
    let world = generate_world(&config()).unwrap();
    assert_eq!(world.tiles.len(), config().width * config().height);
    let metadata = build_metadata(&world, &config());
    let classified: usize = metadata.biome_counts.iter().map(|(_, count)| *count).sum();
    assert_eq!(classified, world.tiles.len());
}
