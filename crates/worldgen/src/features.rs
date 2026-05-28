use serde::{Deserialize, Serialize};

use crate::{Biome, World};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MountainFeature {
    None,
    Foothill,
    AlpineSlope,
    Ridge,
    Summit,
}

pub fn mountain_feature_for_tile(world: &World, idx: usize) -> MountainFeature {
    let tile = &world.tiles[idx];
    match tile.biome {
        Biome::Foothills => MountainFeature::Foothill,
        Biome::Alpine => classify_alpine_feature(world, idx),
        _ => MountainFeature::None,
    }
}

pub fn permanent_snow_cover(world: &World, idx: usize) -> f32 {
    let tile = &world.tiles[idx];
    let height_above_sea = (tile.raw_elevation - world.sea_level).max(0.0);

    let (snow_line, melt_band, max_cover) = match tile.biome {
        Biome::Alpine => {
            let feature = classify_alpine_feature(world, idx);
            let (line_offset, max_cover) = match feature {
                MountainFeature::Summit => (0.00, 0.75),
                MountainFeature::Ridge => (0.04, 0.48),
                MountainFeature::AlpineSlope => (0.10, 0.22),
                MountainFeature::None | MountainFeature::Foothill => (0.10, 0.22),
            };
            let snow_line = (world.sea_level + 0.26 + tile.temperature * 0.20 + line_offset)
                .min(world.sea_level + 0.56);
            (snow_line, 0.12, max_cover)
        }
        Biome::Foothills => {
            if tile.temperature > 0.28 || height_above_sea < 0.34 {
                return 0.0;
            }
            let snow_line =
                (world.sea_level + 0.34 + tile.temperature * 0.14).min(world.sea_level + 0.54);
            (snow_line, 0.12, 0.18)
        }
        Biome::Tundra | Biome::PolarDesert => {
            if tile.temperature > 0.16 || height_above_sea < 0.28 {
                return 0.0;
            }
            let snow_line =
                (world.sea_level + 0.32 + tile.temperature * 0.16).min(world.sea_level + 0.52);
            (snow_line, 0.14, 0.42)
        }
        _ => return 0.0,
    };

    ((tile.raw_elevation - snow_line) / melt_band).clamp(0.0, max_cover)
}

fn classify_alpine_feature(world: &World, idx: usize) -> MountainFeature {
    let tile = &world.tiles[idx];
    let (x, y) = world.coords(idx);
    let elevation = tile.raw_elevation;
    let height_above_sea = elevation - world.sea_level;
    let mut higher_neighbors = 0_u8;
    let mut lower_neighbors = 0_u8;
    let mut alpine_neighbors = 0_u8;
    let mut min_elev = elevation;
    let mut max_elev = elevation;

    for (nx, ny) in world.neighbors8(x, y) {
        let neighbor = &world.tiles[world.idx(nx, ny)];
        min_elev = min_elev.min(neighbor.raw_elevation);
        max_elev = max_elev.max(neighbor.raw_elevation);
        if matches!(neighbor.biome, Biome::Alpine) {
            alpine_neighbors += 1;
        }
        if neighbor.raw_elevation > elevation + 0.004 {
            higher_neighbors += 1;
        }
        if elevation > neighbor.raw_elevation + 0.012 {
            lower_neighbors += 1;
        }
    }

    let relief = max_elev - min_elev;
    if height_above_sea >= 0.38 && higher_neighbors == 0 && lower_neighbors >= 5 {
        MountainFeature::Summit
    } else if height_above_sea >= 0.34
        && alpine_neighbors >= 2
        && relief >= 0.03
        && higher_neighbors <= 3
    {
        MountainFeature::Ridge
    } else {
        MountainFeature::AlpineSlope
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Surface, Tile};

    fn one_tile_world(biome: Biome, elevation: f32, temperature: f32) -> World {
        let mut world = World::new(1, 1, 1, 0.50, 0);
        world.tiles[0] = Tile {
            raw_elevation: elevation,
            hydro_elevation: elevation,
            temperature,
            surface: Surface::Land,
            biome,
            ..Tile::default()
        };
        world
    }

    fn alpine_grid(center_elevation: f32, neighbor_elevations: [f32; 8]) -> World {
        let mut world = World::new(1, 3, 3, 0.50, 0);
        let coords = [
            (0, 0),
            (1, 0),
            (2, 0),
            (0, 1),
            (2, 1),
            (0, 2),
            (1, 2),
            (2, 2),
        ];

        for (i, (x, y)) in coords.into_iter().enumerate() {
            let idx = world.idx(x, y);
            world.tiles[idx] = Tile {
                raw_elevation: neighbor_elevations[i],
                hydro_elevation: neighbor_elevations[i],
                temperature: 0.04,
                surface: Surface::Land,
                biome: Biome::Alpine,
                ..Tile::default()
            };
        }

        let center = world.idx(1, 1);
        world.tiles[center] = Tile {
            raw_elevation: center_elevation,
            hydro_elevation: center_elevation,
            temperature: 0.04,
            surface: Surface::Land,
            biome: Biome::Alpine,
            ..Tile::default()
        };

        world
    }

    #[test]
    fn mountain_feature_marks_only_local_highs_as_summits() {
        let summit = alpine_grid(0.95, [0.82, 0.84, 0.83, 0.81, 0.86, 0.80, 0.85, 0.82]);
        let shoulder = alpine_grid(0.90, [0.91, 0.92, 0.89, 0.88, 0.86, 0.85, 0.84, 0.83]);
        let center = summit.idx(1, 1);

        assert_eq!(
            mountain_feature_for_tile(&summit, center),
            MountainFeature::Summit
        );
        assert_ne!(
            mountain_feature_for_tile(&shoulder, center),
            MountainFeature::Summit
        );
    }

    #[test]
    fn mountain_feature_keeps_non_mountain_tiles_out_of_mountain_layer() {
        let forest = one_tile_world(Biome::TemperateForest, 0.92, 0.06);
        let foothill = one_tile_world(Biome::Foothills, 0.82, 0.12);

        assert_eq!(mountain_feature_for_tile(&forest, 0), MountainFeature::None);
        assert_eq!(
            mountain_feature_for_tile(&foothill, 0),
            MountainFeature::Foothill
        );
    }

    #[test]
    fn snow_cover_does_not_whiten_vegetated_mountain_edges() {
        let forest = one_tile_world(Biome::TemperateForest, 0.92, 0.06);
        let grassland = one_tile_world(Biome::TemperateGrassland, 0.92, 0.06);
        let steppe = one_tile_world(Biome::Steppe, 0.92, 0.06);

        assert_eq!(permanent_snow_cover(&forest, 0), 0.0);
        assert_eq!(permanent_snow_cover(&grassland, 0), 0.0);
        assert_eq!(permanent_snow_cover(&steppe, 0), 0.0);
    }

    #[test]
    fn snow_cover_allows_limited_cold_high_foothill_snow() {
        let lower_foothill = one_tile_world(Biome::Foothills, 0.82, 0.04);
        let high_foothill = one_tile_world(Biome::Foothills, 0.92, 0.04);
        let warm_foothill = one_tile_world(Biome::Foothills, 0.96, 0.34);

        assert_eq!(permanent_snow_cover(&lower_foothill, 0), 0.0);
        assert!(permanent_snow_cover(&high_foothill, 0) > 0.0);
        assert!(permanent_snow_cover(&high_foothill, 0) <= 0.18);
        assert_eq!(permanent_snow_cover(&warm_foothill, 0), 0.0);
    }

    #[test]
    fn snow_cover_keeps_alpine_as_primary_permanent_snow_biome() {
        let alpine = one_tile_world(Biome::Alpine, 0.92, 0.04);
        let foothill = one_tile_world(Biome::Foothills, 0.92, 0.04);

        assert!(permanent_snow_cover(&alpine, 0) > permanent_snow_cover(&foothill, 0));
        assert!(permanent_snow_cover(&alpine, 0) <= 0.75);
    }
}
