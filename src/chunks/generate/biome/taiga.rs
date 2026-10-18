use super::super::plants::Plants;
use super::super::trees::TreeKind;
use super::{smoothstep, Biome, Climate};
use crate::block::BlockType;

// The cold short of the ice, where glaciers give way to forest, up to where pines give way to
// broadleaves, and anywhere not dry. Its colder half is under snow, see `SNOWY_GRASS_TEMPERATURE`.
const ICE: (f64, f64) = (0.08, 0.2);
const MILD: (f64, f64) = (0.45, 0.33);
const DRY: (f64, f64) = (0.28, 0.42);
const TREE_DENSITY: f64 = 0.6;
// Pines take root in snow up to this many blocks over the sea, above which the heights are bare.
const TREE_LINE: i64 = 140;
// Dark, cool green grass, which its pines take after.
const GRASS: [f32; 3] = [44.0, 88.0, 50.0];
// Ferns and thin grass in the pines' shade, and few flowers.
const PLANTS: Plants = Plants {
    grass: 0.12,
    ferns: 0.14,
    flowers: 0.25,
};

// Dense pine forest over the cold lands short of the ice, under snow where it's coldest.
pub struct Taiga;

impl Biome for Taiga {
    fn name(&self) -> &'static str {
        "taiga"
    }

    fn niche(&self, climate: Climate) -> f64 {
        let temperature = climate.temperature as f64;
        smoothstep(ICE.0, ICE.1, temperature)
            * smoothstep(MILD.0, MILD.1, temperature)
            * smoothstep(DRY.0, DRY.1, climate.humidity as f64)
    }

    fn tree_density(&self) -> Option<f64> {
        Some(TREE_DENSITY)
    }

    fn tree_kind(&self, _pick: f64) -> Option<TreeKind> {
        Some(TreeKind::Pine)
    }

    fn roots(&self, ground: BlockType, above_sea: i64) -> bool {
        ground == BlockType::Grass || ground == BlockType::Snow && above_sea < TREE_LINE
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
    use crate::chunks::generate::biome::{BaseColumn, Biomes};

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
    fn test_taiga_lies_between_the_ice_and_the_temperate_forests() {
        let biomes = Biomes::new(1);
        let owner = |t, h| biomes.name(biomes.shape(&column(t, h)).1);
        assert_eq!(Some("glacier"), owner(0.05, 0.6));
        assert_eq!(Some("taiga"), owner(0.25, 0.6));
        assert_eq!(Some("taiga"), owner(0.25, 0.9));
        assert_eq!(Some("forest"), owner(0.5, 0.9));
        assert_eq!(None, owner(0.25, 0.2));
    }

    #[test]
    fn test_taiga_trees_root_in_snow_below_the_tree_line() {
        assert!(Taiga.roots(BlockType::Grass, 300));
        assert!(Taiga.roots(BlockType::Snow, 20));
        assert!(!Taiga.roots(BlockType::Snow, TREE_LINE));
        assert!(!Taiga.roots(BlockType::Sand, 1));
        assert!(!Taiga.roots(BlockType::Ice, 20));
    }
}
