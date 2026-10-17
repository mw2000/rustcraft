use std::io;

use bevy::math::{I64Vec3, U16Vec3};
use serde::{Deserialize, Serialize};

use crate::block::BlockType;
use crate::chunks::chunk::{ChunkCoordinate, CELLS, CHUNK_SIZE};
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

/// A chunk's edits, as they're snapshotted for saving.
pub type ChunkEdits = (ChunkCoordinate, Vec<(U16Vec3, BlockType)>);

// A region file, in postcard (D-6). Blocks are stored as indices into the file's palette of
// block names, never as `BlockType` discriminants, so reordering the block table can't change
// what a save means (§4.1).
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
}

/// Encodes a region's edits. The same edits always encode the same, whatever order they come in.
pub fn encode(mut chunks: Vec<ChunkEdits>) -> Vec<u8> {
    chunks.sort_unstable_by_key(|(coord, _)| coord.0.to_array());
    let mut palette = Vec::new();
    let mut indices = [None; BlockType::ALL.len()];
    let chunks = chunks
        .into_iter()
        .map(|(coord, edits)| {
            let mut blocks: Vec<_> = edits
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
            SavedChunk {
                chunk: coord.0.to_array(),
                blocks,
            }
        })
        .collect();
    postcard::to_stdvec(&RegionFile { palette, chunks }).expect("edits always encode")
}

/// Decodes a region file's edits into `edits`. A block it doesn't know, saved by a newer
/// version, is an error rather than turning into air.
pub fn decode(bytes: &[u8], edits: &mut Edits) -> io::Result<()> {
    let invalid = |message: String| io::Error::new(io::ErrorKind::InvalidData, message);
    let file: RegionFile = postcard::from_bytes(bytes).map_err(|e| invalid(e.to_string()))?;
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
    for chunk in file.chunks {
        let coord = ChunkCoordinate(I64Vec3::from_array(chunk.chunk));
        for (local, index) in chunk.blocks {
            let block = palette
                .get(index as usize)
                .ok_or_else(|| invalid(format!("palette index {index} out of range")))?;
            if local as usize >= CELLS {
                return Err(invalid(format!("block {local} isn't in a chunk")));
            }
            edits.put(coord, local_block(local), *block);
        }
    }
    Ok(())
}

fn local_index(local: U16Vec3) -> u16 {
    (local.y * CHUNK_SIZE + local.z) * CHUNK_SIZE + local.x
}

fn local_block(index: u16) -> U16Vec3 {
    U16Vec3::new(
        index % CHUNK_SIZE,
        index / (CHUNK_SIZE * CHUNK_SIZE),
        index / CHUNK_SIZE % CHUNK_SIZE,
    )
}

#[cfg(test)]
mod tests {
    use bevy::math::{I64Vec3, U16Vec3};

    use super::{decode, encode, ChunkEdits, Region, RegionFile, SavedChunk};
    use crate::block::BlockType;
    use crate::chunks::chunk::ChunkCoordinate;
    use crate::edits::Edits;

    fn chunk(x: i64, y: i64, z: i64) -> ChunkCoordinate {
        ChunkCoordinate(I64Vec3::new(x, y, z))
    }

    fn sorted(edits: &Edits) -> Vec<ChunkEdits> {
        let mut chunks: Vec<_> = edits
            .chunks()
            .map(|coord| {
                let mut blocks: Vec<_> = edits.in_chunk(coord).collect();
                blocks.sort_unstable_by_key(|(local, _)| local.to_array());
                (coord, blocks)
            })
            .collect();
        chunks.sort_unstable_by_key(|(coord, _)| coord.0.to_array());
        chunks
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
    fn test_edits_round_trip_through_a_region_file() {
        let saved: Vec<ChunkEdits> = vec![
            (
                chunk(3, 28, -4),
                vec![
                    (U16Vec3::new(0, 0, 0), BlockType::Air),
                    (U16Vec3::new(15, 15, 15), BlockType::Furnace),
                    (U16Vec3::new(1, 2, 3), BlockType::Air),
                ],
            ),
            (
                chunk(-1, 0, 31),
                vec![(U16Vec3::new(4, 5, 6), BlockType::StoneBricks)],
            ),
        ];
        let bytes = encode(saved.clone());
        let mut edits = Edits::default();
        decode(&bytes, &mut edits).unwrap();
        let mut expected = Edits::default();
        for (coord, blocks) in saved {
            for (local, block) in blocks {
                expected.put(coord, local, block);
            }
        }
        assert_eq!(sorted(&expected), sorted(&edits));
        // loading isn't a change to save again
        assert!(edits.take_changed().is_empty());
        // and the order edits come in doesn't change the file
        assert_eq!(bytes, encode(sorted(&edits).into_iter().rev().collect()));
        // three blocks named once each, and seven edits of a few bytes
        assert!(bytes.len() < 64, "{} bytes", bytes.len());
    }

    #[test]
    fn test_blocks_are_saved_by_name_in_a_palette() {
        let bytes = encode(vec![(
            chunk(0, 0, 0),
            vec![
                (U16Vec3::new(1, 0, 0), BlockType::Bricks),
                (U16Vec3::new(2, 0, 0), BlockType::Air),
                (U16Vec3::new(3, 0, 0), BlockType::Bricks),
            ],
        )]);
        let file: RegionFile = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(vec!["bricks", "air"], file.palette);
        assert_eq!(vec![(1, 0), (2, 1), (3, 0)], file.chunks[0].blocks);
    }

    #[test]
    fn test_an_unknown_block_fails_to_load_rather_than_becoming_air() {
        let bytes = postcard::to_stdvec(&RegionFile {
            palette: vec!["stone", "mithril_ore"],
            chunks: vec![SavedChunk {
                chunk: [0, 0, 0],
                blocks: vec![(0, 0), (1, 1)],
            }],
        })
        .unwrap();
        let error = decode(&bytes, &mut Edits::default()).unwrap_err();
        assert!(error.to_string().contains("mithril_ore"), "{error}");
    }

    #[test]
    fn test_a_damaged_file_is_an_error() {
        let bytes = encode(vec![(
            chunk(0, 0, 0),
            vec![(U16Vec3::new(1, 0, 0), BlockType::Bricks)],
        )]);
        assert!(decode(&bytes[..bytes.len() - 1], &mut Edits::default()).is_err());
    }
}
