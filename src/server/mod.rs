//! What the server decides (INV-4): in singleplayer it runs in the same App as the client, and
//! a dedicated server runs it on its own (§4.4).

mod allowances;
mod beds;
mod chat;
mod chunks;
mod clock;
mod commands;
mod console;
mod containers;
mod crafting;
pub mod drops;
mod edits;
mod farming;
mod host;
mod inventory;
mod players;
mod structures;
mod survival;
mod sync;

pub use chunks::{keep_chunks_near_players, ServerChunks};
pub use clock::run_clock;
pub use commands::Operators;
pub use console::{Console, ConsolePlugin};
pub use host::{host, serve};
pub use players::HeldItem;

use std::path::PathBuf;

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::common_conditions::on_timer;
use bevy_replicon::prelude::{ClientId, ClientState, ClientSystems, ServerSystems};
use bevy_replicon::RepliconPlugins;
use bevy_replicon_renet2::RepliconRenetPlugins;

use crate::clock::TimeOfDay;
use crate::containers::Containers;
use crate::farming::Crops;
use crate::game_mode::GameModePlugin;
use crate::player::name::PlayerName;
use crate::protocol::{ProtocolPlugin, POSE_INTERVAL};
use crate::save::{PlayerState, SavePlugin, WorldSave};
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
            .init_resource::<Crops>()
            .init_resource::<drops::Chance>()
            .init_resource::<chat::ChatAllowances>()
            .init_resource::<Operators>()
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
                    chat::receive_chat,
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
                // with the chunks near players loaded and this frame's edits made, so a crop
                // planted is looked at in the frame it's planted
                farming::grow_crops
                    .after(keep_chunks_near_players)
                    .after(edits::receive_edits)
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
                    structures::send_selections,
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

/// What a dedicated server runs on Bevy's `MinimalPlugins`: the server, saving and the console,
/// with replicon and its transport, and nothing of the client's (§4.4). It needs a
/// `WorldToOpen` with no host, a `Console`, and a transport to take players on, see `host`.
pub struct DedicatedServerPlugins;

impl PluginGroup for DedicatedServerPlugins {
    fn build(self) -> PluginGroupBuilder {
        PluginGroupBuilder::start::<Self>()
            .add(StatesPlugin)
            .add_group(RepliconPlugins)
            .add_group(RepliconRenetPlugins)
            .add(ProtocolPlugin)
            .add(ServerPlugin)
            .add(SavePlugin)
            .add(GameModePlugin)
            .add(ConsolePlugin)
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
    /// The host's player's name, or none for a dedicated server, which has no player of its own.
    pub host: Option<PlayerName>,
}

/// Opens the world and its save, and spawns the host's player, if there's a host, where they
/// were saved, or at the world's spawn if they're new.
pub fn open_world(mut commands: Commands, open: Res<WorldToOpen>) {
    let dir = &open.dir;
    let (save, saved, edits, containers, crops) = WorldSave::open(dir.clone(), open.seed)
        .unwrap_or_else(|error| panic!("couldn't open the world in {}: {error}", dir.display()));
    let world = World::from_save(saved.seed, edits, open.region_distance);
    info!(
        "playing the world in {}, with seed {}",
        dir.display(),
        saved.seed
    );
    // the host's, as `world.toml` holds, and players joining keep their own saves
    if let Some(name) = &open.host {
        let arrival = saved
            .player
            .unwrap_or_else(|| PlayerState::arriving_at(world.spawn()));
        let [x, y, z] = arrival.position;
        info!("spawned at {x:?}, {y:?}, {z:?}");
        commands.spawn(players::spawning(&arrival, ClientId::Server, name.clone()));
    }
    commands.insert_resource(TimeOfDay(open.time_of_day.unwrap_or(saved.time_of_day)));
    commands.insert_resource(players::WorldChecksum::of(saved.seed));
    commands.insert_resource(world);
    commands.insert_resource(containers);
    commands.insert_resource(crops);
    commands.insert_resource(Operators::new(saved.ops));
    commands.insert_resource(save);
    commands.remove_resource::<WorldToOpen>();
}
