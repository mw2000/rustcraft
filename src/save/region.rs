use std::io;

use bevy::math::{I64Vec3, U16Vec3};
use serde::{Deserialize, Serialize};

use crate::block::Block;
use crate::chunks::chunk::{ChunkCoordinate, CELLS, CHUNK_SIZE};
use crate::containers::{Container, Containers};
use crate::edits::Edits;
use crate::farming::Crops;

/// How many chunks wide and deep a region is. It holds every chunk of its columns, all the way up.
const REGION_CHUNKS: i64 = 32;

/// A region of 32×32 columns of chunks, whose edits are saved together in one file, so a world
/// isn't thousands of tiny files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Region {
    pub x: i64,
    pub z: i64,
}

impl Region {
    pub fn containing(chunk: ChunkCoordinate) -> Self {
        Self {
            x: chunk.0.x.div_euclid(REGION_CHUNKS),
            z: chunk.0.z.div_euclid(REGION_CHUNKS),
        }
    }

    pub fn file_name(self) -> String {
        self.file_name_for(EXTENSION)
    }

    /// Its file's name among those with `extension`, `r.<x>.<z>.<extension>`.
    pub fn file_name_for(self, extension: &str) -> String {
        format!("r.{}.{}.{extension}", self.x, self.z)
    }

    /// The region a file's for by its name, if it's a region's.
    pub fn from_file_name(name: &str) -> Option<Self> {
        let mut parts = name.strip_prefix("r.")?.split('.');
        let (x, z) = (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
        Some(Self { x, z })
    }
}

pub const EXTENSION: &str = "edits";

/// A chunk's edits, its containers and when its crops were planted, as they're snapshotted for
/// saving.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkEdits {
    pub coord: ChunkCoordinate,
    pub blocks: Vec<(U16Vec3, Block)>,
    pub containers: Vec<(U16Vec3, Container)>,
    pub crops: Vec<(U16Vec3, f64)>,
}

// The version of the region file format, which since format 3 starts each file after a 0 byte.
// A format 2 file starts with its palette's length instead, which is never 0, as every file
// names the blocks of at least one edit. Format 4 has crops, and format 5 blocks' states in
// its palette, which is laid out as format 4's: a block with no state is written as before.
// Format 6 counts the crops' planting times in `f64`s, as `TimeOfDay` does.
const FORMAT: u8 = 6;

// A region file, in postcard (D-6). Blocks are stored as indices into the file's palette of
// blocks by name and state, as templates write them (`Block`'s text), never as `BlockType`
// discriminants or state bits, so reordering the block table can't change what a save means
// (§4.1). Containers keep their items by name.
#[derive(Serialize, Deserialize)]
struct RegionFile<'a> {
    #[serde(borrow)]
    palette: Vec<&'a str>,
    chunks: Vec<SavedChunk>,
}

#[derive(Serialize, Deserialize)]
struct SavedChunk {
    chunk: [i64; 3],
    // each edit as where it is in the chunk, `(y * 16 + z) * 16 + x`, and its palette index
    blocks: Vec<(u16, u16)>,
    // each container as where it is in the chunk, and what it holds
    containers: Vec<(u16, Container)>,
    // each crop as where it is in the chunk, and when it was planted, in game days
    crops: Vec<(u16, f64)>,
}

// Formats 4 and 5's, from before the clock counted in `f64`s (M-R1), which differ only in what
// their palettes name, and have their crops' planting times as `f32`s. They're read and written
// again as format 6 the next time their region's saved.
mod format_5 {
    use serde::Deserialize;

    use crate::containers::Container;

    #[derive(Deserialize)]
    pub(super) struct RegionFile<'a> {
        #[serde(borrow)]
        pub(super) palette: Vec<&'a str>,
        pub(super) chunks: Vec<SavedChunk>,
    }

    #[derive(Deserialize)]
    pub(super) struct SavedChunk {
        pub(super) chunk: [i64; 3],
        pub(super) blocks: Vec<(u16, u16)>,
        pub(super) containers: Vec<(u16, Container)>,
        pub(super) crops: Vec<(u16, f32)>,
    }
}

// Format 3's, from before crops (M1.9b), which is read and written again as format 4 the next
// time its region's saved.
mod format_3 {
    use serde::Deserialize;

    use crate::containers::Container;

    #[derive(Deserialize)]
    pub(super) struct RegionFile<'a> {
        #[serde(borrow)]
        pub(super) palette: Vec<&'a str>,
        pub(super) chunks: Vec<SavedChunk>,
    }

    #[derive(Deserialize)]
    pub(super) struct SavedChunk {
        pub(super) chunk: [i64; 3],
        pub(super) blocks: Vec<(u16, u16)>,
        pub(super) containers: Vec<(u16, Container)>,
    }
}

// Format 2's, from before containers (M1.6), which is read and written again as format 6 the
// next time its region's saved.
mod format_2 {
    use serde::Deserialize;

    #[derive(Deserialize)]
    pub(super) struct RegionFile<'a> {
        #[serde(borrow)]
        pub(super) palette: Vec<&'a str>,
        pub(super) chunks: Vec<SavedChunk>,
    }

    #[derive(Deserialize)]
    pub(super) struct SavedChunk {
        pub(super) chunk: [i64; 3],
        pub(super) blocks: Vec<(u16, u16)>,
    }
}

/// Encodes a region's edits, containers and crops. The same edits always encode the same,
/// whatever order they come in.
pub fn encode(mut chunks: Vec<ChunkEdits>) -> Vec<u8> {
    chunks.sort_unstable_by_key(|chunk| chunk.coord.0.to_array());
    let mut palette: Vec<Block> = Vec::new();
    let chunks: Vec<_> = chunks
        .into_iter()
        .map(|chunk| {
            let mut blocks: Vec<_> = chunk
                .blocks
                .into_iter()
                .map(|(local, block)| {
                    let index = palette.iter().position(|&b| b == block).unwrap_or_else(|| {
                        palette.push(block);
                        palette.len() - 1
                    });
                    (local_index(local), index as u16)
                })
                .collect();
            blocks.sort_unstable();
            let mut containers: Vec<_> = chunk
                .containers
                .into_iter()
                .map(|(local, container)| (local_index(local), container))
                .collect();
            containers.sort_unstable_by_key(|&(local, _)| local);
            let mut crops: Vec<_> = chunk
                .crops
                .into_iter()
                .map(|(local, planted)| (local_index(local), planted))
                .collect();
            crops.sort_unstable_by_key(|&(local, _)| local);
            SavedChunk {
                chunk: chunk.coord.0.to_array(),
                blocks,
                containers,
                crops,
            }
        })
        .collect();
    let names: Vec<String> = palette.iter().map(Block::to_string).collect();
    let palette = names.iter().map(String::as_str).collect();
    let file = RegionFile { palette, chunks };
    postcard::to_extend(&file, vec![0, FORMAT]).expect("edits always encode")
}

/// Decodes a region file's edits into `edits`, its containers into `containers` and its crops
/// into `crops`. A block or item it doesn't know, or a format newer than this game's, saved by
/// a newer version, is an error rather than turning into air.
pub fn decode(
    bytes: &[u8],
    edits: &mut Edits,
    containers: &mut Containers,
    crops: &mut Crops,
) -> io::Result<()> {
    let invalid = |message: String| io::Error::new(io::ErrorKind::InvalidData, message);
    let file: RegionFile = match bytes {
        [0, FORMAT, file @ ..] => postcard::from_bytes(file).map_err(|e| invalid(e.to_string()))?,
        [0, 4 | 5, file @ ..] => {
            let file: format_5::RegionFile =
                postcard::from_bytes(file).map_err(|e| invalid(e.to_string()))?;
            let chunks = file.chunks.into_iter().map(|chunk| SavedChunk {
                chunk: chunk.chunk,
                blocks: chunk.blocks,
                containers: chunk.containers,
                // an `f32` widens exactly, so a crop keeps the time it was planted
                crops: chunk
                    .crops
                    .into_iter()
                    .map(|(local, planted)| (local, f64::from(planted)))
                    .collect(),
            });
            RegionFile {
                palette: file.palette,
                chunks: chunks.collect(),
            }
        }
        [0, 3, file @ ..] => {
            let file: format_3::RegionFile =
                postcard::from_bytes(file).map_err(|e| invalid(e.to_string()))?;
            let chunks = file.chunks.into_iter().map(|chunk| SavedChunk {
                chunk: chunk.chunk,
                blocks: chunk.blocks,
                containers: chunk.containers,
                crops: Vec::new(),
            });
            RegionFile {
                palette: file.palette,
                chunks: chunks.collect(),
            }
        }
        [0, format, ..] => {
            return Err(invalid(format!(
                "region format {format}, perhaps saved by a newer version"
            )))
        }
        _ => {
            let file: format_2::RegionFile =
                postcard::from_bytes(bytes).map_err(|e| invalid(e.to_string()))?;
            let chunks = file.chunks.into_iter().map(|chunk| SavedChunk {
                chunk: chunk.chunk,
                blocks: chunk.blocks,
                containers: Vec::new(),
                crops: Vec::new(),
            });
            RegionFile {
                palette: file.palette,
                chunks: chunks.collect(),
            }
        }
    };
    let palette = file
        .palette
        .iter()
        .map(|&name| {
            name.parse::<Block>()
                .map_err(|e| invalid(format!("{e}, perhaps saved by a newer version")))
        })
        .collect::<io::Result<Vec<_>>>()?;
    let in_chunk = |local: u16| {
        (usize::from(local) < CELLS)
            .then(|| local_block(local))
            .ok_or_else(|| invalid(format!("block {local} isn't in a chunk")))
    };
    for chunk in file.chunks {
        let coord = ChunkCoordinate(I64Vec3::from_array(chunk.chunk));
        for (local, index) in chunk.blocks {
            let block = palette
                .get(index as usize)
                .ok_or_else(|| invalid(format!("palette index {index} out of range")))?;
            edits.put(coord, in_chunk(local)?, *block);
        }
        let origin = coord.0 * CHUNK_SIZE as i64;
        for (local, container) in chunk.containers {
            if !container.is_whole() {
                return Err(invalid(format!(
                    "a {} can't hold {} slots",
                    container.block.name(),
                    container.slots.len()
                )));
            }
            containers.put(origin + in_chunk(local)?.as_i64vec3(), container);
        }
        for (local, planted) in chunk.crops {
            crops.put(coord, in_chunk(local)?, planted);
        }
    }
    Ok(())
}

/// Where a block is in its chunk, `(y * 16 + z) * 16 + x`, as edits are saved and sent.
pub fn local_index(local: U16Vec3) -> u16 {
    (local.y * CHUNK_SIZE + local.z) * CHUNK_SIZE + local.x
}

/// The block at `index` in a chunk, see `local_index`.
pub fn local_block(index: u16) -> U16Vec3 {
    U16Vec3::new(
        index % CHUNK_SIZE,
        index / (CHUNK_SIZE * CHUNK_SIZE),
        index / CHUNK_SIZE % CHUNK_SIZE,
    )
}

#[cfg(test)]
mod tests {
    use bevy::math::{I64Vec3, U16Vec3};
    use serde::Serialize;

    use super::{decode, encode, ChunkEdits, Region, RegionFile, SavedChunk};
    use crate::block::{Block, BlockType};
    use crate::chunks::chunk::ChunkCoordinate;
    use crate::containers::{Container, Containers, FUEL, INPUT};
    use crate::edits::Edits;
    use crate::farming::Crops;
    use crate::items::{Item, ItemStack};

    fn chunk(x: i64, y: i64, z: i64) -> ChunkCoordinate {
        ChunkCoordinate(I64Vec3::new(x, y, z))
    }

    fn edits(coord: ChunkCoordinate, blocks: Vec<(U16Vec3, Block)>) -> ChunkEdits {
        ChunkEdits {
            coord,
            blocks,
            containers: Vec::new(),
            crops: Vec::new(),
        }
    }

    fn sorted(edits: &Edits, containers: &Containers, crops: &Crops) -> Vec<ChunkEdits> {
        let mut coords: Vec<_> = edits.chunks().chain(containers.chunks()).collect();
        coords.sort_unstable_by_key(|coord| coord.0.to_array());
        coords.dedup();
        coords
            .into_iter()
            .map(|coord| {
                let mut blocks: Vec<_> = edits.in_chunk(coord).collect();
                blocks.sort_unstable_by_key(|(local, _)| local.to_array());
                let mut containers: Vec<_> = containers
                    .in_chunk(coord)
                    .map(|(local, container)| (local, container.clone()))
                    .collect();
                containers.sort_unstable_by_key(|(local, _)| local.to_array());
                let mut crops: Vec<_> = crops.in_chunk(coord).collect();
                crops.sort_unstable_by_key(|(local, _)| local.to_array());
                ChunkEdits {
                    coord,
                    blocks,
                    containers,
                    crops,
                }
            })
            .collect()
    }

    fn decoded(bytes: &[u8]) -> Vec<ChunkEdits> {
        let (mut edits, mut containers, mut crops) =
            (Edits::default(), Containers::default(), Crops::default());
        decode(bytes, &mut edits, &mut containers, &mut crops).unwrap();
        // loading isn't a change to save again
        assert!(edits.take_changed().is_empty());
        assert!(containers.take_changed().is_empty());
        assert!(crops.take_changed().is_empty());
        sorted(&edits, &containers, &crops)
    }

    fn fails(bytes: &[u8]) -> std::io::Error {
        let (mut edits, mut containers, mut crops) =
            (Edits::default(), Containers::default(), Crops::default());
        decode(bytes, &mut edits, &mut containers, &mut crops).unwrap_err()
    }

    // A furnace part way through smelting iron, with coal on the fire.
    fn furnace() -> Container {
        let mut furnace = Container::new(BlockType::Furnace).unwrap();
        furnace.slots[INPUT] = Some(ItemStack::new(Item::Block(BlockType::IronOre), 5));
        furnace.slots[FUEL] = Some(ItemStack::new(Item::Coal, 3));
        furnace.fire.left = 12.5;
        furnace.fire.lasts = 80.0;
        furnace.fire.smelted = 4.0;
        furnace
    }

    // A chest with a few things in it.
    fn chest() -> Container {
        let mut chest = Container::new(BlockType::Chest).unwrap();
        chest.slots[4] = Some(ItemStack::new(Item::Bread, 2));
        chest.slots[26] = Some(ItemStack {
            wear: 9,
            ..ItemStack::new(Item::IronPickaxe, 1)
        });
        chest
    }

    #[test]
    fn test_regions_hold_32_by_32_columns_of_chunks_all_the_way_up() {
        assert_eq!(Region { x: 0, z: 0 }, Region::containing(chunk(31, 63, 0)));
        assert_eq!(Region { x: 1, z: -1 }, Region::containing(chunk(32, 0, -1)));
        assert_eq!(
            Region { x: -1, z: -2 },
            Region::containing(chunk(-32, 5, -33))
        );
        assert_eq!("r.1.-1.edits", Region { x: 1, z: -1 }.file_name());
    }

    #[test]
    fn test_edits_containers_and_crops_round_trip_through_a_region_file() {
        let furnace_at = U16Vec3::new(15, 15, 15);
        let (wheat_at, more_at) = (U16Vec3::new(4, 9, 4), U16Vec3::new(5, 9, 4));
        let saved = vec![
            ChunkEdits {
                coord: chunk(3, 28, -4),
                blocks: vec![
                    (U16Vec3::new(0, 0, 0), BlockType::Air.into()),
                    (U16Vec3::new(1, 2, 3), BlockType::Air.into()),
                    (wheat_at, BlockType::Wheat2.into()),
                    (more_at, BlockType::Wheat0.into()),
                    (furnace_at, BlockType::Furnace.into()),
                ],
                containers: vec![(furnace_at, furnace())],
                crops: vec![(wheat_at, 12.75), (more_at, 13.0625)],
            },
            edits(
                chunk(-1, 0, 31),
                vec![(U16Vec3::new(4, 5, 6), BlockType::Chest.into())],
            ),
            // a chest the world generated, opened, where nothing was edited
            ChunkEdits {
                containers: vec![(U16Vec3::new(7, 1, 9), chest())],
                ..edits(chunk(2, 30, 2), Vec::new())
            },
        ];
        let bytes = encode(saved.clone());
        let mut expected = saved;
        expected.sort_unstable_by_key(|chunk| chunk.coord.0.to_array());
        assert_eq!(expected, decoded(&bytes));
        // and the order edits come in doesn't change the file
        assert_eq!(bytes, encode(expected.into_iter().rev().collect()));
    }

    #[test]
    fn test_edits_take_a_few_bytes_each() {
        let bytes = encode(vec![edits(
            chunk(0, 0, 0),
            vec![
                (U16Vec3::new(0, 0, 0), BlockType::Air.into()),
                (U16Vec3::new(15, 15, 15), BlockType::Furnace.into()),
                (U16Vec3::new(1, 2, 3), BlockType::Air.into()),
            ],
        )]);
        // two blocks named once each, and three edits of a few bytes
        assert!(bytes.len() < 48, "{} bytes", bytes.len());
    }

    #[test]
    fn test_blocks_are_saved_by_name_in_a_palette() {
        let bytes = encode(vec![edits(
            chunk(0, 0, 0),
            vec![
                (U16Vec3::new(1, 0, 0), BlockType::Bricks.into()),
                (U16Vec3::new(2, 0, 0), BlockType::Air.into()),
                (U16Vec3::new(3, 0, 0), BlockType::Bricks.into()),
            ],
        )]);
        assert_eq!([0, 6], bytes[..2]);
        let file: RegionFile = postcard::from_bytes(&bytes[2..]).unwrap();
        assert_eq!(vec!["bricks", "air"], file.palette);
        assert_eq!(vec![(1, 0), (2, 1), (3, 0)], file.chunks[0].blocks);
    }

    // A format 2 file, as saved before containers: a palette, then each chunk's edits.
    type Format2Chunk = ([i64; 3], Vec<(u16, u16)>);

    #[derive(Serialize)]
    struct Format2<'a> {
        palette: Vec<&'a str>,
        chunks: Vec<Format2Chunk>,
    }

    #[test]
    fn test_a_region_file_from_before_containers_still_loads() {
        let bytes = postcard::to_stdvec(&Format2 {
            palette: vec!["furnace", "air"],
            chunks: vec![([2, 30, -1], vec![(5, 0), (300, 1)])],
        })
        .unwrap();
        let loaded = decoded(&bytes);
        let blocks = vec![
            (U16Vec3::new(5, 0, 0), BlockType::Furnace.into()),
            (U16Vec3::new(12, 1, 2), BlockType::Air.into()),
        ];
        assert_eq!(vec![edits(chunk(2, 30, -1), blocks)], loaded);
        // and it's written as format 5 when it's saved again
        let again = encode(loaded.clone());
        assert_eq!([0, 6], again[..2]);
        assert_eq!(loaded, decoded(&again));
    }

    // A format 3 file, as saved before crops: format 4's but for each chunk's crops.
    type Format3Chunk = ([i64; 3], Vec<(u16, u16)>, Vec<(u16, Container)>);

    #[derive(Serialize)]
    struct Format3<'a> {
        palette: Vec<&'a str>,
        chunks: Vec<Format3Chunk>,
    }

    #[test]
    fn test_a_region_file_from_before_crops_still_loads() {
        let file = Format3 {
            palette: vec!["furnace", "wheat_3"],
            chunks: vec![([2, 30, -1], vec![(5, 0), (300, 1)], vec![(5, furnace())])],
        };
        let bytes = postcard::to_extend(&file, vec![0, 3]).unwrap();
        let loaded = decoded(&bytes);
        let expected = ChunkEdits {
            containers: vec![(U16Vec3::new(5, 0, 0), furnace())],
            ..edits(
                chunk(2, 30, -1),
                vec![
                    (U16Vec3::new(5, 0, 0), BlockType::Furnace.into()),
                    (U16Vec3::new(12, 1, 2), BlockType::Wheat3.into()),
                ],
            )
        };
        assert_eq!(vec![expected], loaded);
        let again = encode(loaded.clone());
        assert_eq!([0, 6], again[..2]);
        assert_eq!(loaded, decoded(&again));
    }

    #[test]
    fn test_an_unknown_block_or_format_fails_to_load_rather_than_becoming_air() {
        let file = RegionFile {
            palette: vec!["stone", "mithril_ore"],
            chunks: vec![SavedChunk {
                chunk: [0, 0, 0],
                blocks: vec![(0, 0), (1, 1)],
                containers: Vec::new(),
                crops: Vec::new(),
            }],
        };
        let bytes = postcard::to_extend(&file, vec![0, 6]).unwrap();
        assert!(fails(&bytes).to_string().contains("mithril_ore"));
        let newer = [&[0, 7][..], &bytes[2..]].concat();
        assert!(fails(&newer).to_string().contains("newer version"));
        // nor does a state the game doesn't know
        let file = RegionFile {
            palette: vec!["planks_stairs[facing=sideways]"],
            ..file
        };
        let bytes = postcard::to_extend(&file, vec![0, 6]).unwrap();
        assert!(fails(&bytes).to_string().contains("sideways"));
    }

    #[test]
    fn test_blocks_keep_their_states_in_the_palette_and_a_format_4_file_loads_as_it_was() {
        let stairs: Block = "stone_brick_stairs[facing=west,half=top]".parse().unwrap();
        let log: Block = "log[axis=z]".parse().unwrap();
        let blocks = vec![
            (U16Vec3::new(1, 0, 0), stairs),
            (U16Vec3::new(2, 0, 0), BlockType::StoneBrickStairs.into()),
            (U16Vec3::new(3, 0, 0), log),
            (U16Vec3::new(4, 0, 0), stairs),
        ];
        let saved = vec![edits(chunk(0, 0, 0), blocks)];
        let bytes = encode(saved.clone());
        let file: RegionFile = postcard::from_bytes(&bytes[2..]).unwrap();
        // each block in each state named once
        assert_eq!(
            vec![
                "stone_brick_stairs[facing=west,half=top]",
                "stone_brick_stairs",
                "log[axis=z]"
            ],
            file.palette
        );
        assert_eq!(saved, decoded(&bytes));
        // a format 4 file is laid out the same, its blocks with no state
        let file = RegionFile {
            palette: vec!["log", "furnace"],
            chunks: vec![SavedChunk {
                chunk: [0, 0, 0],
                blocks: vec![(0, 0), (1, 1)],
                containers: Vec::new(),
                crops: Vec::new(),
            }],
        };
        let old = postcard::to_extend(&file, vec![0, 4]).unwrap();
        let blocks = vec![
            (U16Vec3::new(0, 0, 0), BlockType::Log.into()),
            (U16Vec3::new(1, 0, 0), BlockType::Furnace.into()),
        ];
        assert_eq!(vec![edits(chunk(0, 0, 0), blocks)], decoded(&old));
    }

    // Format 5's chunk, as it was laid out, with crops planted at `f32` days.
    #[derive(Serialize)]
    #[allow(clippy::type_complexity)]
    struct Format5 {
        palette: Vec<&'static str>,
        chunks: Vec<(
            [i64; 3],
            Vec<(u16, u16)>,
            Vec<(u16, Container)>,
            Vec<(u16, f32)>,
        )>,
    }

    #[test]
    fn test_a_format_4_or_5_file_with_crops_loads_with_their_times_and_saves_as_format_6() {
        for format in [4, 5] {
            let file = Format5 {
                palette: vec!["wheat_1", "furnace"],
                chunks: vec![(
                    [1, 2, 3],
                    vec![(10, 0), (20, 1)],
                    vec![(20, furnace())],
                    vec![(10, 12.75), (30, 0.1)],
                )],
            };
            let bytes = postcard::to_extend(&file, vec![0, format]).unwrap();
            let loaded = decoded(&bytes);
            assert_eq!(
                vec![
                    (U16Vec3::new(10, 0, 0), 12.75),
                    (U16Vec3::new(14, 0, 1), 0.1f32.into())
                ],
                loaded[0].crops,
                "format {format}"
            );
            assert_eq!(1, loaded[0].containers.len());
            let again = encode(loaded.clone());
            assert_eq!([0, 6], again[..2]);
            assert_eq!(loaded, decoded(&again));
        }
    }

    #[test]
    fn test_crops_planted_millions_of_days_in_keep_their_times_to_a_tiny_fraction() {
        let planted = vec![
            (U16Vec3::new(1, 1, 1), 1_000_000.123_456_789),
            (U16Vec3::new(2, 1, 1), 40_000_000.000_001),
        ];
        let blocks = planted
            .iter()
            .map(|&(at, _)| (at, BlockType::Wheat0.into()))
            .collect();
        let saved = vec![ChunkEdits {
            crops: planted,
            ..edits(chunk(0, 0, 0), blocks)
        }];
        assert_eq!(saved, decoded(&encode(saved.clone())));
    }

    #[test]
    fn test_a_damaged_file_or_a_container_with_the_wrong_slots_is_an_error() {
        let mut chunks = vec![ChunkEdits {
            containers: vec![(U16Vec3::ZERO, furnace())],
            ..edits(
                chunk(0, 0, 0),
                vec![(U16Vec3::ZERO, BlockType::Furnace.into())],
            )
        }];
        let bytes = encode(chunks.clone());
        fails(&bytes[..bytes.len() - 1]);
        chunks[0].containers[0].1.slots.push(None);
        fails(&encode(chunks));
    }
}
