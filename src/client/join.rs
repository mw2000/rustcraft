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
use crate::chunks::generate::checksum::world_checksum;
use crate::edits::Edits;
use crate::inventory::Inventory;
use crate::player::{arriving, PlayerLook};
use crate::protocol::{
    CommandOutput, EditReach, InventoryChanged, PlayerPose, Welcome, DEFAULT_PORT,
    NETCODE_PROTOCOL_ID, PROTOCOL_VERSION,
};
use crate::settings::Settings;
use crate::world::World;

// About as often as a player's movement needs telling for reaching and keeping chunks near them.
const POSE_INTERVAL: Duration = Duration::from_millis(50);

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

/// Starts joining the server at `server` over UDP, as a client with no world of its own until
/// the server's `Welcome`.
pub fn join(
    commands: &mut Commands,
    channels: &RepliconChannels,
    server: SocketAddr,
) -> Result<(), NetcodeError> {
    let any: std::net::IpAddr = if server.is_ipv4() {
        Ipv4Addr::UNSPECIFIED.into()
    } else {
        Ipv6Addr::UNSPECIFIED.into()
    };
    let socket = NativeSocket::new(UdpSocket::bind((any, 0))?)?;
    let (client, transport) = connect(channels, socket, server)?;
    commands.insert_resource(client);
    commands.insert_resource(transport);
    Ok(())
}

/// A client connecting to `server` on `socket`, whose resources start it once inserted. In
/// netcode's unsecure mode, as the server is (`server::serve`).
pub fn connect(
    channels: &RepliconChannels,
    socket: impl ClientSocket,
    server: SocketAddr,
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
        user_data: None,
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let transport = NetcodeClientTransport::new(now, authentication, socket)?;
    Ok((client, transport))
}

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
pub(super) struct Checking {
    seed: u32,
    server: u64,
    task: Task<u64>,
}

/// Spawns this client's player where the server has them, starts checking the world it's
/// welcomed to, on a task as that takes a fraction of a second (INV-7), and asks for the edits
/// of the chunks it draws at full detail.
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
    reach.write(EditReach(
        settings.renderer.lod_layout().full_detail_reach(),
    ));
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
        output.write(CommandOutput { text, error: true });
        if let Some(mut client) = client {
            client.disconnect();
        }
        return;
    }
    let region_distance = settings.renderer.lod_layout().region_reach() as u32;
    let world = World::from_save(checking.seed, Edits::default(), region_distance);
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

/// Tells the server where this client's player is, every `POSE_INTERVAL`.
pub(super) fn send_pose(
    time: Res<Time>,
    player: Query<(&Transform, &PlayerLook), With<LocalPlayer>>,
    mut next: Local<Duration>,
    mut poses: MessageWriter<PlayerPose>,
) {
    let Ok((body, look)) = player.single() else {
        return;
    };
    if time.elapsed() < *next {
        return;
    }
    *next = time.elapsed() + POSE_INTERVAL;
    poses.write(PlayerPose {
        position: body.translation,
        yaw: look.yaw,
        pitch: look.pitch,
    });
}

/// Says in the command bar, and the log, when joining fails or the server goes.
pub(super) fn report_connection(
    mut transitions: MessageReader<StateTransitionEvent<ClientState>>,
    client: Option<Res<RenetClient>>,
    mut output: MessageWriter<CommandOutput>,
) {
    for transition in transitions.read() {
        let why = client
            .as_ref()
            .and_then(|client| client.disconnect_reason())
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
        output.write(CommandOutput { text, error: true });
    }
}

/// Says why the server refused this client, as it disconnects it.
pub(super) fn refused(_: On<ProtocolMismatch>, mut output: MessageWriter<CommandOutput>) {
    let text = format!(
        "The server plays a different version of rustcraft from this one, {} (protocol {})",
        env!("CARGO_PKG_VERSION"),
        PROTOCOL_VERSION
    );
    error!("{text}");
    output.write(CommandOutput { text, error: true });
}
