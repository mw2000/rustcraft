//! What the server decides (INV-4): in singleplayer it runs in the same App as the client, and
//! a dedicated server runs it on its own (§4.4).

mod edits;

use std::path::PathBuf;

use bevy::prelude::*;
use bevy_replicon::prelude::{ClientId, ClientState, ClientSystems, ServerSystems};

use crate::clock::TimeOfDay;
use crate::player::{Player, PlayerOwner};
use crate::save::{PlayerState, WorldSave};
use crate::world::World;

/// The server's systems. Needs `RepliconPlugins` and `ProtocolPlugin`.
pub struct ServerPlugin;

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<edits::EditAllowances>()
            .add_systems(Startup, open_world.run_if(resource_exists::<WorldToOpen>))
            .add_systems(
                PostUpdate,
                // after replicon hands the server the requests made this frame, which with no
                // transport it does in `ClientSystems::Send`, and before it sends the answers,
                // so a singleplayer edit is made and answered in the frame it's asked for
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

/// The world for the server to open as it starts, see `open_world`.
#[derive(Resource)]
pub struct WorldToOpen {
    /// The directory it's saved in, or is to be if it's new.
    pub dir: PathBuf,
    /// The seed it takes if it's new, or else a random one.
    pub seed: Option<u32>,
    /// The time of day to start at instead of the saved one.
    pub time_of_day: Option<f32>,
    /// How far, in chunks, terrain is built from full-detail regions, see `World::new`.
    pub region_distance: u32,
}

/// Opens the world and its save, and spawns the host's player where they were saved, or at the
/// world's spawn if they're new.
pub fn open_world(mut commands: Commands, open: Res<WorldToOpen>) {
    let dir = &open.dir;
    let (save, saved, edits) = WorldSave::open(dir.clone(), open.seed)
        .unwrap_or_else(|error| panic!("couldn't open the world in {}: {error}", dir.display()));
    let world = World::from_save(saved.seed, edits, open.region_distance);
    info!(
        "playing the world in {}, with seed {}",
        dir.display(),
        saved.seed
    );
    // the host's, as the one player `world.toml` holds until M0.6 saves each player's
    let arrival = saved
        .player
        .unwrap_or_else(|| PlayerState::arriving_at(world.spawn()));
    let [x, y, z] = arrival.position;
    info!("spawned at {x:?}, {y:?}, {z:?}");
    let mut body = Player::default();
    body.movement = arrival.movement;
    commands.spawn((
        body,
        PlayerOwner(ClientId::Server),
        arrival.look(),
        arrival.game_mode,
        arrival.hotbar,
        arrival.body(),
    ));
    commands.insert_resource(TimeOfDay(open.time_of_day.unwrap_or(saved.time_of_day)));
    commands.insert_resource(world);
    commands.insert_resource(save);
    commands.remove_resource::<WorldToOpen>();
}
