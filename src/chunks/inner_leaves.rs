//! The faces between leaves, `ChunkMeshes::inner_leaves`, drawn by a child of each group or LOD
//! node that has any. Only those within `INNER_LEAVES_REACH` of the camera are seen: farther
//! out leaves fill their holes with the shade of the leaves behind, see `foliage.wesl`. Every
//! one still casts the sun's shadows, so a crown shades the ground as all its leaves do however
//! far off it is, rather than as its holed outside alone would.

use bevy::{
    asset::{Assets, Handle},
    camera::{primitives::Aabb, visibility::RenderLayers, Camera},
    ecs::{
        change_detection::DetectChangesMut,
        component::Component,
        entity::Entity,
        hierarchy::{ChildOf, Children},
        query::With,
        system::{Commands, Query},
    },
    math::Vec3,
    pbr::MeshMaterial3d,
    prelude::Mesh3d,
    render::mesh::Mesh,
    transform::components::{GlobalTransform, Transform},
};

use super::chunk::CHUNK_SIZE;
use super::generate::generator::INNER_LEAVES_REACH;
use super::lod::LodKey;
use super::material::ChunkMaterial;

/// The render layer only the sun sees, for what casts shadows but isn't drawn. The camera
/// keeps the default layer, 0, and the sun has both.
pub const SHADOW_ONLY_LAYER: usize = 31;

/// The layers the sun's shadows are cast from: the default one and `SHADOW_ONLY_LAYER`.
pub fn sun_layers() -> RenderLayers {
    RenderLayers::from_layers(&[0, SHADOW_ONLY_LAYER])
}

/// Draws the faces between the leaves of the group or LOD node at `key`, its parent.
#[derive(Component)]
pub struct InnerLeafFaces {
    pub key: LodKey,
}

// The layers the faces between leaves are drawn in: seen near the camera, and only casting
// shadows past it.
fn layers(near: bool) -> RenderLayers {
    if near {
        sun_layers()
    } else {
        RenderLayers::layer(SHADOW_ONLY_LAYER)
    }
}

/// Whether any of the node at `key` is within `INNER_LEAVES_REACH` of `eye`, so a node is never
/// missing faces that would show through a hole.
pub fn inner_leaves_near(key: LodKey, eye: Vec3) -> bool {
    let min = key.min_block().as_vec3();
    let max = min + Vec3::splat((key.width() * CHUNK_SIZE as i64) as f32);
    eye.clamp(min, max).distance(eye) < INNER_LEAVES_REACH
}

/// Shows the faces between leaves of the groups and nodes near the camera, and leaves the
/// rest casting shadows alone.
pub fn show_inner_leaves(
    camera: Query<&GlobalTransform, (With<Camera>, With<ChildOf>)>,
    mut inner_leaves: Query<(&InnerLeafFaces, &mut RenderLayers)>,
) {
    let Ok(camera) = camera.single() else {
        return;
    };
    let eye = camera.translation();
    for (faces, mut layers_now) in &mut inner_leaves {
        layers_now.set_if_neq(layers(inner_leaves_near(faces.key, eye)));
    }
}

/// The children of a group or LOD node: the one drawing its water and the one drawing the
/// faces between its leaves, if it has them.
pub fn split_children(
    children: Option<&Children>,
    inner: &Query<(), With<InnerLeafFaces>>,
) -> (Option<Entity>, Option<Entity>) {
    let mut split = (None, None);
    for &child in children.into_iter().flatten() {
        if inner.contains(child) {
            split.1 = Some(child);
        } else {
            split.0 = Some(child);
        }
    }
    split
}

/// Puts `mesh`, the faces between the leaves of the node at `key` drawn by `entity`, on its
/// child `existing`, spawning one if there isn't one, or despawning it if there's no mesh.
/// A new one only casts shadows until `show_inner_leaves` next looks at it.
#[allow(clippy::too_many_arguments)]
pub fn draw_inner_leaves(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    material: &Handle<ChunkMaterial>,
    entity: Entity,
    key: LodKey,
    mesh: Option<Mesh>,
    existing: Option<Entity>,
    aabb: Aabb,
) {
    match (mesh, existing) {
        (Some(mesh), Some(child)) => {
            commands.entity(child).insert(Mesh3d(meshes.add(mesh)));
        }
        (Some(mesh), None) => {
            let material = material.clone();
            commands.entity(entity).with_children(|parent| {
                parent.spawn((
                    InnerLeafFaces { key },
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(material),
                    Transform::default(),
                    aabb,
                    layers(false),
                ));
            });
        }
        (None, Some(child)) => commands.entity(child).despawn(),
        (None, None) => {}
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::{I64Vec3, Vec3};

    use super::{inner_leaves_near, layers, sun_layers, SHADOW_ONLY_LAYER};
    use crate::chunks::chunk::ChunkCoordinate;
    use crate::chunks::generate::generator::INNER_LEAVES_REACH;
    use crate::chunks::lod::LodKey;
    use bevy::camera::visibility::RenderLayers;

    #[test]
    fn test_inner_leaves_are_near_if_any_of_their_node_is() {
        let key = LodKey::containing(ChunkCoordinate(I64Vec3::ZERO), 1);
        // inside the group, and just off its far corner
        assert!(inner_leaves_near(key, Vec3::splat(10.0)));
        assert!(inner_leaves_near(
            key,
            Vec3::new(32.0 + INNER_LEAVES_REACH - 1.0, 5.0, 5.0)
        ));
        assert!(!inner_leaves_near(
            key,
            Vec3::new(32.0 + INNER_LEAVES_REACH + 1.0, 5.0, 5.0)
        ));
        assert!(!inner_leaves_near(
            key,
            Vec3::new(-INNER_LEAVES_REACH - 1.0, 5.0, 5.0)
        ));
        // a coarser node's whole width counts
        let node = LodKey::containing(ChunkCoordinate(I64Vec3::ZERO), 3);
        assert!(inner_leaves_near(
            node,
            Vec3::new(128.0 + INNER_LEAVES_REACH - 1.0, 5.0, 5.0)
        ));
    }

    #[test]
    fn test_inner_leaves_always_cast_shadows_and_are_seen_only_near() {
        let camera = RenderLayers::default();
        for near in [false, true] {
            assert!(sun_layers().intersects(&layers(near)));
            assert_eq!(near, camera.intersects(&layers(near)));
        }
        // the sun still sees everything the camera does
        assert!(sun_layers().intersects(&camera));
        assert!(!camera.intersects(&RenderLayers::layer(SHADOW_ONLY_LAYER)));
    }
}
