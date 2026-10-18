//! What a player's game does with what the server decides (§4.4): in singleplayer it runs in the
//! same App as the server.

pub mod clock;
pub mod edits;
pub mod player;

use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy_replicon::prelude::ServerSystems;

use crate::chunks::group::rebuild_groups;
use crate::clock::TimeOfDay;
use crate::server::is_authority;

/// The client's systems. Needs `RepliconPlugins` and `ProtocolPlugin`.
pub struct ClientPlugin;

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TimeOfDay>()
            .add_observer(player::mark_local_player)
            .add_systems(
                PostStartup,
                // the scene's camera, spawned once the server has spawned the player, is in
                // place before anything looks through it
                player::attach_camera.before(TransformSystems::Propagate),
            )
            .add_systems(
                PostUpdate,
                // after replicon hands over what the server sent, which in singleplayer it does
                // in `ServerSystems::Send`, the same frame, and rebuilding at once rather than in
                // the next frame's `Update`, so an edit is redrawn as soon as it was before the
                // server
                (edits::apply_block_changes, rebuild_groups)
                    .chain()
                    .after(ServerSystems::Send),
            )
            // after replicon hands over the server's syncs, which over a network it does in
            // `PreUpdate`
            .add_systems(Update, clock::follow_clock.run_if(not(is_authority)));
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use bevy_replicon::prelude::ClientId;

    use super::player::LocalPlayer;
    use super::ClientPlugin;
    use crate::player::{Player, PlayerOwner, EYE_HEIGHT};

    #[test]
    fn test_the_camera_hangs_from_the_hosts_player() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, ClientPlugin));
        let camera = app.world_mut().spawn(Camera3d::default()).id();
        let guest = ClientId::Client(Entity::from_raw_u32(7).unwrap());
        let guest = app
            .world_mut()
            .spawn((Player::default(), PlayerOwner(guest)))
            .id();
        let host = app
            .world_mut()
            .spawn((Player::default(), PlayerOwner(ClientId::Server)))
            .id();
        // just the startup, as the rest of the client needs terrain to draw
        app.world_mut().run_schedule(PostStartup);
        assert!(app.world().entity(host).contains::<LocalPlayer>());
        assert!(!app.world().entity(guest).contains::<LocalPlayer>());
        assert_eq!(host, app.world().get::<ChildOf>(camera).unwrap().parent());
        let eyes = app.world().get::<Transform>(camera).unwrap().translation;
        assert_eq!(Vec3::Y * EYE_HEIGHT, eyes);
    }
}
