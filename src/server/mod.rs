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
pub mod creatures;
pub mod drops;
mod edits;
mod farming;
pub mod fluids;
mod host;
mod inventory;
mod players;
mod structures;
mod survival;
mod sync;
mod weather;

pub use chunks::{keep_chunks_near_players, ServerChunks};
pub use clock::run_clock;
pub use commands::Operators;
pub use console::{Console, ConsolePlugin};
pub use host::{host, serve};
pub use players::{HeldItem, Worn};
pub use survival::Blow;

use std::path::PathBuf;

use bevy::app::PluginGroupBuilder;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::common_conditions::on_timer;
use bevy_replicon::prelude::{
    ClientId, ClientState, ClientSystems, ConnectedClient, ServerSystems,
};
use bevy_replicon::RepliconPlugins;
use bevy_replicon_renet2::netcode::NetcodeServerTransport;
use bevy_replicon_renet2::renet2::RenetServer;
use bevy_replicon_renet2::RepliconRenetPlugins;

use crate::clock::{SubTick, TimeOfDay};
use crate::containers::Containers;
use crate::creatures::{Creature, Missile};
use crate::farming::Crops;
use crate::game_mode::GameModePlugin;
use crate::player::name::PlayerName;
use crate::player::Player;
use crate::protocol::{ProtocolPlugin, POSE_INTERVAL};
use crate::save::{PlayerState, SavePlugin, WorldSave};
use crate::weather::Weather;
use crate::world::{order_leaving, LeaveWorld, LeaveWorldSystems, World};

/// The server's systems. Needs `RepliconPlugins` and `ProtocolPlugin`.
pub struct ServerPlugin;

impl Plugin for ServerPlugin {
    fn build(&self, app: &mut App) {
        order_leaving(app);
        app.add_message::<WorldUnopened>()
            .add_message::<Blow>()
            .init_resource::<edits::EditAllowances>()
            .init_resource::<TimeOfDay>()
            .init_resource::<SubTick>()
            .init_resource::<clock::ClockRate>()
            .init_resource::<Weather>()
            .init_resource::<ServerChunks>()
            .init_resource::<Containers>()
            .init_resource::<Crops>()
            .init_resource::<fluids::Fluids>()
            .init_resource::<drops::Chance>()
            .init_resource::<chat::ChatAllowances>()
            .init_resource::<Operators>()
            .init_resource::<creatures::Dormant>()
            .init_resource::<creatures::SpawnRate>()
            .init_resource::<creatures::Spawners>()
            .init_resource::<creatures::Settled>()
            .add_observer(players::welcome)
            .add_observer(survival::start_living)
            .add_observer(players::see_off)
            // at the start of the frame after it's asked for, so the rest of the frame has it
            .add_systems(PreUpdate, open_world.run_if(resource_exists::<WorldToOpen>))
            .add_systems(
                LeaveWorld,
                (
                    disconnect_joiners.in_set(LeaveWorldSystems::Disconnect),
                    close_world.in_set(LeaveWorldSystems::Clear),
                ),
            )
            .add_systems(
                Update,
                (
                    (run_clock, weather::run_weather).chain(),
                    containers::run_furnaces,
                    (drops::fall, drops::merge, drops::pick_up).chain(),
                    (
                        creatures::wake,
                        creatures::settle,
                        creatures::build_spawners,
                        creatures::spawn,
                        creatures::take_paths,
                        creatures::make_din,
                        creatures::grow,
                        creatures::think,
                        creatures::move_creatures,
                        creatures::use_doors,
                        creatures::fight,
                        creatures::fly_missiles,
                        creatures::breed,
                        creatures::despawn,
                        creatures::vanish,
                        creatures::sleep,
                    )
                        .chain(),
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
                    (
                        edits::receive_break_starts,
                        edits::receive_edits,
                        edits::use_doors,
                    )
                        .chain(),
                    // opened before it's clicked on
                    (
                        containers::open_containers,
                        crafting::open_stations,
                        inventory::receive_inventory_requests,
                    )
                        .chain(),
                    commands::run_commands,
                    chat::receive_chat,
                    (clock::wind_clock, clock::sync_clock).chain(),
                    (
                        creatures::receive_hits,
                        creatures::receive_uses,
                        (creatures::open_trades, creatures::make_trades).chain(),
                    ),
                    players::relay_swings,
                    players::receive_skins,
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
                    sync::send_edit_summaries,
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
                // with this frame's edits made, which wake the fluids beside them, and the
                // chunks near players loaded, which they flow in, before what they change is
                // sent with the areas moved
                fluids::flow
                    .after(keep_chunks_near_players)
                    .after(edits::use_doors)
                    .after(farming::grow_crops)
                    .after(sync::follow_players)
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
                    survival::send_bruises,
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
                    creatures::send_offers,
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
                // once this frame's requests have changed it, so a client hears of it at once
                weather::sync_weather
                    .after(ClientSystems::Send)
                    .before(ServerSystems::Send)
                    .run_if(is_authority),
            )
            .add_systems(
                PostUpdate,
                (players::send_host_pose, creatures::send_poses)
                    .before(ServerSystems::Send)
                    .run_if(is_authority)
                    .run_if(on_timer(POSE_INTERVAL)),
            )
            .add_systems(
                PostUpdate,
                // with every player where they've moved, so each client's shown the creatures
                // round where its player is now
                creatures::follow_whereabouts
                    .after(players::move_players)
                    .before(ServerSystems::Send)
                    .run_if(is_authority),
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

/// Whether this App decides the game: it does once its server has opened a world, unless it's a
/// client of a server elsewhere, so in singleplayer, when hosting, and as a dedicated server, but
/// not in the menus, nor in the world a client was playing on as it's disconnected.
pub fn is_authority(state: Res<State<ClientState>>, world: Option<Res<World>>) -> bool {
    *state.get() == ClientState::Disconnected && world.is_some_and(|world| world.is_served_here())
}

/// The world for the server to open as it starts, see `open_world`.
#[derive(Resource)]
pub struct WorldToOpen {
    /// The directory it's saved in, or is to be if it's new.
    pub dir: PathBuf,
    /// The seed it takes if it's new, or else a random one.
    pub seed: Option<u32>,
    /// The time of day to start at instead of the saved one, in days, as `TimeOfDay::from_days`
    /// takes it.
    pub time_of_day: Option<f64>,
    /// How far, in chunks, terrain is built from full-detail regions, see `World::new`.
    pub region_distance: u32,
    /// The host's player's name, or none for a dedicated server, which has no player of its own.
    pub host: Option<PlayerName>,
}

/// Why the world asked for couldn't be opened, such as its being from a newer version of the
/// game, as `open_world` finds.
#[derive(Message, Debug, Clone)]
pub struct WorldUnopened(pub String);

/// Opens the world and its save, and spawns the host's player, if there's a host, where they
/// were saved, or at the world's spawn if they're new.
pub fn open_world(
    mut commands: Commands,
    open: Res<WorldToOpen>,
    mut unopened: MessageWriter<WorldUnopened>,
) {
    commands.remove_resource::<WorldToOpen>();
    let dir = &open.dir;
    let (mut save, saved, edits, containers, crops) = match WorldSave::open(dir.clone(), open.seed)
    {
        Ok(opened) => opened,
        Err(error) => {
            let text = format!("Couldn't open the world in {}: {error}", dir.display());
            error!("{text}");
            unopened.write(WorldUnopened(text));
            return;
        }
    };
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
            .unwrap_or_else(|| PlayerState::arriving_at(world.spawn(), saved.new_player_mode));
        let [x, y, z] = arrival.position;
        info!("spawned at {x:?}, {y:?}, {z:?}");
        players::spawn_player(
            &mut commands,
            &save,
            &arrival,
            ClientId::Server,
            name.clone(),
        );
    }
    let clock = open
        .time_of_day
        .map(TimeOfDay::from_days)
        .unwrap_or(TimeOfDay(saved.time));
    commands.insert_resource(clock);
    commands.insert_resource(Weather::new(saved.seed, clock));
    commands.insert_resource(SubTick::default());
    commands.insert_resource(players::WorldChecksum::of(saved.seed));
    commands.insert_resource(world);
    commands.insert_resource(containers);
    commands.insert_resource(crops);
    commands.insert_resource(creatures::Dormant::of(save.take_creatures()));
    let settled = saved.settled.iter().map(|&[x, z]| (x, z));
    commands.insert_resource(creatures::Settled(settled.collect()));
    commands.insert_resource(Operators::new(saved.ops));
    commands.insert_resource(save);
}

// Sees off everyone who's joined as the world's left, which keeps their players' states to save,
// and stops taking players, which tells their games they've been disconnected.
fn disconnect_joiners(clients: Query<Entity, With<ConnectedClient>>, mut commands: Commands) {
    for client in &clients {
        commands.entity(client).despawn();
    }
    commands.remove_resource::<NetcodeServerTransport>();
    commands.remove_resource::<RenetServer>();
}

// Puts away the world the server had open, once it's saved: its players, its creatures, the
// missiles in flight and the items lying in it, and its resources as they were before it opened.
#[allow(clippy::type_complexity)]
fn close_world(
    things: Query<
        Entity,
        Or<(
            With<Player>,
            With<Creature>,
            With<Missile>,
            With<drops::DroppedItem>,
        )>,
    >,
    mut commands: Commands,
) {
    for thing in &things {
        commands.entity(thing).try_despawn();
    }
    commands.remove_resource::<World>();
    commands.remove_resource::<players::WorldChecksum>();
    commands.insert_resource(edits::EditAllowances::default());
    commands.insert_resource(TimeOfDay::default());
    commands.insert_resource(SubTick::default());
    commands.insert_resource(clock::ClockRate::default());
    commands.insert_resource(Weather::default());
    commands.insert_resource(ServerChunks::default());
    commands.insert_resource(Containers::default());
    commands.insert_resource(Crops::default());
    commands.insert_resource(fluids::Fluids::default());
    commands.insert_resource(chat::ChatAllowances::default());
    commands.insert_resource(Operators::default());
    commands.insert_resource(creatures::Dormant::default());
    commands.insert_resource(creatures::Spawners::default());
    commands.insert_resource(creatures::Settled::default());
}
