mod config;
mod features;
mod generate;
mod metadata;
mod render;
mod world;

pub use config::WorldConfig;
pub use features::{MountainFeature, mountain_feature_for_tile, permanent_snow_cover};
pub use generate::{biome_for_tile, generate_world};
pub use metadata::{WorldMetadata, build_metadata};
pub use render::{RenderConfig, render_world};
pub use world::{Biome, Surface, Tile, World};

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> WorldConfig {
        WorldConfig {
            seed: 42,
            width: 96,
            height: 96,
            render_scale: 2,
            ..WorldConfig::default()
        }
    }

    #[test]
    fn generation_is_deterministic() {
        let a = generate_world(&test_config()).unwrap();
        let b = generate_world(&test_config()).unwrap();
        assert_eq!(a.tiles.len(), b.tiles.len());
        for (left, right) in a.tiles.iter().zip(b.tiles.iter()) {
            assert_eq!(left.surface, right.surface);
            assert_eq!(left.biome, right.biome);
            assert!((left.raw_elevation - right.raw_elevation).abs() < f32::EPSILON);
            assert!((left.hydro_elevation - right.hydro_elevation).abs() < f32::EPSILON);
            assert!((left.contributing_area - right.contributing_area).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn drainage_never_routes_uphill() {
        let world = generate_world(&test_config()).unwrap();
        for tile in &world.tiles {
            if let Some(next) = tile.downstream {
                assert!(world.tiles[next].hydro_elevation <= tile.hydro_elevation + 0.0002);
            }
        }
    }

    #[test]
    fn biome_thresholds_are_stable() {
        assert_eq!(
            biome_for_tile(Surface::Land, 0.6, 0.5, 0.8, 0.9),
            Biome::Rainforest
        );
        assert_eq!(
            biome_for_tile(Surface::Land, 0.58, 0.5, 0.45, 0.12),
            Biome::Desert
        );
        assert_eq!(
            biome_for_tile(Surface::Land, 0.9, 0.5, 0.4, 0.5),
            Biome::Alpine
        );
        assert_eq!(
            biome_for_tile(Surface::Ocean, 0.1, 0.5, 0.4, 0.5),
            Biome::Ocean
        );
    }

    #[test]
    fn metadata_counts_are_populated() {
        let world = generate_world(&test_config()).unwrap();
        let metadata = build_metadata(&world, &test_config());
        assert_eq!(metadata.width, 96);
        assert_eq!(metadata.world_size, test_config().world_size);
        assert_eq!(metadata.effective_world_size, 96.0);
        assert!(!metadata.biome_counts.is_empty());
        assert!(
            metadata.land_tiles + metadata.ocean_tiles + metadata.lake_tiles >= world.tiles.len()
        );
        assert!(metadata.largest_basin_area > 0);
    }

    #[test]
    fn metadata_records_explicit_world_size() {
        let config = WorldConfig {
            world_size: 64,
            ..test_config()
        };
        let world = generate_world(&config).unwrap();
        let metadata = build_metadata(&world, &config);
        assert_eq!(metadata.world_size, 64);
        assert_eq!(metadata.effective_world_size, 64.0);
    }
}
