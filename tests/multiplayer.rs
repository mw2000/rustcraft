//! A server and a client joining it, as two Apps talking over renet2's in-memory transport, which
//! is netcode and replicon as over UDP, with the sockets swapped for channels.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::ecs::message::{MessageCursor, Messages};
use bevy::math::I64Vec3;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimeUpdateStrategy;
use bevy_replicon::prelude::*;
use bevy_replicon_renet2::netcode::{in_memory_server_addr, new_memory_sockets};
use bevy_replicon_renet2::renet2::RenetClient;
use bevy_replicon_renet2::RepliconRenetPlugins;
use rustcraft::block::BlockType;
use rustcraft::chunks::chunk::ChunkCoordinate;
use rustcraft::chunks::chunk_loader::ChunkLoader;
use rustcraft::chunks::generate::checksum::world_checksum;
use rustcraft::chunks::generate::WORLD_HEIGHT;
use rustcraft::client::join::{connect, GenerationChecksum};
use rustcraft::client::player::LocalPlayer;
use rustcraft::client::ClientPlugin;
use rustcraft::game_mode::GameMode;
use rustcraft::inventory::Inventory;
use rustcraft::items::{Item, ItemStack};
use rustcraft::player::PlayerOwner;
use rustcraft::protocol::{BlockChanged, CommandOutput, ProtocolPlugin, RequestEdit};
use rustcraft::server::drops::DroppedItem;
use rustcraft::server::{serve, ServerPlugin, WorldToOpen};
use rustcraft::settings::Settings;
use rustcraft::world::World;

// A frame's worth, so replicon's fixed-rate server ticks, which send what's written, keep up.
const FRAME: Duration = Duration::from_millis(20);

// A world hosted with seed 3 in `dir`, its host playing, and a client to join it on the other
// end of an in-memory socket. The client is given `version` too, as a game of another version
// would have.
fn server_and_client(dir: &std::path::Path, version: Option<&str>) -> (App, App) {
    server_and_client_with(dir, version, GenerationChecksum::default())
}

// The same, with a client that works out what seeds generate with `checksum`.
fn server_and_client_with(
    dir: &std::path::Path,
    version: Option<&str>,
    checksum: GenerationChecksum,
) -> (App, App) {
    let mut server = App::new();
    server
        .add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins,
            RepliconRenetPlugins,
            ProtocolPlugin,
            ServerPlugin,
        ))
        .insert_resource(WorldToOpen {
            dir: dir.to_owned(),
            seed: Some(3),
            time_of_day: None,
            region_distance: 16,
        })
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    server.finish();
    server.update();

    let mut client = App::new();
    client
        .add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins,
            RepliconRenetPlugins,
            ProtocolPlugin,
            ClientPlugin,
        ))
        .insert_resource(Settings::default())
        .insert_resource(ChunkLoader::new(
            Settings::default().renderer.lod_layout(),
            WORLD_HEIGHT,
            false,
            Handle::default(),
            Handle::default(),
        ))
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .insert_resource(checksum);
    if let Some(version) = version {
        client
            .world_mut()
            .resource_mut::<ProtocolHasher>()
            .add_custom(version);
    }
    client.finish();

    let (server_socket, mut client_sockets) = new_memory_sockets(vec![1], true, true);
    let channels = server.world().resource::<RepliconChannels>();
    let (renet, transport) = serve(channels, server_socket).unwrap();
    server.insert_resource(renet).insert_resource(transport);
    let channels = client.world().resource::<RepliconChannels>();
    let (renet, transport) = connect(
        channels,
        client_sockets.pop().unwrap(),
        in_memory_server_addr(),
    )
    .unwrap();
    client.insert_resource(renet).insert_resource(transport);
    (server, client)
}

// Runs both until `done`, giving the server time to generate chunks.
fn update_until(
    server: &mut App,
    client: &mut App,
    mut done: impl FnMut(&mut App, &mut App) -> bool,
) {
    let start = Instant::now();
    while !done(server, client) {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "they never got there"
        );
        std::thread::sleep(Duration::from_millis(1));
        client.update();
        server.update();
    }
}

// A directory for a world, emptied.
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustcraft-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn owners(app: &mut App) -> Vec<ClientId> {
    let mut owners = app.world_mut().query::<&PlayerOwner>();
    owners.iter(app.world()).map(|owner| owner.0).collect()
}

fn player_of(app: &mut App, client: ClientId) -> Entity {
    let mut players = app.world_mut().query::<(Entity, &PlayerOwner)>();
    let mut players = players.iter(app.world());
    players.find(|(_, owner)| owner.0 == client).unwrap().0
}

fn joined(client: &App) -> bool {
    client.world().contains_resource::<World>()
}

fn client_has_edit(client: &mut App, block: I64Vec3, to: BlockType) -> bool {
    let coord = ChunkCoordinate::containing(block);
    let local = ChunkCoordinate::local_block(block);
    client
        .world_mut()
        .get_resource_mut::<World>()
        .is_some_and(|mut world| {
            world
                .edits_mut()
                .in_chunk(coord)
                .any(|edit| edit == (local, to))
        })
}

#[test]
fn test_a_client_joins_plays_and_leaves() {
    let dir = temp_dir("joining");
    let (mut server, mut client) = server_and_client(&dir, None);
    let mut changes = MessageCursor::<BlockChanged>::default();
    update_until(&mut server, &mut client, |_, client| joined(client));

    // welcomed to the server's world, with its own player where the server put it
    let seed = server.world().resource::<World>().seed();
    assert_eq!(seed, client.world().resource::<World>().seed());
    assert_eq!(
        ClientState::Connected,
        **client.world().resource::<State<ClientState>>()
    );
    let mut authorized = server.world_mut().query::<&AuthorizedClient>();
    assert_eq!(1, authorized.iter(server.world()).count());
    let guest = owners(&mut server)
        .into_iter()
        .find(|&owner| owner != ClientId::Server)
        .expect("the server spawned no player for the client");
    let spawn = server.world().resource::<World>().spawn();
    let mut local = client
        .world_mut()
        .query_filtered::<&Transform, With<LocalPlayer>>();
    assert_eq!(spawn, local.single(client.world()).unwrap().translation);

    // the client's edit, checked and made by the server, and sent back to it
    let below = spawn.floor().as_i64vec3() - I64Vec3::Y;
    update_until(&mut server, &mut client, |server, _| {
        let world = server.world().resource::<World>();
        world.is_chunk_generated(ChunkCoordinate::containing(below))
    });
    client.world_mut().write_message(RequestEdit {
        block: below,
        to: BlockType::Bricks,
        slot: 0,
        seq: 1,
    });
    update_until(&mut server, &mut client, |_, client| {
        client_has_edit(client, below, BlockType::Bricks)
    });
    let world = server.world().resource::<World>();
    assert_eq!(BlockType::Bricks, world.get_block_at(below));
    let messages = client.world().resource::<Messages<BlockChanged>>();
    let made = changes.read(messages).last().copied();
    assert_eq!(Some(Some((guest, 1))), made.map(|change| change.by));

    // and the host's, which the client never asked for
    let beside = below + I64Vec3::X;
    server.world_mut().write_message(RequestEdit {
        block: beside,
        to: BlockType::Planks,
        slot: 0,
        seq: 1,
    });
    update_until(&mut server, &mut client, |_, client| {
        client_has_edit(client, beside, BlockType::Planks)
    });

    // its inventory, as the server changes it
    let guests_player = player_of(&mut server, guest);
    let gift = ItemStack::new(Item::Block(BlockType::Bricks), 5);
    let mut inventory = server
        .world_mut()
        .get_mut::<Inventory>(guests_player)
        .unwrap();
    assert_eq!(None, inventory.add(gift));
    let expected = inventory.clone();
    update_until(&mut server, &mut client, |_, client| {
        let mut local = client
            .world_mut()
            .query_filtered::<&Inventory, With<LocalPlayer>>();
        *local.single(client.world()).unwrap() == expected
    });

    // and the item a block it breaks in survival drops, which the server replicates to it
    server
        .world_mut()
        .entity_mut(guests_player)
        .insert(GameMode::Survival);
    client.world_mut().write_message(RequestEdit {
        block: below,
        to: BlockType::Air,
        slot: 0,
        seq: 2,
    });
    update_until(&mut server, &mut client, |_, client| {
        let mut drops = client.world_mut().query::<&DroppedItem>();
        let dropped = drops.iter(client.world()).map(|item| item.stack.item);
        dropped.eq([Item::Block(BlockType::Bricks)])
    });

    // leaving takes its player with it
    client
        .world_mut()
        .resource_mut::<RenetClient>()
        .disconnect();
    update_until(&mut server, &mut client, |server, _| {
        owners(server) == [ClientId::Server]
    });
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_a_client_of_another_version_is_refused() {
    let dir = temp_dir("refused");
    let (mut server, mut client) = server_and_client(&dir, Some("0.0.0"));
    let mut output = MessageCursor::<CommandOutput>::default();
    let mut said = Vec::new();
    update_until(&mut server, &mut client, |_, client| {
        let messages = client.world().resource::<Messages<CommandOutput>>();
        said.extend(output.read(messages).map(|line| line.text.clone()));
        said.iter().any(|text| text.contains("different version"))
    });
    assert!(!joined(&client));
    assert_eq!(vec![ClientId::Server], owners(&mut server));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_a_client_generating_a_different_world_is_refused() {
    let dir = temp_dir("different-world");
    // as a game on a machine whose floating point differs might
    let checksum = GenerationChecksum(|seed| world_checksum(seed) ^ 1);
    let (mut server, mut client) = server_and_client_with(&dir, None, checksum);
    let mut output = MessageCursor::<CommandOutput>::default();
    let mut said = Vec::new();
    update_until(&mut server, &mut client, |_, client| {
        let messages = client.world().resource::<Messages<CommandOutput>>();
        said.extend(output.read(messages).map(|line| line.text.clone()));
        said.iter()
            .any(|text| text.contains("generates a different world"))
    });
    // and leaves, with no world to play on
    update_until(&mut server, &mut client, |server, _| {
        owners(server) == [ClientId::Server]
    });
    assert!(!joined(&client));
    std::fs::remove_dir_all(&dir).unwrap();
}
