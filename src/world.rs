use std::{fmt::Debug, sync::Arc};

use bevy::app::App;
use bevy::ecs::schedule::{IntoScheduleConfigs, ScheduleLabel, SystemSet};
use bevy::prelude::Resource;
use bevy::{
    math::{I64Vec3, IVec3, Vec3},
    platform::collections::HashMap,
};

use crate::block::{Block, BlockType};
use crate::chunks::generate::light::{BlockLight, BLOCK_LIGHT_REACH};
use crate::chunks::generate::noise::NoiseGenerator;
use crate::chunks::generate::region_store::{resident_cap_for, RegionStore};
use crate::chunks::generate::WORLD_HEIGHT;
use crate::chunks::lod::LodEdits;
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

/// Run to leave the world being played, saving it and putting everything that belongs to it
/// away, so the App is as it was before it opened and another world can be opened in it: from
/// the pause menu's Save and Quit to Title, and when the server's lost.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct LeaveWorld;

/// The steps of `LeaveWorld`, in order.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum LeaveWorldSystems {
    /// Seeing off whoever's joined, and letting go of the server joined.
    Disconnect,
    /// The last save, of everyone still there.
    Save,
    /// Putting away what's left: entities, and resources back as they started.
    Clear,
}

/// Puts `LeaveWorld`'s steps in order, for each plugin that adds to them.
pub fn order_leaving(app: &mut App) {
    use LeaveWorldSystems::*;
    app.configure_sets(LeaveWorld, (Disconnect, Save, Clear).chain());
}

/// What keeps a chunk loaded, see `World::hold`.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Holder {
    /// The client, drawing it.
    Client,
    /// The server, for the players near it.
    Server,
}

impl Holder {
    fn bit(self) -> u8 {
        1 << self as u8
    }
}

#[derive(Resource)]
pub struct World {
    seed: u32,
    pub height: u64,
    chunks: HashMap<ChunkCoordinate, Arc<ChunkData>>,
    // what holds each chunk loaded or on its way, as a bit for each `Holder`
    holders: HashMap<ChunkCoordinate, u8>,
    edits: Edits,
    // the edits as LOD nodes draw them, worked out from `edits`, or for a client joined to a
    // server, sent by it, as it has only the edits near its player (§4.4)
    lod_edits: LodEdits,
    // whether this App's server decides what happens in it, see `is_served_here`
    served_here: bool,
    // chunks whose roofs edits have changed, see `take_reroofed`
    reroofed: Vec<Reroofed>,
    // blocks whose edits have changed block light, see `take_relit`
    relit: Vec<I64Vec3>,
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
            holders: HashMap::new(),
            lod_edits: LodEdits::of(&edits),
            served_here: true,
            edits,
            reroofed: Vec::new(),
            relit: Vec::new(),
            below: Arc::new(below),
            above: Arc::new(ChunkData::default()),
            region_store: Arc::new(
                RegionStore::new(noise_generator)
                    .with_resident_cap(resident_cap_for(region_distance_blocks)),
            ),
            spawn: Vec3::new(spawn_x as f32, spawn_height as f32 + 20.0, spawn_z as f32),
        }
    }

    /// The world with the given seed for a client joined to a server, which is sent the edits
    /// near its player and, as LOD nodes draw them, those further off. Also the title screen's
    /// scenery, which has no server and no edits.
    pub fn joined(seed: u32, region_distance: u32) -> Self {
        Self {
            served_here: false,
            ..Self::from_save(seed, Edits::default(), region_distance)
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

    /// Whether this App's server decides what happens in this world: not for a client's copy
    /// of one served elsewhere, nor the title screen's scenery.
    pub fn is_served_here(&self) -> bool {
        self.served_here
    }

    pub fn edits(&self) -> &Edits {
        &self.edits
    }

    pub fn edits_mut(&mut self) -> &mut Edits {
        &mut self.edits
    }

    pub fn lod_edits(&self) -> &LodEdits {
        &self.lod_edits
    }

    pub fn lod_edits_mut(&mut self) -> &mut LodEdits {
        &mut self.lod_edits
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
    pub fn edit(&mut self, block: I64Vec3, to: impl Into<Block>) -> bool {
        let to = to.into();
        let coord = ChunkCoordinate::containing(block);
        if !Self::layers(self.height).contains(&coord.0.y) {
            return false;
        }
        if let Some(data) = self.chunks.get_mut(&coord) {
            let local = ChunkCoordinate::local_block(block);
            let was = data.get(local);
            if was == to {
                return false;
            }
            // copied only if a meshing task still holds the old blocks
            Arc::make_mut(data).set_block_at(local, to);
            self.reroof_below(coord);
            // only what the client holds is drawn, as for roofs
            if self.holders.get(&coord) != Some(&Holder::Server.bit())
                && self.relights(block, was.kind, to.kind)
            {
                self.relit.push(block);
            }
        }
        self.edits.set(block, to);
        if self.served_here {
            self.lod_edits.update(&self.edits, block);
        }
        true
    }

    // Whether changing `block` from `was` to `to` changes block light: it starts or stops
    // giving off light, or it lets light through differently where some block's light reaches.
    // Most edits are far from any, so they leave meshes rebuilt only as far as sky light goes.
    fn relights(&self, block: I64Vec3, was: BlockType, to: BlockType) -> bool {
        if was.emits() > 0 || to.emits() > 0 {
            return true;
        }
        if was.light_opacity() == to.light_opacity() {
            return false;
        }
        let reach = I64Vec3::splat(BLOCK_LIGHT_REACH as i64);
        let (first, last) = (
            ChunkCoordinate::containing(block - reach).0,
            ChunkCoordinate::containing(block + reach).0,
        );
        let size = CHUNK_SIZE as i64;
        (first.x..=last.x).any(|x| {
            (first.y..=last.y).any(|y| {
                (first.z..=last.z).any(|z| {
                    let coord = I64Vec3::new(x, y, z);
                    self.chunks
                        .get(&ChunkCoordinate(coord))
                        .is_some_and(|data| {
                            data.emitters().any(|(local, level)| {
                                let from = coord * size + local.as_i64vec3();
                                (from - block).abs().element_sum() < level as i64
                            })
                        })
                })
            })
        })
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
            // only what the client holds is drawn, so a server on its own records nothing
            if self.holders.get(&below) != Some(&Holder::Server.bit()) {
                let sealed = |roof| roof == ColumnSet::ALL;
                self.reroofed.push(Reroofed {
                    chunk: below,
                    sealed_changed: sealed(data.roofed) != sealed(roof),
                });
            }
            Arc::make_mut(data).roofed = roof;
            roof = roof.union(data.opaque_columns());
        }
    }

    /// The loaded chunks whose roofs changed since this was last called, for the chunks lit
    /// through them to be meshed again. Those the server alone holds are left out, as nothing
    /// draws them.
    pub fn take_reroofed(&mut self) -> Vec<Reroofed> {
        std::mem::take(&mut self.reroofed)
    }

    /// The blocks whose edits changed block light since this was last called, for the chunks
    /// that light reaches to be meshed again. As for `take_reroofed`, those in chunks the server
    /// alone holds are left out.
    pub fn take_relit(&mut self) -> Vec<I64Vec3> {
        std::mem::take(&mut self.relit)
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

    /// Has `holder` keep the chunk at `coord` loaded once it's generated, until it lets go of
    /// it. Returns whether anything held it already, in which case it's loaded or on its way.
    pub fn hold(&mut self, coord: ChunkCoordinate, holder: Holder) -> bool {
        let holders = self.holders.entry(coord).or_default();
        let held = *holders != 0;
        *holders |= holder.bit();
        held
    }

    /// Lets go of the chunk at `coord` for `holder`, and unloads it once nothing holds it, so
    /// the client unloading behind the camera leaves what the server keeps for a player, and
    /// the other way round.
    pub fn release(&mut self, coord: ChunkCoordinate, holder: Holder) {
        if let Some(holders) = self.holders.get_mut(&coord) {
            *holders &= !holder.bit();
            if *holders != 0 {
                return;
            }
            self.holders.remove(&coord);
        }
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
            || (local.y + 1..CHUNK_SIZE).any(|y| data.get_block_at(local.with_y(y)).blocks_light())
    }

    /// The blocks in the loaded chunks giving off light near enough `block` to light it, each
    /// with the level it gives off, nearest first.
    pub fn emitters_near(&self, block: I64Vec3) -> Vec<(I64Vec3, u8)> {
        let reach = I64Vec3::splat(BLOCK_LIGHT_REACH as i64);
        let (first, last) = (
            ChunkCoordinate::containing(block - reach).0,
            ChunkCoordinate::containing(block + reach).0,
        );
        let size = CHUNK_SIZE as i64;
        let mut near = Vec::new();
        for x in first.x..=last.x {
            for y in first.y..=last.y {
                for z in first.z..=last.z {
                    let coord = I64Vec3::new(x, y, z);
                    let Some(data) = self.chunk(ChunkCoordinate(coord)) else {
                        continue;
                    };
                    near.extend(data.emitters().filter_map(|(local, level)| {
                        let from = coord * size + local.as_i64vec3();
                        let lights = (from - block).abs().element_sum() < level as i64;
                        lights.then_some((from, level))
                    }));
                }
            }
        }
        near.sort_by_key(|(from, _)| (*from - block).abs().element_sum());
        near
    }

    /// The light the blocks giving it off cast on `block` (§5.5), spread round walls as meshes
    /// are lit, and 0 where none are near, which is all that's looked at then.
    pub fn block_light(&self, block: I64Vec3) -> u8 {
        let emitters: Vec<(IVec3, u8)> = self
            .emitters_near(block)
            .into_iter()
            .map(|(from, level)| ((from - block).as_ivec3(), level))
            .collect();
        if emitters.is_empty() {
            return 0;
        }
        let opacity = |at: IVec3| self.get_block_at(block + at.as_i64vec3()).light_opacity();
        BlockLight::new(IVec3::ZERO, IVec3::ONE, &emitters, opacity).level(IVec3::ZERO)
    }

    /// Whether the chunk and the chunks against its faces are all opaque, so it has no faces to draw.
    pub fn is_chunk_buried(&self, coord: ChunkCoordinate) -> bool {
        let opaque = |c| self.chunk(c).is_some_and(|data| data.opaque());
        opaque(coord) && coord.adjacent().into_iter().all(opaque)
    }

    /// The block at `block`, or air in a chunk not generated yet.
    pub fn get_block_at(&self, block: I64Vec3) -> BlockType {
        self.get(block).kind
    }

    /// The block at `block` with its state, or air in a chunk not generated yet.
    pub fn get(&self, block: I64Vec3) -> Block {
        self.chunk(ChunkCoordinate::containing(block))
            .map_or(Block::AIR, |data| {
                data.get(ChunkCoordinate::local_block(block))
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

    use super::{Holder, Reroofed, World};
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
        world.release(ChunkCoordinate(I64Vec3::Y), Holder::Client);
        assert!(!world.is_chunk_buried(centre));
    }

    #[test]
    fn test_block_light_spreads_from_torches_round_walls_and_fades_out() {
        let mut world = World::new(16);
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    let coord = ChunkCoordinate(I64Vec3::new(x, y, z));
                    world.insert_chunk(coord, ChunkData::default());
                }
            }
        }
        assert_eq!(0, world.block_light(I64Vec3::ZERO));
        world.edit(I64Vec3::ZERO, BlockType::Torch);
        let light = |at: [i64; 3]| world.block_light(I64Vec3::from_array(at));
        assert_eq!(
            (14, 13, 1, 0),
            (
                light([0; 3]),
                light([1, 0, 0]),
                light([7, 6, 0]),
                light([7, 7, 0])
            )
        );
        // and across chunk borders, and round a wall, the long way
        assert_eq!(10, light([-2, 0, -2]));
        let mut world = world;
        for y in -3..=3 {
            for z in -3..=3 {
                world.edit(I64Vec3::new(2, y, z), BlockType::Stone);
            }
        }
        assert!(world.block_light(I64Vec3::new(3, 0, 0)) < 13 - 2);
        assert_eq!(
            vec![(I64Vec3::ZERO, 14)],
            world.emitters_near(I64Vec3::new(3, 0, 0))
        );
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

        world.release(coord, Holder::Client);
        world.insert_chunk(coord, generate(&world));
        for (block, to) in edits {
            assert_eq!(to, world.get_block_at(block));
        }
        let local = ChunkCoordinate::local_block(untouched);
        assert_eq!(generated.get_block_at(local), world.get_block_at(untouched));
    }

    #[test]
    fn test_a_chunk_is_unloaded_only_once_nothing_holds_it() {
        let mut world = World::new(16);
        let coord = ChunkCoordinate(I64Vec3::new(1, 2, 3));
        assert!(!world.hold(coord, Holder::Client));
        assert!(
            world.hold(coord, Holder::Server),
            "on its way for the client"
        );
        world.insert_chunk(coord, ChunkData::default());
        // the client unloading it behind the camera leaves it for the server's players
        world.release(coord, Holder::Client);
        assert!(world.is_chunk_generated(coord));
        world.release(coord, Holder::Server);
        assert!(!world.is_chunk_generated(coord));
        assert!(!world.hold(coord, Holder::Client), "forgotten once let go");
    }

    #[test]
    fn test_reroofs_are_recorded_only_where_theyre_drawn() {
        let mut world = World::new(16);
        let (top, under) = (
            ChunkCoordinate(I64Vec3::new(0, 2, 0)),
            ChunkCoordinate(I64Vec3::new(0, 1, 0)),
        );
        for coord in [top, under] {
            world.hold(coord, Holder::Server);
            world.insert_chunk(coord, ChunkData::default());
        }
        let roof = top.0 * CHUNK_SIZE as i64;
        assert!(world.edit(roof, BlockType::Stone));
        assert!(world.is_under_ground(under.0 * CHUNK_SIZE as i64));
        assert_eq!(Vec::<Reroofed>::new(), world.take_reroofed());

        world.hold(under, Holder::Client);
        assert!(world.edit(roof + I64Vec3::X, BlockType::Stone));
        let reroofed = world.take_reroofed();
        assert_eq!(
            vec![under],
            reroofed.iter().map(|r| r.chunk).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_torches_and_walls_by_them_relight_the_chunks_their_light_reaches() {
        use crate::chunks::generate::generator::{chunks_meshed_with, generate_chunk_mesh};
        let mut world = World::new(16);
        let size = CHUNK_SIZE as i64;
        for x in -1..=2 {
            for y in 39..=41 {
                for z in -1..=1 {
                    world
                        .insert_chunk(ChunkCoordinate(I64Vec3::new(x, y, z)), ChunkData::default());
                }
            }
        }
        // a torch 12 blocks short of the next chunk to +x, further than sky light reaches
        let torch = I64Vec3::new(4, 40 * size + 1, 8);
        let beside = ChunkCoordinate(I64Vec3::new(1, 40, 0));
        assert!(!chunks_meshed_with(torch, false).any(|c| c == beside));
        let block_lit = |world: &World| {
            let mesh = generate_chunk_mesh(
                world.get_chunk_data(beside).unwrap(),
                world.adjacent_chunk_data(beside),
                beside,
                world.region_store.noise(),
                false,
            );
            mesh.opaque
                .contains_attribute(bevy::mesh::Mesh::ATTRIBUTE_TANGENT)
        };
        // a floor to light, across both chunks
        for x in 0..2 * size {
            world.edit(I64Vec3::new(x, torch.y - 1, 8), BlockType::Stone);
        }
        assert_eq!(
            Vec::<I64Vec3>::new(),
            world.take_relit(),
            "no light to change yet"
        );
        assert!(!block_lit(&world));

        world.edit(torch, BlockType::Torch);
        assert_eq!(vec![torch], world.take_relit());
        assert!(chunks_meshed_with(torch, true).any(|c| c == beside));
        assert!(block_lit(&world));
        // a wall by it changes where its light goes, while one out of its reach doesn't
        let wall = torch + I64Vec3::Z;
        world.edit(wall, BlockType::Stone);
        world.edit(torch + I64Vec3::new(40, 0, 0), BlockType::Stone);
        assert_eq!(vec![wall], world.take_relit());
        // and breaking it takes its light away again
        world.edit(torch, BlockType::Air);
        assert_eq!(vec![torch], world.take_relit());
        assert!(!block_lit(&world));
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
