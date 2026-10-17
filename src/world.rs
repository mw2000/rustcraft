use std::{fmt::Debug, sync::Arc};

use bevy::prelude::Resource;
use bevy::{
    math::{I64Vec3, Vec3},
    platform::collections::HashMap,
};

use crate::block::BlockType;
use crate::chunks::generate::noise::NoiseGenerator;
use crate::chunks::generate::region_store::{resident_cap_for, RegionStore};
use crate::chunks::generate::WORLD_HEIGHT;
use crate::edits::Edits;

use super::chunks::chunk::{ChunkCoordinate, ChunkData, ColumnSet, CHUNK_SIZE};

/// A loaded chunk whose roof an edit changed, see `World::take_reroofed`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Reroofed {
    pub chunk: ChunkCoordinate,
    /// Whether that roofed it over entirely or opened it, which decides whether it and the
    /// chunks against it are sealed, see `World::is_chunk_sealed`.
    pub sealed_changed: bool,
}

#[derive(Resource)]
pub struct World {
    seed: u32,
    pub height: u64,
    chunks: HashMap<ChunkCoordinate, Arc<ChunkData>>,
    edits: Edits,
    // chunks whose roofs edits have changed, see `take_reroofed`
    reroofed: Vec<Reroofed>,
    // what lies below and above the world, which isn't generated, see `beyond_world`
    below: Arc<ChunkData>,
    above: Arc<ChunkData>,
    pub region_store: Arc<RegionStore>,
    spawn: Vec3,
}

impl World {
    /// `region_distance` is how far, in chunks, terrain is built from full-detail regions,
    /// see `LodLayout::region_reach`.
    pub fn new(region_distance: u32) -> Self {
        let seed = Self::requested_seed().unwrap_or_else(rand::random);
        Self::from_save(seed, Edits::default(), region_distance)
    }

    /// The world with the given seed and edits, as saved.
    pub fn from_save(seed: u32, edits: Edits, region_distance: u32) -> Self {
        let height = WORLD_HEIGHT;
        let noise_generator = Arc::new(NoiseGenerator::new(seed));
        let (spawn_x, spawn_z, spawn_height) = noise_generator.find_spawn();
        let region_distance_blocks = region_distance as i64 * CHUNK_SIZE as i64;
        let mut below = ChunkData::from_cells(std::iter::repeat_n(
            BlockType::Stone,
            (CHUNK_SIZE as usize).pow(3),
        ));
        below.roofed = ColumnSet::ALL;
        Self {
            seed,
            height,
            chunks: HashMap::new(),
            edits,
            reroofed: Vec::new(),
            below: Arc::new(below),
            above: Arc::new(ChunkData::default()),
            region_store: Arc::new(
                RegionStore::new(noise_generator)
                    .with_resident_cap(resident_cap_for(region_distance_blocks)),
            ),
            spawn: Vec3::new(spawn_x as f32, spawn_height as f32 + 20.0, spawn_z as f32),
        }
    }

    /// The seed `RC_SEED` asks a new world to have, to repeat a world for chasing bugs that
    /// depend on its terrain.
    pub fn requested_seed() -> Option<u32> {
        std::env::var("RC_SEED").ok().and_then(|s| s.parse().ok())
    }

    pub fn seed(&self) -> u32 {
        self.seed
    }

    pub fn edits_mut(&mut self) -> &mut Edits {
        &mut self.edits
    }

    pub fn spawn(&self) -> Vec3 {
        self.spawn
    }

    /// Loads a generated chunk, with the edits made in it put back. Edits over it can have opened
    /// or roofed its columns, so they're taken from the chunk over it if that's loaded, and passed
    /// on to those under it.
    pub fn insert_chunk(&mut self, coord: ChunkCoordinate, mut data: ChunkData) -> Arc<ChunkData> {
        self.edits.apply(coord, &mut data);
        let layers = Self::layers(self.height);
        let edited = |y| {
            let chunk = ChunkCoordinate(I64Vec3::new(coord.0.x, y, coord.0.z));
            self.edits.in_chunk(chunk).next().is_some()
        };
        let edited_above = (coord.0.y + 1..layers.end).any(edited);
        if edited_above {
            if let Some(above) = self.chunks.get(&ChunkCoordinate(coord.0 + I64Vec3::Y)) {
                data.roofed = above.roofed.union(above.opaque_columns());
            }
        }
        let data = Arc::new(data);
        self.chunks.insert(coord, data.clone());
        if edited_above || edited(coord.0.y) {
            self.reroof_below(coord);
        }
        data
    }

    /// Changes the block at `block` to `to`, in the edit layer and at once in its chunk if
    /// that's loaded, so collision sees it straight away. Returns whether anything changed:
    /// nothing does beyond the top or bottom of the world, or if the block is already `to`.
    pub fn edit(&mut self, block: I64Vec3, to: BlockType) -> bool {
        let coord = ChunkCoordinate::containing(block);
        if !Self::layers(self.height).contains(&coord.0.y) {
            return false;
        }
        if let Some(data) = self.chunks.get_mut(&coord) {
            let local = ChunkCoordinate::local_block(block);
            if data.get_block_at(local) == to {
                return false;
            }
            // copied only if a meshing task still holds the old blocks
            Arc::make_mut(data).set_block_at(local, to);
            self.reroof_below(coord);
        }
        self.edits.set(block, to);
        true
    }

    // Brings the roofs of the loaded chunks under `coord` in line with it: a column is roofed
    // over a chunk when it's roofed over the one above it or that has an opaque block in it.
    // Under a chunk that already agrees, every one does. A scan of the runs of each chunk
    // changed, and almost always none are.
    fn reroof_below(&mut self, coord: ChunkCoordinate) {
        let Some(data) = self.chunks.get(&coord) else {
            return;
        };
        let mut roof = data.roofed.union(data.opaque_columns());
        let mut below = coord;
        loop {
            below.0.y -= 1;
            let Some(data) = self.chunks.get_mut(&below) else {
                break;
            };
            if data.roofed == roof {
                break;
            }
            let sealed = |roof| roof == ColumnSet::ALL;
            self.reroofed.push(Reroofed {
                chunk: below,
                sealed_changed: sealed(data.roofed) != sealed(roof),
            });
            Arc::make_mut(data).roofed = roof;
            roof = roof.union(data.opaque_columns());
        }
    }

    /// The loaded chunks whose roofs changed since this was last called, for the chunks lit
    /// through them to be meshed again.
    pub fn take_reroofed(&mut self) -> Vec<Reroofed> {
        std::mem::take(&mut self.reroofed)
    }

    /// The chunk layers the world is generated in, from the bottom up.
    pub fn layers(height: u64) -> std::ops::Range<i64> {
        0..height.div_ceil(CHUNK_SIZE as u64) as i64
    }

    // Below the world is solid and above it open sky, standing in for chunks that are never
    // generated, so the chunks at its bottom and top still have neighbours to mesh against.
    fn chunk(&self, coord: ChunkCoordinate) -> Option<&Arc<ChunkData>> {
        let layers = Self::layers(self.height);
        if coord.0.y < layers.start {
            Some(&self.below)
        } else if coord.0.y >= layers.end {
            Some(&self.above)
        } else {
            self.chunks.get(&coord)
        }
    }

    pub fn get_chunk_data(&self, coord: ChunkCoordinate) -> Option<Arc<ChunkData>> {
        self.chunk(coord).cloned()
    }

    pub fn clear_chunk(&mut self, coord: ChunkCoordinate) {
        self.chunks.remove(&coord);
    }

    pub fn adjacent_chunk_data(&self, coord: ChunkCoordinate) -> Vec<Option<Arc<ChunkData>>> {
        coord
            .neighbors_26()
            .into_iter()
            .map(|c| self.get_chunk_data(c))
            .collect()
    }

    pub fn is_chunk_generated(&self, coord: ChunkCoordinate) -> bool {
        self.chunk(coord).is_some()
    }

    /// Whether the chunk and the chunks beside it have ground over every column, so it can only
    /// be seen from in the caves. A cave reaching the sky down a shaft opens the columns over
    /// it, and one coming out of a hillside opens those of the chunk beside it there. The
    /// chunks above and below make no difference: a shaft through either runs through this one.
    pub fn is_chunk_sealed(&self, coord: ChunkCoordinate) -> bool {
        let sealed = |c| {
            self.chunk(c)
                .is_some_and(|data| data.roofed == ColumnSet::ALL)
        };
        let beside = [I64Vec3::X, I64Vec3::NEG_X, I64Vec3::Z, I64Vec3::NEG_Z];
        sealed(coord)
            && beside
                .into_iter()
                .all(|side| sealed(ChunkCoordinate(coord.0 + side)))
    }

    /// Whether something opaque is over `block`, which keeps the sky from it.
    pub fn is_under_ground(&self, block: I64Vec3) -> bool {
        let coord = ChunkCoordinate::containing(block);
        let Some(data) = self.chunk(coord) else {
            return false;
        };
        let local = ChunkCoordinate::local_block(block);
        data.roofed.contains(local.x, local.z)
            || (local.y + 1..CHUNK_SIZE).any(|y| data.get_block_at(local.with_y(y)).is_opaque())
    }

    /// Whether the chunk and the chunks against its faces are all opaque, so it has no faces to draw.
    pub fn is_chunk_buried(&self, coord: ChunkCoordinate) -> bool {
        let opaque = |c| self.chunk(c).is_some_and(|data| data.opaque());
        opaque(coord) && coord.adjacent().into_iter().all(opaque)
    }

    /// The block at `block`, or air in a chunk not generated yet.
    pub fn get_block_at(&self, block: I64Vec3) -> BlockType {
        self.chunk(ChunkCoordinate::containing(block))
            .map_or(BlockType::Air, |data| {
                data.get_block_at(ChunkCoordinate::local_block(block))
            })
    }
}

impl Debug for World {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("World").field("seed", &self.seed).finish()
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::I64Vec3;

    use super::World;
    use crate::block::BlockType;
    use crate::chunks::chunk::{ChunkCoordinate, ChunkData, CHUNK_SIZE};
    use crate::chunks::generate::generator::generate_chunk;

    #[test]
    fn test_chunk_is_buried_only_when_it_and_its_face_neighbours_are_opaque() {
        let filled = |block| {
            let mut data = ChunkData::default();
            for x in 0..CHUNK_SIZE {
                for y in 0..CHUNK_SIZE {
                    for z in 0..CHUNK_SIZE {
                        data.set_block_at(bevy::math::U16Vec3::new(x, y, z), block);
                    }
                }
            }
            data
        };
        let mut world = World::new(16);
        let centre = ChunkCoordinate(I64Vec3::ZERO);
        world.insert_chunk(centre, filled(BlockType::Stone));
        // its diagonal neighbours don't matter, as faces only meet the chunks against them
        for side in centre.adjacent() {
            world.insert_chunk(side, filled(BlockType::Dirt));
        }
        assert!(world.is_chunk_buried(centre));

        // ice and leaves show what's behind them
        world.insert_chunk(ChunkCoordinate(I64Vec3::Y), filled(BlockType::Ice));
        assert!(!world.is_chunk_buried(centre));
        world.insert_chunk(ChunkCoordinate(I64Vec3::Y), ChunkData::default());
        assert!(!world.is_chunk_buried(centre));
        // nor is a chunk buried by one not generated yet
        world.clear_chunk(ChunkCoordinate(I64Vec3::Y));
        assert!(!world.is_chunk_buried(centre));
    }

    #[test]
    fn test_edits_survive_their_chunk_being_unloaded_and_generated_again() {
        let mut world = World::new(16);
        let coord = ChunkCoordinate::containing(world.spawn().as_i64vec3());
        let generate = |world: &World| generate_chunk(world.region_store.clone(), coord);
        let generated = generate(&world);
        world.insert_chunk(coord, generate(&world));
        let origin = coord.0 * CHUNK_SIZE as i64;
        let (broken, built, untouched) = (
            origin + I64Vec3::new(3, 3, 3),
            origin + I64Vec3::new(9, 12, 4),
            origin + I64Vec3::new(12, 5, 14),
        );
        // whatever was generated there, something else
        let other = |block: I64Vec3| match world.get_block_at(block) {
            BlockType::Air => BlockType::Log,
            _ => BlockType::Air,
        };
        let edits = [(broken, other(broken)), (built, other(built))];
        for (block, to) in edits {
            assert!(world.edit(block, to));
            assert!(!world.edit(block, to), "already {to:?}");
            assert_eq!(to, world.get_block_at(block));
        }
        assert!(!world.edit(I64Vec3::new(0, -1, 0), BlockType::Air));

        world.clear_chunk(coord);
        world.insert_chunk(coord, generate(&world));
        for (block, to) in edits {
            assert_eq!(to, world.get_block_at(block));
        }
        let local = ChunkCoordinate::local_block(untouched);
        assert_eq!(generated.get_block_at(local), world.get_block_at(untouched));
    }

    #[test]
    fn test_beyond_the_world_is_solid_below_and_open_above() {
        let world = World::new(16);
        let top = World::layers(world.height).end;
        let (below, inside, above) = (
            ChunkCoordinate(I64Vec3::new(3, -1, 2)),
            ChunkCoordinate(I64Vec3::new(3, 0, 2)),
            ChunkCoordinate(I64Vec3::new(3, top, 2)),
        );
        assert!(world.is_chunk_generated(below) && world.is_chunk_generated(above));
        assert!(!world.is_chunk_generated(inside));
        assert!(world.get_chunk_data(below).unwrap().opaque());
        assert!(world.get_chunk_data(above).unwrap().empty());
        assert_eq!(BlockType::Stone, world.get_block_at(I64Vec3::new(0, -1, 0)));
    }
}
