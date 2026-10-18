use bevy::math::I64Vec3;

use super::trees::hash;
use super::WORLD_HEIGHT;
use crate::block::BlockType;
use crate::chunks::chunk::CHUNK_SIZE;

const SEED_OFFSET: u64 = 4000;

/// An ore, the band of heights its veins are found in, how many there are, and under which
/// climates.
struct Ore {
    block: BlockType,
    // the biomes it's found under, by name, or none for under every climate
    climates: &'static [&'static str],
    // the lowest and highest blocks its veins start at, thinning out over `FADE` blocks inside
    // each end
    bottom: i64,
    top: i64,
    // veins in each cell of the grid, on average, in the thick of the band
    veins: f64,
    // the fewest and most blocks in a vein, at most `MOST_BLOCKS`
    size: (usize, usize),
}

// The ores in the stone, by depth, with sea level at 448 and lowland ground a little over it.
// Coal starts at the surface, so the first dig finds it, and so do bare cliffs, and it goes
// down past the middle of the world. Copper starts just under the sea, a short dig down from
// lowland, and goes down through the middle depths. Iron starts fifty blocks or more under
// lowland ground and goes on to the bottom of the world, so it takes a deep dig or a walk down
// through the caves. A tunnel through the thick of the bands passes a vein of coal or copper
// on its walls every thirty blocks or so, and of iron every sixty, and caverns show far more.
//
// The far climates each have an ore of their own, for bright steel (§5.3): gold deep under the
// deserts and savannas, frost crystal under the glaciers and taiga from a little under the
// sea, and jade under the jungles. Their veins are a quarter as many as iron's but nearly three
// times as big, so they're rarer, and one found is worth the journey. Embers are deepest of
// all, in the bottom hundred blocks under every climate.
const ORES: [Ore; 7] = [
    Ore {
        block: BlockType::CoalOre,
        climates: &[],
        bottom: 192,
        top: WORLD_HEIGHT as i64,
        veins: 3.0,
        size: (3, 12),
    },
    Ore {
        block: BlockType::CopperOre,
        climates: &[],
        bottom: 128,
        top: 456,
        veins: 2.5,
        size: (3, 8),
    },
    Ore {
        block: BlockType::IronOre,
        climates: &[],
        bottom: 16,
        top: 392,
        veins: 1.5,
        size: (2, 6),
    },
    Ore {
        block: BlockType::GoldOre,
        climates: &["desert", "savanna"],
        bottom: 48,
        top: 288,
        veins: 0.4,
        size: (6, 16),
    },
    Ore {
        block: BlockType::FrostCrystalOre,
        climates: &["glacier", "taiga"],
        bottom: 192,
        top: 440,
        veins: 0.4,
        size: (6, 16),
    },
    Ore {
        block: BlockType::JadeOre,
        climates: &["jungle"],
        bottom: 160,
        top: 424,
        veins: 0.4,
        size: (6, 16),
    },
    Ore {
        block: BlockType::EmberOre,
        climates: &[],
        bottom: 16,
        top: 112,
        veins: 0.4,
        size: (4, 10),
    },
];

/// Whether `block` is one of the ores veins are made of.
#[cfg(test)]
pub fn is_ore(block: BlockType) -> bool {
    ore_climates(block).is_some()
}

/// The biomes `block`'s veins are found under, if it's an ore, or none for every climate.
#[cfg(test)]
pub fn ore_climates(block: BlockType) -> Option<&'static [&'static str]> {
    Some(ORES.iter().find(|ore| ore.block == block)?.climates)
}

impl Ore {
    // Whether it's found under a column of `biome`, the biome's name or None for the base
    // terrain.
    fn found_under(&self, biome: Option<&str>) -> bool {
        self.climates.is_empty() || biome.is_some_and(|name| self.climates.contains(&name))
    }
}

// Veins start in a grid of cells as big as chunks, a few in each, and grow from there a block
// at a time, staying within `REACH` blocks of where they started each way. That's less than a
// cell, so a chunk takes veins only from its own cell and those around it, and it's small, so
// few of their veins could reach it.
const CELL: i64 = CHUNK_SIZE as i64;
const REACH: i64 = 2;
// Room for the biggest vein, which fits easily in the box it grows in.
const MOST_BLOCKS: usize = 16;
// Where a cell's veins of an ore start comes from one hash, twelve bits each, so there's room
// for five, and a vein's box has a bit each in a `u128`, see `vein`.
const _: () = {
    assert!(REACH < CELL && (2 * REACH + 1).pow(3) <= 128);
    let mut i = 0;
    while i < ORES.len() {
        assert!(ORES[i].veins <= 5.0 && ORES[i].size.1 <= MOST_BLOCKS);
        i += 1;
    }
};
// Veins thin out over this many blocks inside each end of their band, so a band has no hard
// edge to find.
const FADE: f64 = 32.0;

/// Turns the stone of the chunk whose lowest block is `origin` to ore where veins run through
/// it, with `cells` in `ChunkData::from_cells` order, `ground` the top of the highest ground
/// over it and `biome` the name of the biome owning each of its columns, by its `x` and `z` in
/// the chunk, or None for the base terrain. Only for full detail: LODs don't draw underground.
pub fn place_ores(
    seed: u32,
    origin: I64Vec3,
    ground: i64,
    biome: impl Fn(i64, i64) -> Option<&'static str>,
    cells: &mut [BlockType],
) {
    let size = CHUNK_SIZE as i64;
    // stone is all under the ground
    if origin.y >= ground {
        return;
    }
    let seed = hash(seed as u64 ^ SEED_OFFSET, 0, 0);
    let (low, high) = (origin - REACH, origin + size + REACH);
    for cell in neighbours(origin.div_euclid(I64Vec3::splat(CELL))) {
        let cell_seed = hash(hash(seed, cell.x, cell.y), cell.z, 0);
        let bottom = cell.y * CELL;
        for (index, ore) in ORES.iter().enumerate() {
            if bottom + CELL <= ore.bottom || bottom >= ore.top {
                continue;
            }
            // where each of the cell's veins of this ore starts, in twelve bits each
            let starts = hash(cell_seed, index as i64, 0);
            let attempts = ore.veins.ceil() as u32;
            for attempt in 0..attempts {
                let at = |axis: u32| (starts >> (12 * attempt + 4 * axis) & 0xf) as i64;
                let start = cell * CELL + I64Vec3::new(at(0), at(1), at(2));
                if start.cmplt(low).any() || start.cmpge(high).any() {
                    continue;
                }
                let h = hash(starts, attempt as i64, 0);
                let into_band = (start.y - ore.bottom + 1).min(ore.top - start.y) as f64;
                if unit(h) >= ore.veins / attempts as f64 * (into_band / FADE).min(1.0) {
                    continue;
                }
                let (fewest, most) = ore.size;
                let blocks = fewest + (h >> 32) as usize % (most - fewest + 1);
                for block in vein(h, blocks) {
                    let local = start + block - origin;
                    if local.cmplt(I64Vec3::ZERO).any() || local.cmpge(I64Vec3::splat(size)).any() {
                        continue;
                    }
                    // a vein under the edge of its climate stops at the edge, column by column,
                    // which each chunk knows for its own
                    let at = &mut cells[((local.x * size + local.z) * size + local.y) as usize];
                    if *at == BlockType::Stone && ore.found_under(biome(local.x, local.z)) {
                        *at = ore.block;
                    }
                }
            }
        }
    }
}

// The cell `chunk` and the 26 around it.
fn neighbours(chunk: I64Vec3) -> impl Iterator<Item = I64Vec3> {
    (0..27).map(move |i| chunk + I64Vec3::new(i / 9 % 3, i / 3 % 3, i % 3) - 1)
}

const SIDES: [I64Vec3; 6] = [
    I64Vec3::X,
    I64Vec3::NEG_X,
    I64Vec3::Y,
    I64Vec3::NEG_Y,
    I64Vec3::Z,
    I64Vec3::NEG_Z,
];

// The low 28 bits of `h` as a number from 0 up to 1.
fn unit(h: u64) -> f64 {
    (h & 0xfff_ffff) as f64 / (1 << 28) as f64
}

// A vein of `blocks` blocks, as offsets from where it starts, grown a block at a time onto the
// side of one already in it, so it comes out a lumpy clump. Growth onto a block already in it,
// or out of reach, tries again, a bounded number of times.
fn vein(h: u64, blocks: usize) -> impl Iterator<Item = I64Vec3> {
    // which blocks of the box it grows in are in it, a bit each
    const ACROSS: i64 = 2 * REACH + 1;
    let bit = |offset: I64Vec3| {
        let corner = offset + REACH;
        1u128 << ((corner.x * ACROSS + corner.y) * ACROSS + corner.z)
    };
    let mut vein = [I64Vec3::ZERO; MOST_BLOCKS];
    let (mut grown, mut taken) = (1, bit(I64Vec3::ZERO));
    // each hash makes four draws of two bytes: the block grown onto, then which side of it
    let mut draws = 0;
    for step in 0..4 * blocks as i64 {
        if grown == blocks {
            break;
        }
        if step % 4 == 0 {
            draws = hash(h, step, 1);
        }
        let (onto, side) = (draws as u8 as usize, (draws >> 8) as u8 as usize);
        draws >>= 16;
        let next = vein[(onto * grown) >> 8] + SIDES[(side * 6) >> 8];
        if next.abs().max_element() <= REACH && taken & bit(next) == 0 {
            taken |= bit(next);
            vein[grown] = next;
            grown += 1;
        }
    }
    vein.into_iter().take(grown)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::chunks::chunk::{ChunkCoordinate, CELLS};
    use crate::chunks::generate::generator::generate_chunk;
    use crate::chunks::generate::noise::NoiseGenerator;
    use crate::chunks::generate::region_store::RegionStore;

    // Ores through solid stone under `biome` in the chunks at chunk `x`, `z` from the bottom of
    // the world to the top, a chunk's cells each.
    fn stone_column(seed: u32, x: i64, z: i64, biome: Option<&'static str>) -> Vec<Vec<BlockType>> {
        let size = CHUNK_SIZE as i64;
        (0..WORLD_HEIGHT as i64 / size)
            .map(|y| {
                let mut cells = vec![BlockType::Stone; CELLS];
                let origin = I64Vec3::new(x, y, z) * size;
                place_ores(seed, origin, i64::MAX, |_, _| biome, &mut cells);
                cells
            })
            .collect()
    }

    // Every biome there is, by name, and the base terrain.
    fn every_biome() -> Vec<Option<&'static str>> {
        let biomes = crate::chunks::generate::biome::Biomes::new(0);
        std::iter::once(None)
            .chain(biomes.names().map(Some))
            .collect()
    }

    // The block at `index` of a chunk's cells, from the chunk's lowest block.
    fn local(index: usize) -> I64Vec3 {
        let size = CHUNK_SIZE as i64;
        let index = index as i64;
        I64Vec3::new(index / size / size, index % size, index / size % size)
    }

    #[test]
    fn test_ores_come_out_the_same_whatever_was_generated_first() {
        let size = CHUNK_SIZE as i64;
        let noise = Arc::new(NoiseGenerator::new(3));
        let (spawn_x, spawn_z, _) = noise.find_spawn();
        let coords: Vec<_> = (-1..=1)
            .flat_map(|x| (18..29).flat_map(move |y| (-1..=1).map(move |z| (x, y, z))))
            .map(|(x, y, z)| {
                ChunkCoordinate(I64Vec3::new(spawn_x / size + x, y, spawn_z / size + z))
            })
            .collect();
        let generate = |order: &mut dyn Iterator<Item = &ChunkCoordinate>| {
            let regions = Arc::new(RegionStore::new(noise.clone()));
            let mut chunks: Vec<_> = order
                .map(|&coord| (coord.0.to_array(), generate_chunk(regions.clone(), coord)))
                .map(|(coord, chunk)| (coord, chunk.iter_non_air().collect::<Vec<_>>()))
                .collect();
            chunks.sort_by_key(|(coord, _)| *coord);
            chunks
        };
        let forwards = generate(&mut coords.iter());
        assert!(forwards == generate(&mut coords.iter().rev()));
        for ore in [BlockType::CoalOre, BlockType::CopperOre, BlockType::IronOre] {
            let found = forwards
                .iter()
                .flat_map(|(_, blocks)| blocks)
                .any(|&(_, block)| block == ore);
            assert!(found, "no {ore:?} under spawn");
        }
    }

    #[test]
    fn test_ores_only_replace_stone() {
        let others = [
            BlockType::Air,
            BlockType::Water,
            BlockType::Dirt,
            BlockType::Sand,
            BlockType::Sandstone,
            BlockType::Gravel,
            BlockType::Snow,
        ];
        let mut replaced = 0;
        for (i, origin) in (0..64).map(|i| (i, I64Vec3::new(i % 4, 20 + i / 16, i / 4 % 4) * 16)) {
            // every other block stone, the rest a mix of everything else
            let before: Vec<_> = (0..CELLS)
                .map(|c| {
                    if (c + i as usize).is_multiple_of(2) {
                        BlockType::Stone
                    } else {
                        others[c / 2 % others.len()]
                    }
                })
                .collect();
            let mut after = before.clone();
            // a mix of climates, column by column, so the regional ores come up too
            let biomes = every_biome();
            let biome = |x: i64, z: i64| biomes[(x + 3 * z) as usize % biomes.len()];
            place_ores(1, origin, i64::MAX, biome, &mut after);
            for (&was, &is) in before.iter().zip(&after) {
                if was != is {
                    assert_eq!(BlockType::Stone, was);
                    assert!(ORES.iter().any(|ore| ore.block == is));
                    replaced += 1;
                }
            }
        }
        assert!(replaced > 0);
    }

    #[test]
    fn test_ores_keep_to_their_depths() {
        let size = CHUNK_SIZE as i64;
        // ore blocks of each kind in each 64 blocks of height, under the base terrain
        let mut bands = [[0usize; ORES.len()]; WORLD_HEIGHT as usize / 64];
        for (x, z) in (0..64).map(|i| (i % 8 * 5, i / 8 * 7)) {
            for (y, cells) in stone_column(2, x, z, None).iter().enumerate() {
                for (index, block) in cells.iter().enumerate() {
                    if let Some(ore) = ORES.iter().position(|ore| ore.block == *block) {
                        bands[((y as i64 * size + local(index).y) / 64) as usize][ore] += 1;
                    }
                }
            }
        }
        // as a share of a band's 64 chunks' 64 x 16 x 16 blocks
        let share = |band: usize, ore: usize| bands[band][ore] as f64 / (64.0 * 64.0 * 256.0);
        for (band, counts) in bands.iter().enumerate() {
            let (bottom, top) = (band as i64 * 64, band as i64 * 64 + 64);
            for (ore, &count) in counts.iter().enumerate() {
                // nothing out of reach of its band
                if top <= ORES[ore].bottom - REACH || bottom >= ORES[ore].top + REACH {
                    assert_eq!(0, count, "{:?} at {bottom}", ORES[ore].block);
                }
            }
        }
        // in the thick of each band, coal is common, then copper, and iron rarer
        let thick = |ore: usize| {
            let Ore { bottom, top, .. } = ORES[ore];
            let fade = FADE as i64;
            let bands = ((bottom + fade + 63) / 64) as usize..((top - fade) / 64) as usize;
            bands.clone().map(|band| share(band, ore)).sum::<f64>() / bands.len() as f64
        };
        let [coal, copper, iron] = [0, 1, 2].map(thick);
        assert!((0.004..0.008).contains(&coal), "coal {coal}");
        assert!((0.002..0.005).contains(&copper), "copper {copper}");
        assert!((0.001..0.0025).contains(&iron), "iron {iron}");
        assert!(coal > copper && copper > iron);
        // and none of the far climates' ores
        for (ore, _) in ORES
            .iter()
            .enumerate()
            .filter(|(_, ore)| !ore.climates.is_empty())
        {
            assert!(bands.iter().all(|counts| counts[ore] == 0));
        }
    }

    #[test]
    fn test_regional_ores_come_up_only_under_their_climates_rarer_than_iron() {
        let size = CHUNK_SIZE as i64;
        for biome in every_biome() {
            // ore blocks of each kind, and how many are in the thick of their bands
            let (mut found, mut thick) = ([0usize; ORES.len()], [0usize; ORES.len()]);
            for (x, z) in (0..32).map(|i| (i % 8 * 3, i / 8 * 5)) {
                for (y, cells) in stone_column(7, x, z, biome).iter().enumerate() {
                    for (index, block) in cells.iter().enumerate() {
                        let Some(ore) = ORES.iter().position(|ore| ore.block == *block) else {
                            continue;
                        };
                        let height = y as i64 * size + local(index).y;
                        let Ore { bottom, top, .. } = ORES[ore];
                        assert!((bottom - REACH..top + REACH).contains(&height), "{block:?}");
                        found[ore] += 1;
                        let fade = FADE as i64;
                        thick[ore] += (bottom + fade..top - fade).contains(&height) as usize;
                    }
                }
            }
            for (ore, kind) in ORES.iter().enumerate() {
                assert_eq!(
                    kind.found_under(biome),
                    found[ore] > 0,
                    "{biome:?} {:?}",
                    kind.block
                );
            }
            // as a share of the stone in the thick of each band
            let share = |ore: usize| {
                let Ore { bottom, top, .. } = ORES[ore];
                thick[ore] as f64 / ((top - bottom) as f64 - 2.0 * FADE) / (32.0 * 256.0)
            };
            for ore in (3..ORES.len()).filter(|&ore| found[ore] > 0) {
                let rate = share(ore);
                let block = ORES[ore].block;
                assert!((0.0005..0.8 * share(2)).contains(&rate), "{block:?} {rate}");
            }
        }
    }

    #[test]
    fn test_every_climate_an_ore_is_found_under_is_a_biome() {
        let biomes = every_biome();
        for ore in &ORES {
            for name in ore.climates {
                assert!(biomes.contains(&Some(name)), "{name}");
            }
        }
    }

    #[test]
    fn test_ore_comes_in_veins() {
        let size = CHUNK_SIZE as i64;
        let (mut ore, mut beside_ore) = (0, 0);
        for cells in (0..16).flat_map(|i| stone_column(5, i, -i, Some("jungle"))) {
            for (index, &block) in cells.iter().enumerate() {
                if block == BlockType::Stone {
                    continue;
                }
                ore += 1;
                let beside = SIDES.iter().any(|side| {
                    let next = local(index) + side;
                    next.cmpge(I64Vec3::ZERO).all()
                        && next.cmplt(I64Vec3::splat(size)).all()
                        && cells[((next.x * size + next.z) * size + next.y) as usize] == block
                });
                beside_ore += beside as usize;
            }
        }
        assert!(ore > 1000, "only {ore} ore blocks");
        // veins cut by a chunk's edge can leave a block alone on its side of it
        assert!(
            beside_ore as f64 > 0.9 * ore as f64,
            "{beside_ore} of {ore}"
        );
    }
}
