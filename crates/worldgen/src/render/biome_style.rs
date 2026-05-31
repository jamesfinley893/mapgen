use crate::Biome;

pub(super) fn coastal_hinterland_biome(temperature: f32, moisture: f32) -> Biome {
    if temperature < 0.12 {
        if moisture < 0.35 {
            Biome::PolarDesert
        } else {
            Biome::Tundra
        }
    } else if temperature < 0.28 {
        if moisture < 0.30 {
            Biome::Steppe
        } else {
            Biome::BorealForest
        }
    } else if temperature < 0.48 {
        if moisture < 0.18 {
            Biome::Desert
        } else if moisture < 0.29 {
            Biome::Steppe
        } else if moisture < 0.43 {
            Biome::TemperateGrassland
        } else if moisture < 0.58 {
            Biome::Woodland
        } else {
            Biome::TemperateForest
        }
    } else if temperature < 0.72 {
        if moisture < 0.16 {
            Biome::Desert
        } else if moisture < 0.26 {
            Biome::Steppe
        } else if moisture < 0.48 {
            Biome::Savanna
        } else if moisture < 0.62 {
            Biome::Woodland
        } else {
            Biome::TropicalForest
        }
    } else if moisture < 0.16 {
        Biome::Desert
    } else if moisture < 0.46 {
        Biome::Savanna
    } else if moisture < 0.68 {
        Biome::TropicalForest
    } else {
        Biome::Rainforest
    }
}
