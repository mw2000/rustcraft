//! The cracks over the block the player's breaking in survival (§5.1): a cube a hair bigger
//! than the block, drawing the stage of `cracks.png` it's got to, from the first crack to the
//! last as `Breaking` says.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::mesh::VertexAttributeValues;
use bevy::transform::TransformSystems;

use crate::breaking::CRACK_STAGES;
use crate::chunks::material::block_atlas;
use crate::interaction::Breaking;

// Just over a block, so the cracks aren't lost in its faces, and just under the outline.
const CRACKS_SIZE: f32 = 1.002;
// Texels in from a tile's edge its faces sample, so they never take in the next stage's.
const TILE_INSET: f32 = 0.5 / 16.0;

/// Draws the cracks. Needs the renderer.
pub struct DrawCracksPlugin;

impl Plugin for DrawCracksPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_cracks).add_systems(
            PostUpdate,
            // where they are this frame, before it's worked out where that is on screen
            draw_cracks.before(TransformSystems::Propagate),
        );
    }
}

// The cube drawn, and its mesh for each stage, every one drawing its own tile of the strip.
#[derive(Component)]
struct Cracks {
    stages: Vec<Handle<Mesh>>,
}

fn spawn_cracks(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let material = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(block_atlas("assets/textures/cracks.png"))),
        perceptual_roughness: 1.0,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let stages: Vec<_> = (0..CRACK_STAGES)
        .map(|stage| meshes.add(cube(stage)))
        .collect();
    commands.spawn((
        Mesh3d(stages[0].clone()),
        MeshMaterial3d(material),
        Cracks { stages },
        Transform::from_scale(Vec3::splat(CRACKS_SIZE)),
        Visibility::Hidden,
        NotShadowCaster,
    ));
}

// A cube a block across, each face drawing the whole of tile `stage`.
fn cube(stage: usize) -> Mesh {
    let mut mesh = Mesh::from(Cuboid::from_length(1.0));
    if let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute_mut(Mesh::ATTRIBUTE_UV_0) {
        for [u, v] in uvs {
            let inset = |t: f32| TILE_INSET + t * (1.0 - 2.0 * TILE_INSET);
            *u = (stage as f32 + inset(*u)) / CRACK_STAGES as f32;
            *v = inset(*v);
        }
    }
    mesh
}

// Changed only as the block or its stage do, rather than every frame it's held.
fn draw_cracks(
    breaking: Res<Breaking>,
    mut cracks: Query<(&Cracks, &mut Mesh3d, &mut Transform, &mut Visibility)>,
) {
    for (cracks, mut mesh, mut transform, mut visibility) in &mut cracks {
        let Some((block, stage)) = breaking.cracks() else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        visibility.set_if_neq(Visibility::Inherited);
        let centre = block.as_vec3() + 0.5;
        if transform.translation != centre {
            transform.translation = centre;
        }
        if mesh.0 != cracks.stages[stage] {
            mesh.0 = cracks.stages[stage].clone();
        }
    }
}
