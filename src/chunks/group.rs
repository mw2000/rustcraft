use bevy::{
    asset::{Assets, RenderAssetUsages},
    camera::{primitives::Aabb, visibility::Visibility},
    ecs::{
        bundle::Bundle,
        component::Component,
        entity::Entity,
        hierarchy::Children,
        query::{Has, With},
        system::{Commands, Query, Res, ResMut},
    },
    math::Vec3,
    pbr::MeshMaterial3d,
    prelude::Mesh3d,
    render::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues},
    tasks::futures,
    tasks::{AsyncComputeTaskPool, ComputeTaskPool, Task},
    transform::components::Transform,
};

use super::{
    chunk::ChunkCoordinate,
    chunk_loader::{ChunkLoader, Unsettled},
    generate::generator::{generate_chunk_mesh, ChunkMeshes},
    lod::LodKey,
    material::ChunkMaterial,
};
use crate::chunks::chunk::{CHUNK_SIZE, MESH_OFFSET};
use crate::world::World;

/// A level-1 node's worth of full-detail chunks drawn as one mesh. Level 1 is where terrain
/// swaps between full detail and LODs, so a group is always wholly shown or wholly hidden.
#[derive(Component)]
pub struct ChunkGroup {
    pub key: LodKey,
}

#[derive(Component)]
pub struct BuildChunkGroup {
    task: Task<ChunkMeshes>,
    chunks: Vec<Entity>,
}

/// A group whose mesh is merged and ready, whether or not it has any opaque faces.
#[derive(Component)]
pub struct BuiltGroup;

/// A chunk meshed with something to draw, whose mesh waits in `ChunkLoader::held` for the
/// rest of its group.
#[derive(Component)]
pub struct MeshedChunk;

/// A chunk drawn as part of its group.
#[derive(Component)]
pub struct GroupedChunk;

fn group_aabb() -> Aabb {
    let width = (2 * CHUNK_SIZE) as f32;
    Aabb::from_min_max(Vec3::ZERO, Vec3::splat(width))
}

/// Starts merging the meshes of each group whose chunks have all settled, one way or the other.
pub fn build_groups(
    mut commands: Commands,
    mut loader: ResMut<ChunkLoader>,
    unsettled: Query<(), Unsettled>,
) {
    let task_pool = AsyncComputeTaskPool::get();
    let ready: Vec<LodKey> = loader
        .waiting
        .iter()
        .copied()
        .filter(|key| {
            key.chunks().all(|chunk| {
                loader
                    .chunk_entity(chunk)
                    .is_some_and(|entity| !unsettled.contains(entity))
            })
        })
        .collect();

    for key in ready {
        loader.waiting.remove(&key);
        let chunks: Vec<ChunkCoordinate> = key.chunks().collect();
        let entities: Vec<Entity> = chunks
            .iter()
            .filter_map(|&c| loader.chunk_entity(c))
            .collect();
        let origin = key.min_chunk();
        let parts: Vec<(Vec3, ChunkMeshes)> = chunks
            .iter()
            .filter_map(|c| {
                let offset = (c.0 - origin).as_vec3() * CHUNK_SIZE as f32;
                loader.held.remove(c).map(|meshes| (offset, meshes))
            })
            .collect();
        let build = BuildChunkGroup {
            task: task_pool.spawn(async move { merge_chunk_meshes(parts) }),
            chunks: entities,
        };
        // hidden until refresh_lod_visibility shows it, so it never flashes up over the LOD
        // node it's replacing
        let entity = commands
            .spawn(new_group(key, build, Visibility::Hidden))
            .id();
        loader.groups.insert(key, entity);
    }
}

fn new_group(key: LodKey, build: BuildChunkGroup, visibility: Visibility) -> impl Bundle {
    (
        ChunkGroup { key },
        build,
        Transform::from_translation(key.min_block().as_vec3() + MESH_OFFSET),
        visibility,
    )
}

/// Remeshes the groups edits have changed, once their chunks have settled, from the world's
/// blocks as they are now. Their chunks are meshed again from scratch, since a group keeps only
/// its merged mesh. It's done on the compute pool, which is kept for work the next frames need,
/// so it isn't queued behind loading on the async compute pool, and a group keeps drawing its
/// old mesh until `load_groups` swaps in the new one.
pub fn rebuild_groups(
    mut commands: Commands,
    mut loader: ResMut<ChunkLoader>,
    world: Res<World>,
    unsettled: Query<(), Unsettled>,
    building: Query<(), With<BuildChunkGroup>>,
) {
    if loader.rebuilding.is_empty() {
        return;
    }
    let task_pool = ComputeTaskPool::get();
    // a group already being merged is rebuilt once that's done, or it could land over this
    let ready: Vec<LodKey> = loader
        .rebuilding
        .iter()
        .copied()
        .filter(|&key| {
            key.chunks().all(|chunk| {
                loader
                    .chunk_entity(chunk)
                    .is_some_and(|entity| !unsettled.contains(entity))
            }) && loader
                .group_entity(key)
                .is_none_or(|group| !building.contains(group))
        })
        .collect();

    for key in ready {
        loader.rebuilding.remove(&key);
        // anything held for the group to merge is out of date now
        loader.waiting.remove(&key);
        let origin = key.min_chunk();
        let mut parts = Vec::new();
        for coord in key.chunks() {
            loader.held.remove(&coord);
            let Some(data) = world.get_chunk_data(coord) else {
                continue;
            };
            if !data.empty() && !world.is_chunk_buried(coord) {
                let offset = (coord.0 - origin).as_vec3() * CHUNK_SIZE as f32;
                parts.push((offset, coord, data, world.adjacent_chunk_data(coord)));
            }
        }
        let (regions, solid_leaves) = (world.region_store.clone(), loader.solid_leaves);
        let build = BuildChunkGroup {
            task: task_pool.spawn(async move {
                let meshes = parts
                    .into_iter()
                    .map(|(offset, coord, data, adjacent)| {
                        let noise = regions.noise();
                        let meshes =
                            generate_chunk_mesh(data, adjacent, coord, noise, solid_leaves);
                        (offset, meshes)
                    })
                    .collect();
                merge_chunk_meshes(meshes)
            }),
            chunks: key
                .chunks()
                .filter_map(|c| loader.chunk_entity(c))
                .collect(),
        };
        match loader.groups.get(&key) {
            Some(&group) => {
                commands.entity(group).insert(build);
            }
            None => {
                // shown from the start, so the first block placed in empty sky appears the
                // moment its mesh does. Within full detail, where edits are made, no LOD node
                // draws there to show over, and should one still be, refresh_lod_visibility
                // hides the group when it's built, as it would any other.
                let group = commands
                    .spawn(new_group(key, build, Visibility::Inherited))
                    .id();
                loader.groups.insert(key, group);
            }
        }
    }
}

/// Draws groups whose merged meshes are ready. A rebuilt group has its meshes swapped in the
/// same frame, and keeps its materials, so it neither flickers nor restarts a fade.
#[allow(clippy::type_complexity)]
pub fn load_groups(
    mut commands: Commands,
    loader: Res<ChunkLoader>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut building: Query<(
        Entity,
        &mut BuildChunkGroup,
        Has<BuiltGroup>,
        Has<Mesh3d>,
        Option<&Children>,
    )>,
) {
    for (entity, mut build, built, has_opaque, children) in building.iter_mut() {
        let Some(merged) = futures::check_ready(&mut build.task) else {
            continue;
        };
        if merged.has_geometry() && !built {
            commands.entity(entity).insert((BuiltGroup, group_aabb()));
        }
        if merged.opaque.count_vertices() > 0 {
            let mut group = commands.entity(entity);
            group.insert(Mesh3d(meshes.add(merged.opaque)));
            if !has_opaque {
                group.insert(MeshMaterial3d(loader.material.clone()));
            }
        } else if has_opaque {
            commands
                .entity(entity)
                .remove::<(Mesh3d, MeshMaterial3d<ChunkMaterial>)>();
        }
        // water is drawn by the group's only child
        match (merged.water, children.and_then(|c| c.first().copied())) {
            (Some(water), Some(child)) => {
                commands.entity(child).insert(Mesh3d(meshes.add(water)));
            }
            (Some(water), None) => {
                let water_material = loader.water_material.clone();
                commands.entity(entity).with_children(|parent| {
                    parent.spawn((
                        Mesh3d(meshes.add(water)),
                        MeshMaterial3d(water_material),
                        Transform::default(),
                        group_aabb(),
                    ));
                });
            }
            (None, Some(child)) => commands.entity(child).despawn(),
            (None, None) => {}
        }
        for &chunk in &build.chunks {
            if let Ok(mut chunk) = commands.get_entity(chunk) {
                chunk.remove::<MeshedChunk>().try_insert(GroupedChunk);
            }
        }
        commands.entity(entity).remove::<BuildChunkGroup>();
    }
}

fn float3(mesh: &Mesh, attribute: bevy::render::mesh::MeshVertexAttribute) -> &[[f32; 3]] {
    mesh.attribute(attribute)
        .and_then(VertexAttributeValues::as_float3)
        .expect("float3 attribute")
}

fn float2(mesh: &Mesh, attribute: bevy::render::mesh::MeshVertexAttribute) -> Option<&[[f32; 2]]> {
    match mesh.attribute(attribute)? {
        VertexAttributeValues::Float32x2(values) => Some(values),
        _ => None,
    }
}

fn float4(mesh: &Mesh, attribute: bevy::render::mesh::MeshVertexAttribute) -> &[[f32; 4]] {
    match mesh.attribute(attribute) {
        Some(VertexAttributeValues::Float32x4(values)) => values,
        _ => panic!("float4 attribute"),
    }
}

// One mesh from `parts`, each moved by its offset. Parts with no vertices are skipped.
fn merge_meshes(parts: Vec<(Vec3, Mesh)>) -> Option<Mesh> {
    let parts: Vec<(Vec3, Mesh)> = parts
        .into_iter()
        .filter(|(_, mesh)| mesh.count_vertices() > 0)
        .collect();
    if parts.is_empty() {
        return None;
    }
    let vertices: usize = parts.iter().map(|(_, mesh)| mesh.count_vertices()).sum();
    let mut positions = Vec::with_capacity(vertices);
    let mut normals = Vec::with_capacity(vertices);
    let mut uvs = Vec::with_capacity(vertices);
    let mut colors = Vec::with_capacity(vertices);
    let mut lights = Vec::with_capacity(vertices);
    let mut indices = Vec::new();
    for (offset, mesh) in &parts {
        let start = positions.len() as u32;
        positions.extend(
            float3(mesh, Mesh::ATTRIBUTE_POSITION)
                .iter()
                .map(|&p| (Vec3::from(p) + *offset).to_array()),
        );
        normals.extend_from_slice(float3(mesh, Mesh::ATTRIBUTE_NORMAL));
        uvs.extend_from_slice(float2(mesh, Mesh::ATTRIBUTE_UV_0).expect("uvs"));
        colors.extend_from_slice(float4(mesh, Mesh::ATTRIBUTE_COLOR));
        // block faces carry light, water doesn't, and a mesh has either on all its vertices or none
        if let Some(light) = float2(mesh, Mesh::ATTRIBUTE_UV_1) {
            lights.extend_from_slice(light);
        }
        let Some(Indices::U32(part)) = mesh.indices() else {
            panic!("chunk meshes have u32 indices");
        };
        indices.extend(part.iter().map(|i| i + start));
    }
    let mut merged = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices));
    if !lights.is_empty() {
        assert_eq!(vertices, lights.len(), "only some vertices carry light");
        merged.insert_attribute(Mesh::ATTRIBUTE_UV_1, lights);
    }
    Some(merged)
}

/// The chunks' meshes as one, each moved by its offset in blocks.
pub fn merge_chunk_meshes(parts: Vec<(Vec3, ChunkMeshes)>) -> ChunkMeshes {
    let mut opaque = Vec::new();
    let mut water = Vec::new();
    for (offset, meshes) in parts {
        opaque.push((offset, meshes.opaque));
        water.extend(meshes.water.map(|mesh| (offset, mesh)));
    }
    ChunkMeshes {
        // an empty mesh, so there's always something to hand back
        opaque: merge_meshes(opaque).unwrap_or_else(|| {
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::RENDER_WORLD,
            )
        }),
        water: merge_meshes(water),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bevy::math::{I64Vec3, U16Vec3, Vec3};
    use bevy::render::mesh::{Indices, Mesh};

    use super::merge_chunk_meshes;
    use crate::block::BlockType;
    use crate::chunks::chunk::{ChunkCoordinate, ChunkData};
    use crate::chunks::generate::generator::{generate_chunk_mesh, ChunkMeshes};
    use crate::chunks::generate::noise::NoiseGenerator;

    fn block_mesh(at: U16Vec3) -> ChunkMeshes {
        let mut chunk = ChunkData::default();
        chunk.set_block_at(at, BlockType::Stone);
        generate_chunk_mesh(
            Arc::new(chunk),
            vec![None; 26],
            ChunkCoordinate(I64Vec3::ZERO),
            &NoiseGenerator::new(1),
            false,
        )
    }

    fn positions(mesh: &Mesh) -> Vec<Vec3> {
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            .unwrap()
            .as_float3()
            .unwrap()
            .iter()
            .map(|&p| p.into())
            .collect()
    }

    #[test]
    fn test_merged_meshes_keep_every_vertex_where_it_was() {
        let (a, b) = (
            block_mesh(U16Vec3::new(4, 4, 4)),
            block_mesh(U16Vec3::new(4, 4, 4)),
        );
        let alone = positions(&a.opaque);
        let offset = Vec3::new(16.0, 0.0, 32.0);
        let merged = merge_chunk_meshes(vec![(Vec3::ZERO, a), (offset, b)]);
        let merged_positions = positions(&merged.opaque);
        assert_eq!(2 * alone.len(), merged_positions.len());
        assert_eq!(alone, merged_positions[..alone.len()]);
        let moved: Vec<Vec3> = alone.iter().map(|p| *p + offset).collect();
        assert_eq!(moved, merged_positions[alone.len()..]);
    }

    #[test]
    fn test_merged_indices_point_into_their_own_part() {
        let (a, b) = (
            block_mesh(U16Vec3::new(1, 1, 1)),
            block_mesh(U16Vec3::new(2, 2, 2)),
        );
        let (a_vertices, a_indices) =
            (a.opaque.count_vertices(), a.opaque.indices().unwrap().len());
        let merged = merge_chunk_meshes(vec![(Vec3::ZERO, a), (Vec3::new(16.0, 0.0, 0.0), b)]);
        let Some(Indices::U32(indices)) = merged.opaque.indices() else {
            panic!()
        };
        assert!(indices[..a_indices]
            .iter()
            .all(|&i| (i as usize) < a_vertices));
        assert!(indices[a_indices..]
            .iter()
            .all(|&i| (i as usize) >= a_vertices));
        assert!(indices
            .iter()
            .all(|&i| (i as usize) < merged.opaque.count_vertices()));
    }

    #[test]
    fn test_chunks_with_nothing_to_draw_merge_to_nothing() {
        let merged = merge_chunk_meshes(Vec::new());
        assert!(!merged.has_geometry());
    }

    #[test]
    fn test_empty_parts_are_skipped() {
        let empty = merge_chunk_meshes(Vec::new());
        let block = block_mesh(U16Vec3::new(3, 3, 3));
        let vertices = block.opaque.count_vertices();
        let merged = merge_chunk_meshes(vec![(Vec3::ZERO, empty), (Vec3::ZERO, block)]);
        assert_eq!(vertices, merged.opaque.count_vertices());
    }

    mod edits {
        use bevy::asset::{AssetApp, AssetPlugin, Handle};
        use bevy::ecs::schedule::IntoScheduleConfigs;
        use bevy::ecs::system::RunSystemOnce;
        use bevy::math::I64Vec3;
        use bevy::platform::collections::HashSet;
        use bevy::prelude::{App, Entity, Mesh, Mesh3d, MinimalPlugins, Update, Visibility};

        use super::super::{load_groups, rebuild_groups, BuildChunkGroup};
        use crate::block::BlockType;
        use crate::chunks::chunk::{ChunkCoordinate, ChunkData};
        use crate::chunks::chunk_loader::{apply_edits, Chunk, ChunkLoader};
        use crate::chunks::fade::Fade;
        use crate::chunks::lod::{LodKey, LodLayout};
        use crate::edits::EditBlock;
        use crate::world::World;

        // The world with the chunks in the box from `min` to `max` loaded, empty, and settled.
        fn loaded(ecs: &mut bevy::ecs::world::World, min: I64Vec3, max: I64Vec3) {
            let mut world = World::new(16);
            let mut loader = ChunkLoader::new(
                LodLayout::new(32, 128, 2),
                world.height,
                false,
                Handle::default(),
                Handle::default(),
            );
            for x in min.x..=max.x {
                for y in min.y..=max.y {
                    for z in min.z..=max.z {
                        let coord = ChunkCoordinate(I64Vec3::new(x, y, z));
                        world.insert_chunk(coord, ChunkData::default());
                        loader.track(coord, ecs.spawn(Chunk { coord }).id());
                    }
                }
            }
            ecs.insert_resource(world);
            ecs.insert_resource(loader);
        }

        fn group(x: i64, y: i64, z: i64) -> LodKey {
            LodKey {
                level: 1,
                coord: I64Vec3::new(x, y, z),
            }
        }

        #[test]
        fn test_an_edit_rebuilds_the_loaded_groups_whose_meshes_it_changes() {
            let mut ecs = bevy::ecs::world::World::new();
            loaded(&mut ecs, I64Vec3::new(-1, 19, -1), I64Vec3::new(1, 21, 1));
            ecs.init_resource::<bevy::ecs::message::Messages<EditBlock>>();
            // in the top corner of chunk (0, 20, 0), against the chunks to +x, +y and -z
            ecs.write_message(EditBlock {
                block: I64Vec3::new(15, 20 * 16 + 15, 0),
                to: BlockType::Stone,
            });
            // and one in a chunk not loaded, which is put in when it's generated
            ecs.write_message(EditBlock {
                block: I64Vec3::new(400, 20 * 16, 400),
                to: BlockType::Stone,
            });
            ecs.run_system_once(apply_edits).unwrap();
            // the chunk below too, whose light falls through the chunk above it
            let expected: HashSet<LodKey> = [
                group(0, 9, -1),
                group(0, 9, 0),
                group(0, 10, -1),
                group(0, 10, 0),
            ]
            .into_iter()
            .collect();
            assert_eq!(expected, ecs.resource::<ChunkLoader>().rebuilding);
            assert_eq!(
                BlockType::Stone,
                ecs.resource::<World>()
                    .get_block_at(I64Vec3::new(15, 20 * 16 + 15, 0))
            );
        }

        #[test]
        fn test_a_rebuilt_group_swaps_in_its_new_mesh_on_the_same_entity() {
            let mut app = App::new();
            app.add_plugins((MinimalPlugins, AssetPlugin::default()))
                .init_asset::<Mesh>()
                .add_message::<EditBlock>()
                .add_systems(Update, (apply_edits, rebuild_groups, load_groups).chain());
            // group (0, 10, 0) and every chunk round it
            loaded(
                app.world_mut(),
                I64Vec3::new(-1, 19, -1),
                I64Vec3::new(2, 22, 2),
            );
            let key = group(0, 10, 0);
            let edit = |app: &mut App, to| {
                app.world_mut().write_message(EditBlock {
                    block: I64Vec3::new(5, 20 * 16 + 5, 5),
                    to,
                });
                for _ in 0..1000 {
                    app.update();
                    let loader = app.world().resource::<ChunkLoader>();
                    if let Some(entity) = loader.group_entity(key) {
                        if !loader.rebuilding.contains(&key)
                            && !app.world().entity(entity).contains::<BuildChunkGroup>()
                        {
                            return entity;
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                panic!("the group was never rebuilt");
            };
            let has_mesh =
                |app: &App, entity: Entity| app.world().entity(entity).contains::<Mesh3d>();

            // a block placed where nothing was drawn gets a group to draw it, shown in full
            let placed = edit(&mut app, BlockType::Stone);
            assert!(has_mesh(&app, placed));
            let group = app.world().entity(placed);
            assert_eq!(Some(&Visibility::Inherited), group.get::<Visibility>());
            assert!(!group.contains::<Fade>());
            // which is emptied, not replaced, when it's broken
            let broken = edit(&mut app, BlockType::Air);
            assert_eq!(placed, broken);
            assert!(!has_mesh(&app, broken));
            assert_eq!(placed, edit(&mut app, BlockType::Log));
            assert!(has_mesh(&app, placed));
        }
    }
}
