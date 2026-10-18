use super::super::plants::Plants;
use super::super::trees::TreeKind;
use super::{smoothstep, Biome, Climate};

// The hottest of the wet climates, where forest gives way to jungle.
const HOT: (f64, f64) = (0.62, 0.72);
const WET: (f64, f64) = (0.6, 0.7);
const TREE_DENSITY: f64 = 0.85;
// The share of its trees that are giants over the rest, which are broadleaves under them.
const GIANTS: f64 = 0.6;
// Deep green grass, darker and lusher than the wettest forest's, and its leaves with it.
const GRASS: [f32; 3] = [22.0, 104.0, 16.0];
// Thick undergrowth, with ferns among the grass and few flowers in the shade.
const PLANTS: Plants = Plants {
    grass: 0.42,
    ferns: 0.16,
    flowers: 0.3,
};

// Giant trees crowded over thick undergrowth, in the hot, wet lowlands.
pub struct Jungle;

impl Biome for Jungle {
    fn name(&self) -> &'static str {
        "jungle"
    }

    fn niche(&self, climate: Climate) -> f64 {
        smoothstep(HOT.0, HOT.1, climate.temperature as f64)
            * smoothstep(WET.0, WET.1, climate.humidity as f64)
    }

    fn tree_density(&self) -> Option<f64> {
        Some(TREE_DENSITY)
    }

    fn tree_kind(&self, pick: f64) -> Option<TreeKind> {
        Some(if pick < GIANTS {
            TreeKind::Jungle
        } else {
            TreeKind::Broadleaf
        })
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
    fn test_jungles_grow_where_hot_and_wet_and_take_over_from_forest() {
        assert_eq!(1.0, Jungle.strength(&column(0.85, 0.85)));
        assert_eq!(0.0, Jungle.strength(&column(0.5, 0.85)));
        assert_eq!(0.0, Jungle.strength(&column(0.85, 0.5)));
        let biomes = Biomes::new(1);
        let (_, owner, jungle) = biomes.shape(&column(0.85, 0.85));
        assert_eq!(Some("jungle"), biomes.name(owner));
        let (_, owner, forest) = biomes.shape(&column(0.5, 0.85));
        assert_eq!(Some("forest"), biomes.name(owner));
        // its own density, not forest's piled on top of it
        assert!((jungle - TREE_DENSITY).abs() < 1e-9, "{jungle}");
        assert!(jungle > forest);
    }

    #[test]
    fn test_jungle_grows_giants_among_broadleaves() {
        assert_eq!(Some(TreeKind::Jungle), Jungle.tree_kind(0.1));
        assert_eq!(Some(TreeKind::Broadleaf), Jungle.tree_kind(0.9));
    }
}
