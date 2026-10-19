use bevy::{
    camera::Camera3d,
    ecs::{
        change_detection::DetectChanges,
        hierarchy::ChildOf,
        message::MessageReader,
        query::{With, Without},
        system::{Query, Res},
        world::Ref,
    },
    input::mouse::MouseMotion,
    log::info,
    math::{I64Vec3, Quat, Vec3},
    time::Time,
    transform::components::Transform,
};

use std::f32::consts::PI;

use super::physics::{step, MoveInput};
use super::{Movement, Player, PlayerLook, EYE_HEIGHT};
use crate::client::player::WorldCamera;
use crate::collision::raycast;
use crate::controls::{Action, Controls};
use crate::preferences::{Preferences, View};
use crate::survival::Bruise;
use crate::{
    chunks::chunk::ChunkCoordinate, client::player::LocalPlayer, game_mode::GameMode, world::World,
};

// How far the mouse turns the view, per pixel it moves, at a sensitivity of 1. Mouse motion is
// already a distance, so it isn't scaled by the frame's length, which would turn the view less
// the higher the frame rate.
const LOOK_SENSITIVITY: f32 = 0.1_f32.to_radians();

type Moving<'a> = (
    &'a mut Transform,
    &'a mut Player,
    &'a PlayerLook,
    &'a GameMode,
    Ref<'a, Bruise>,
);

/// Moves the player by the keys held: WASD across, Space and Ctrl up and down or jumping,
/// Shift sprinting, and F between flying and walking where the game mode allows, or whatever
/// they're bound to, knocked back by the blows the server says they take.
pub fn player_move(
    time: Res<Time>,
    world: Res<World>,
    mut players: Query<Moving, With<LocalPlayer>>,
    controls: Controls,
) {
    let (mut transform, mut player, look, mode, bruise) =
        players.single_mut().expect("player does not exist");
    if bruise.is_changed() && !bruise.is_added() {
        player.knock(bruise.push);
    }
    if controls.just_pressed(Action::Fly) && mode.can_fly() {
        player.movement = match player.movement {
            Movement::Walk => Movement::Fly,
            Movement::Fly => Movement::Walk,
        };
        player.stop();
        info!("{:?}ing", player.movement);
    }
    let block = |at| {
        world
            .is_chunk_generated(ChunkCoordinate::containing(at))
            .then(|| world.get(at))
    };
    transform.translation = step(
        transform.translation,
        &mut player,
        *look,
        move_input(&controls),
        time.delta_secs(),
        block,
    );
}

fn move_input(controls: &Controls) -> MoveInput {
    let axis = |negative, positive| {
        if controls.pressed(negative) {
            -1.0
        } else if controls.pressed(positive) {
            1.0
        } else {
            0.0
        }
    };
    MoveInput {
        across: Vec3::new(
            axis(Action::Left, Action::Right),
            0.0,
            axis(Action::Forward, Action::Back),
        ),
        rise: -axis(Action::Jump, Action::Descend),
        jump: controls.pressed(Action::Jump),
        sprint: controls.pressed(Action::Sprint),
    }
}

/// Turns the player's look with the mouse, as sensitive as the Controls settings have it, and
/// upside down if they invert it.
pub fn player_look(
    mut looks: Query<&mut PlayerLook, With<LocalPlayer>>,
    mut motion: MessageReader<MouseMotion>,
    preferences: Res<Preferences>,
) {
    let mut look = looks.single_mut().expect("player does not exist");
    let controls = &preferences.controls;
    let turn = LOOK_SENSITIVITY * controls.sensitivity;
    let up = if controls.invert_y { -1.0 } else { 1.0 };
    for ev in motion.read() {
        look.turn(-ev.delta.x * turn, -ev.delta.y * turn * up);
    }
}

// How far out the camera is in third person, in blocks, how far short of a block in its way
// it's kept, and how far apart the rays checking for one are, round the line out to it, so its
// near plane's never in a block.
const THIRD_PERSON_DISTANCE: f32 = 4.0;
const CLEARANCE: f32 = 0.3;
const RAY_SPREAD: f32 = 0.15;

/// Puts the camera where the player it hangs from is seen from, as the view setting has it: in
/// their eyes, turned the way they look, or out behind them or in front looking back, pulled in
/// short of any block in the way (M-A4).
pub fn follow_look(
    looks: Query<(&Transform, &PlayerLook), Without<Camera3d>>,
    mut cameras: Query<(&ChildOf, &mut Transform), WorldCamera>,
    preferences: Res<Preferences>,
    world: Res<World>,
) {
    for (parent, mut camera) in &mut cameras {
        let Ok((feet, look)) = looks.get(parent.parent()) else {
            continue;
        };
        let eyes = feet.translation + Vec3::Y * EYE_HEIGHT;
        let stops = |block| world.get_block_at(block).is_solid();
        *camera = camera_place(preferences.view, *look, |out, reach| {
            clear_reach(eyes, out, reach, stops)
        });
    }
}

/// Where the camera is from the player's feet in `view`, looking as `look` does, out from their
/// eyes as far as `clear` says it can go, given the way out and how far it would.
pub fn camera_place(view: View, look: PlayerLook, clear: impl Fn(Vec3, f32) -> f32) -> Transform {
    let eyes = Vec3::Y * EYE_HEIGHT;
    let (out, rotation) = match view {
        View::FirstPerson => {
            return Transform::from_translation(eyes).with_rotation(look.rotation())
        }
        View::Behind => (-look.forward(), look.rotation()),
        View::InFront => (look.forward(), look.rotation() * Quat::from_rotation_y(PI)),
    };
    let distance = clear(out, THIRD_PERSON_DISTANCE);
    Transform::from_translation(eyes + out * distance).with_rotation(rotation)
}

/// How far out from `eyes` towards `out` the camera can go, up to `reach`, before a block that
/// `stops` it, less a little to keep clear of it: the nearest of a few rays round the line,
/// so the camera's corners keep out too.
pub fn clear_reach(eyes: Vec3, out: Vec3, reach: f32, stops: impl Fn(I64Vec3) -> bool) -> f32 {
    let across = out.any_orthonormal_vector();
    let up = out.cross(across);
    [Vec3::ZERO, across, -across, up, -up]
        .into_iter()
        .map(|offset| {
            let hit = raycast(eyes + offset * RAY_SPREAD, out, reach + CLEARANCE, &stops);
            hit.map_or(reach, |hit| (hit.point - eyes).dot(out) - CLEARANCE)
        })
        .fold(reach, f32::min)
        .max(0.0)
}

#[cfg(test)]
mod tests {
    use bevy::math::I64Vec3;
    use bevy::prelude::*;

    use super::{camera_place, clear_reach, follow_look, CLEARANCE, THIRD_PERSON_DISTANCE};
    use crate::player::{Player, PlayerLook, EYE_HEIGHT};
    use crate::preferences::{Preferences, View};
    use crate::world::World;

    #[test]
    fn test_the_camera_follows_the_players_look() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<Preferences>()
            .insert_resource(World::new(16))
            .add_systems(Update, follow_look);
        let look = PlayerLook {
            yaw: 2.0,
            pitch: -0.4,
        };
        let player = app.world_mut().spawn((Player::default(), look)).id();
        let camera = app
            .world_mut()
            .spawn((
                Camera3d::default(),
                Transform::from_xyz(0.0, EYE_HEIGHT, 0.0),
                ChildOf(player),
            ))
            .id();
        let rotation = |app: &App| app.world().get::<Transform>(camera).unwrap().rotation;
        app.update();
        assert!((rotation(&app) * Vec3::NEG_Z - look.forward()).length() < 1e-5);

        let mut turned = look;
        turned.turn(1.0, 0.2);
        app.world_mut().entity_mut(player).insert(turned);
        app.update();
        assert!(rotation(&app).angle_between(turned.rotation()) < 1e-5);
        let eyes = app.world().get::<Transform>(camera).unwrap().translation;
        assert_eq!(Vec3::Y * EYE_HEIGHT, eyes);

        // behind, out in the open, as far as the third person goes
        app.world_mut().resource_mut::<Preferences>().view = View::Behind;
        app.update();
        let camera = *app.world().get::<Transform>(camera).unwrap();
        let out = camera.translation - Vec3::Y * EYE_HEIGHT;
        assert!(
            (out + turned.forward() * THIRD_PERSON_DISTANCE).length() < 1e-4,
            "{out}"
        );
    }

    #[test]
    fn test_the_views_go_round_and_third_person_looks_back_at_the_player() {
        let views: Vec<_> = std::iter::successors(Some(View::FirstPerson), |v| Some(v.next()))
            .take(4)
            .collect();
        assert_eq!(
            [
                View::FirstPerson,
                View::Behind,
                View::InFront,
                View::FirstPerson
            ],
            views[..]
        );
        let look = PlayerLook {
            yaw: 0.7,
            pitch: 0.3,
        };
        let open = |_: Vec3, reach: f32| reach;
        let eyes = Vec3::Y * EYE_HEIGHT;
        for view in [View::Behind, View::InFront] {
            let camera = camera_place(view, look, open);
            // looking at the eyes from four blocks off
            let to_eyes = (eyes - camera.translation).normalize();
            assert!(
                (camera.forward().as_vec3() - to_eyes).length() < 1e-4,
                "{view:?}"
            );
            assert!((camera.translation.distance(eyes) - THIRD_PERSON_DISTANCE).abs() < 1e-4);
        }
        // in front is where they're looking
        let in_front = camera_place(View::InFront, look, open).translation - eyes;
        assert!(in_front.dot(look.forward()) > 0.0);
    }

    #[test]
    fn test_the_third_person_camera_is_pulled_in_short_of_a_wall() {
        // a wall two blocks behind the eyes, filling x from 2 on
        let eyes = Vec3::new(0.5, 10.5, 0.5);
        let wall = |block: I64Vec3| block.x >= 2;
        let reach = clear_reach(eyes, Vec3::X, THIRD_PERSON_DISTANCE, wall);
        assert!((reach - (1.5 - CLEARANCE)).abs() < 1e-4, "{reach}");
        // out in the open it goes all the way, and in a block not at all
        assert_eq!(4.0, clear_reach(eyes, Vec3::X, 4.0, |_| false));
        assert_eq!(0.0, clear_reach(eyes, Vec3::X, 4.0, |_| true));
        // a block only off to the side of the line still stops it, as its corners would go in
        let post =
            |block: I64Vec3| block == I64Vec3::new(3, 10, 0) || block == I64Vec3::new(3, 11, 0);
        let near_post = Vec3::new(0.5, 10.95, 0.5);
        assert!(clear_reach(near_post, Vec3::X, 4.0, post) < 2.5);
    }
}
