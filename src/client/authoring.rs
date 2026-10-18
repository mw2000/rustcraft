//! What a player's game draws for authoring structures (§5.6, `crate::authoring`): their
//! selection, and the structure voids and markers near them, which the world doesn't draw.

use std::f32::consts::FRAC_PI_2;

use bevy::math::I64Vec3;
use bevy::prelude::*;

use super::player::LocalPlayer;
use crate::authoring::Selection;
use crate::block::BlockType;
use crate::chunks::chunk::{ChunkCoordinate, CHUNK_SIZE};
use crate::game_mode::GameMode;
use crate::world::World;

// How far round the player, in chunks, structure voids and markers are drawn.
const SHOWN_CHUNKS: i64 = 2;
// Gizmos a little thicker than the default, as the outline is, so they're seen.
const LINE_WIDTH: f32 = 3.0;
// The selection in orange, apart from the block outline's black, and its floor fainter.
const SELECTION_COLOUR: Color = Color::srgb(1.0, 0.6, 0.1);
const FLOOR_COLOUR: Color = Color::srgba(1.0, 0.6, 0.1, 0.5);
// Just outside the blocks, as the outline is, so its edges aren't hidden in their faces.
const SELECTION_MARGIN: f32 = 0.01;
// Structure voids as small light blue boxes and markers as bigger gold ones, as their icons.
const VOID_COLOUR: Color = Color::srgb(0.47, 0.82, 1.0);
const VOID_SIZE: f32 = 0.25;
const MARKER_COLOUR: Color = Color::srgb(0.94, 0.78, 0.24);
const MARKER_SIZE: f32 = 0.5;

#[derive(Default, Reflect, GizmoConfigGroup)]
struct AuthoringGizmos;

/// Draws the player's selection, and in creative the structure voids and markers near them.
pub struct AuthoringPlugin;

impl Plugin for AuthoringPlugin {
    fn build(&self, app: &mut App) {
        let config = GizmoConfig {
            line: GizmoLineConfig {
                width: LINE_WIDTH,
                ..default()
            },
            ..default()
        };
        app.insert_gizmo_config(AuthoringGizmos, config)
            .add_systems(
                Update,
                (draw_selection, draw_authoring_blocks).run_if(resource_exists::<World>),
            );
    }
}

// Each corner set as a block's outline, the box between them, and the floor of the structure
// placed in it as a rectangle round it at that height.
fn draw_selection(
    player: Query<&Selection, With<LocalPlayer>>,
    mut gizmos: Gizmos<AuthoringGizmos>,
) {
    let Ok(selection) = player.single() else {
        return;
    };
    let cube = |min: Vec3, size: Vec3| {
        let size = size + 2.0 * SELECTION_MARGIN;
        Transform::from_translation(min - SELECTION_MARGIN + size / 2.0).with_scale(size)
    };
    for corner in selection.corners.iter().flatten() {
        gizmos.cube(cube(corner.as_vec3(), Vec3::ONE), SELECTION_COLOUR);
    }
    let Some((min, size)) = selection.bounds() else {
        return;
    };
    let (min, size) = (min.as_vec3(), size.as_vec3());
    gizmos.cube(cube(min, size), SELECTION_COLOUR);
    if let Some(floor) = selection.floor {
        let centre = Vec3::new(min.x + size.x / 2.0, floor as f32, min.z + size.z / 2.0);
        let flat = Isometry3d::new(centre, Quat::from_rotation_x(FRAC_PI_2));
        gizmos.rect(flat, size.xz() + 2.0 * SELECTION_MARGIN, FLOOR_COLOUR);
    }
}

// They're never generated, so only the edit layer has them: it's looked up for the chunks
// round the player, a lookup each.
fn draw_authoring_blocks(
    world: Res<World>,
    player: Query<(&Transform, &GameMode), With<LocalPlayer>>,
    mut gizmos: Gizmos<AuthoringGizmos>,
) {
    let Ok((body, mode)) = player.single() else {
        return;
    };
    if !mode.has_every_block() {
        return;
    }
    let centre = ChunkCoordinate::containing(body.translation.floor().as_i64vec3()).0;
    for x in -SHOWN_CHUNKS..=SHOWN_CHUNKS {
        for y in -SHOWN_CHUNKS..=SHOWN_CHUNKS {
            for z in -SHOWN_CHUNKS..=SHOWN_CHUNKS {
                let coord = ChunkCoordinate(centre + I64Vec3::new(x, y, z));
                let origin = coord.0 * CHUNK_SIZE as i64;
                for (local, block) in world.edits().in_chunk(coord) {
                    let (size, colour) = match block {
                        BlockType::StructureVoid => (VOID_SIZE, VOID_COLOUR),
                        BlockType::LootMarker => (MARKER_SIZE, MARKER_COLOUR),
                        _ => continue,
                    };
                    let at = (origin + local.as_i64vec3()).as_vec3() + 0.5;
                    let cube = Transform::from_translation(at).with_scale(Vec3::splat(size));
                    gizmos.cube(cube, colour);
                }
            }
        }
    }
}
