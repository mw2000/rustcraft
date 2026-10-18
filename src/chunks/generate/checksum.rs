//! A checksum of what a seed generates, which a client joining a server compares with the
//! server's, so the two never play on different worlds (D-3, §4.4).

use std::sync::Arc;

use bevy::math::{I64Vec2, I64Vec3, Vec3Swizzles};

use super::generator::generate_chunk;
use super::noise::NoiseGenerator;
use super::region_store::RegionStore;
use crate::block::BlockType;
use crate::chunks::chunk::{ChunkCoordinate, ChunkData, CELLS, CHUNK_SIZE};

// Columns of chunks sampled besides the spawn's, in blocks: thousands of blocks out in each
// direction, so floating point has had room to drift, the third far enough north to be cold
// and in taiga, and the last in jungle, as the spawn is in savanna.
const FAR_COLUMNS: [I64Vec2; 4] = [
    I64Vec2::new(2_900, -1_700),
    I64Vec2::new(-7_400, 5_300),
    I64Vec2::new(11_976, 12_500),
    I64Vec2::new(-2_112, -2_112),
];
// The chunk layers sampled in each column besides those at, over and just under its ground: a
// cavern's depth, near the bottom where ores are richest, and halfway between.
const DEEP_LAYERS: [i64; 3] = [1, 6, 20];
// The nearest structure to the spawn within this many blocks each way is sampled too, its floor
// and the chunk over it, so the structures a world builds are checked like its terrain.
const STRUCTURE_SEARCH: i64 = 4096;

/// The checksum of the sample chunks `seed` generates: a few at the spawn and a few thousands
/// of blocks away, from the surface with its plants and trees down through caves and ores, and
/// a structure.
/// FNV-1a over each block's name and each column's roof, so it's the same on every machine and
/// in every version that generates the same blocks, whatever the block table's order.
pub fn world_checksum(seed: u32) -> u64 {
    samples(seed)
        .iter()
        .fold(Fnv::default(), |hash, chunk| hash.chunk(chunk))
        .0
}

// The sample chunks, in a fixed order. Each one far off takes a region of noise, so this takes
// a fraction of a second: only call it from a task.
fn samples(seed: u32) -> Vec<ChunkData> {
    let noise = Arc::new(NoiseGenerator::new(seed));
    let (spawn_x, spawn_z, _) = noise.find_spawn();
    let regions = Arc::new(RegionStore::new(noise));
    let spawn = I64Vec2::new(spawn_x, spawn_z);
    let structure = regions
        .structures_near(spawn - STRUCTURE_SEARCH, spawn + STRUCTURE_SEARCH)
        .into_iter()
        .min_by_key(|placed| (placed.min.xz() - spawn).length_squared())
        .map(|placed| ChunkCoordinate::containing(placed.min))
        .into_iter()
        .flat_map(|floor| [floor, ChunkCoordinate(floor.0 + I64Vec3::Y)]);
    std::iter::once(spawn)
        .chain(FAR_COLUMNS)
        .flat_map(|column| {
            let ground = regions.get_height(column.x, column.y) as i64;
            let ground = ChunkCoordinate::containing(I64Vec3::new(column.x, ground, column.y));
            [ground.0.y + 1, ground.0.y, ground.0.y - 2]
                .into_iter()
                .chain(DEEP_LAYERS)
                .map(move |layer| ChunkCoordinate(ground.0.with_y(layer)))
        })
        .chain(structure)
        .map(|coord| generate_chunk(regions.clone(), coord))
        .collect()
}

// 64-bit FNV-1a, which is fully specified, unlike `DefaultHasher`.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn write(self, bytes: &[u8]) -> Self {
        Self(bytes.iter().fold(self.0, |hash, &byte| {
            (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3)
        }))
    }

    fn chunk(mut self, chunk: &ChunkData) -> Self {
        let mut cells = [BlockType::Air; CELLS];
        chunk.decode_into(&mut cells);
        for block in cells {
            // and the name's end, so names that run together still differ
            self = self.write(block.name().as_bytes()).write(&[0]);
        }
        for x in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                self = self.write(&[chunk.roofed.contains(x, z) as u8]);
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{samples, world_checksum, Fnv};
    use crate::block::BlockType;
    use crate::chunks::chunk::CELLS;

    // What seed 3 generates. Any change to what's generated changes this: if it's meant to,
    // update it deliberately, knowing that worlds will generate differently from before, so a
    // game of the version before can't join one of this version, and saved edits sit on new
    // terrain. CI runs this on Linux, Windows and macOS to see whether they agree (D-3).
    // Last changed by M2.6, which brought jungle, savanna and taiga, changing the trees, plants
    // and grass of much of the world, the spawn's savanna and the cold column's taiga among it,
    // and sampled a jungle column too.
    const GOLDEN: u64 = 0xfe17_9ccc_4c93_e061;

    #[test]
    fn test_generation_matches_its_golden_checksum() {
        assert_eq!(GOLDEN, world_checksum(3), "{:#018x}", world_checksum(3));
    }

    #[test]
    fn test_the_samples_take_in_plants_trees_caves_ores_and_a_structure() {
        let samples = samples(3);
        let blocks: Vec<_> = samples
            .iter()
            .flat_map(|chunk| chunk.iter_non_air().map(|(_, block)| block))
            .collect();
        assert!(blocks.iter().any(BlockType::is_plant));
        assert!(blocks.contains(&BlockType::Leaves));
        assert!(blocks.contains(&BlockType::StoneBricks));
        for ore in [BlockType::CoalOre, BlockType::CopperOre, BlockType::IronOre] {
            assert!(blocks.contains(&ore), "no {ore:?}");
        }
        // air deep underground is a cave
        let deep = samples
            .as_chunks::<6>()
            .0
            .iter()
            .flat_map(|column| &column[3..]);
        let caves = deep.filter(|chunk| chunk.iter_non_air().count() < CELLS);
        assert!(caves.count() >= 2);
    }

    #[test]
    fn test_the_samples_reach_into_the_newest_biomes() {
        let noise = super::NoiseGenerator::new(3);
        let (x, z, _) = noise.find_spawn();
        let biome = |x: i64, z: i64| noise.biomes().name(noise.sample(x as f64, z as f64).biome);
        assert_eq!(Some("savanna"), biome(x, z));
        let [.., taiga, jungle] = super::FAR_COLUMNS;
        assert_eq!(Some("taiga"), biome(taiga.x, taiga.y));
        assert_eq!(Some("jungle"), biome(jungle.x, jungle.y));
    }

    #[test]
    fn test_fnv_1a_is_the_specified_hash() {
        // from the FNV reference's test vectors
        assert_eq!(0xcbf2_9ce4_8422_2325, Fnv::default().0);
        assert_eq!(0xaf63_dc4c_8601_ec8c, Fnv::default().write(b"a").0);
        assert_eq!(0x8594_4171_f739_67e8, Fnv::default().write(b"foobar").0);
    }
}
