use std::io;

use bevy::math::{I64Vec3, U16Vec3};
use serde::{Deserialize, Serialize};

use crate::block::BlockType;
use crate::chunks::chunk::{ChunkCoordinate, CELLS, CHUNK_SIZE};
use crate::containers::{Container, Containers};
use crate::edits::Edits;

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
        format!("r.{}.{}.{EXTENSION}", self.x, self.z)
    }
}

pub const EXTENSION: &str = "edits";

/// A chunk's edits and its containers, as they're snapshotted for saving.
#[derive(Debug, Clone, PartialEq)]
pub struct ChunkEdits {
    pub coord: ChunkCoordinate,
    pub blocks: Vec<(U16Vec3, BlockType)>,
    pub containers: Vec<(U16Vec3, Container)>,
}

// The version of the region file format, which since format 3 starts each file after a 0 byte.
// A format 2 file starts with its palette's length instead, which is never 0, as every file
// names the blocks of at least one edit.
const FORMAT: u8 = 3;

// A region file, in postcard (D-6). Blocks are stored as indices into the file's palette of
// block names, never as `BlockType` discriminants, so reordering the block table can't change
// what a save means (§4.1). Containers keep their items by name.
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
}

// Format 2's, from before containers (M1.6), which is read and written again as format 3 the
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

/// Encodes a region's edits and containers. The same edits always encode the same, whatever
/// order they come in.
pub fn encode(mut chunks: Vec<ChunkEdits>) -> Vec<u8> {
    chunks.sort_unstable_by_key(|chunk| chunk.coord.0.to_array());
    let mut palette = Vec::new();
    let mut indices = [None; BlockType::ALL.len()];
    let chunks = chunks
        .into_iter()
        .map(|chunk| {
            let mut blocks: Vec<_> = chunk
                .blocks
                .into_iter()
                .map(|(local, block)| {
                    let index = *indices[block as usize].get_or_insert_with(|| {
                        palette.push(block.name());
                        palette.len() as u16 - 1
                    });
                    (local_index(local), index)
                })
                .collect();
            blocks.sort_unstable();
            let mut containers: Vec<_> = chunk
                .containers
                .into_iter()
                .map(|(local, container)| (local_index(local), container))
                .collect();
            containers.sort_unstable_by_key(|&(local, _)| local);
            SavedChunk {
                chunk: chunk.coord.0.to_array(),
                blocks,
                containers,
            }
        })
        .collect();
    let file = RegionFile { palette, chunks };
    postcard::to_extend(&file, vec![0, FORMAT]).expect("edits always encode")
}

/// Decodes a region file's edits into `edits` and its containers into `containers`. A block or
/// item it doesn't know, or a format newer than this game's, saved by a newer version, is an
/// error rather than turning into air.
pub fn decode(bytes: &[u8], edits: &mut Edits, containers: &mut Containers) -> io::Result<()> {
    let invalid = |message: String| io::Error::new(io::ErrorKind::InvalidData, message);
    let file: RegionFile = match bytes {
        [0, FORMAT, file @ ..] => postcard::from_bytes(file).map_err(|e| invalid(e.to_string()))?,
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
            BlockType::from_name(name).ok_or_else(|| {
                invalid(format!(
                    "unknown block \"{name}\", perhaps saved by a newer version"
                ))
            })
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
    use crate::block::BlockType;
    use crate::chunks::chunk::ChunkCoordinate;
    use crate::containers::{Container, Containers, FUEL, INPUT};
    use crate::edits::Edits;
    use crate::items::{Item, ItemStack};

    fn chunk(x: i64, y: i64, z: i64) -> ChunkCoordinate {
        ChunkCoordinate(I64Vec3::new(x, y, z))
    }

    fn edits(coord: ChunkCoordinate, blocks: Vec<(U16Vec3, BlockType)>) -> ChunkEdits {
        ChunkEdits {
            coord,
            blocks,
            containers: Vec::new(),
        }
    }

    fn sorted(edits: &Edits, containers: &Containers) -> Vec<ChunkEdits> {
        let mut chunks: Vec<_> = edits
            .chunks()
            .map(|coord| {
                let mut blocks: Vec<_> = edits.in_chunk(coord).collect();
                blocks.sort_unstable_by_key(|(local, _)| local.to_array());
                let mut containers: Vec<_> = containers
                    .in_chunk(coord)
                    .map(|(local, container)| (local, container.clone()))
                    .collect();
                containers.sort_unstable_by_key(|(local, _)| local.to_array());
                ChunkEdits {
                    coord,
                    blocks,
                    containers,
                }
            })
            .collect();
        chunks.sort_unstable_by_key(|chunk| chunk.coord.0.to_array());
        chunks
    }

    fn decoded(bytes: &[u8]) -> Vec<ChunkEdits> {
        let (mut edits, mut containers) = (Edits::default(), Containers::default());
        decode(bytes, &mut edits, &mut containers).unwrap();
        // loading isn't a change to save again
        assert!(edits.take_changed().is_empty());
        assert!(containers.take_changed().is_empty());
        sorted(&edits, &containers)
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
    fn test_edits_and_containers_round_trip_through_a_region_file() {
        let furnace_at = U16Vec3::new(15, 15, 15);
        let saved = vec![
            ChunkEdits {
                coord: chunk(3, 28, -4),
                blocks: vec![
                    (U16Vec3::new(0, 0, 0), BlockType::Air),
                    (U16Vec3::new(1, 2, 3), BlockType::Air),
                    (furnace_at, BlockType::Furnace),
                ],
                containers: vec![(furnace_at, furnace())],
            },
            edits(
                chunk(-1, 0, 31),
                vec![(U16Vec3::new(4, 5, 6), BlockType::StoneBricks)],
            ),
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
                (U16Vec3::new(0, 0, 0), BlockType::Air),
                (U16Vec3::new(15, 15, 15), BlockType::Furnace),
                (U16Vec3::new(1, 2, 3), BlockType::Air),
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
                (U16Vec3::new(1, 0, 0), BlockType::Bricks),
                (U16Vec3::new(2, 0, 0), BlockType::Air),
                (U16Vec3::new(3, 0, 0), BlockType::Bricks),
            ],
        )]);
        assert_eq!([0, 3], bytes[..2]);
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
            (U16Vec3::new(5, 0, 0), BlockType::Furnace),
            (U16Vec3::new(12, 1, 2), BlockType::Air),
        ];
        assert_eq!(vec![edits(chunk(2, 30, -1), blocks)], loaded);
        // and it's written as format 3 when it's saved again
        let again = encode(loaded.clone());
        assert_eq!([0, 3], again[..2]);
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
            }],
        };
        let bytes = postcard::to_extend(&file, vec![0, 3]).unwrap();
        let error = decode(&bytes, &mut Edits::default(), &mut Containers::default());
        assert!(error.unwrap_err().to_string().contains("mithril_ore"));
        let newer = [&[0, 4][..], &bytes[2..]].concat();
        let error = decode(&newer, &mut Edits::default(), &mut Containers::default());
        assert!(error.unwrap_err().to_string().contains("newer version"));
    }

    #[test]
    fn test_a_damaged_file_or_a_container_with_the_wrong_slots_is_an_error() {
        let mut chunks = vec![ChunkEdits {
            containers: vec![(U16Vec3::ZERO, furnace())],
            ..edits(chunk(0, 0, 0), vec![(U16Vec3::ZERO, BlockType::Furnace)])
        }];
        let bytes = encode(chunks.clone());
        let load = |bytes: &[u8]| {
            decode(bytes, &mut Edits::default(), &mut Containers::default()).is_err()
        };
        assert!(load(&bytes[..bytes.len() - 1]));
        chunks[0].containers[0].1.slots.push(None);
        assert!(load(&encode(chunks)));
    }
}
