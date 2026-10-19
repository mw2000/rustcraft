use std::time::{Duration, Instant};

use bevy::prelude::Resource;
use bevy::{
    asset::Handle,
    camera::Camera,
    ecs::hierarchy::ChildOf,
    ecs::{
        component::Component,
        entity::Entity,
        query::{Or, With, Without},
        system::{Commands, Query, Res, ResMut},
    },
    math::{Dir3, I64Vec3, Vec3},
    platform::collections::{HashMap, HashSet},
    tasks::futures,
    tasks::{AsyncComputeTaskPool, Task},
    transform::components::GlobalTransform,
};

use super::{
    chunk::{ChunkCoordinate, ChunkData},
    generate::generator::{chunks_meshed_with, generate_chunk, generate_chunk_mesh, ChunkMeshes},
    group::{GroupedChunk, MeshedChunk},
    lod::{GenerateLodMesh, LodKey, LodLayout, LodLoader},
    material::{ChunkMaterial, WaterMaterial},
};
use crate::world::{Holder, Reroofed, World};

#[derive(Component)]
pub struct Chunk {
    pub coord: ChunkCoordinate,
}

/// Generated with something in it, waiting for its neighbours before it's meshed.
#[derive(Component)]
pub struct DirtyChunk;

#[derive(Component)]
pub struct GenerateChunkData {
    task: Task<ChunkData>,
}

/// Waiting to be meshed, then being meshed.
#[derive(Component)]
pub struct GenerateChunkMesh {
    task: Option<Task<ChunkMeshes>>,
}

/// A chunk still on its way to being meshed.
pub type Unsettled = Or<(
    With<GenerateChunkData>,
    With<DirtyChunk>,
    With<GenerateChunkMesh>,
)>;

#[derive(Resource)]
pub struct ChunkLoader {
    layout: LodLayout,
    render_distance: u32,
    chunk_to_entity: HashMap<ChunkCoordinate, Entity>,
    spawn_queue: ChunkSpawnQueue,
    pub(super) material: Handle<ChunkMaterial>,
    pub(super) water_material: Handle<WaterMaterial>,
    max_in_flight_generating: usize,
    // see `DebugSettings::solid_leaves`
    pub(super) solid_leaves: bool,
    // the entity drawing each group of chunks, see `group::ChunkGroup`
    pub(super) groups: HashMap<LodKey, Entity>,
    // meshed chunks' meshes, until the rest of their group is ready to merge with them
    pub(super) held: HashMap<ChunkCoordinate, ChunkMeshes>,
    // groups with a held mesh, not yet being merged
    pub(super) waiting: HashSet<LodKey>,
    // groups with an edit in them since they were meshed, see `group::rebuild_groups`
    pub(crate) rebuilding: HashSet<LodKey>,
    // groups to rebuild for the sealed chunks the camera has come near, see
    // `mesh_nearing_sealed_chunks`
    pub(super) revealing: HashSet<LodKey>,
    // sealed chunks left unmeshed for being far from the camera, see `wants_mesh`
    pub(super) sealed: HashSet<ChunkCoordinate>,
    // whether the camera has ground over it, see `World::is_under_ground`
    camera_under_ground: bool,
    // the camera's chunk, and whether it was under ground, when `mesh_nearing_sealed_chunks`
    // last looked
    sealed_anchor: Option<(ChunkCoordinate, bool)>,
}

// Loading spends up to a time budget a frame rather than a fixed count of chunks, so it keeps up
// at any render distance and on any machine.
const GATHER_TIME_BUDGET: Duration = Duration::from_millis(3);
const MESH_APPLY_TIME_BUDGET: Duration = Duration::from_millis(4);
// How much of the spawn queue `gather_chunks` takes between checks of the time.
const SPAWN_BATCH_SIZE: usize = 64;
const SCAN_BATCH_SIZE: usize = 50_000;
// Meshing reads every chunk in the 3x3x3 around a chunk, up to sqrt(3) away, so chunk data is
// generated this much past the meshed chunks.
const NEIGHBOR_DATA_PADDING: u32 = 2;
// Sealed chunks are seen only from in the caves, so they're meshed only this near the camera,
// in chunks. From under ground, a cave can be seen along: rays cast every way from cave air ran
// at most about 110 blocks before meeting rock (99.9% under 75), and a chunk this far off is at
// least (9 - sqrt(3)) * 16, about 116 blocks, away.
const SEALED_MESH_RADIUS_UNDER_GROUND: f32 = 9.0;
// From the open, caves show only through their mouths and where they come out of hillsides,
// whose chunks and those beside them aren't sealed, so this is only for climbing down into one.
const SEALED_MESH_RADIUS_IN_THE_OPEN: f32 = 3.0;

// A cap on concurrent generation tasks, should generation fall behind: 512 at a render distance of
// 32, scaled with the area of the shell of chunks loading in as the camera moves.
fn max_in_flight(render_distance: u32) -> usize {
    let scale =
        (render_distance + NEIGHBOR_DATA_PADDING) as f64 / (32 + NEIGHBOR_DATA_PADDING) as f64;
    (512.0 * scale * scale).ceil() as usize
}

impl ChunkLoader {
    pub fn new(
        layout: LodLayout,
        world_height: u64,
        solid_leaves: bool,
        material: Handle<ChunkMaterial>,
        water_material: Handle<WaterMaterial>,
    ) -> Self {
        let render_distance = layout.full_detail_reach();
        Self {
            layout,
            render_distance,
            chunk_to_entity: HashMap::new(),
            spawn_queue: ChunkSpawnQueue::new(render_distance + NEIGHBOR_DATA_PADDING)
                .with_layers(World::layers(world_height)),
            material,
            water_material,
            max_in_flight_generating: max_in_flight(render_distance),
            solid_leaves,
            groups: HashMap::new(),
            held: HashMap::new(),
            waiting: HashSet::new(),
            rebuilding: HashSet::new(),
            revealing: HashSet::new(),
            sealed: HashSet::new(),
            camera_under_ground: false,
            sealed_anchor: None,
        }
    }

    pub fn group_entity(&self, key: LodKey) -> Option<Entity> {
        self.groups.get(&key).copied()
    }

    pub fn anchor(&self) -> ChunkCoordinate {
        self.spawn_queue.anchor
    }

    pub fn chunk_entity(&self, coord: ChunkCoordinate) -> Option<Entity> {
        self.chunk_to_entity.get(&coord).copied()
    }

    /// Lets go of every chunk this has loaded, as it's put away.
    pub fn release_all(&self, world: &mut World) {
        for &coord in self.chunk_to_entity.keys() {
            world.release(coord, Holder::Client);
        }
    }

    /// Whether the chunk at `coord` is drawn as it will be: loaded, meshed if there's anything
    /// to see of it, and in its group's mesh, for the loading screen to wait for.
    pub fn is_drawn(&self, coord: ChunkCoordinate, unsettled: &Query<(), Unsettled>) -> bool {
        self.chunk_to_entity
            .get(&coord)
            .is_some_and(|&entity| !unsettled.contains(entity))
            && !self.waiting.contains(&LodKey::containing(coord, 1))
    }

    #[cfg(test)]
    pub(crate) fn track(&mut self, coord: ChunkCoordinate, entity: Entity) {
        self.chunk_to_entity.insert(coord, entity);
    }
}

impl ChunkLoader {
    /// Has every loaded group rebuilt whose mesh a change to the block at `block` reaches,
    /// along with those lit through roofs the world's edits have changed, and as far as block
    /// light reaches from blocks whose edits changed it.
    pub fn rebuild_edited(&mut self, world: &mut World, block: I64Vec3) {
        self.rebuild(chunks_meshed_with(block, false));
        for relit in world.take_relit() {
            self.rebuild(chunks_meshed_with(relit, true));
        }
        self.rebuild_reroofed(world);
    }

    // Has the loaded groups holding `chunks` rebuilt; one not loaded yet has the edits put in as
    // it's generated.
    fn rebuild(&mut self, chunks: impl IntoIterator<Item = ChunkCoordinate>) {
        for chunk in chunks {
            if self.chunk_to_entity.contains_key(&chunk) {
                self.rebuilding.insert(LodKey::containing(chunk, 1));
            }
        }
    }

    // Has the groups rebuilt that a change to a chunk's roof reaches: the chunks under it, lit
    // through it, and if it's become sealed or stopped being, it and those against it.
    fn rebuild_reroofed(&mut self, world: &mut World) {
        for Reroofed {
            chunk,
            sealed_changed,
        } in world.take_reroofed()
        {
            let under = (-1..=1).flat_map(|x| {
                (-1..=1).map(move |z| ChunkCoordinate(chunk.0 + I64Vec3::new(x, -1, z)))
            });
            self.rebuild(under);
            if sealed_changed {
                self.rebuild(std::iter::once(chunk).chain(chunk.adjacent()));
            }
        }
    }

    /// Whether the chunk at `coord`, with its neighbours loaded, is to be meshed: not if it's
    /// buried, with no faces to draw, nor if it's sealed and far from the camera.
    pub(super) fn wants_mesh(&self, world: &World, coord: ChunkCoordinate) -> bool {
        !world.is_chunk_buried(coord)
            && !(chunk_distance(coord, self.spawn_queue.anchor) > self.sealed_mesh_radius()
                && world.is_chunk_sealed(coord))
    }

    fn sealed_mesh_radius(&self) -> f32 {
        if self.camera_under_ground {
            SEALED_MESH_RADIUS_UNDER_GROUND
        } else {
            SEALED_MESH_RADIUS_IN_THE_OPEN
        }
    }
}

/// Has the sealed chunks the camera has come near meshed, rebuilding their groups. Those it
/// leaves keep their meshes until they unload, since dropping them would only be churn for
/// nothing seen.
pub fn mesh_nearing_sealed_chunks(mut chunk_loader: ResMut<ChunkLoader>) {
    let loader = &mut *chunk_loader;
    let (anchor, radius) = (loader.spawn_queue.anchor, loader.sealed_mesh_radius());
    if loader.sealed_anchor == Some((anchor, loader.camera_under_ground)) {
        return;
    }
    loader.sealed_anchor = Some((anchor, loader.camera_under_ground));
    let near: Vec<ChunkCoordinate> = loader
        .sealed
        .iter()
        .copied()
        .filter(|&chunk| chunk_distance(chunk, anchor) <= radius)
        .collect();
    for chunk in near {
        loader.sealed.remove(&chunk);
        loader.revealing.insert(LodKey::containing(chunk, 1));
    }
}

#[allow(clippy::type_complexity)]
pub fn gather_chunks(
    mut commands: Commands,
    mut chunk_loader: ResMut<ChunkLoader>,
    mut world: ResMut<World>,
    camera_query: Query<&GlobalTransform, (With<Camera>, With<ChildOf>)>,
    generating_chunks_query: Query<(), With<GenerateChunkData>>,
) {
    let loader = &mut *chunk_loader;
    let mut in_flight = generating_chunks_query.iter().count();
    let camera = camera_query.single().expect("could not find camera");
    let camera_block = camera.translation().floor().as_i64vec3();
    let camera_chunk = ChunkCoordinate::containing(camera_block);
    loader.camera_under_ground = world.is_under_ground(camera_block);
    let task_pool = AsyncComputeTaskPool::get();
    let start = Instant::now();

    while in_flight < loader.max_in_flight_generating {
        let mut next_chunks = loader.spawn_queue.next_batch(
            camera_chunk,
            &loader.chunk_to_entity,
            SPAWN_BATCH_SIZE,
            SCAN_BATCH_SIZE,
        );
        sort_by_viewport_bias(&mut next_chunks, camera_chunk, camera.forward());
        in_flight += next_chunks.len();
        for coord in next_chunks {
            world.hold(coord, Holder::Client);
            let mut chunk = commands.spawn(Chunk { coord });
            match world.get_chunk_data(coord) {
                // loaded already for the server's players
                Some(data) => {
                    if !data.empty() {
                        chunk.insert(DirtyChunk);
                    }
                }
                None => {
                    let regions = world.region_store.clone();
                    let task = task_pool.spawn(async move { generate_chunk(regions, coord) });
                    chunk.insert(GenerateChunkData { task });
                }
            }
            loader.chunk_to_entity.insert(coord, chunk.id());
        }
        if loader.spawn_queue.is_exhausted() || start.elapsed() >= GATHER_TIME_BUDGET {
            break;
        }
    }
}

pub fn generate_chunks(
    mut commands: Commands,
    mut world: ResMut<World>,
    mut chunk_loader: ResMut<ChunkLoader>,
    mut chunks_query: Query<(Entity, &Chunk, &mut GenerateChunkData)>,
) {
    for (entity, chunk, mut generating) in &mut chunks_query {
        if let Some(data) = futures::check_ready(&mut generating.task) {
            let mut entity = commands.entity(entity);
            entity.remove::<GenerateChunkData>();
            if !world.insert_chunk(chunk.coord, data).empty() {
                entity.insert(DirtyChunk);
            }
        }
    }
    chunk_loader.rebuild_reroofed(&mut world);
}

#[allow(clippy::type_complexity)]
pub fn mark_chunks(
    mut commands: Commands,
    world: Res<World>,
    mut chunk_loader: ResMut<ChunkLoader>,
    chunks_query: Query<(Entity, &Chunk), (With<DirtyChunk>, Without<GenerateChunkMesh>)>,
) {
    for (entity, chunk) in &chunks_query {
        // meshing reads the whole 3x3x3 neighbourhood, for ambient occlusion at the corners
        if !chunk_loader
            .layout
            .full_detail(chunk.coord, chunk_loader.spawn_queue.anchor)
            || !chunk
                .coord
                .neighbors_26()
                .into_iter()
                .all(|adj| world.is_chunk_generated(adj))
        {
            continue;
        }
        commands.entity(entity).remove::<DirtyChunk>();
        // most chunks underground: meshing them would only find every face covered
        if chunk_loader.wants_mesh(&world, chunk.coord) {
            commands
                .entity(entity)
                .insert(GenerateChunkMesh { task: None });
        } else if !world.is_chunk_buried(chunk.coord) {
            chunk_loader.sealed.insert(chunk.coord);
        }
    }
}

pub fn load_chunks(
    mut commands: Commands,
    world: Res<World>,
    mut chunks_query: Query<(Entity, &Chunk, &mut GenerateChunkMesh)>,
    mut chunk_loader: ResMut<ChunkLoader>,
) {
    let task_pool = AsyncComputeTaskPool::get();
    let start = Instant::now();

    for (entity, chunk, mut meshing) in &mut chunks_query {
        match &mut meshing.task {
            Some(task) => {
                if let Some(meshes) = futures::check_ready(task) {
                    let key = LodKey::containing(chunk.coord, 1);
                    let mut chunk_entity = commands.entity(entity);
                    chunk_entity.remove::<GenerateChunkMesh>();
                    // buried chunks mesh to nothing, and are left without a mesh or a draw
                    if meshes.has_geometry() {
                        if chunk_loader.groups.contains_key(&key) {
                            // reloaded after its group was drawn, whose mesh already has it
                            chunk_entity.insert(GroupedChunk);
                        } else {
                            chunk_entity.insert(MeshedChunk);
                            chunk_loader.held.insert(chunk.coord, meshes);
                            chunk_loader.waiting.insert(key);
                        }
                    }
                }
            }
            None => {
                // a neighbour found in `mark_chunks` may have been unloaded since, which would
                // bake a border into the mesh
                let coord = chunk.coord;
                if coord
                    .adjacent()
                    .into_iter()
                    .all(|adj| world.is_chunk_generated(adj))
                {
                    if let Some(data) = world.get_chunk_data(coord) {
                        let adjacent = world.adjacent_chunk_data(coord);
                        let regions = world.region_store.clone();
                        let solid_leaves = chunk_loader.solid_leaves;
                        meshing.task = Some(task_pool.spawn(async move {
                            generate_chunk_mesh(
                                data,
                                adjacent,
                                coord,
                                regions.noise(),
                                solid_leaves,
                            )
                        }));
                    }
                }
            }
        }

        if start.elapsed() >= MESH_APPLY_TIME_BUDGET {
            break;
        }
    }
}

#[allow(clippy::type_complexity)]
pub fn unload_chunks(
    mut commands: Commands,
    mut world: ResMut<World>,
    mut chunk_loader: ResMut<ChunkLoader>,
    chunks_query: Query<(Entity, &Chunk), (Without<GenerateChunkData>, Without<GenerateChunkMesh>)>,
    lods: Res<LodLoader>,
    pending_lods: Query<(), With<GenerateLodMesh>>,
) {
    let loader = &mut *chunk_loader;
    let reach = (loader.render_distance + NEIGHBOR_DATA_PADDING) as f32;
    let mut unloaded = HashSet::new();
    for (entity, chunk) in &chunks_query {
        if chunk_distance(chunk.coord, loader.spawn_queue.anchor) > reach
            && lods.chunk_covered(chunk.coord, &pending_lods)
        {
            commands.entity(entity).despawn();
            loader.chunk_to_entity.remove(&chunk.coord);
            loader.held.remove(&chunk.coord);
            loader.sealed.remove(&chunk.coord);
            world.release(chunk.coord, Holder::Client);
            unloaded.insert(LodKey::containing(chunk.coord, 1));
        }
    }

    // a group goes once all its chunks have, since one reloaded in the meantime is still in its mesh
    for key in unloaded {
        if key
            .chunks()
            .all(|chunk| !loader.chunk_to_entity.contains_key(&chunk))
        {
            loader.waiting.remove(&key);
            loader.rebuilding.remove(&key);
            loader.revealing.remove(&key);
            if let Some(group) = loader.groups.remove(&key) {
                commands.entity(group).despawn();
            }
        }
    }
}

// Euclidean: render distance is a sphere.
fn chunk_distance(chunk: ChunkCoordinate, other: ChunkCoordinate) -> f32 {
    (chunk.0 - other.0).as_vec3().length()
}

fn offset_distance(offset: I64Vec3) -> f32 {
    offset.as_vec3().length()
}

// Every offset within `render_distance`, nearest first.
fn build_offsets(render_distance: u32) -> Vec<I64Vec3> {
    let r = render_distance as i64;
    let mut offsets: Vec<I64Vec3> = (-r..=r)
        .flat_map(|x| (-r..=r).flat_map(move |y| (-r..=r).map(move |z| I64Vec3::new(x, y, z))))
        .filter(|&o| offset_distance(o) <= render_distance as f32)
        .collect();
    offsets.sort_by(|a, b| offset_distance(*a).total_cmp(&offset_distance(*b)));
    offsets
}

/// Which chunks to load next: a distance-sorted table of offsets from the camera's chunk, scanned
/// with a cursor that rewinds just far enough to cover chunks newly in range when the camera
/// moves, see `rewind_for_shift`. What's loaded is asked of `chunk_to_entity` every time, so
/// nothing is tracked twice.
#[derive(Debug)]
struct ChunkSpawnQueue {
    offsets: Vec<I64Vec3>,
    anchor: ChunkCoordinate,
    cursor: usize,
    // the chunk layers loaded, as above and below the world isn't generated, see `World::chunk`
    layers: std::ops::Range<i64>,
}

impl ChunkSpawnQueue {
    fn new(render_distance: u32) -> Self {
        Self {
            offsets: build_offsets(render_distance),
            anchor: ChunkCoordinate(I64Vec3::ZERO),
            cursor: 0,
            layers: i64::MIN..i64::MAX,
        }
    }

    fn with_layers(self, layers: std::ops::Range<i64>) -> Self {
        Self { layers, ..self }
    }

    // Chunks scanned out to D from the old anchor are scanned out to D - shift from the new one,
    // so only the outermost `shift` of what was scanned needs scanning again.
    fn rewind_for_shift(&mut self, shift: f32) {
        let scanned = self
            .cursor
            .checked_sub(1)
            .map_or(0.0, |last| offset_distance(self.offsets[last]));
        let target = (scanned - shift).max(0.0);
        self.cursor = self
            .offsets
            .partition_point(|o| offset_distance(*o) < target);
    }

    fn next_batch(
        &mut self,
        camera_chunk: ChunkCoordinate,
        loaded: &HashMap<ChunkCoordinate, Entity>,
        max_candidates: usize,
        max_scanned: usize,
    ) -> Vec<ChunkCoordinate> {
        if camera_chunk != self.anchor {
            self.rewind_for_shift(chunk_distance(camera_chunk, self.anchor));
            self.anchor = camera_chunk;
        }
        let mut candidates = Vec::with_capacity(max_candidates);
        let end = (self.cursor + max_scanned).min(self.offsets.len());
        while self.cursor < end && candidates.len() < max_candidates {
            let coord = ChunkCoordinate(self.anchor.0 + self.offsets[self.cursor]);
            self.cursor += 1;
            if self.layers.contains(&coord.0.y) && !loaded.contains_key(&coord) {
                candidates.push(coord);
            }
        }
        candidates
    }

    // Whether every offset has been scanned, which an empty batch alone doesn't say.
    fn is_exhausted(&self) -> bool {
        self.cursor >= self.offsets.len()
    }
}

// Nearest first, and among chunks as near, those the camera faces first.
fn sort_by_viewport_bias(
    candidates: &mut [ChunkCoordinate],
    camera_chunk: ChunkCoordinate,
    camera_forward: Dir3,
) {
    let camera_pos = camera_chunk.centre();
    let facing = |chunk: ChunkCoordinate| {
        let to_chunk = chunk.centre() - camera_pos;
        if to_chunk == Vec3::ZERO {
            1.0
        } else {
            camera_forward.dot(to_chunk.normalize())
        }
    };
    candidates.sort_by(|&a, &b| {
        chunk_distance(a, camera_chunk)
            .total_cmp(&chunk_distance(b, camera_chunk))
            .then_with(|| facing(b).total_cmp(&facing(a)))
    });
}

#[cfg(test)]
mod tests {
    use bevy::{
        ecs::entity::Entity,
        math::{Dir3, I64Vec3, Vec3},
    };

    use super::{
        build_offsets, chunk_distance, max_in_flight, mesh_nearing_sealed_chunks, offset_distance,
        sort_by_viewport_bias, ChunkCoordinate, ChunkLoader, ChunkSpawnQueue, HashMap, LodKey,
        LodLayout,
    };
    use crate::block::BlockType;
    use crate::chunks::chunk::{ChunkData, ColumnSet, CELLS};
    use crate::world::World;

    #[test]
    fn test_sealed_chunks_are_meshed_only_near_the_camera() {
        // stone with a cave in it at each of these, under ground, in stone under ground
        let (near, far) = (I64Vec3::new(2, 20, 0), I64Vec3::new(12, 20, 0));
        let mut world = World::new(16);
        for centre in [near, far] {
            let around = ChunkCoordinate(centre).adjacent();
            for coord in around.into_iter().chain([ChunkCoordinate(centre)]) {
                let mut data = ChunkData::from_cells(std::iter::repeat_n(BlockType::Stone, CELLS));
                data.roofed = ColumnSet::ALL;
                if coord.0 == centre {
                    data.set_block_at(bevy::math::U16Vec3::splat(8), BlockType::Air);
                }
                world.insert_chunk(coord, data);
            }
        }
        let mut loader = ChunkLoader::new(
            LodLayout::new(16, 128, 2),
            world.height,
            false,
            Default::default(),
            Default::default(),
        );
        loader.spawn_queue.anchor = ChunkCoordinate(I64Vec3::new(0, 20, 0));
        assert!(world.is_chunk_sealed(ChunkCoordinate(far)));
        assert!(
            chunk_distance(ChunkCoordinate(far), loader.anchor()) > loader.sealed_mesh_radius()
        );
        assert!(loader.wants_mesh(&world, ChunkCoordinate(near)));
        assert!(!loader.wants_mesh(&world, ChunkCoordinate(far)));
        // from under ground, sight carries further along the caves
        loader.spawn_queue.anchor = ChunkCoordinate(far - I64Vec3::X * 6);
        assert!(!loader.wants_mesh(&world, ChunkCoordinate(far)));
        loader.camera_under_ground = true;
        assert!(loader.wants_mesh(&world, ChunkCoordinate(far)));
        loader.camera_under_ground = false;
        loader.spawn_queue.anchor = ChunkCoordinate(I64Vec3::new(0, 20, 0));

        // one with a column open to the sky isn't sealed, however far
        let mut open = (*world.get_chunk_data(ChunkCoordinate(far)).unwrap()).clone();
        open.roofed = ColumnSet::default();
        world.insert_chunk(ChunkCoordinate(far), open);
        assert!(loader.wants_mesh(&world, ChunkCoordinate(far)));

        // and the camera coming near a sealed chunk left unmeshed has its group rebuilt
        loader.sealed.insert(ChunkCoordinate(far));
        loader.spawn_queue.anchor = ChunkCoordinate(far - I64Vec3::X * 3);
        let mut ecs = bevy::ecs::world::World::new();
        ecs.insert_resource(loader);
        bevy::ecs::system::RunSystemOnce::run_system_once(&mut ecs, mesh_nearing_sealed_chunks)
            .unwrap();
        let loader = ecs.resource::<ChunkLoader>();
        assert!(loader.sealed.is_empty());
        assert!(loader
            .revealing
            .contains(&LodKey::containing(ChunkCoordinate(far), 1)));
    }

    #[test]
    fn test_full_detail_chunks_are_within_full_detail_reach() {
        // and with full detail past where the levels scale from, as the shipped settings have
        // it, which the server's residency and the edits sent follow (`EditReach`)
        for layout in [
            LodLayout::new(32, 128, 2),
            LodLayout::new(16, 128, 2).with_full_detail_distance(24),
        ] {
            let reach = layout.full_detail_reach();
            let anchor = ChunkCoordinate(I64Vec3::new(3, -2, 5));
            let r = reach as i64 + 4;
            for x in -r..=r {
                for y in -r..=r {
                    for z in -r..=r {
                        let c = ChunkCoordinate(anchor.0 + I64Vec3::new(x, y, z));
                        if layout.full_detail(c, anchor) {
                            assert!(chunk_distance(c, anchor) <= reach as f32);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_build_offsets_count_and_bounds() {
        let offsets = build_offsets(2);
        assert_eq!(33, offsets.len());
        for o in &offsets {
            assert!(offset_distance(*o) <= 2.0);
        }
    }

    #[test]
    fn test_build_offsets_sorted_nearest_first() {
        let offsets = build_offsets(3);
        assert_eq!(I64Vec3::ZERO, offsets[0]);
        assert!(offsets
            .windows(2)
            .all(|w| offset_distance(w[0]) <= offset_distance(w[1])));
    }

    #[test]
    fn test_next_batch_rewind_is_proportional_not_full_reset() {
        let mut queue = ChunkSpawnQueue::new(10);
        let loaded = HashMap::new();
        let anchor = ChunkCoordinate(I64Vec3::ZERO);

        queue.next_batch(anchor, &loaded, 1000, 100_000);
        let cursor_before = queue.cursor;
        assert!(cursor_before > 0);

        // a normal 1-chunk step should only rewind the outermost shell, not reset to 0
        let new_anchor = ChunkCoordinate(I64Vec3::new(1, 0, 0));
        queue.next_batch(new_anchor, &loaded, 0, 0);

        assert!(queue.cursor > 0);
        assert!(queue.cursor <= cursor_before);
    }

    #[test]
    fn test_next_batch_rewinds_fully_on_large_jump() {
        let mut queue = ChunkSpawnQueue::new(10);
        let loaded = HashMap::new();
        let anchor = ChunkCoordinate(I64Vec3::ZERO);

        queue.next_batch(anchor, &loaded, 1000, 100_000);
        assert!(queue.cursor > 0);

        // a jump far larger than anything scanned so far invalidates all of it
        let far_anchor = ChunkCoordinate(I64Vec3::new(1000, 0, 0));
        queue.next_batch(far_anchor, &loaded, 0, 0);

        assert_eq!(0, queue.cursor);
    }

    #[test]
    fn test_next_batch_skips_already_loaded() {
        let mut queue = ChunkSpawnQueue::new(2);
        let anchor = ChunkCoordinate(I64Vec3::ZERO);
        let mut loaded = HashMap::new();
        loaded.insert(anchor, Entity::PLACEHOLDER);

        let batch = queue.next_batch(anchor, &loaded, 5, 1000);
        assert!(!batch.contains(&anchor));
    }

    #[test]
    fn test_next_batch_skips_layers_outside_the_world() {
        let mut queue = ChunkSpawnQueue::new(3).with_layers(0..2);
        let anchor = ChunkCoordinate(I64Vec3::ZERO);
        let batch = queue.next_batch(anchor, &HashMap::new(), 1000, 1000);
        assert!(!batch.is_empty());
        assert!(batch.iter().all(|c| (0..2).contains(&c.0.y)));
        assert!(queue.is_exhausted());
    }

    #[test]
    fn test_next_batch_respects_max_candidates() {
        let mut queue = ChunkSpawnQueue::new(5);
        let anchor = ChunkCoordinate(I64Vec3::ZERO);
        let loaded = HashMap::new();

        let batch = queue.next_batch(anchor, &loaded, 7, 100_000);
        assert_eq!(7, batch.len());
    }

    #[test]
    fn test_next_batch_respects_max_scanned_even_if_under_candidate_cap() {
        let mut queue = ChunkSpawnQueue::new(5);
        let anchor = ChunkCoordinate(I64Vec3::ZERO);
        let mut loaded = HashMap::new();
        for o in queue.offsets.clone() {
            loaded.insert(ChunkCoordinate(anchor.0 + o), Entity::PLACEHOLDER);
        }

        let batch = queue.next_batch(anchor, &loaded, 64, 10);
        assert_eq!(0, batch.len());
        assert_eq!(10, queue.cursor);
    }

    #[test]
    fn test_next_batch_terminal_state_is_idempotent() {
        let mut queue = ChunkSpawnQueue::new(1); // origin + 6 face neighbours = 7 offsets
        let anchor = ChunkCoordinate(I64Vec3::ZERO);
        let loaded = HashMap::new();

        let batch1 = queue.next_batch(anchor, &loaded, 100, 1000);
        assert_eq!(7, batch1.len());
        assert_eq!(7, queue.cursor);

        let batch2 = queue.next_batch(anchor, &loaded, 100, 1000);
        assert_eq!(0, batch2.len());
        assert_eq!(7, queue.cursor);
    }

    #[test]
    fn test_is_exhausted_false_until_fully_scanned() {
        let mut queue = ChunkSpawnQueue::new(1); // 7 offsets total
        let anchor = ChunkCoordinate(I64Vec3::ZERO);
        let loaded = HashMap::new();

        assert!(!queue.is_exhausted());

        queue.next_batch(anchor, &loaded, 100, 3); // scans only 3 of 7
        assert!(!queue.is_exhausted());

        queue.next_batch(anchor, &loaded, 100, 1000); // scans the rest
        assert!(queue.is_exhausted());
    }

    #[test]
    fn test_is_exhausted_can_be_true_even_with_an_empty_batch() {
        // an exhausting call can still return zero candidates, if everything left to
        // scan happens to already be loaded - is_exhausted must not be inferred from an
        // empty batch alone (see its doc comment)
        let mut queue = ChunkSpawnQueue::new(1);
        let anchor = ChunkCoordinate(I64Vec3::ZERO);
        let mut loaded = HashMap::new();
        for o in queue.offsets.clone() {
            loaded.insert(ChunkCoordinate(anchor.0 + o), Entity::PLACEHOLDER);
        }

        let batch = queue.next_batch(anchor, &loaded, 100, 1000);
        assert!(batch.is_empty());
        assert!(queue.is_exhausted());
    }

    #[test]
    fn test_sort_by_viewport_bias_prefers_forward_chunks() {
        let camera_chunk = ChunkCoordinate(I64Vec3::ZERO);
        let forward = Dir3::new(Vec3::new(1.0, 0.0, 0.0)).unwrap();
        let mut candidates = vec![
            ChunkCoordinate(I64Vec3::new(-3, 0, 0)),
            ChunkCoordinate(I64Vec3::new(3, 0, 0)),
        ];

        sort_by_viewport_bias(&mut candidates, camera_chunk, forward);

        assert_eq!(ChunkCoordinate(I64Vec3::new(3, 0, 0)), candidates[0]);
    }

    #[test]
    fn test_sort_by_viewport_bias_never_puts_farther_chunk_before_closer_one() {
        let camera_chunk = ChunkCoordinate(I64Vec3::ZERO);
        // a chunk directly ahead but far away, vs a chunk behind but close - distance
        // must win, even though the far chunk has a much better viewport score
        let forward = Dir3::new(Vec3::new(1.0, 0.0, 0.0)).unwrap();
        let far_ahead = ChunkCoordinate(I64Vec3::new(10, 0, 0));
        let close_behind = ChunkCoordinate(I64Vec3::new(-1, 0, 0));
        let mut candidates = vec![far_ahead, close_behind];

        sort_by_viewport_bias(&mut candidates, camera_chunk, forward);

        assert_eq!(close_behind, candidates[0]);
    }

    #[test]
    fn test_in_flight_cap_scales_with_shell_area() {
        assert_eq!(512, max_in_flight(32));
        let ratio = max_in_flight(64) as f64 / max_in_flight(32) as f64;
        assert!((3.0..4.0).contains(&ratio), "expected ~4x, got {ratio}x");
    }
}
