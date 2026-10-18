//! What a player's game does with what the server decides (§4.4): in singleplayer it runs in the
//! same App as the server.

pub mod edits;

use bevy::prelude::*;
use bevy_replicon::prelude::ServerSystems;

use crate::chunks::group::rebuild_groups;

/// The client's systems. Needs `RepliconPlugins` and `ProtocolPlugin`.
pub struct ClientPlugin;

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            // after replicon hands over what the server sent, which in singleplayer it does in
            // `ServerSystems::Send`, the same frame, and rebuilding at once rather than in the
            // next frame's `Update`, so an edit is redrawn as soon as it was before the server
            (edits::apply_block_changes, rebuild_groups)
                .chain()
                .after(ServerSystems::Send),
        );
    }
}
