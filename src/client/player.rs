use bevy::prelude::*;
use bevy_replicon::prelude::ClientId;

use crate::player::{PlayerOwner, EYE_HEIGHT};
use crate::protocol::SetSkin;
use crate::skin::Skin;

/// On the player this App's client plays as: the one its input moves and its camera hangs from.
#[derive(Component)]
pub struct LocalPlayer;

/// A camera that draws a model of its own rather than the world, such as the skin's preview in
/// Settings, which what follows the world's camera leaves alone.
#[derive(Component)]
pub struct ModelCamera;

/// The camera the world's seen through.
pub type WorldCamera = (With<Camera3d>, Without<ModelCamera>);

/// The skin this player's chosen to wear, or none for the default their name gives them, and
/// why the one they last chose couldn't be worn, if it couldn't.
#[derive(Resource, Default)]
pub struct OwnSkin {
    pub skin: Option<Skin>,
    pub problem: Option<String>,
}

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
    cameras: Query<Entity, (WorldCamera, Without<ChildOf>)>,
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
    cameras: Query<Entity, (WorldCamera, With<ChildOf>)>,
) {
    for camera in &cameras {
        commands.entity(camera).remove::<ChildOf>();
    }
}

/// Tells the server the skin the local player wears as they arrive and whenever it's changed,
/// and puts it on them, as the server only shows it to everyone else.
pub(super) fn wear_own_skin(
    own: Res<OwnSkin>,
    local: Query<(Entity, Option<&Skin>), With<LocalPlayer>>,
    mut told: Local<Option<Entity>>,
    mut skins: MessageWriter<SetSkin>,
    mut commands: Commands,
) {
    let Ok((player, worn)) = local.single() else {
        *told = None;
        return;
    };
    if *told == Some(player) && !own.is_changed() {
        return;
    }
    *told = Some(player);
    skins.write(SetSkin(own.skin.clone()));
    match &own.skin {
        Some(skin) if worn != Some(skin) => {
            commands.entity(player).insert(skin.clone());
        }
        None if worn.is_some() => {
            commands.entity(player).remove::<Skin>();
        }
        _ => {}
    }
}
