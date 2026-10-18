use bevy::{
    camera::Camera,
    ecs::{
        hierarchy::ChildOf,
        message::MessageReader,
        query::With,
        system::{Query, Res},
    },
    input::{keyboard::KeyCode, mouse::MouseMotion, ButtonInput},
    log::info,
    math::Vec3,
    time::Time,
    transform::components::Transform,
};

use super::physics::{step, MoveInput};
use super::{Movement, Player, PlayerLook};
use crate::{
    chunks::chunk::ChunkCoordinate, client::player::LocalPlayer, game_mode::GameMode, world::World,
};

const FLY_TOGGLE_KEY: KeyCode = KeyCode::KeyF;
// How far the mouse turns the view, per pixel it moves. Mouse motion is already a distance, so
// it isn't scaled by the frame's length, which would turn the view less the higher the frame rate.
const LOOK_SENSITIVITY: f32 = 0.1_f32.to_radians();

/// Moves the player by the keys held: WASD across, Space and Ctrl up and down or jumping,
/// Shift sprinting, and F between flying and walking where the game mode allows.
pub fn player_move(
    time: Res<Time>,
    world: Res<World>,
    mut players: Query<(&mut Transform, &mut Player, &PlayerLook, &GameMode), With<LocalPlayer>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let (mut transform, mut player, look, mode) =
        players.single_mut().expect("player does not exist");
    if keys.just_pressed(FLY_TOGGLE_KEY) && mode.can_fly() {
        player.movement = match player.movement {
            Movement::Walk => Movement::Fly,
            Movement::Fly => Movement::Walk,
        };
        player.velocity = Vec3::ZERO;
        info!("{:?}ing", player.movement);
    }
    let block = |at| {
        world
            .is_chunk_generated(ChunkCoordinate::containing(at))
            .then(|| world.get_block_at(at))
    };
    transform.translation = step(
        transform.translation,
        &mut player,
        *look,
        move_input(&keys),
        time.delta_secs(),
        block,
    );
}

fn move_input(keys: &ButtonInput<KeyCode>) -> MoveInput {
    let axis = |negative, positive| {
        if keys.pressed(negative) {
            -1.0
        } else if keys.pressed(positive) {
            1.0
        } else {
            0.0
        }
    };
    MoveInput {
        across: Vec3::new(
            axis(KeyCode::KeyA, KeyCode::KeyD),
            0.0,
            axis(KeyCode::KeyW, KeyCode::KeyS),
        ),
        rise: -axis(KeyCode::Space, KeyCode::ControlLeft),
        jump: keys.pressed(KeyCode::Space),
        sprint: keys.pressed(KeyCode::ShiftLeft),
    }
}

/// Turns the player's look with the mouse.
pub fn player_look(
    mut looks: Query<&mut PlayerLook, With<LocalPlayer>>,
    mut motion: MessageReader<MouseMotion>,
) {
    let mut look = looks.single_mut().expect("player does not exist");
    for ev in motion.read() {
        look.turn(
            -ev.delta.x * LOOK_SENSITIVITY,
            -ev.delta.y * LOOK_SENSITIVITY,
        );
    }
}

/// Turns the camera the way the player it hangs from looks.
pub fn follow_look(
    looks: Query<&PlayerLook>,
    mut cameras: Query<(&ChildOf, &mut Transform), With<Camera>>,
) {
    for (parent, mut camera) in &mut cameras {
        if let Ok(look) = looks.get(parent.parent()) {
            camera.rotation = look.rotation();
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use super::follow_look;
    use crate::player::{Player, PlayerLook, EYE_HEIGHT};

    #[test]
    fn test_the_camera_follows_the_players_look() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
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
    }
}
