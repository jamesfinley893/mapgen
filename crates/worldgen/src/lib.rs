mod config;
mod features;
mod generate;
mod metadata;
mod render;
mod world;

pub use config::WorldConfig;
pub use features::{mountain_feature_for_tile, permanent_snow_cover};
pub use generate::{biome_for_tile, generate_world};
pub use metadata::{WorldMetadata, build_metadata};
pub use render::render_world;
pub use world::{Biome, MountainFeature, Tile, World};

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> WorldConfig {
        WorldConfig {
            seed: 42,
            width: 96,
            height: 96,
            ..WorldConfig::default()
        }
    }

    #[test]
    fn generation_is_deterministic() {
        let a = generate_world(&test_config()).unwrap();
        let b = generate_world(&test_config()).unwrap();
        assert_eq!(a.tiles.len(), b.tiles.len());
        for (left, right) in a.tiles.iter().zip(b.tiles.iter()) {
            assert_eq!(left.biome, right.biome);
            assert!((left.elevation - right.elevation).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn biome_thresholds_are_stable() {
        assert_eq!(
            biome_for_tile(false, false, 0.6, 0.5, 0.8, 0.9),
            Biome::Rainforest
        );
        assert_eq!(
            biome_for_tile(false, false, 0.58, 0.5, 0.45, 0.12),
            Biome::Desert
        );
        assert_eq!(
            biome_for_tile(false, false, 0.9, 0.5, 0.4, 0.5),
            Biome::Alpine
        );
        assert_eq!(
            biome_for_tile(true, false, 0.1, 0.5, 0.4, 0.5),
            Biome::Ocean
        );
        assert_eq!(
            biome_for_tile(false, true, 0.55, 0.5, 0.4, 0.5),
            Biome::Coast
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
        assert_eq!(
            metadata.land_tiles + metadata.ocean_tiles,
            world.tiles.len()
        );
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

    #[test]
    fn metadata_reports_effective_world_sea_level() {
        let config = test_config();
        let mut world = generate_world(&config).unwrap();
        world.sea_level = 0.47;
        let metadata = build_metadata(&world, &config);
        assert_eq!(metadata.sea_level, world.sea_level);
    }
}
