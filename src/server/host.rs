use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use bevy_replicon::prelude::RepliconChannels;
use bevy_replicon_renet2::netcode::{
    NativeSocket, NetcodeError, NetcodeServerTransport, ServerAuthentication, ServerSetupConfig,
    ServerSocket,
};
use bevy_replicon_renet2::renet2::{ConnectionConfig, RenetServer};
use bevy_replicon_renet2::RenetChannelsExt;

use crate::protocol::NETCODE_PROTOCOL_ID;

// Friends playing together, not a public server.
const MAX_PLAYERS: usize = 16;

/// Opens the world to players joining over UDP on `port`, from any network this machine is on,
/// as well as the host's own player. Returns the address to give friends on the local network.
pub fn host(
    commands: &mut Commands,
    channels: &RepliconChannels,
    port: u16,
) -> Result<SocketAddr, NetcodeError> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port))?;
    let (server, transport) = serve(channels, NativeSocket::new(socket)?)?;
    commands.insert_resource(server);
    commands.insert_resource(transport);
    Ok(SocketAddr::new(local_address(), port))
}

/// A server taking players on `socket`, whose resources start it once inserted.
///
/// Netcode's unsecure mode: players aren't authenticated and what's sent isn't encrypted,
/// which is fine among friends until there's a way to hand out connect tokens.
pub fn serve(
    channels: &RepliconChannels,
    socket: impl ServerSocket,
) -> Result<(RenetServer, NetcodeServerTransport), NetcodeError> {
    let server = RenetServer::new(ConnectionConfig::from_channels(
        channels.server_configs(),
        channels.client_configs(),
    ));
    let config = ServerSetupConfig {
        current_time: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default(),
        max_token_lifetime: 5,
        max_clients: MAX_PLAYERS,
        protocol_id: NETCODE_PROTOCOL_ID,
        socket_addresses: vec![vec![socket.addr()?]],
        authentication: ServerAuthentication::Unsecure,
    };
    Ok((server, NetcodeServerTransport::new(config, socket)?))
}

// This machine's address on the network it reaches others by, found by asking which it would
// send from (which sends nothing), or this machine's own if it's on none.
fn local_address() -> IpAddr {
    UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
        .and_then(|socket| {
            socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9))?;
            socket.local_addr()
        })
        .map_or(Ipv4Addr::LOCALHOST.into(), |address| address.ip())
}
