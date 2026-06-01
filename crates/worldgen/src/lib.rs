mod config;
mod features;
mod generate;
mod metadata;
mod render;
mod world;

pub use config::{DEFAULT_WORLD_SIZE, LEGACY_WORLD_SIZE, WorldConfig};
pub use features::{mountain_feature_for_tile, permanent_snow_cover};
pub use generate::{
    GenerationProfile, GenerationStageTiming, biome_for_tile, generate_world,
    generate_world_with_profile,
};
pub use metadata::{WorldMetadata, build_metadata};
pub use render::render_world;
pub use world::{Biome, Landform, MountainFeature, Surface, Tile, World};

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> WorldConfig {
        WorldConfig {
            seed: 42,
            width: 96,
            height: 96,
            world_size: 96,
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
            assert_eq!(left.landform, right.landform);
            assert!((left.raw_elevation - right.raw_elevation).abs() < f32::EPSILON);
            assert!((left.terrain_texture - right.terrain_texture).abs() < f32::EPSILON);
            assert!((left.ecotone_strength - right.ecotone_strength).abs() < f32::EPSILON);
            assert!((left.shore_influence - right.shore_influence).abs() < f32::EPSILON);
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
    fn default_world_config_is_native_two_x_same_extent() {
        let config = WorldConfig::default();
        let world = World {
            seed: config.seed,
            width: config.width,
            height: config.height,
            sea_level: config.sea_level,
            world_size: config.world_size,
            tiles: Vec::new(),
        };

        assert_eq!(config.width, 768);
        assert_eq!(config.height, 768);
        assert_eq!(config.world_size, DEFAULT_WORLD_SIZE);
        assert_eq!(world.effective_world_size(), DEFAULT_WORLD_SIZE as f32);
        assert_eq!(world.width as f32 / world.effective_world_size(), 1.0);
    }

    #[test]
    fn original_density_larger_world_remains_available() {
        let world = World {
            seed: 1,
            width: 1536,
            height: 1536,
            sea_level: 0.52,
            world_size: LEGACY_WORLD_SIZE,
            tiles: Vec::new(),
        };

        assert_eq!(world.effective_world_size(), LEGACY_WORLD_SIZE as f32);
        assert_eq!(world.width as f32 / world.effective_world_size(), 4.0);
    }

    #[test]
    fn generation_profile_records_core_stages() {
        let config = WorldConfig {
            width: 64,
            height: 64,
            world_size: 64,
            ..test_config()
        };
        let (_, profile) = generate_world_with_profile(&config).unwrap();
        let stages = profile
            .stages()
            .iter()
            .map(|stage| stage.name)
            .collect::<Vec<_>>();

        assert!(stages.contains(&"terrain: tectonic sampling"));
        assert!(stages.contains(&"terrain: bathymetry detail"));
        assert!(stages.contains(&"terrain: alpine breakup"));
        assert!(stages.contains(&"hydrology"));
        assert!(stages.contains(&"biomes"));
        assert!(profile.total_duration() > std::time::Duration::ZERO);
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
