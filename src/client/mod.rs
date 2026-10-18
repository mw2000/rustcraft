//! What a player's game does with what the server decides (§4.4): in singleplayer it runs in the
//! same App as the server.

pub mod authoring;
pub mod clock;
pub mod command_bar;
pub mod cracks;
pub mod creatures;
pub mod drops;
pub mod edits;
pub mod join;
pub mod models;
pub mod others;
pub mod player;

use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy_replicon::prelude::{ClientState, ServerSystems};

use crate::chunks::group::rebuild_groups;
use crate::clock::TimeOfDay;
use crate::protocol::Welcome;
use crate::server::is_authority;
use crate::world::World;

/// The client's systems. Needs `RepliconPlugins` and `ProtocolPlugin`.
pub struct ClientPlugin;

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TimeOfDay>()
            .init_resource::<join::GenerationChecksum>()
            .init_resource::<edits::Predictions>()
            .init_resource::<join::Teleports>()
            .add_observer(player::mark_local_player)
            .add_systems(
                PostStartup,
                // the scene's camera, spawned once the server has spawned the player, is in
                // place before anything looks through it
                player::attach_camera.before(TransformSystems::Propagate),
            )
            .add_observer(join::refused)
            .add_systems(
                PostUpdate,
                // a client joining a server has its player once it's welcomed, with the camera
                // on them before transforms propagate, so the next frame finds it where the
                // player is, and its world once it's checked it
                (
                    join::arrive.run_if(on_message::<Welcome>),
                    join::check_world.run_if(resource_exists::<join::Checking>),
                    (
                        player::attach_camera,
                        join::follow_inventory,
                        join::follow_game_mode,
                        join::follow_selection,
                        join::follow_teleports,
                        join::follow_vitals,
                    ),
                )
                    .chain()
                    .before(TransformSystems::Propagate),
            )
            .add_systems(
                PostUpdate,
                // after replicon hands over what the server sent, which in singleplayer it does
                // in `ServerSystems::Send`, the same frame, and rebuilding at once rather than in
                // the next frame's `Update`, so an edit is redrawn as soon as it was before the
                // server. A chunk's edits go in before the changes since, which are newer.
                (
                    edits::predict_edits
                        .run_if(not(is_authority))
                        .run_if(resource_exists::<World>),
                    edits::apply_chunk_edits,
                    edits::apply_block_changes,
                    edits::apply_edit_summaries,
                    (edits::roll_back_refused, rebuild_groups).run_if(resource_exists::<World>),
                )
                    .chain()
                    .after(ServerSystems::Send)
                    .after(join::check_world),
            )
            // after replicon hands over the server's syncs, which over a network it does in
            // `PreUpdate`
            .add_systems(
                Update,
                (
                    clock::follow_clock.run_if(not(is_authority)),
                    join::send_pose.run_if(in_state(ClientState::Connected)),
                    join::report_connection,
                    (
                        others::see_players,
                        creatures::see_creatures,
                        others::receive_poses,
                        creatures::follow_creatures,
                        others::move_seen,
                    )
                        .chain(),
                ),
            );
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
