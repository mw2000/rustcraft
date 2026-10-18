use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bevy::state::state::StateTransitionEvent;
use bevy::tasks::{futures, AsyncComputeTaskPool, Task};
use bevy_replicon::prelude::*;
use bevy_replicon_renet2::netcode::{
    ClientAuthentication, ClientSocket, NativeSocket, NetcodeClientTransport, NetcodeError,
};
use bevy_replicon_renet2::renet2::{ConnectionConfig, RenetClient};
use bevy_replicon_renet2::RenetChannelsExt;

use super::edits::{EditOrder, Predictions};
use super::player::LocalPlayer;
use crate::authoring::Selection;
use crate::chunks::generate::checksum::world_checksum;
use crate::game_mode::GameMode;
use crate::inventory::Hotbar;
use crate::inventory::Inventory;
use crate::player::name::PlayerName;
use crate::player::{arriving, Player, PlayerLook};
use crate::protocol::{
    name_to_user_data, CommandOutput, EditReach, GameModeChanged, InventoryChanged, PlayerPose,
    SelectionChanged, Teleport, VitalsChanged, Welcome, DEFAULT_PORT, NETCODE_PROTOCOL_ID,
    POSE_INTERVAL, PROTOCOL_VERSION,
};
use crate::settings::Settings;
use crate::survival::{Asleep, Vitals};
use crate::world::World;

/// The server at `address`, `host:port` or just `host` for the default port.
pub fn server_address(address: &str) -> std::io::Result<SocketAddr> {
    use std::net::ToSocketAddrs;
    let mut found = match address.to_socket_addrs() {
        Ok(found) => found,
        Err(_) => (address, DEFAULT_PORT).to_socket_addrs()?,
    };
    found
        .next()
        .ok_or_else(|| std::io::ErrorKind::NotFound.into())
}

/// Starts joining the server at `server` over UDP as `name`, as a client with no world of its
/// own until the server's `Welcome`.
pub fn join(
    commands: &mut Commands,
    channels: &RepliconChannels,
    server: SocketAddr,
    name: &PlayerName,
) -> Result<(), NetcodeError> {
    let any: std::net::IpAddr = if server.is_ipv4() {
        Ipv4Addr::UNSPECIFIED.into()
    } else {
        Ipv6Addr::UNSPECIFIED.into()
    };
    let socket = NativeSocket::new(UdpSocket::bind((any, 0))?)?;
    let (client, transport) = connect(channels, socket, server, name)?;
    commands.insert_resource(client);
    commands.insert_resource(transport);
    Ok(())
}

/// A client connecting to `server` on `socket` as `name`, which it sends as it connects, whose
/// resources start it once inserted. In netcode's unsecure mode, as the server is
/// (`server::serve`).
pub fn connect(
    channels: &RepliconChannels,
    socket: impl ClientSocket,
    server: SocketAddr,
    name: &PlayerName,
) -> Result<(RenetClient, NetcodeClientTransport), NetcodeError> {
    let client = RenetClient::new(
        ConnectionConfig::from_channels(channels.server_configs(), channels.client_configs()),
        socket.is_reliable(),
    );
    let authentication = ClientAuthentication::Unsecure {
        protocol_id: NETCODE_PROTOCOL_ID,
        // only told apart by it, so random to keep players from clashing
        client_id: rand::random(),
        socket_id: 0,
        server_addr: server,
        user_data: Some(name_to_user_data(name)),
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let transport = NetcodeClientTransport::new(now, authentication, socket)?;
    Ok((client, transport))
}

/// Why this client's left the server it was joining or playing on, as the menus tell the player:
/// it couldn't be reached, it plays another version, it generates another world, or the
/// connection was lost.
#[derive(Message, Debug, Clone)]
pub struct LeftServer(pub String);

/// How this client works out the checksum of what a seed generates: `world_checksum`, unless a
/// test swaps it to play a client that generates a different world from the server's.
#[derive(Resource, Clone, Copy)]
pub struct GenerationChecksum(pub fn(u32) -> u64);

impl Default for GenerationChecksum {
    fn default() -> Self {
        Self(world_checksum)
    }
}

/// The world a client's been welcomed to, while it works out whether it generates the same
/// terrain from the seed as the server (D-3).
#[derive(Resource)]
pub struct Checking {
    seed: u32,
    server: u64,
    task: Task<u64>,
}

/// Spawns this client's player where the server has them, starts checking the world it's
/// welcomed to, on a task as that takes a fraction of a second (INV-7), and asks for the edits
/// of the chunks it draws at full detail, and as LOD nodes draw them as far as it draws.
pub(super) fn arrive(
    mut welcomes: MessageReader<Welcome>,
    checksum: Res<GenerationChecksum>,
    settings: Res<Settings>,
    mut predictions: ResMut<Predictions>,
    mut reach: MessageWriter<EditReach>,
    mut commands: Commands,
) {
    let Some(welcome) = welcomes.read().last() else {
        return;
    };
    predictions.me = Some(welcome.you);
    commands.insert_resource(EditOrder::default());
    let layout = settings.renderer.lod_layout();
    reach.write(EditReach {
        full_detail: layout.full_detail_reach(),
        view: layout.render_distance().ceil() as u32,
    });
    let (seed, GenerationChecksum(checksum)) = (welcome.seed, *checksum);
    commands.insert_resource(Checking {
        seed,
        server: welcome.checksum,
        task: AsyncComputeTaskPool::get().spawn(async move { checksum(seed) }),
    });
    commands.spawn((arriving(&welcome.player), LocalPlayer));
    let [x, y, z] = welcome.player.position;
    info!("joined a world with seed {seed}, at {x:?}, {y:?}, {z:?}");
}

/// Makes the world this client's welcomed to, generating its terrain from the seed (INV-1),
/// once it's found that it generates the same as the server's. If it doesn't, it says so and
/// leaves, rather than play on a world that isn't the one the others see.
pub(super) fn check_world(
    mut checking: ResMut<Checking>,
    settings: Res<Settings>,
    client: Option<ResMut<RenetClient>>,
    mut output: MessageWriter<CommandOutput>,
    mut left: MessageWriter<LeftServer>,
    mut commands: Commands,
) {
    let Some(checksum) = futures::check_ready(&mut checking.task) else {
        return;
    };
    commands.remove_resource::<Checking>();
    if checksum != checking.server {
        let text = format!(
            "Your copy of rustcraft generates a different world from the host's, so you can't \
             play on it together (checksum {checksum:016x}, the host's {:016x}). The host may \
             be on another kind of computer (D-3).",
            checking.server
        );
        error!("{text}");
        left.write(LeftServer(text.clone()));
        output.write(CommandOutput { text, error: true });
        if let Some(mut client) = client {
            client.disconnect();
        }
        return;
    }
    let region_distance = settings.renderer.lod_layout().region_reach() as u32;
    let world = World::joined(checking.seed, region_distance);
    commands.insert_resource(world);
}

/// Puts the inventory the server says this client's player has on them.
pub(super) fn follow_inventory(
    mut changes: MessageReader<InventoryChanged>,
    mut player: Query<&mut Inventory, With<LocalPlayer>>,
) {
    if let (Some(InventoryChanged(inventory)), Ok(mut held)) =
        (changes.read().last(), player.single_mut())
    {
        held.clone_from(inventory);
    }
}

/// Puts the game mode the server says this client's player has on them.
pub(super) fn follow_game_mode(
    mut changes: MessageReader<GameModeChanged>,
    mut player: Query<&mut GameMode, With<LocalPlayer>>,
) {
    if let (Some(GameModeChanged(mode)), Ok(mut held)) =
        (changes.read().last(), player.single_mut())
    {
        *held = *mode;
    }
}

/// Puts the selection the server says this client's player has on them.
pub(super) fn follow_selection(
    mut changes: MessageReader<SelectionChanged>,
    mut player: Query<&mut Selection, With<LocalPlayer>>,
) {
    if let (Some(SelectionChanged(selection)), Ok(mut held)) =
        (changes.read().last(), player.single_mut())
    {
        *held = *selection;
    }
}

/// How many times the server has put this client's player somewhere, which it tells with each
/// pose.
#[derive(Resource, Default)]
pub(super) struct Teleports(u32);

/// Puts this client's player where the server says, stopped.
pub(super) fn follow_teleports(
    mut teleports: MessageReader<Teleport>,
    mut count: ResMut<Teleports>,
    mut player: Query<(&mut Transform, &mut Player), With<LocalPlayer>>,
) {
    for teleport in teleports.read() {
        count.0 += 1;
        if let Ok((mut body, mut player)) = player.single_mut() {
            body.translation = teleport.position;
            player.stop();
        }
    }
}

/// Tells the server where this client's player is, every `POSE_INTERVAL`.
pub(super) fn send_pose(
    time: Res<Time>,
    teleports: Res<Teleports>,
    player: Query<(&Transform, &PlayerLook, &Player, &Hotbar), With<LocalPlayer>>,
    mut next: Local<Duration>,
    mut poses: MessageWriter<PlayerPose>,
) {
    let Ok((body, look, player, hotbar)) = player.single() else {
        return;
    };
    if time.elapsed() < *next {
        return;
    }
    *next = time.elapsed() + POSE_INTERVAL;
    poses.write(PlayerPose {
        at: time.elapsed().as_millis() as u32,
        position: body.translation,
        yaw: look.yaw,
        pitch: look.pitch,
        movement: player.movement,
        slot: hotbar.selected as u8,
        teleports: teleports.0,
    });
}

/// Says in the command bar, and the log, when joining fails or the server goes, but not when
/// this client let go of it itself, leaving the world.
pub(super) fn report_connection(
    mut transitions: MessageReader<StateTransitionEvent<ClientState>>,
    client: Option<Res<RenetClient>>,
    mut output: MessageWriter<CommandOutput>,
    mut left: MessageWriter<LeftServer>,
) {
    for transition in transitions.read() {
        let Some(client) = &client else {
            continue;
        };
        let why = client
            .disconnect_reason()
            .map_or_else(String::new, |reason| format!(": {reason}"));
        let text = match (transition.exited, transition.entered) {
            (Some(ClientState::Connecting), Some(ClientState::Disconnected)) => {
                format!("Couldn't reach the server{why}")
            }
            (Some(ClientState::Connected), Some(ClientState::Disconnected)) => {
                format!("Lost the connection to the server{why}")
            }
            _ => continue,
        };
        error!("{text}");
        left.write(LeftServer(text.clone()));
        output.write(CommandOutput { text, error: true });
    }
}

/// Says why the server refused this client, as it disconnects it.
pub(super) fn refused(
    _: On<ProtocolMismatch>,
    mut output: MessageWriter<CommandOutput>,
    mut left: MessageWriter<LeftServer>,
) {
    let text = format!(
        "The server plays a different version of rustcraft from this one, {} (protocol {})",
        env!("CARGO_PKG_VERSION"),
        PROTOCOL_VERSION
    );
    error!("{text}");
    left.write(LeftServer(text.clone()));
    output.write(CommandOutput { text, error: true });
}

// Lets go of the server joined as the world's left: removing the client is what disconnects it.
pub(super) fn leave_server(mut commands: Commands) {
    commands.remove_resource::<NetcodeClientTransport>();
    commands.remove_resource::<RenetClient>();
}

/// Puts the vitals the server says this client's player has on them, and lies them down or
/// gets them up as it says.
pub(super) fn follow_vitals(
    mut changes: MessageReader<VitalsChanged>,
    mut player: Query<(Entity, &Transform, &mut Vitals), With<LocalPlayer>>,
    mut commands: Commands,
) {
    let (Some(&VitalsChanged { vitals, asleep }), Ok((entity, body, mut held))) =
        (changes.read().last(), player.single_mut())
    else {
        return;
    };
    *held = vitals;
    if asleep {
        commands.entity(entity).insert(Asleep {
            at: body.translation,
        });
    } else {
        commands.entity(entity).remove::<Asleep>();
    }
}
