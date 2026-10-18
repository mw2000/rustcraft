use std::sync::Arc;

use bevy::math::{I64Vec3, IVec3};
use bevy::platform::collections::{HashMap, HashSet};

use super::key::LodKey;
use crate::block::BlockType;
use crate::chunks::chunk::{ChunkCoordinate, CHUNK_SIZE};
use crate::edits::Edits;

/// Levels up to this take edits in. A cell any coarser is 512 blocks wide, which a build would
/// have to fill half of, 67 million blocks, to show.
pub const EDITED_LEVELS: u8 = 8;

/// What edits have made of a cell of blocks: how many of its blocks are built of something
/// LOD nodes draw, and the commonest of those, and how many are cleared, to air or to something
/// only full detail draws, such as a torch.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct CellEdits {
    pub block: BlockType,
    pub built: u32,
    pub cleared: u32,
}

impl CellEdits {
    /// A block edited to `block`.
    pub fn of_block(block: BlockType) -> Self {
        if block.style().is_drawn_far() {
            Self {
                block,
                built: 1,
                cleared: 0,
            }
        } else {
            Self {
                block: BlockType::Air,
                built: 0,
                cleared: 1,
            }
        }
    }

    /// The cell that the 8 `cells` half as wide make up, in `children` order. Its commonest
    /// block is the one most of their built blocks are, the first of them if two tie, so it's
    /// the same wherever it's worked out.
    pub fn merge(cells: impl IntoIterator<Item = CellEdits>) -> Self {
        let mut tally = [(BlockType::Air, 0); 8];
        let mut kinds = 0;
        let mut merged = Self::default();
        for cell in cells {
            merged.built += cell.built;
            merged.cleared += cell.cleared;
            if cell.built == 0 {
                continue;
            }
            match tally[..kinds]
                .iter_mut()
                .find(|(block, _)| *block == cell.block)
            {
                Some((_, count)) => *count += cell.built,
                None => {
                    tally[kinds] = (cell.block, cell.built);
                    kinds += 1;
                }
            }
        }
        let mut most = 0;
        for &(block, count) in &tally[..kinds] {
            if count > most {
                (most, merged.block) = (count, block);
            }
        }
        merged
    }

    /// What a cell of `level`, `2^level` blocks wide, is drawn as for its edits, as structures
    /// are (§5.6): its commonest block built where at least half its blocks are built, or air
    /// where at least half are built or cleared. Otherwise it's left as generated.
    pub fn drawn(self, level: u8) -> Option<BlockType> {
        let half = 1 << (3 * level as u32 - 1);
        if self.built >= half {
            Some(self.block)
        } else if self.built + self.cleared >= half {
            Some(BlockType::Air)
        } else {
            None
        }
    }
}

/// The 8 cells, or blocks, half as wide that make up `cell`, in a fixed order.
pub fn children(cell: I64Vec3) -> impl Iterator<Item = I64Vec3> {
    (0..8).map(move |i| cell * 2 + I64Vec3::new(i & 1, (i >> 1) & 1, i >> 2))
}

/// The 2-block cell at `cell`, in level 1's cells, as the edit layer has it.
pub fn summarise(edits: &Edits, cell: I64Vec3) -> CellEdits {
    CellEdits::merge(children(cell).map(|block| {
        edits.get(block).map_or(CellEdits::default(), |block| {
            CellEdits::of_block(block.kind)
        })
    }))
}

/// The edits made in one LOD node's cells, and in the ring of cells round it that its mesh looks
/// at, by where they are in it, from -1 to `CHUNK_SIZE` each way.
#[derive(Clone)]
pub struct NodeEdits {
    level: u8,
    cells: HashMap<u16, CellEdits>,
    // how many of them change what's drawn
    drawn: usize,
}

impl NodeEdits {
    fn new(level: u8) -> Self {
        Self {
            level,
            cells: HashMap::new(),
            drawn: 0,
        }
    }

    fn put(&mut self, at: IVec3, edits: CellEdits) {
        let drawn = |cell: Option<CellEdits>| cell.and_then(|c| c.drawn(self.level)).is_some();
        let was = self.cells.insert(pack(at), edits);
        self.drawn = self.drawn + drawn(Some(edits)) as usize - drawn(was) as usize;
    }

    /// The cells whose edits change what's drawn, and what each is drawn as.
    pub fn drawn(&self) -> impl Iterator<Item = (IVec3, BlockType)> + '_ {
        self.cells
            .iter()
            .filter_map(|(&at, cell)| Some((unpack(at), cell.drawn(self.level)?)))
    }
}

// A padded cell's place, -1 to `CHUNK_SIZE` each way, packed into 5 bits apiece.
fn pack(at: IVec3) -> u16 {
    let at = (at + 1).as_u16vec3();
    (at.x << 10) | (at.y << 5) | at.z
}

fn unpack(at: u16) -> IVec3 {
    IVec3::new((at >> 10) as i32, (at >> 5 & 31) as i32, (at & 31) as i32) - 1
}

/// The edit layer as LOD nodes draw it (§4.2): for each level up to `EDITED_LEVELS`, its cells
/// that have edits in them, each made up of the 8 below it, the bottom ones of blocks. A level's
/// cells are its nodes' cells, so a node finds what to draw at once, and an edit redraws only the
/// nodes whose cells it changes the look of, which most don't, beyond the finest.
#[derive(Default)]
pub struct LodEdits {
    // each node with edited cells in it or the ring round it
    nodes: HashMap<LodKey, Arc<NodeEdits>>,
    // the nodes whose cells have changed what they draw, kept from when something first asks,
    // so a server drawing nothing keeps none, see `take_redrawn`
    redrawn: Option<HashSet<LodKey>>,
}

impl LodEdits {
    /// The LOD edits of everything in `edits`.
    pub fn of(edits: &Edits) -> Self {
        let mut lod_edits = Self::default();
        let cells: HashSet<I64Vec3> = edits
            .chunks()
            .flat_map(|coord| {
                let origin = coord.0 * CHUNK_SIZE as i64;
                edits.in_chunk(coord).map(move |(local, _)| {
                    (origin + local.as_i64vec3()).div_euclid(I64Vec3::splat(2))
                })
            })
            .collect();
        lod_edits.set(cells.into_iter().map(|cell| (cell, summarise(edits, cell))));
        lod_edits
    }

    /// Brings the cells over `block` up to date with `edits`, as it's been edited.
    pub fn update(&mut self, edits: &Edits, block: I64Vec3) {
        let mut cell = block.div_euclid(I64Vec3::splat(2));
        let mut merged = summarise(edits, cell);
        // each cell is made of those under it alone, so the first unchanged leaves the rest
        for level in 1..=EDITED_LEVELS {
            if !self.put(level, cell, merged) {
                break;
            }
            cell = cell.div_euclid(I64Vec3::splat(2));
            merged = self.merged(level + 1, cell);
        }
    }

    /// Puts in 2-block cells, level 1's, as edits have made them, and works out the coarser
    /// cells over them.
    pub fn set(&mut self, cells: impl IntoIterator<Item = (I64Vec3, CellEdits)>) {
        let mut changed: HashSet<I64Vec3> = HashSet::new();
        for (cell, edits) in cells {
            if self.put(1, cell, edits) {
                changed.insert(cell);
            }
        }
        for level in 2..=EDITED_LEVELS {
            let parents: HashSet<I64Vec3> = changed
                .iter()
                .map(|cell| cell.div_euclid(I64Vec3::splat(2)))
                .collect();
            changed.clear();
            for cell in parents {
                if self.put(level, cell, self.merged(level, cell)) {
                    changed.insert(cell);
                }
            }
        }
    }

    // The cell of `level` at `cell`, as the 8 under it make it.
    fn merged(&self, level: u8, cell: I64Vec3) -> CellEdits {
        // all in one node, as a node is an even number of cells across
        let key = node_of(level - 1, cell * 2);
        let node = self.nodes.get(&key);
        CellEdits::merge(children(cell).map(|child| {
            let at = pack((child - key.min_cell()).as_ivec3());
            node.and_then(|node| node.cells.get(&at).copied())
                .unwrap_or_default()
        }))
    }

    /// The cell of `level` at `cell`, in that level's cells, if anything in it's been edited.
    pub fn cell(&self, level: u8, cell: I64Vec3) -> Option<CellEdits> {
        let key = node_of(level, cell);
        let at = (cell - key.min_cell()).as_ivec3();
        self.nodes.get(&key)?.cells.get(&pack(at)).copied()
    }

    /// The 2-block cells in the chunk at `coord` with edits in them, in level 1's cells.
    pub fn chunk_cells(
        &self,
        coord: ChunkCoordinate,
    ) -> impl Iterator<Item = (I64Vec3, CellEdits)> + '_ {
        let key = LodKey::containing(coord, 1);
        let size = CHUNK_SIZE as i64 / 2;
        let min = coord.0 * size;
        let node = self.nodes.get(&key);
        node.into_iter().flat_map(move |node| {
            node.cells.iter().filter_map(move |(&at, &edits)| {
                let cell = key.min_cell() + unpack(at).as_i64vec3();
                let local = cell - min;
                (local.cmpge(I64Vec3::ZERO).all() && local.cmplt(I64Vec3::splat(size)).all())
                    .then_some((cell, edits))
            })
        })
    }

    /// The edits the node at `key` draws, if any change what it draws.
    pub fn node(&self, key: LodKey) -> Option<Arc<NodeEdits>> {
        self.nodes.get(&key).filter(|node| node.drawn > 0).cloned()
    }

    /// The nodes whose cells have changed what they draw since this was last called. Nothing's
    /// kept until it first is.
    pub fn take_redrawn(&mut self) -> HashSet<LodKey> {
        self.redrawn.replace(HashSet::new()).unwrap_or_default()
    }

    // Puts `edits` in as the cell of `level` at `cell`, in the node it's in and in those it's
    // in the ring of, and returns whether that changed it.
    fn put(&mut self, level: u8, cell: I64Vec3, edits: CellEdits) -> bool {
        let was = self.cell(level, cell);
        if was == Some(edits) {
            return false;
        }
        let redraws = was.and_then(|c| c.drawn(level)) != edits.drawn(level);
        let owner = node_of(level, cell);
        let local = (cell - owner.min_cell()).as_ivec3();
        let size = CHUNK_SIZE as i32;
        // along each axis, the node it's in, and the one before or after it too if it's on
        // that one's edge
        let across = |c: i32| {
            let before = (c == 0).then_some((-1, size));
            let after = (c == size - 1).then_some((1, -1));
            std::iter::once((0, c)).chain(before).chain(after)
        };
        for (dx, x) in across(local.x) {
            for (dy, y) in across(local.y) {
                for (dz, z) in across(local.z) {
                    let key = LodKey {
                        level,
                        coord: owner.coord + I64Vec3::new(dx, dy, dz),
                    };
                    let node = self
                        .nodes
                        .entry(key)
                        .or_insert_with(|| Arc::new(NodeEdits::new(level)));
                    // a mesh being built from it keeps the old one
                    Arc::make_mut(node).put(IVec3::new(x, y, z), edits);
                    if let Some(redrawn) = self.redrawn.as_mut().filter(|_| redraws) {
                        redrawn.insert(key);
                    }
                }
            }
        }
        true
    }
}

// The node of `level` holding the cell of that level at `cell`.
fn node_of(level: u8, cell: I64Vec3) -> LodKey {
    LodKey {
        level,
        coord: cell.div_euclid(I64Vec3::splat(CHUNK_SIZE as i64)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Edits `blocks` to `to` in `edits` and `lod_edits`, a block at a time, as `World::edit` does.
    fn edit(
        edits: &mut Edits,
        lod_edits: &mut LodEdits,
        blocks: impl IntoIterator<Item = I64Vec3>,
        to: BlockType,
    ) {
        for block in blocks {
            edits.set(block, to);
            lod_edits.update(edits, block);
        }
    }

    // Every block in the box from `min` to `size` blocks across.
    fn cube(min: I64Vec3, size: I64Vec3) -> impl Iterator<Item = I64Vec3> {
        (0..size.x).flat_map(move |x| {
            (0..size.y).flat_map(move |y| (0..size.z).map(move |z| min + I64Vec3::new(x, y, z)))
        })
    }

    fn levels(redrawn: &HashSet<LodKey>) -> Vec<u8> {
        let mut levels: Vec<u8> = redrawn.iter().map(|key| key.level).collect();
        levels.sort_unstable();
        levels.dedup();
        levels
    }

    #[test]
    fn test_a_cell_draws_what_most_of_it_is_made() {
        let bricks = CellEdits::of_block(BlockType::Bricks);
        let air = CellEdits::of_block(BlockType::Air);
        // torches are too small to draw, so they're as good as clearing
        assert_eq!(air, CellEdits::of_block(BlockType::Torch));
        let cell = |cells: &[CellEdits]| {
            CellEdits::merge(
                cells
                    .iter()
                    .copied()
                    .chain(std::iter::repeat(CellEdits::default()))
                    .take(8),
            )
        };
        let planks = CellEdits::of_block(BlockType::Planks);
        assert_eq!(None, cell(&[bricks, bricks, bricks]).drawn(1));
        assert_eq!(
            Some(BlockType::Air),
            cell(&[bricks, bricks, air, air]).drawn(1)
        );
        assert_eq!(
            Some(BlockType::Bricks),
            cell(&[planks, bricks, bricks, planks, bricks]).drawn(1)
        );
        // the first of the commonest, wherever it's worked out
        assert_eq!(
            Some(BlockType::Planks),
            cell(&[planks, bricks, bricks, planks]).drawn(1)
        );
        let coarse = CellEdits::merge(
            [cell(&[bricks; 8]); 4]
                .into_iter()
                .chain([cell(&[air; 8]); 4]),
        );
        assert_eq!(Some(BlockType::Bricks), coarse.drawn(2));
        assert_eq!((32, 32), (coarse.built, coarse.cleared));
    }

    #[test]
    fn test_an_edit_redraws_only_the_levels_whose_cells_it_changes_the_look_of() {
        let (mut edits, mut lod_edits) = (Edits::default(), LodEdits::default());
        lod_edits.take_redrawn();
        // well inside a level-4 node, so no ring of a neighbour's
        let corner = I64Vec3::new(1000, 600, -1000).div_euclid(I64Vec3::splat(256)) * 256 + 80;
        // a block, and three more in its cell, which then has half of it built
        edit(&mut edits, &mut lod_edits, [corner], BlockType::Bricks);
        assert!(lod_edits.take_redrawn().is_empty());
        let cell = cube(corner, I64Vec3::new(2, 2, 1)).skip(1);
        edit(&mut edits, &mut lod_edits, cell, BlockType::Bricks);
        let redrawn = lod_edits.take_redrawn();
        assert_eq!(vec![1], levels(&redrawn));
        assert_eq!(1, redrawn.len());
        // a block 8 across, which a level-3 cell is and a level-4 cell, 16 across, isn't half of
        edit(
            &mut edits,
            &mut lod_edits,
            cube(corner, I64Vec3::splat(8)),
            BlockType::Bricks,
        );
        assert_eq!(vec![1, 2, 3], levels(&lod_edits.take_redrawn()));
        assert_eq!(
            None,
            lod_edits
                .cell(4, corner.div_euclid(I64Vec3::splat(16)))
                .unwrap()
                .drawn(4)
        );
        // a slab beside it, half of some level-1 cells and a quarter of level-2 ones
        let slab = cube(corner + I64Vec3::X * 8, I64Vec3::new(8, 8, 1));
        edit(&mut edits, &mut lod_edits, slab, BlockType::Bricks);
        assert_eq!(vec![1], levels(&lod_edits.take_redrawn()));
        // and on to half the level-4 cell
        let rest = cube(corner + I64Vec3::new(8, 0, 1), I64Vec3::new(8, 8, 7))
            .chain(cube(corner + I64Vec3::Z * 8, I64Vec3::new(16, 8, 8)));
        edit(&mut edits, &mut lod_edits, rest, BlockType::Bricks);
        assert_eq!(vec![1, 2, 3, 4], levels(&lod_edits.take_redrawn()));
        let node = LodKey::containing(ChunkCoordinate::containing(corner), 4);
        let drawn: Vec<_> = lod_edits.node(node).unwrap().drawn().collect();
        assert_eq!(1, drawn.len());
        assert_eq!(BlockType::Bricks, drawn[0].1);
    }

    #[test]
    fn test_a_cell_on_a_nodes_edge_is_in_its_neighbours_ring_and_redraws_it() {
        let (mut edits, mut lod_edits) = (Edits::default(), LodEdits::default());
        lod_edits.take_redrawn();
        // the level-1 cell in the corner of the level-1 node at the origin, against seven more
        edit(
            &mut edits,
            &mut lod_edits,
            cube(I64Vec3::ZERO, I64Vec3::splat(2)),
            BlockType::Stone,
        );
        let redrawn = lod_edits.take_redrawn();
        assert_eq!(8, redrawn.len());
        let beyond = LodKey {
            level: 1,
            coord: I64Vec3::NEG_ONE,
        };
        assert!(redrawn.contains(&beyond));
        let ring: Vec<_> = lod_edits.node(beyond).unwrap().drawn().collect();
        assert_eq!(vec![(IVec3::splat(16), BlockType::Stone)], ring);
        let own: Vec<_> = lod_edits
            .node(LodKey {
                level: 1,
                coord: I64Vec3::ZERO,
            })
            .unwrap()
            .drawn()
            .collect();
        assert_eq!(vec![(IVec3::ZERO, BlockType::Stone)], own);
        assert!(lod_edits
            .node(LodKey {
                level: 1,
                coord: I64Vec3::ONE
            })
            .is_none());
    }

    #[test]
    fn test_worked_out_at_once_or_an_edit_at_a_time_they_agree() {
        let (mut edits, mut lod_edits) = (Edits::default(), LodEdits::default());
        let corner = I64Vec3::new(-37, 500, 21);
        edit(
            &mut edits,
            &mut lod_edits,
            cube(corner, I64Vec3::new(20, 30, 30)),
            BlockType::Bricks,
        );
        edit(
            &mut edits,
            &mut lod_edits,
            cube(corner + 3, I64Vec3::new(5, 40, 5)),
            BlockType::Air,
        );
        let whole = LodEdits::of(&edits);
        for level in 1..=EDITED_LEVELS {
            for block in cube(corner - 8, I64Vec3::new(36, 56, 46)) {
                let cell = block.div_euclid(I64Vec3::splat(1 << level));
                assert_eq!(
                    whole.cell(level, cell),
                    lod_edits.cell(level, cell),
                    "{level} {cell}"
                );
            }
        }
        // and a chunk's 2-block cells are those of level 1 in it
        let coord = ChunkCoordinate::containing(corner + I64Vec3::new(5, 20, 15));
        let cells: Vec<_> = lod_edits.chunk_cells(coord).collect();
        assert_eq!(512, cells.len());
        for (cell, edits) in cells {
            assert_eq!(ChunkCoordinate::containing(cell * 2), coord);
            assert_eq!(Some(edits), lod_edits.cell(1, cell));
        }
    }
}
