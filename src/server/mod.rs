//! What the server decides (INV-4): in singleplayer it runs in the same App as the client, and
//! a dedicated server runs it on its own (§4.4).

mod edits;

use bevy::prelude::*;
use bevy_replicon::prelude::{ClientState, ClientSystems, ServerSystems};

/// The server's systems. Needs `RepliconPlugins` and `ProtocolPlugin`.
pub struct ServerPlugin;

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<edits::EditAllowances>().add_systems(
            PostUpdate,
            // after replicon hands the server the requests made this frame, which with no
            // transport it does in `ClientSystems::Send`, and before it sends the answers, so
            // a singleplayer edit is made and answered in the frame it's asked for
            edits::receive_edits
                .after(ClientSystems::Send)
                .before(ServerSystems::Send)
                .run_if(is_authority),
        );
    }
}

/// Whether this App decides the game: it does unless it's a client connected to a server
/// elsewhere, so in singleplayer, when hosting, and as a dedicated server.
pub fn is_authority(state: Res<State<ClientState>>) -> bool {
    *state.get() == ClientState::Disconnected
}
