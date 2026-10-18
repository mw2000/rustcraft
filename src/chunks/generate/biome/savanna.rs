use super::super::plants::Plants;
use super::super::trees::TreeKind;
use super::{smoothstep, Biome, Climate};

// As hot as desert, but between it and the forests: from where the desert's sand gives way,
// at the same humidity, to where the grass turns lush.
const HOT: (f64, f64) = (0.55, 0.65);
const DAMP: (f64, f64) = (0.35, 0.45);
const WET: (f64, f64) = (0.66, 0.54);
// Lone trees well apart, though more of them than open country's.
const TREE_DENSITY: f64 = 0.06;
// Golden grass, yellower than the climate's own, which only turns so yellow in desert.
const GRASS: [f32; 3] = [188.0, 168.0, 70.0];
// Long grass all over, and flowers thick in their patches.
const PLANTS: Plants = Plants {
    grass: 0.45,
    ferns: 0.0,
    flowers: 1.0,
};

// Open grassland under flat-topped trees, in hot climates too wet for desert and too dry for
// forest.
pub struct Savanna;

impl Biome for Savanna {
    fn name(&self) -> &'static str {
        "savanna"
    }

    fn niche(&self, climate: Climate) -> f64 {
        let humidity = climate.humidity as f64;
        smoothstep(HOT.0, HOT.1, climate.temperature as f64)
            * smoothstep(DAMP.0, DAMP.1, humidity)
            * smoothstep(WET.0, WET.1, humidity)
    }

    fn tree_density(&self) -> Option<f64> {
        Some(TREE_DENSITY)
    }

    fn tree_kind(&self, _pick: f64) -> Option<TreeKind> {
        Some(TreeKind::Acacia)
    }

    fn grass(&self) -> Option<[f32; 3]> {
        Some(GRASS)
    }

    fn plants(&self) -> Option<Plants> {
        Some(PLANTS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::generate::biome::{BaseColumn, Biomes, Desert};

    fn column(temperature: f32, humidity: f32) -> BaseColumn {
        BaseColumn {
            x: 0.0,
            z: 0.0,
            cell: 1.0,
            height: 500.0,
            continentalness: 0.3,
            ruggedness: -1.0,
            climate: Climate {
                temperature,
                humidity,
            },
        }
    }

    #[test]
    fn test_savanna_lies_between_desert_and_forest_in_the_heat() {
        let biomes = Biomes::new(1);
        let owner = |t, h| biomes.name(biomes.shape(&column(t, h)).1);
        assert_eq!(Some("desert"), owner(0.8, 0.2));
        assert_eq!(Some("savanna"), owner(0.8, 0.5));
        assert_eq!(Some("jungle"), owner(0.8, 0.8));
        assert_eq!(None, owner(0.45, 0.5));
    }

    #[test]
    fn test_savanna_takes_over_from_desert_without_a_gap() {
        let desert = Desert::new(1);
        for step in 0..=20 {
            let humidity = 0.3 + step as f32 * 0.01;
            let c = column(0.8, humidity);
            let sum = desert.strength(&c) + Savanna.strength(&c);
            assert!((sum - 1.0).abs() < 1e-9, "{sum} at humidity {humidity}");
        }
    }
}
