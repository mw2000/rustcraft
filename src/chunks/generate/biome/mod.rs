mod desert;
mod foothills;
mod forest;
mod glacier;
mod jungle;
mod savanna;
mod taiga;

use bevy::color::{ColorToComponents, LinearRgba, Srgba};
use bevy::math::Vec3;

use super::plants::Plants;
use super::trees::TreeKind;
use crate::block::BlockType;
pub use desert::Desert;
pub use foothills::Foothills;
pub use forest::Forest;
pub use glacier::Glacier;
pub use jungle::Jungle;
pub use savanna::Savanna;
pub use taiga::Taiga;

// How hot and how wet a column is, each 0..1. See `NoiseGenerator::climate`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Climate {
    pub temperature: f32,
    pub humidity: f32,
}

impl Default for Climate {
    fn default() -> Self {
        Self {
            temperature: 0.5,
            humidity: 0.5,
        }
    }
}

// A column of the base terrain, before any biome has changed it.
#[derive(Debug, Copy, Clone)]
pub struct BaseColumn {
    pub x: f64,
    pub z: f64,
    // how wide the cell this column stands for is, in blocks: 1 at full detail, wider for
    // coarse LOD cells, which leave out what's too fine to show, see `fractal::resolved`
    pub cell: f64,
    pub height: f64,
    // how far inland, negative out to sea
    pub continentalness: f64,
    // where mountain ranges rise: they start at 0 and grow with it
    pub ruggedness: f64,
    pub climate: Climate,
}

// A biome takes over from the base terrain where the climate or lie of the land suits it:
// it can reshape the ground and swap the blocks the base terrain would have used.
pub trait Biome: Send + Sync {
    fn name(&self) -> &'static str;

    // The most this biome can hold on a column in this climate, 0..1, from the climate alone:
    // its strength wherever the climate alone decides it, and 1 where the climate can't rule it
    // out. What follows the climate rather than the ground (grass, plants) blends in with this.
    fn niche(&self, _climate: Climate) -> f64 {
        1.0
    }

    // How strongly this biome holds on a column, 0..1, never more than its niche. The ground it
    // shapes and its trees blend in with this strength, and it owns the column's blocks where
    // it's over 0.5.
    fn strength(&self, column: &BaseColumn) -> f64 {
        self.niche(column.climate)
    }

    // The column's height, given the base terrain's.
    fn height(&self, column: &BaseColumn) -> f64 {
        column.height
    }

    // The chance, 0..1, of a tree on each spot one could grow, or None to leave the base
    // terrain's. Trees only grow on grass.
    fn tree_density(&self) -> Option<f64> {
        None
    }

    // The kind of tree that grows where it owns the ground, given the tree's own roll 0..1, or
    // None for the base terrain's, which goes by the temperature.
    fn tree_kind(&self, _pick: f64) -> Option<TreeKind> {
        None
    }

    // Whether trees take root on `ground`, `above_sea` blocks up, where it owns the ground.
    fn roots(&self, ground: BlockType, _above_sea: i64) -> bool {
        ground == BlockType::Grass
    }

    // The colour of its grass, in sRGB, or None to leave the climate's.
    fn grass(&self) -> Option<[f32; 3]> {
        None
    }

    // The plants its grass grows, or None to leave the climate's.
    fn plants(&self) -> Option<Plants> {
        None
    }

    // The block to use where the base terrain would put `base`, `above_sea` blocks up, on
    // ground with the given gradient (see `terrain_block`).
    fn block(&self, base: BlockType, _above_sea: i64, _gradient: f64) -> BlockType {
        base
    }
}

// Which biome owns a column, as an index into `Biomes`, or the base terrain.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Hash)]
pub struct BiomeId(u8);

impl BiomeId {
    pub const BASE: BiomeId = BiomeId(0);
}

// The base terrain's scattering of lone trees, see `Biome::tree_density`.
pub(super) const BASE_TREE_DENSITY: f64 = 0.02;

// Every biome that can take over from the base terrain.
pub struct Biomes {
    biomes: Vec<Box<dyn Biome>>,
}

impl Biomes {
    pub fn new(seed: u32) -> Self {
        // the more particular first: where two hold equally, the earlier owns the column, and
        // its trees, grass and plants blend in over the later's
        Self::with(vec![
            Box::new(Glacier),
            Box::new(Desert::new(seed)),
            Box::new(Jungle),
            Box::new(Savanna),
            Box::new(Taiga),
            Box::new(Forest),
            Box::new(Foothills::new(seed)),
        ])
    }

    pub fn with(biomes: Vec<Box<dyn Biome>>) -> Self {
        assert!(biomes.len() < u8::MAX as usize);
        Self { biomes }
    }

    // The column's height, the biome owning it and its tree density, starting from the base terrain.
    pub fn shape(&self, column: &BaseColumn) -> (f64, BiomeId, f64) {
        let mut height = column.height;
        let mut trees = BASE_TREE_DENSITY;
        let (mut owner, mut strongest) = (BiomeId::BASE, 0.5);
        // the last first, so the first listed blend their trees in over the rest's
        for (i, biome) in self.biomes.iter().enumerate().rev() {
            let strength = biome.strength(column);
            if strength <= 0.0 {
                continue;
            }
            height += (biome.height(column) - column.height) * strength;
            if let Some(density) = biome.tree_density() {
                trees += (density - trees) * strength;
            }
            if strength > 0.5 && strength >= strongest {
                (owner, strongest) = (BiomeId(i as u8 + 1), strength);
            }
        }
        (height, owner, trees.clamp(0.0, 1.0))
    }

    /// The most tree density a column in this climate can have, without sampling its terrain.
    /// Blending towards each biome's density by its strength raises it by no more than the sum
    /// of what each could add at full strength, and strength is never more than niche.
    pub fn tree_bound(&self, climate: Climate) -> f64 {
        let gain: f64 = self
            .biomes
            .iter()
            .filter_map(|b| {
                let density = b.tree_density()?;
                Some((density - BASE_TREE_DENSITY).max(0.0) * b.niche(climate))
            })
            .sum();
        (BASE_TREE_DENSITY + gain).min(1.0)
    }

    /// The kind of tree `biome` grows, given the tree's own roll 0..1, or None for the base
    /// terrain's.
    pub fn tree_kind(&self, biome: BiomeId, pick: f64) -> Option<TreeKind> {
        self.owner(biome)?.tree_kind(pick)
    }

    /// Whether trees take root on `ground`, `above_sea` blocks up, in `biome`.
    pub fn roots(&self, biome: BiomeId, ground: BlockType, above_sea: i64) -> bool {
        match self.owner(biome) {
            Some(owner) => owner.roots(ground, above_sea),
            None => ground == BlockType::Grass,
        }
    }

    /// The linear colour of grass in this climate: the climate's own, drawn towards each
    /// biome's by how well the climate suits it, so it changes as smoothly as the climate does.
    pub fn grass_colour(&self, climate: Climate) -> Vec3 {
        let mut colour = climate_grass(climate);
        for biome in self.biomes.iter().rev() {
            if let Some(grass) = biome.grass() {
                let niche = biome.niche(climate);
                if niche > 0.0 {
                    colour = colour.lerp(Vec3::from(grass) / 255.0, niche as f32);
                }
            }
        }
        LinearRgba::from(Srgba::rgb(colour.x, colour.y, colour.z)).to_vec3()
    }

    /// The linear colour of leaves in this climate.
    pub fn leaf_colour(&self, climate: Climate) -> Vec3 {
        self.grass_tint(climate) * LEAF_TEXTURE_MEAN
    }

    /// What to multiply the grass texture by to draw `grass_colour`.
    pub fn grass_tint(&self, climate: Climate) -> Vec3 {
        self.grass_colour(climate) / GRASS_TEXTURE_MEAN
    }

    /// The plants grass grows in this climate, blended between biomes as `grass_colour` is.
    pub fn plants(&self, climate: Climate) -> Plants {
        let mut plants = Plants::of_climate(climate);
        for biome in self.biomes.iter().rev() {
            if let Some(theirs) = biome.plants() {
                let niche = biome.niche(climate);
                if niche > 0.0 {
                    plants = plants.lerp(theirs, niche);
                }
            }
        }
        plants
    }

    /// Every biome's name, which structures' placements choose from.
    pub fn names(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.biomes.iter().map(|b| b.name())
    }

    fn owner(&self, biome: BiomeId) -> Option<&dyn Biome> {
        biome.0.checked_sub(1).map(|i| &*self.biomes[i as usize])
    }

    // The biome's name, or None for the base terrain.
    pub fn name(&self, biome: BiomeId) -> Option<&'static str> {
        self.owner(biome).map(|b| b.name())
    }

    // The block `biome` uses in place of the base terrain's `base`.
    pub fn block(
        &self,
        biome: BiomeId,
        base: BlockType,
        above_sea: i64,
        gradient: f64,
    ) -> BlockType {
        match self.owner(biome) {
            Some(owner) => owner.block(base, above_sea, gradient),
            None => base,
        }
    }
}

// 0 at `edge0`, rising smoothly to 1 at `edge1`; `edge1` may be below `edge0` to fall instead.
#[inline]
pub(super) fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// Grass at the four corners of the climate square, in sRGB: hot and dry is parched
// yellow, hot and wet lush green, and the cold side duller versions of both.
const GRASS_HOT_DRY: [f32; 3] = [200.0, 170.0, 60.0];
const GRASS_HOT_WET: [f32; 3] = [40.0, 130.0, 12.0];
const GRASS_COLD_DRY: [f32; 3] = [140.0, 140.0, 60.0];
const GRASS_COLD_WET: [f32; 3] = [30.0, 96.0, 24.0];

// The mean linear value of the greyscale grass and leaves tiles in blocks.png, which vertex
// colours tint: leaves share grass's tint, and their darker tile makes them a deeper green.
const GRASS_TEXTURE_MEAN: f32 = 0.5;
const LEAF_TEXTURE_MEAN: f32 = 0.25;

// The sRGB colour, 0..1, of the base terrain's grass in this climate.
fn climate_grass(climate: Climate) -> Vec3 {
    let srgb = |c: [f32; 3]| Vec3::from(c) / 255.0;
    let (t, h) = (
        climate.temperature.clamp(0.0, 1.0),
        climate.humidity.clamp(0.0, 1.0),
    );
    let dry = srgb(GRASS_COLD_DRY).lerp(srgb(GRASS_HOT_DRY), t);
    let wet = srgb(GRASS_COLD_WET).lerp(srgb(GRASS_HOT_WET), t);
    dry.lerp(wet, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Raise(f64);

    impl Biome for Raise {
        fn name(&self) -> &'static str {
            "raise"
        }

        fn strength(&self, column: &BaseColumn) -> f64 {
            column.climate.temperature as f64
        }

        fn height(&self, column: &BaseColumn) -> f64 {
            column.height + self.0
        }

        fn block(&self, _base: BlockType, _above_sea: i64, _gradient: f64) -> BlockType {
            BlockType::Snow
        }
    }

    fn column(temperature: f32) -> BaseColumn {
        BaseColumn {
            x: 0.0,
            z: 0.0,
            cell: 1.0,
            height: 500.0,
            continentalness: 0.3,
            ruggedness: -1.0,
            climate: Climate {
                temperature,
                humidity: 0.5,
            },
        }
    }

    #[test]
    fn test_biome_height_blends_in_with_strength() {
        let biomes = Biomes::with(vec![Box::new(Raise(10.0))]);
        assert_eq!(
            (500.0, BiomeId::BASE, BASE_TREE_DENSITY),
            biomes.shape(&column(0.0))
        );
        assert_eq!(502.5, biomes.shape(&column(0.25)).0);
        assert_eq!(510.0, biomes.shape(&column(1.0)).0);
    }

    #[test]
    fn test_biome_owns_blocks_where_it_holds_over_half() {
        let biomes = Biomes::with(vec![Box::new(Raise(10.0))]);
        let owner = |t| biomes.shape(&column(t)).1;
        assert_eq!(
            BlockType::Grass,
            biomes.block(owner(0.4), BlockType::Grass, 10, 0.0)
        );
        assert_eq!(
            BlockType::Snow,
            biomes.block(owner(0.6), BlockType::Grass, 10, 0.0)
        );
    }

    #[test]
    fn test_strongest_biome_owns_the_column() {
        let biomes = Biomes::with(vec![Box::new(Raise(10.0)), Box::new(Desert::new(1))]);
        let hot_dry = BaseColumn {
            climate: Climate {
                temperature: 1.0,
                humidity: 0.0,
            },
            ..column(1.0)
        };
        let (_, owner, _) = biomes.shape(&hot_dry);
        // both hold fully, so the first listed keeps it
        assert_eq!(BiomeId(1), owner);
        assert_eq!(Some("raise"), biomes.name(owner));
        assert_eq!(None, biomes.name(BiomeId::BASE));
    }

    fn climate(temperature: f32, humidity: f32) -> Climate {
        Climate {
            temperature,
            humidity,
        }
    }

    // How much yellower than green, as red over green.
    fn yellowness(colour: Vec3) -> f32 {
        colour.x / colour.y
    }

    fn grass_colour(climate: Climate) -> Vec3 {
        Biomes::new(1).grass_colour(climate)
    }

    #[test]
    fn test_hot_dry_grass_is_yellower_than_wet() {
        let dry = grass_colour(climate(1.0, 0.0));
        let wet = grass_colour(climate(1.0, 1.0));
        assert!(
            yellowness(dry) > 0.8,
            "dry grass should be yellow, was {dry}"
        );
        assert!(
            yellowness(wet) < 0.2,
            "wet grass should be green, was {wet}"
        );
    }

    #[test]
    fn test_drier_and_hotter_is_always_yellower() {
        for step in 0..10 {
            let x = step as f32 / 10.0;
            let y = x + 0.1;
            assert!(
                yellowness(grass_colour(climate(0.5, x)))
                    > yellowness(grass_colour(climate(0.5, y)))
            );
            assert!(
                yellowness(grass_colour(climate(y, 0.3)))
                    > yellowness(grass_colour(climate(x, 0.3)))
            );
        }
    }

    #[test]
    fn test_tint_draws_grass_colour_from_texture() {
        let biomes = Biomes::new(1);
        let c = climate(0.3, 0.8);
        assert!(
            (biomes.grass_tint(c) * GRASS_TEXTURE_MEAN - biomes.grass_colour(c)).length() < 1e-6
        );
    }

    #[test]
    fn test_new_biomes_grass_stands_out_from_their_neighbours() {
        let brightness = |c: Vec3| c.x + c.y + c.z;
        // jungle is darker than the forest beside it, savanna yellower than the grass beside it,
        // and taiga darker than the open country beside it
        let (jungle, forest) = (
            grass_colour(climate(0.9, 0.9)),
            grass_colour(climate(0.5, 0.9)),
        );
        assert!(
            brightness(jungle) < brightness(forest) * 0.8,
            "{jungle} {forest}"
        );
        let (savanna, open) = (
            grass_colour(climate(0.8, 0.5)),
            grass_colour(climate(0.5, 0.5)),
        );
        assert!(
            yellowness(savanna) > yellowness(open) * 1.5,
            "{savanna} {open}"
        );
        let (taiga, open) = (
            grass_colour(climate(0.3, 0.5)),
            grass_colour(climate(0.5, 0.5)),
        );
        assert!(brightness(taiga) < brightness(open) * 0.6, "{taiga} {open}");
    }

    #[test]
    fn test_grass_colour_changes_smoothly_across_the_climate() {
        // a step a hundredth of the climate across, about fifty blocks, never changes it much:
        // biomes' ramps are a tenth of the climate wide or more, rising by up to 0.15 a step, and
        // move it most where savanna's gold gives way to jungle's green, so a change of grass
        // takes several hundred blocks
        let biomes = Biomes::new(1);
        for t in 0..100 {
            for h in 0..100 {
                let at = |t: i32, h: i32| {
                    biomes.grass_colour(climate(t as f32 / 100.0, h as f32 / 100.0))
                };
                let here = at(t, h);
                for next in [at(t + 1, h), at(t, h + 1)] {
                    assert!(
                        (here - next).length() < 0.075,
                        "{here} to {next} at {t}, {h}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_tree_bound_still_rules_out_most_spots_in_open_country_and_savanna() {
        // so coarse LODs skip sampling the terrain for most cells there, see `rules_out`
        let biomes = Biomes::new(1);
        for (t, h) in [(0.5, 0.5), (0.8, 0.5), (0.9, 0.1)] {
            let bound = biomes.tree_bound(climate(t, h));
            assert!(bound < 0.1, "{bound} at {t}, {h}");
        }
        for (t, h) in [(0.9, 0.9), (0.3, 0.6), (0.5, 0.9), (0.8, 0.5)] {
            let column = BaseColumn {
                climate: climate(t, h),
                ..column(0.0)
            };
            let (_, _, trees) = biomes.shape(&column);
            assert!(trees <= biomes.tree_bound(column.climate) + 1e-9);
        }
    }

    #[test]
    fn test_tree_density_blends_between_biomes_without_jumps() {
        let biomes = Biomes::new(1);
        let trees = |t: f32, h: f32| {
            biomes
                .shape(&BaseColumn {
                    climate: climate(t, h),
                    ..column(0.0)
                })
                .2
        };
        for t in 0..100 {
            for h in 0..100 {
                let (t, h) = (t as f32 / 100.0, h as f32 / 100.0);
                let here = trees(t, h);
                for next in [trees(t + 0.01, h), trees(t, h + 0.01)] {
                    assert!((here - next).abs() < 0.15, "{here} to {next} at {t}, {h}");
                }
                assert!(here <= biomes.tree_bound(climate(t, h)) + 1e-9);
            }
        }
    }
}
