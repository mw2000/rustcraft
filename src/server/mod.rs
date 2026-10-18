//! What the server decides (INV-4): in singleplayer it runs in the same App as the client, and
//! a dedicated server runs it on its own (§4.4).

mod beds;
mod chunks;
mod clock;
mod commands;
mod containers;
mod crafting;
pub mod drops;
mod edits;
mod host;
mod inventory;
mod players;
mod survival;
mod sync;

pub use chunks::{keep_chunks_near_players, ServerChunks};
pub use clock::run_clock;
pub use host::{host, serve};
pub use players::HeldItem;

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::time::common_conditions::on_timer;
use bevy_replicon::prelude::{ClientId, ClientState, ClientSystems, ServerSystems};

use crate::clock::TimeOfDay;
use crate::containers::Containers;
use crate::player::name::PlayerName;
use crate::protocol::POSE_INTERVAL;
use crate::save::{PlayerState, WorldSave};
use crate::world::World;

/// The server's systems. Needs `RepliconPlugins` and `ProtocolPlugin`.
pub struct ServerPlugin;

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<edits::EditAllowances>()
            .init_resource::<TimeOfDay>()
            .init_resource::<clock::ClockRate>()
            .init_resource::<ServerChunks>()
            .init_resource::<Containers>()
            .init_resource::<drops::Chance>()
            .add_observer(players::welcome)
            .add_observer(survival::start_living)
            .add_observer(players::see_off)
            .add_systems(Startup, open_world.run_if(resource_exists::<WorldToOpen>))
            .add_systems(
                Update,
                (
                    run_clock,
                    containers::run_furnaces,
                    (drops::fall, drops::merge, drops::pick_up).chain(),
                )
                    .run_if(is_authority),
            )
            .add_systems(
                PostUpdate,
                // after the client's loader has taken the chunks it wants this frame, so in
                // singleplayer the server finds those near the player on their way already, and
                // before the edits checked against them, with every player where they've moved
                (players::move_players, keep_chunks_near_players)
                    .chain()
                    .before(edits::receive_edits)
                    .run_if(is_authority),
            )
            .add_systems(
                PostUpdate,
                // after replicon hands the server the requests made this frame, which with no
                // transport it does in `ClientSystems::Send`, and before it sends the answers,
                // so a singleplayer edit or command is made and answered in the frame it's
                // asked for
                (
                    (edits::receive_break_starts, edits::receive_edits).chain(),
                    // opened before it's clicked on
                    (
                        containers::open_containers,
                        inventory::receive_inventory_requests,
                    )
                        .chain(),
                    crafting::receive_crafts,
                    commands::run_commands,
                    (clock::wind_clock, clock::sync_clock).chain(),
                )
                    .after(ClientSystems::Send)
                    .before(ServerSystems::Send)
                    .run_if(is_authority),
            )
            .add_systems(
                PostUpdate,
                // with every player where they've moved, before the edits are sent
                (
                    sync::receive_reaches,
                    sync::follow_players,
                    sync::send_chunk_edits,
                )
                    .chain()
                    .after(players::move_players)
                    .before(ServerSystems::Send)
                    .run_if(is_authority),
            )
            .add_systems(
                PostUpdate,
                // with every player where they've moved, as falls are judged by it, and the
                // requests made this frame received, before what's changed is sent
                (
                    survival::receive_eating,
                    survival::receive_respawns,
                    beds::use_beds,
                    survival::live,
                    beds::get_up,
                    beds::skip_night,
                    survival::send_vitals,
                )
                    .chain()
                    .after(ClientSystems::Send)
                    .after(players::move_players)
                    .before(ServerSystems::Send)
                    .run_if(is_authority),
            )
            .add_systems(
                PostUpdate,
                // once this frame's requests and commands have changed them
                (
                    players::send_inventories,
                    players::send_game_modes,
                    players::hold_items,
                    containers::send_containers,
                )
                    .after(edits::receive_edits)
                    .after(inventory::receive_inventory_requests)
                    .after(commands::run_commands)
                    .after(survival::live)
                    .before(ServerSystems::Send)
                    .run_if(is_authority),
            )
            .add_systems(
                PostUpdate,
                players::send_host_pose
                    .before(ServerSystems::Send)
                    .run_if(is_authority)
                    .run_if(on_timer(POSE_INTERVAL)),
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
    /// The host's player's name.
    pub name: PlayerName,
}

/// Opens the world and its save, and spawns the host's player where they were saved, or at the
/// world's spawn if they're new.
pub fn open_world(mut commands: Commands, open: Res<WorldToOpen>) {
    let dir = &open.dir;
    let (save, saved, edits, containers) = WorldSave::open(dir.clone(), open.seed)
        .unwrap_or_else(|error| panic!("couldn't open the world in {}: {error}", dir.display()));
    let world = World::from_save(saved.seed, edits, open.region_distance);
    info!(
        "playing the world in {}, with seed {}",
        dir.display(),
        saved.seed
    );
    // the host's, as `world.toml` holds, and players joining keep their own saves
    let arrival = saved
        .player
        .unwrap_or_else(|| PlayerState::arriving_at(world.spawn()));
    let [x, y, z] = arrival.position;
    info!("spawned at {x:?}, {y:?}, {z:?}");
    commands.spawn(players::spawning(
        &arrival,
        ClientId::Server,
        open.name.clone(),
    ));
    commands.insert_resource(TimeOfDay(open.time_of_day.unwrap_or(saved.time_of_day)));
    commands.insert_resource(players::WorldChecksum::of(saved.seed));
    commands.insert_resource(world);
    commands.insert_resource(containers);
    commands.insert_resource(save);
    commands.remove_resource::<WorldToOpen>();
}
