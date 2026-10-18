use super::biome::{smoothstep, Climate};
use super::noise::NoiseGenerator;
use super::trees::hash;
use crate::block::BlockType;

const SEED_OFFSET: u64 = 5000;

// Ground steeper than this (see `terrain_block`) grows no plants, as it's too steep for trees.
const MAX_GRADIENT: f64 = 1.5;
// The share of grass with tall grass on it, before tufts, in the lushest climates and in the
// least lush: dense where it's wet and warm, sparse where it's dry or cold.
const MOST_GRASS: f64 = 0.4;
const LEAST_GRASS: f64 = 0.04;
// Tall grass grows in tufts and thins in clearings, this many blocks across.
const TUFT_SPACING: i64 = 5;
// Flowers grow in patches about this far apart, each of one kind, where the patch noise is
// over `PATCH_EDGE`, and as many as `MOST_FLOWERS` of the grass in the middle of a patch.
const PATCH_SPACING: i64 = 14;
const PATCH_EDGE: f64 = 0.75;
const MOST_FLOWERS: f64 = 0.5;
// Which kind a patch is changes over this many blocks, so neighbouring patches are often alike.
const KIND_SPACING: i64 = 40;
const FLOWERS: [BlockType; 3] = [
    BlockType::Poppy,
    BlockType::Dandelion,
    BlockType::Cornflower,
];

/// The plant on the grass of column `x`, `z`, if one grows there: decided from the column
/// alone (INV-3), by its hash and the climate. `ground` is the column's top block and
/// `gradient` its slope. Only for full detail: LODs don't draw plants.
pub fn plant_on(
    noise: &NoiseGenerator,
    x: i64,
    z: i64,
    ground: BlockType,
    gradient: f64,
) -> Option<BlockType> {
    if ground != BlockType::Grass || gradient > MAX_GRADIENT {
        return None;
    }
    let seed = noise.seed() as u64 + SEED_OFFSET;
    let chance = unit(hash(seed, x, z));
    let flowers =
        MOST_FLOWERS * smoothstep(PATCH_EDGE, 1.0, value_noise(seed + 1, x, z, PATCH_SPACING));
    let tufts = 0.4 + 1.2 * value_noise(seed + 2, x, z, TUFT_SPACING);
    // most columns have nothing, and leave before sampling the climate
    if chance >= flowers.max(MOST_GRASS * tufts) {
        return None;
    }
    let lushness = lushness(noise.climate(x as f64, z as f64));
    // flowers want some lushness, but less than grass does
    if chance < flowers * lushness.sqrt() {
        let kind = value_noise(seed + 3, x, z, KIND_SPACING);
        // the noise gathers round the middle, so this splits it into thirds
        let i = (kind > 0.43) as usize + (kind > 0.57) as usize;
        return Some(FLOWERS[i]);
    }
    let cover = LEAST_GRASS + (MOST_GRASS - LEAST_GRASS) * lushness;
    (chance < cover * tufts).then_some(BlockType::TallGrass)
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

    // Every plant over a square of grass `across` blocks wide from `x`, `z`, on gentle ground.
    fn plants(noise: &NoiseGenerator, x: i64, z: i64, across: i64) -> Vec<Option<BlockType>> {
        (x..x + across)
            .flat_map(|x| (z..z + across).map(move |z| (x, z)))
            .map(|(x, z)| plant_on(noise, x, z, BlockType::Grass, 0.2))
            .collect()
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
                    assert_eq!(None, plant_on(&noise, x, z, ground, 0.2));
                }
                assert_eq!(None, plant_on(&noise, x, z, BlockType::Grass, 2.0));
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
    fn test_flowers_are_rarer_than_tall_grass_and_grow_in_patches() {
        let noise = NoiseGenerator::new(3);
        let grown: Vec<_> = (0..16)
            .flat_map(|i| plants(&noise, i * 1000, i * 700, 64))
            .flatten()
            .collect();
        let count = |plant| grown.iter().filter(|&&p| p == plant).count();
        let flowers: usize = FLOWERS.iter().map(|&f| count(f)).sum();
        assert!(flowers > 0, "no flowers");
        assert!(count(BlockType::TallGrass) > 4 * flowers);
        for flower in FLOWERS {
            assert!(count(flower) > 0, "no {flower:?}");
        }
        // most of a patch's neighbours are bare of flowers, as patches are far apart
        let patch = |x, z| smoothstep(PATCH_EDGE, 1.0, value_noise(1, x, z, PATCH_SPACING)) > 0.0;
        let patched = (0..256)
            .flat_map(|x| (0..256).map(move |z| (x, z)))
            .filter(|&(x, z)| patch(x, z))
            .count();
        assert!(patched < 256 * 256 / 5, "{patched}");
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
