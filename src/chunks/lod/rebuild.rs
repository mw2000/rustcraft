use super::apply::draw_meshes;
use super::loader::{lod_transform, GenerateLodMesh, LodLoader, LodNode};
use crate::chunks::generate::generator::{generate_lod_mesh, ChunkMeshes};
use crate::chunks::inner_leaves::{split_children, InnerLeafFaces};
use crate::world::World;
use bevy::asset::Assets;
use bevy::camera::visibility::Visibility;
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::Children;
use bevy::ecs::query::{Has, Or, With};
use bevy::ecs::system::{Commands, Query, ResMut};
use bevy::prelude::Mesh3d;
use bevy::render::mesh::Mesh;
use bevy::tasks::{futures, AsyncComputeTaskPool, Task};

/// A node being meshed again for the edits in it, drawing its old mesh, if it had one, until the
/// new one's ready.
#[derive(Component)]
pub struct RebuildLodMesh {
    task: Task<Option<ChunkMeshes>>,
}

/// Has the nodes meshed again whose edits have changed what they draw. A node still being meshed
/// waits for that to finish, as it may have missed them, so edits coming one after another are
/// taken in together. One with nothing to draw before gets an entity for its new mesh.
#[allow(clippy::type_complexity)]
pub fn rebuild_lods(
    mut commands: Commands,
    mut lods: ResMut<LodLoader>,
    mut world: ResMut<World>,
    meshing: Query<(), Or<(With<GenerateLodMesh>, With<RebuildLodMesh>)>>,
) {
    let redrawn = world.lod_edits_mut().take_redrawn();
    let lods = &mut *lods;
    lods.redrawn.extend(redrawn);
    if lods.redrawn.is_empty() {
        return;
    }
    let task_pool = AsyncComputeTaskPool::get();
    let mut started = Vec::new();
    lods.redrawn.retain(|&key| {
        // not drawn, so it takes its edits in when it is
        let Some(&slot) = lods.nodes.get(&key) else {
            return false;
        };
        if slot.is_some_and(|entity| meshing.contains(entity)) {
            return true;
        }
        let (regions, edits) = (world.region_store.clone(), world.lod_edits().node(key));
        let rebuild = RebuildLodMesh {
            task: task_pool.spawn(async move { generate_lod_mesh(regions, key, edits.as_deref()) }),
        };
        let entity = match slot {
            Some(entity) => commands.entity(entity).insert(rebuild).id(),
            None => commands
                .spawn((
                    LodNode { key },
                    rebuild,
                    lod_transform(key),
                    Visibility::Hidden,
                ))
                .id(),
        };
        started.push((key, entity));
        false
    });
    for (key, entity) in started {
        lods.add_node(key, Some(entity));
    }
}

/// Swaps in nodes' new meshes as they're ready, on the same entities with the same materials, so
/// a node neither flickers nor fades. One with nothing left to draw is despawned, and one that
/// had nothing before is shown as nodes are.
#[allow(clippy::type_complexity)]
pub fn load_rebuilt_lods(
    mut commands: Commands,
    mut lods: ResMut<LodLoader>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut rebuilding: Query<(
        Entity,
        &LodNode,
        &mut RebuildLodMesh,
        Has<Mesh3d>,
        Option<&Children>,
    )>,
    inner: Query<(), With<InnerLeafFaces>>,
) {
    for (entity, node, mut rebuild, has_opaque, children) in &mut rebuilding {
        let Some(result) = futures::check_ready(&mut rebuild.task) else {
            continue;
        };
        commands.entity(entity).remove::<RebuildLodMesh>();
        // retired while it was meshed, and fading out or gone
        if lods.nodes.get(&node.key) != Some(&Some(entity)) {
            continue;
        }
        match result {
            Some(new) => {
                draw_meshes(
                    &mut commands,
                    &lods,
                    &mut meshes,
                    entity,
                    node.key,
                    new,
                    has_opaque,
                    split_children(children, &inner),
                );
            }
            None => {
                commands.entity(entity).despawn();
                lods.add_node(node.key, None);
            }
        }
        lods.dirty_areas.push(node.key);
    }
}
