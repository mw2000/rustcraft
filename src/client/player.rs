use bevy::prelude::*;
use bevy_replicon::prelude::ClientId;

use crate::player::{PlayerOwner, EYE_HEIGHT};

/// On the player this App's client plays as: the one its input moves and its camera hangs from.
#[derive(Component)]
pub struct LocalPlayer;

/// Marks the host's player as local, which in singleplayer is the only one. A client joining a
/// server (M0.6) will instead be told which player is theirs.
pub(super) fn mark_local_player(
    add: On<Add<PlayerOwner>>,
    owners: Query<&PlayerOwner>,
    mut commands: Commands,
) {
    if owners
        .get(add.entity)
        .is_ok_and(|owner| owner.0 == ClientId::Server)
    {
        commands.entity(add.entity).insert(LocalPlayer);
    }
}

/// Hangs the camera from the local player's eyes.
pub(super) fn attach_camera(
    mut commands: Commands,
    local: Query<Entity, With<LocalPlayer>>,
    cameras: Query<Entity, (With<Camera3d>, Without<ChildOf>)>,
) {
    let Ok(player) = local.single() else {
        return;
    };
    for camera in &cameras {
        commands
            .entity(camera)
            .insert((ChildOf(player), Transform::from_xyz(0.0, EYE_HEIGHT, 0.0)));
    }
}

/// Takes the camera down from the player as the world's left, so it isn't put away with them.
pub(super) fn detach_camera(
    mut commands: Commands,
    cameras: Query<Entity, (With<Camera3d>, With<ChildOf>)>,
) {
    for camera in &cameras {
        commands.entity(camera).remove::<ChildOf>();
    }
}
