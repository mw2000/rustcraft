//! Flapping wings, shared by the butterflies and the birds: a flat wing, a winged body to spawn,
//! and its beat.

use bevy::{
    asset::RenderAssetUsages,
    light::NotShadowCaster,
    math::ops,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};

use super::{in_world, AmbientCritter};

/// Beats every wing.
pub(super) struct WingPlugin;

impl Plugin for WingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, flap_wings.run_if(in_world));
    }
}

/// How a wing beats: how fast, in radians a second, how far either way, held how far up, and
/// whether it now and then glides with its wings out, as birds do.
#[derive(Clone, Copy)]
pub(super) struct Flap {
    pub beat: f32,
    pub sweep: f32,
    pub lift: f32,
    pub glides: bool,
}

#[derive(Component)]
struct Wing {
    side: f32,
    flap: Flap,
    phase: f32,
}

/// A flat wing from its corners: at the body and at the tip, along the leading edge and then
/// the trailing one. It's mirrored for the other side, and carries uvs across it for a texture.
pub(super) fn wing_mesh(corners: [[f32; 3]; 4]) -> Mesh {
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, corners.to_vec())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 4])
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
    )
    .with_inserted_indices(Indices::U32(vec![0, 2, 1, 1, 2, 3]))
}

/// What a winged critter is drawn with: its body, and its wing, mirrored for the other side.
pub(super) struct WingedLook<'a> {
    pub body: (&'a Handle<Mesh>, &'a Handle<StandardMaterial>),
    pub wing: (&'a Handle<Mesh>, &'a Handle<StandardMaterial>),
}

/// Spawns `critter` at `at`, unseen until it's grown, with a body and a pair of wings beating
/// `phase` radians into their stroke.
pub(super) fn spawn_winged(
    commands: &mut Commands,
    critter: impl Bundle,
    at: Vec3,
    look: WingedLook,
    flap: Flap,
    phase: f32,
) -> Entity {
    commands
        .spawn((
            critter,
            AmbientCritter,
            Transform::from_translation(at).with_scale(Vec3::ZERO),
            Visibility::default(),
        ))
        .with_children(|parent| {
            parent.spawn((
                Mesh3d(look.body.0.clone()),
                MeshMaterial3d(look.body.1.clone()),
                Transform::default(),
                NotShadowCaster,
            ));
            for side in [1.0, -1.0] {
                parent.spawn((
                    Wing { side, flap, phase },
                    Mesh3d(look.wing.0.clone()),
                    MeshMaterial3d(look.wing.1.clone()),
                    Transform::from_scale(Vec3::new(side, 1.0, 1.0)),
                    NotShadowCaster,
                ));
            }
        })
        .id()
}

/// How far a wing's turned up at `t` seconds, in radians.
fn wing_angle(flap: Flap, phase: f32, t: f32) -> f32 {
    let glide = if flap.glides {
        ops::sin(t * 0.4 + phase * 3.0).max(0.0).powi(6)
    } else {
        0.0
    };
    flap.lift + ops::sin(t * flap.beat + phase) * flap.sweep * (1.0 - glide)
}

fn flap_wings(time: Res<Time>, mut wings: Query<(&Wing, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (wing, mut transform) in &mut wings {
        transform.rotation =
            Quat::from_rotation_z(wing.side * wing_angle(wing.flap, wing.phase, t));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wings_beat_within_their_sweep_and_gliders_hold_them_out() {
        let flap = Flap {
            beat: 18.0,
            sweep: 0.8,
            lift: 0.1,
            glides: true,
        };
        let mut glided = false;
        for i in 0..20_000 {
            let t = i as f32 * 0.01;
            let angle = wing_angle(flap, 1.0, t);
            assert!((angle - flap.lift).abs() <= flap.sweep + 1e-4, "{angle}");
            // the stroke's held to almost nothing at the top of a glide
            let gliding = ops::sin(t * 0.4 + 3.0).max(0.0).powi(6) > 0.99;
            if gliding {
                glided = true;
                assert!((angle - flap.lift).abs() < 0.02, "{angle}");
            }
        }
        assert!(glided);
    }
}
