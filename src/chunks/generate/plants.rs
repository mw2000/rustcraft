use super::biome::{smoothstep, Climate};
use super::noise::NoiseGenerator;
use super::trees::hash;
use crate::block::BlockType;

const SEED_OFFSET: u64 = 5000;

// Ground steeper than this (see `terrain_block`) grows no plants, as it's too steep for trees.
const MAX_GRADIENT: f64 = 1.5;
// The share of grass with grasses on it, before tufts, in the lushest climates and in the
// least lush: dense where it's wet and warm, sparse where it's dry or cold.
const MOST_GRASS: f64 = 0.4;
const LEAST_GRASS: f64 = 0.04;
// The most of the grass any biome covers with grasses and ferns, before tufts, and the most a
// crown's shade adds in ferns.
const MOST_PLANTS: f64 = 0.6;
// Grasses grow in tufts and thin in clearings, this many blocks across.
const TUFT_SPACING: i64 = 5;
// Flowers grow in drifts about this far apart, each of one kind, where the drift noise is over
// `DRIFT_EDGE`, and as many as `MOST_FLOWERS` of the grass in the middle of a drift.
const DRIFT_SPACING: i64 = 14;
const DRIFT_EDGE: f64 = 0.75;
const MOST_FLOWERS: f64 = 0.5;
// Which kind a drift is changes over this many blocks, so neighbouring drifts are often alike.
const KIND_SPACING: i64 = 40;
const FLOWERS: [BlockType; 3] = [
    BlockType::Poppy,
    BlockType::Dandelion,
    BlockType::Cornflower,
];
// How much of the ground under a tree's crown, where it's wet enough, is ferns besides the
// biome's own, and the humidities they take to between none and that many.
const SHADE_FERNS: f64 = 0.25;
const SHADE_WET: (f64, f64) = (0.5, 0.7);
// How much of the grasses are short tufts, in the open and in the shade, and how much is wild
// grass gone to seed; the rest is tall grass.
const SHORT_SHARE: f64 = 0.45;
const SHADED_SHORT_SHARE: f64 = 0.65;
const WILD_SHARE: f64 = 0.15;
// The temperatures grasses and flowers thin out between, to few at the snow line (0.25, see
// `terrain_block`), as ferns don't.
const COLD: (f64, f64) = (0.22, 0.38);

/// What grows on a climate's grass: the shares of it with tall grass and with ferns, before
/// tufts, and how much of each patch of flowers blooms. Biomes have their own, see
/// `Biomes::plants`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Plants {
    pub grass: f64,
    pub ferns: f64,
    pub flowers: f64,
}

impl Plants {
    /// The base terrain's, lusher the wetter and warmer it is. Flowers want some lushness, but
    /// less than grass does.
    pub fn of_climate(climate: Climate) -> Self {
        let lushness = lushness(climate);
        Self {
            grass: LEAST_GRASS + (MOST_GRASS - LEAST_GRASS) * lushness,
            ferns: 0.0,
            flowers: lushness.sqrt(),
        }
    }

    pub fn lerp(self, other: Plants, t: f64) -> Self {
        let mix = |a: f64, b: f64| a + (b - a) * t;
        Self {
            grass: mix(self.grass, other.grass),
            ferns: mix(self.ferns, other.ferns),
            flowers: mix(self.flowers, other.flowers),
        }
    }
}

/// The plant on the grass of column `x`, `z`, if one grows there: decided from the column
/// alone (INV-3), by its hash and the climate. `ground` is the column's top block and
/// `gradient` its slope, and `shaded` says whether a tree's crown is over it, asked only if it
/// could matter. Only for full detail: LODs don't draw plants.
pub fn plant_on(
    noise: &NoiseGenerator,
    x: i64,
    z: i64,
    ground: BlockType,
    gradient: f64,
    shaded: impl FnOnce() -> bool,
) -> Option<BlockType> {
    if ground != BlockType::Grass || gradient > MAX_GRADIENT {
        return None;
    }
    let seed = noise.seed() as u64 + SEED_OFFSET;
    let chance = unit(hash(seed, x, z));
    let flowers =
        MOST_FLOWERS * smoothstep(DRIFT_EDGE, 1.0, value_noise(seed + 1, x, z, DRIFT_SPACING));
    let tufts = 0.4 + 1.2 * value_noise(seed + 2, x, z, TUFT_SPACING);
    // most columns have nothing, and leave before sampling the climate
    if chance >= flowers.max((MOST_PLANTS + SHADE_FERNS) * tufts) {
        return None;
    }
    let climate = noise.climate(x as f64, z as f64);
    let plants = noise.biomes().plants(climate);
    let warm = smoothstep(COLD.0, COLD.1, climate.temperature as f64);
    if chance < flowers * plants.flowers * warm {
        // one kind to a drift, its middle's, the grid point nearest
        let middle = |c: i64| (c + DRIFT_SPACING / 2).div_euclid(DRIFT_SPACING) * DRIFT_SPACING;
        let kind = value_noise(seed + 3, middle(x), middle(z), KIND_SPACING);
        // the noise gathers round the middle, so this splits it into thirds
        let i = (kind > 0.43) as usize + (kind > 0.57) as usize;
        return Some(FLOWERS[i]);
    }
    let grass = plants.grass * tufts * warm;
    let ferns = plants.ferns * tufts;
    let shade_ferns =
        SHADE_FERNS * tufts * smoothstep(SHADE_WET.0, SHADE_WET.1, climate.humidity as f64);
    if chance >= grass + ferns + shade_ferns {
        return None;
    }
    // the crown's shade gives ferns room, and shorter grass
    let shaded = shaded();
    if chance >= grass + ferns + if shaded { shade_ferns } else { 0.0 } {
        return None;
    }
    if chance >= grass {
        return Some(BlockType::Fern);
    }
    let kind = unit(hash(seed + 4, x, z));
    let short = if shaded {
        SHADED_SHORT_SHARE
    } else {
        SHORT_SHARE
    };
    Some(if kind < short {
        BlockType::ShortGrass
    } else if kind >= 1.0 - WILD_SHARE {
        BlockType::WildGrass
    } else {
        BlockType::TallGrass
    })
}

// How lush a climate is, 0..1: wet and warm.
fn lushness(climate: Climate) -> f64 {
    smoothstep(0.25, 0.65, climate.humidity as f64)
        * smoothstep(0.15, 0.4, climate.temperature as f64)
}

// A hash as 0..1.
fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1u64 << 53) as f64
}

// Noise 0..1 changing smoothly over `spacing` blocks: random values on a grid that far apart,
// blended between. Whole-block arithmetic, so it's the same on every machine (INV-2).
fn value_noise(seed: u64, x: i64, z: i64, spacing: i64) -> f64 {
    let (cx, cz) = (x.div_euclid(spacing), z.div_euclid(spacing));
    let ease = |c: i64| smoothstep(0.0, 1.0, c.rem_euclid(spacing) as f64 / spacing as f64);
    let (tx, tz) = (ease(x), ease(z));
    let at = |dx, dz| unit(hash(seed, cx + dx, cz + dz));
    let near = at(0, 0) + (at(1, 0) - at(0, 0)) * tx;
    let far = at(0, 1) + (at(1, 1) - at(0, 1)) * tx;
    near + (far - near) * tz
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRASSES: [BlockType; 3] = [
        BlockType::TallGrass,
        BlockType::ShortGrass,
        BlockType::WildGrass,
    ];

    // Every plant over a square of grass `across` blocks wide from `x`, `z`, on gentle ground,
    // all of it under a tree's crown or none.
    fn grown(
        noise: &NoiseGenerator,
        x: i64,
        z: i64,
        across: i64,
        shaded: bool,
    ) -> Vec<Option<BlockType>> {
        (x..x + across)
            .flat_map(|x| (z..z + across).map(move |z| (x, z)))
            .map(|(x, z)| plant_on(noise, x, z, BlockType::Grass, 0.2, || shaded))
            .collect()
    }

    fn plants(noise: &NoiseGenerator, x: i64, z: i64, across: i64) -> Vec<Option<BlockType>> {
        grown(noise, x, z, across, false)
    }

    // Where a climate holds, near the origin of a world of seed 3, in blocks, if anywhere.
    fn find(noise: &NoiseGenerator, holds: impl Fn(Climate) -> bool) -> Option<(i64, i64)> {
        (0..400)
            .flat_map(|i| (0..400).map(move |j| (i * 64, j * 64)))
            .find(|&(x, z)| holds(noise.climate(x as f64, z as f64)))
    }

    #[test]
    fn test_plants_grow_the_same_every_time_and_only_on_gentle_grass() {
        let noise = NoiseGenerator::new(3);
        let grown = plants(&noise, 100, -40, 64);
        assert_eq!(grown, plants(&noise, 100, -40, 64));
        assert!(grown.iter().any(Option::is_some));
        for x in 100..164 {
            for z in -40..24 {
                for ground in [BlockType::Sand, BlockType::Snow, BlockType::Dirt] {
                    assert_eq!(None, plant_on(&noise, x, z, ground, 0.2, || true));
                }
                assert_eq!(None, plant_on(&noise, x, z, BlockType::Grass, 2.0, || true));
            }
        }
        // another world grows others
        assert_ne!(grown, plants(&NoiseGenerator::new(4), 100, -40, 64));
    }

    #[test]
    fn test_wet_warm_grass_is_lusher_than_dry_or_cold() {
        let climate = |temperature, humidity| Climate {
            temperature,
            humidity,
        };
        let lush = lushness(climate(0.6, 0.8));
        assert_eq!(1.0, lush);
        assert!(lushness(climate(0.6, 0.3)) < 0.1 * lush);
        assert_eq!(0.0, lushness(climate(0.1, 0.8)));
    }

    #[test]
    fn test_flowers_are_rarer_than_grasses_and_grow_in_drifts() {
        let noise = NoiseGenerator::new(3);
        let grown: Vec<_> = (0..16)
            .flat_map(|i| plants(&noise, i * 1000, i * 700, 64))
            .flatten()
            .collect();
        let count = |plant| grown.iter().filter(|&&p| p == plant).count();
        let flowers: usize = FLOWERS.iter().map(|&f| count(f)).sum();
        assert!(flowers > 0, "no flowers");
        let grasses: usize = GRASSES.iter().map(|&g| count(g)).sum();
        assert!(grasses > 4 * flowers);
        for plant in FLOWERS.into_iter().chain(GRASSES) {
            assert!(count(plant) > 0, "no {plant:?}");
        }
        // short tufts are the most of the grasses, and wild grass gone to seed the fewest
        let (short, tall, wild) = (
            count(BlockType::ShortGrass),
            count(BlockType::TallGrass),
            count(BlockType::WildGrass),
        );
        assert!(short > tall && tall > wild, "{short} {tall} {wild}");
        // most of a drift's neighbours are bare of flowers, as drifts are far apart
        let drift = |x, z| smoothstep(DRIFT_EDGE, 1.0, value_noise(1, x, z, DRIFT_SPACING)) > 0.0;
        let drifted = (0..256)
            .flat_map(|x| (0..256).map(move |z| (x, z)))
            .filter(|&(x, z)| drift(x, z))
            .count();
        assert!(drifted < 256 * 256 / 5, "{drifted}");
    }

    #[test]
    fn test_a_drift_is_one_kind_of_flower() {
        let noise = NoiseGenerator::new(3);
        let middle = |c: i64| (c + DRIFT_SPACING / 2).div_euclid(DRIFT_SPACING);
        let mut drifts = std::collections::HashMap::new();
        for i in 0..16 {
            let (x0, z0) = (i * 1000, i * 700);
            for x in x0..x0 + 64 {
                for z in z0..z0 + 64 {
                    match plant_on(&noise, x, z, BlockType::Grass, 0.2, || false) {
                        Some(flower) if FLOWERS.contains(&flower) => {
                            let kind = drifts.entry((middle(x), middle(z))).or_insert(flower);
                            assert_eq!(*kind, flower, "at {x}, {z}");
                        }
                        _ => {}
                    }
                }
            }
        }
        assert!(drifts.len() > 3, "{} drifts", drifts.len());
    }

    #[test]
    fn test_crowns_shade_in_ferns_and_short_grass_where_its_wet() {
        let noise = NoiseGenerator::new(3);
        let count = |plants: &[Option<BlockType>], kind| {
            plants.iter().filter(|&&p| p == Some(kind)).count()
        };
        let wet = |c: Climate| c.humidity > 0.75 && c.temperature > 0.45;
        let (x, z) = find(&noise, wet).expect("nowhere wet");
        let (open, shade) = (
            grown(&noise, x, z, 48, false),
            grown(&noise, x, z, 48, true),
        );
        assert!(
            count(&shade, BlockType::Fern) > count(&open, BlockType::Fern) + 50,
            "{} ferns in the shade, {} in the open",
            count(&shade, BlockType::Fern),
            count(&open, BlockType::Fern)
        );
        let short = |plants: &[Option<BlockType>]| {
            count(plants, BlockType::ShortGrass) as f64
                / GRASSES.iter().map(|&g| count(plants, g)).sum::<usize>() as f64
        };
        assert!(short(&shade) > short(&open) + 0.1);
        // and everything that grew in the open grows in the shade too, the grasses perhaps
        // shorter
        for (open, shade) in open.iter().zip(&shade) {
            match open {
                Some(grass) if GRASSES.contains(grass) => {
                    assert!(GRASSES.contains(&shade.unwrap()));
                }
                Some(_) => assert_eq!(open, shade),
                None => {}
            }
        }
        // where it's dry, shade is no different, as ferns won't grow
        let dry = |c: Climate| c.humidity < 0.4 && c.temperature > 0.5;
        let (x, z) = find(&noise, dry).expect("nowhere dry");
        let kinds = |plants: Vec<Option<BlockType>>| {
            plants
                .into_iter()
                .map(|p| p.filter(|p| *p == BlockType::Fern))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            kinds(grown(&noise, x, z, 48, false)),
            kinds(grown(&noise, x, z, 48, true))
        );
    }

    #[test]
    fn test_grasses_and_flowers_thin_out_towards_the_snow_but_ferns_dont() {
        let noise = NoiseGenerator::new(3);
        let chill = |c: Climate| (0.25..0.27).contains(&c.temperature) && c.humidity > 0.4;
        let mild = |c: Climate| (0.5..0.6).contains(&c.temperature) && c.humidity > 0.4;
        let count = |(x, z): (i64, i64)| {
            plants(&noise, x - 32, z - 32, 64)
                .into_iter()
                .flatten()
                .filter(|p| *p != BlockType::Fern)
                .count()
        };
        let (cold, warm) = (
            count(find(&noise, chill).expect("nowhere chilly")),
            count(find(&noise, mild).expect("nowhere mild")),
        );
        assert!(
            cold * 4 < warm,
            "{cold} near the snow, {warm} where it's mild"
        );
        assert_eq!(0.0, smoothstep(COLD.0, COLD.1, 0.2));
    }

    #[test]
    fn test_biomes_grow_their_own_plants_within_the_bound() {
        let biomes = crate::chunks::generate::biome::Biomes::new(1);
        let climate = |temperature, humidity| Climate {
            temperature,
            humidity,
        };
        let jungle = biomes.plants(climate(0.9, 0.9));
        let savanna = biomes.plants(climate(0.8, 0.5));
        let taiga = biomes.plants(climate(0.3, 0.6));
        let forest = biomes.plants(climate(0.5, 0.9));
        let open = biomes.plants(climate(0.5, 0.5));
        assert!(jungle.ferns > 0.1 && taiga.ferns > 0.1 && forest.ferns > 0.05);
        assert_eq!(0.0, savanna.ferns + open.ferns);
        assert!(savanna.flowers > Plants::of_climate(climate(0.8, 0.5)).flowers);
        assert!(savanna.grass > open.grass && taiga.grass < open.grass);
        assert!(forest.grass < Plants::of_climate(climate(0.5, 0.9)).grass);
        // `plant_on` leaves early where no climate could grow anything
        for t in 0..=20 {
            for h in 0..=20 {
                let plants = biomes.plants(climate(t as f32 / 20.0, h as f32 / 20.0));
                assert!(plants.grass + plants.ferns <= MOST_PLANTS);
                assert!(plants.flowers <= 1.0);
            }
        }
    }

    #[test]
    fn test_value_noise_is_smooth_and_in_range() {
        for x in -50..50 {
            let (a, b) = (value_noise(7, x, 3, 10), value_noise(7, x + 1, 3, 10));
            assert!((0.0..=1.0).contains(&a));
            assert!((a - b).abs() < 0.25, "{a} to {b} at {x}");
        }
    }
}
