//! A server and a client joining it, as two Apps talking over renet2's in-memory transport, which
//! is netcode and replicon as over UDP, with the sockets swapped for channels.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use bevy::ecs::message::{MessageCursor, Messages};
use bevy::math::I64Vec3;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimeUpdateStrategy;
use bevy_replicon::prelude::*;
use bevy_replicon_renet2::netcode::{
    in_memory_server_addr, new_memory_sockets, MemorySocketClient,
};
use bevy_replicon_renet2::renet2::RenetClient;
use bevy_replicon_renet2::RepliconRenetPlugins;
use rustcraft::block::BlockType;
use rustcraft::breaking::break_time;
use rustcraft::chunks::chunk::ChunkCoordinate;
use rustcraft::chunks::chunk_loader::ChunkLoader;
use rustcraft::chunks::generate::checksum::world_checksum;
use rustcraft::chunks::generate::generator::generate_chunk;
use rustcraft::chunks::generate::WORLD_HEIGHT;
use rustcraft::client::join::{connect, GenerationChecksum};
use rustcraft::client::others::SeenPlayer;
use rustcraft::client::player::LocalPlayer;
use rustcraft::client::ClientPlugin;
use rustcraft::containers::{Containers, FUEL, INPUT, OUTPUT};
use rustcraft::game_mode::GameMode;
use rustcraft::inventory::Inventory;
use rustcraft::items::{Item, ItemStack};
use rustcraft::player::name::PlayerName;
use rustcraft::player::{Movement, PlayerLook, PlayerOwner};
use rustcraft::protocol::{
    BlockChanged, ChunkEdits, CommandOutput, ContainerChanged, EditRejected, PlayerMoved,
    PlayerPose, ProtocolPlugin, RequestEdit, Respawn, RunCommand, StartBreaking, Teleport,
    UseBlock,
};
use rustcraft::save::SavePlugin;
use rustcraft::server::drops::DroppedItem;
use rustcraft::server::{serve, ServerPlugin, WorldToOpen};
use rustcraft::settings::Settings;
use rustcraft::survival::Vitals;
use rustcraft::world::World;

// A frame's worth, so replicon's fixed-rate server ticks, which send what's written, keep up.
const FRAME: Duration = Duration::from_millis(20);
// Far beyond the reach of a client drawing as far as the default settings, 36 chunks.
const FAR: I64Vec3 = I64Vec3::new(100 * 16, 0, 0);

// A world hosted with seed 3 in `dir`, its host playing in creative, so its edits are made at
// once, and a client to join it on the other end of an in-memory socket. The client is given
// `version` too, as a game of another version would have.
fn server_and_client(dir: &std::path::Path, version: Option<&str>) -> (App, App) {
    server_and_client_with(dir, version, GenerationChecksum::default())
}

// The same, with a client that works out what seeds generate with `checksum`.
fn server_and_client_with(
    dir: &std::path::Path,
    version: Option<&str>,
    checksum: GenerationChecksum,
) -> (App, App) {
    let (server, mut sockets) = host(dir, 1, false);
    let client = joiner(&server, sockets.pop().unwrap(), "guest", version, checksum);
    (server, client)
}

// A world hosted with seed 3 in `dir` by a player called "host" in creative, saved if `saving`,
// with an in-memory socket for each of `clients` to join it on.
fn host(dir: &std::path::Path, clients: u16, saving: bool) -> (App, Vec<MemorySocketClient>) {
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
            name: PlayerName::new("host"),
        })
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    if saving {
        server.add_plugins(SavePlugin);
    }
    server.finish();
    server.update();
    let host = player_of(&mut server, ClientId::Server);
    server
        .world_mut()
        .entity_mut(host)
        .insert(GameMode::Creative);
    let (socket, clients) = new_memory_sockets((1..=clients).collect(), true, true);
    let channels = server.world().resource::<RepliconChannels>();
    let (renet, transport) = serve(channels, socket).unwrap();
    server.insert_resource(renet).insert_resource(transport);
    (server, clients)
}

// A client joining `server` on `socket` as `name`, of `version` too if it's given, working out
// what seeds generate with `checksum`.
fn joiner(
    server: &App,
    socket: MemorySocketClient,
    name: &str,
    version: Option<&str>,
    checksum: GenerationChecksum,
) -> App {
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
    let channels = server.world().resource::<RepliconChannels>();
    let name = PlayerName::new(name);
    let (renet, transport) = connect(channels, socket, in_memory_server_addr(), &name).unwrap();
    client.insert_resource(renet).insert_resource(transport);
    client
}

// Runs them all until `done`, giving the server, the first, time to generate chunks.
fn update_all_until(apps: &mut [App], mut done: impl FnMut(&mut [App]) -> bool) {
    let start = Instant::now();
    while !done(apps) {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "they never got there"
        );
        std::thread::sleep(Duration::from_millis(1));
        for app in apps.iter_mut().rev() {
            app.update();
        }
    }
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

// Runs both until the client's been sent a message `M` that's `wanted`, and returns it.
fn update_until_sent<M: Message + Clone>(
    server: &mut App,
    client: &mut App,
    cursor: &mut MessageCursor<M>,
    wanted: impl Fn(&M) -> bool,
) -> M {
    let mut found = None;
    update_until(server, client, |_, client| {
        found = found
            .take()
            .or_else(|| received(client, cursor).into_iter().find(&wanted));
        found.is_some()
    });
    found.unwrap()
}

// The messages `M` sent to the client since `cursor` last looked, to call every frame.
fn received<M: Message + Clone>(client: &App, cursor: &mut MessageCursor<M>) -> Vec<M> {
    let messages = client.world().resource::<Messages<M>>();
    cursor.read(messages).cloned().collect()
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

// The player of the one client joined, on the server.
fn guests_player(server: &mut App) -> Entity {
    let guest = owners(server)
        .into_iter()
        .find(|&owner| owner != ClientId::Server);
    player_of(server, guest.expect("no client has joined"))
}

// Moves `player` to `to` on the server, as the host moves their own.
fn move_player(app: &mut App, player: Entity, to: Vec3) {
    app.world_mut()
        .get_mut::<Transform>(player)
        .unwrap()
        .translation = to;
}

// Sends a client's player to `to` with `/tp`, as it can't go far in one go by itself.
fn teleport(client: &mut App, to: Vec3) {
    let line = format!("/tp {} {} {}", to.x, to.y, to.z);
    client.world_mut().write_message(RunCommand(line));
}

fn local_player(client: &mut App) -> Entity {
    let mut local = client
        .world_mut()
        .query_filtered::<Entity, With<LocalPlayer>>();
    local.single(client.world()).unwrap()
}

// Something other than what's at `block`, to change it to.
fn other_than(world: &World, block: I64Vec3) -> BlockType {
    match world.get_block_at(block) {
        BlockType::Air => BlockType::Bricks,
        _ => BlockType::Air,
    }
}

// Has the chunk holding `block` generated in the app's world, as the server or a client's
// renderer would, and returns what's at `block`.
fn generate_at(app: &mut App, block: I64Vec3) -> BlockType {
    let mut world = app.world_mut().resource_mut::<World>();
    let coord = ChunkCoordinate::containing(block);
    let data = generate_chunk(world.region_store.clone(), coord);
    world.insert_chunk(coord, data);
    world.get_block_at(block)
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
    // building from nothing
    let guests = player_of(&mut server, guest);
    server
        .world_mut()
        .entity_mut(guests)
        .insert(GameMode::Creative);
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
    let made = update_until_sent(&mut server, &mut client, &mut changes, |change| {
        change.block == below
    });
    assert_eq!(Some((guest, 1)), made.by);
    let world = server.world().resource::<World>();
    assert_eq!(BlockType::Bricks, world.get_block_at(below));
    assert!(client_has_edit(&mut client, below, BlockType::Bricks));

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

    // and the item a block it breaks in survival drops, which the server replicates to it:
    // planks, which a hand breaks in 3 seconds and gets
    server
        .world_mut()
        .entity_mut(guests_player)
        .insert(GameMode::Survival);
    server
        .world_mut()
        .resource_mut::<World>()
        .edit(below, BlockType::Planks);
    client
        .world_mut()
        .write_message(StartBreaking { block: below });
    let takes = break_time(BlockType::Planks, None);
    for _ in 0..(takes / FRAME.as_secs_f32()) as usize + 10 {
        client.update();
        server.update();
    }
    client.world_mut().write_message(RequestEdit {
        block: below,
        to: BlockType::Air,
        slot: 0,
        seq: 2,
    });
    update_until(&mut server, &mut client, |_, client| {
        let mut drops = client.world_mut().query::<&DroppedItem>();
        let dropped = drops.iter(client.world()).map(|item| item.stack.item);
        dropped.eq([Item::Block(BlockType::Planks)])
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
fn test_two_players_at_one_furnace_both_see_it_smelt() {
    let dir = temp_dir("furnace");
    let (mut server, mut client) = server_and_client(&dir, None);
    update_until(&mut server, &mut client, |_, client| joined(client));
    // beside them both, at the spawn
    let spawn = server.world().resource::<World>().spawn();
    let furnace = spawn.floor().as_i64vec3() + I64Vec3::X * 2;
    update_until(&mut server, &mut client, |server, _| {
        let world = server.world().resource::<World>();
        world.is_chunk_generated(ChunkCoordinate::containing(furnace))
    });
    server
        .world_mut()
        .resource_mut::<World>()
        .edit(furnace, BlockType::Furnace);
    server
        .world_mut()
        .write_message(UseBlock { block: furnace });
    client
        .world_mut()
        .write_message(UseBlock { block: furnace });
    let mut hosts = MessageCursor::<ContainerChanged>::default();
    let mut guests = MessageCursor::<ContainerChanged>::default();
    update_until_sent(&mut server, &mut client, &mut guests, |changed| {
        changed.at == furnace && changed.container.is_some()
    });

    // given ore and coal by someone, which both see smelt into an ingot
    let mut containers = server.world_mut().resource_mut::<Containers>();
    let open = containers.get_mut(furnace).unwrap();
    open.slots[INPUT] = Some(ItemStack::new(Item::Block(BlockType::CopperOre), 2));
    open.slots[FUEL] = Some(ItemStack::new(Item::Coal, 1));
    let ingot = Some(ItemStack::new(Item::CopperIngot, 1));
    let (mut host_saw, mut guest_saw) = (false, false);
    update_until(&mut server, &mut client, |server, client| {
        let smelted =
            |changed: ContainerChanged| changed.container.is_some_and(|c| c.slots[OUTPUT] == ingot);
        host_saw |= received(server, &mut hosts).into_iter().any(smelted);
        guest_saw |= received(client, &mut guests).into_iter().any(smelted);
        host_saw && guest_saw
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

#[test]
fn test_a_client_is_sent_the_edits_near_it_as_it_joins_and_moves() {
    let dir = temp_dir("edits-near");
    let (mut server, mut client) = server_and_client(&dir, None);
    // made before it joins, under its feet and far off
    let mut world = server.world_mut().resource_mut::<World>();
    let near = world.spawn().floor().as_i64vec3() - I64Vec3::Y * 30;
    let far = near + FAR;
    for block in [near, far] {
        world.edit(block, BlockType::Bricks);
    }
    update_until(&mut server, &mut client, |_, client| {
        client_has_edit(client, near, BlockType::Bricks)
    });
    // and shown by the chunk as it's generated
    assert_eq!(BlockType::Bricks, generate_at(&mut client, near));
    for _ in 0..20 {
        client.update();
        server.update();
    }
    assert!(!client_has_edit(&mut client, far, BlockType::Bricks));

    teleport(&mut client, far.as_vec3());
    update_until(&mut server, &mut client, |_, client| {
        client_has_edit(client, far, BlockType::Bricks)
    });
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_a_change_far_from_a_client_reaches_it_only_as_it_comes_near() {
    let dir = temp_dir("edits-far");
    let (mut server, mut client) = server_and_client(&dir, None);
    update_until(&mut server, &mut client, |_, client| joined(client));
    // the host goes far off and changes a block there
    let host_at = server.world().resource::<World>().spawn() + FAR.as_vec3();
    let host = player_of(&mut server, ClientId::Server);
    move_player(&mut server, host, host_at);
    let block = host_at.floor().as_i64vec3() + I64Vec3::new(2, 0, 0);
    update_until(&mut server, &mut client, |server, _| {
        let world = server.world().resource::<World>();
        world.is_chunk_generated(ChunkCoordinate::containing(block))
    });
    let to = other_than(server.world().resource::<World>(), block);
    server.world_mut().write_message(RequestEdit {
        block,
        to,
        slot: 0,
        seq: 1,
    });
    let mut changes = MessageCursor::<BlockChanged>::default();
    let mut told = Vec::new();
    update_until(&mut server, &mut client, |server, client| {
        told.extend(received(client, &mut changes));
        server.world().resource::<World>().get_block_at(block) == to
    });
    for _ in 0..20 {
        client.update();
        server.update();
        told.extend(received(&client, &mut changes));
    }
    assert!(told.is_empty(), "{told:?}");
    assert!(!client_has_edit(&mut client, block, to));

    teleport(&mut client, host_at);
    update_until(&mut server, &mut client, |_, client| {
        client_has_edit(client, block, to)
    });
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_a_client_sees_its_edit_at_once_and_undone_if_its_refused() {
    let dir = temp_dir("prediction");
    let (mut server, mut client) = server_and_client(&dir, None);
    update_until(&mut server, &mut client, |_, client| joined(client));
    // where its player stands, which the server won't build into
    let spawn = client.world().resource::<World>().spawn();
    let feet = spawn.floor().as_i64vec3();
    let was = generate_at(&mut client, feet);
    client.world_mut().write_message(RequestEdit {
        block: feet,
        to: BlockType::Stone,
        slot: 0,
        seq: 1,
    });
    client.update();
    let shown = |client: &App| client.world().resource::<World>().get_block_at(feet);
    assert_eq!(BlockType::Stone, shown(&client));

    let mut rejections = MessageCursor::<EditRejected>::default();
    update_until_sent(&mut server, &mut client, &mut rejections, |rejection| {
        rejection.seq == 1
    });
    assert_eq!(was, shown(&client));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_a_survival_break_is_seen_at_once_and_undone_if_its_sooner_than_it_takes() {
    let dir = temp_dir("breaking");
    let (mut server, mut client) = server_and_client(&dir, None);
    update_until(&mut server, &mut client, |_, client| joined(client));
    let player = local_player(&mut client);
    assert_eq!(
        GameMode::Survival,
        *client.world().get::<GameMode>(player).unwrap()
    );
    // the ground under the joiner's spawn, which is over it, once the server has it
    let spawn = client
        .world()
        .resource::<World>()
        .spawn()
        .floor()
        .as_i64vec3();
    let column = |down| spawn - I64Vec3::Y * down;
    update_until(&mut server, &mut client, |server, _| {
        let world = server.world().resource::<World>();
        (0..40).all(|down| world.is_chunk_generated(ChunkCoordinate::containing(column(down))))
    });
    let world = server.world().resource::<World>();
    let ground = (1..40)
        .map(column)
        .find(|&block| world.get_block_at(block).is_solid())
        .expect("there's no ground under the spawn");
    // the joiner stands on it, to break it by hand
    let feet = (ground + I64Vec3::Y).as_vec3() + Vec3::new(0.5, 0.0, 0.5);
    teleport(&mut client, feet);
    let guest = owners(&mut server)
        .into_iter()
        .find(|&owner| owner != ClientId::Server)
        .unwrap();
    let guests = player_of(&mut server, guest);
    update_until(&mut server, &mut client, |server, _| {
        server.world().get::<Transform>(guests).unwrap().translation == feet
    });
    let was = generate_at(&mut client, ground);
    let takes = break_time(was, None);
    assert!(takes > 0.2, "{was:?}");
    let shown = |client: &App| client.world().resource::<World>().get_block_at(ground);
    let break_it = |client: &mut App, seq| {
        client.world_mut().write_message(RequestEdit {
            block: ground,
            to: BlockType::Air,
            slot: 0,
            seq,
        });
        client.update();
    };

    // at once, it's shown gone and then put back
    client
        .world_mut()
        .write_message(StartBreaking { block: ground });
    break_it(&mut client, 1);
    assert_eq!(BlockType::Air, shown(&client));
    let mut rejections = MessageCursor::<EditRejected>::default();
    update_until_sent(&mut server, &mut client, &mut rejections, |r| r.seq == 1);
    assert_eq!(was, shown(&client));

    // held as long as it takes, it's shown gone and stays gone
    client
        .world_mut()
        .write_message(StartBreaking { block: ground });
    for _ in 0..(takes / FRAME.as_secs_f32()) as usize + 10 {
        client.update();
        server.update();
    }
    break_it(&mut client, 2);
    assert_eq!(BlockType::Air, shown(&client));
    let mut changes = MessageCursor::<BlockChanged>::default();
    update_until_sent(&mut server, &mut client, &mut changes, |change| {
        change.by.is_some_and(|(_, seq)| seq == 2)
    });
    assert_eq!(BlockType::Air, shown(&client));
    let on_server = server.world().resource::<World>().get_block_at(ground);
    assert_eq!(BlockType::Air, on_server);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_edits_sent_before_a_client_has_its_world_wait_for_it() {
    let dir = temp_dir("before-world");
    // a client that can't finish checking the world until the test lets it, so the edits
    // come first
    static CHECKED: AtomicBool = AtomicBool::new(false);
    let slow = GenerationChecksum(|seed| {
        while !CHECKED.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(1));
        }
        world_checksum(seed)
    });
    let (mut server, mut client) = server_and_client_with(&dir, None, slow);
    let spawn = server
        .world()
        .resource::<World>()
        .spawn()
        .floor()
        .as_i64vec3();
    let (made, changed) = (spawn + I64Vec3::X * 2, spawn + I64Vec3::Z * 2);
    server
        .world_mut()
        .resource_mut::<World>()
        .edit(made, BlockType::Bricks);
    // once the client's been sent the edits round it, and the server can check the host's
    let mut sent = MessageCursor::<ChunkEdits>::default();
    let mut edits_sent = false;
    update_until(&mut server, &mut client, |server, client| {
        edits_sent |= !received(client, &mut sent).is_empty();
        let world = server.world().resource::<World>();
        edits_sent && world.is_chunk_generated(ChunkCoordinate::containing(changed))
    });
    server.world_mut().write_message(RequestEdit {
        block: changed,
        to: BlockType::Planks,
        slot: 0,
        seq: 1,
    });
    let mut changes = MessageCursor::<BlockChanged>::default();
    update_until_sent(&mut server, &mut client, &mut changes, |change| {
        change.block == changed
    });
    assert!(!joined(&client));
    CHECKED.store(true, Ordering::Release);
    update_until(&mut server, &mut client, |_, client| joined(client));
    assert!(client_has_edit(&mut client, made, BlockType::Bricks));
    assert!(client_has_edit(&mut client, changed, BlockType::Planks));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_joining_among_many_edits() {
    let dir = temp_dir("many-edits");
    let (mut server, mut client) = server_and_client(&dir, None);
    // a build of 50,000 blocks round the spawn, a box 50 by 20 by 50
    let mut world = server.world_mut().resource_mut::<World>();
    let corner = world.spawn().floor().as_i64vec3() - I64Vec3::new(25, 40, 25);
    for x in 0..50 {
        for y in 0..20 {
            for z in 0..50 {
                world.edit(corner + I64Vec3::new(x, y, z), BlockType::Bricks);
            }
        }
    }
    let edited_chunks = world.edits().chunks().count();
    let edits = |client: &mut App| {
        let mut world = client.world_mut().get_resource_mut::<World>()?;
        let edits = world.edits_mut();
        let chunks: Vec<_> = edits.chunks().collect();
        Some(
            chunks
                .iter()
                .map(|&c| edits.in_chunk(c).count())
                .sum::<usize>(),
        )
    };
    // from the first chunk's edits arriving to the last
    let mut sent = MessageCursor::<ChunkEdits>::default();
    let (mut bytes, mut frames, mut chunks) = (0, 0, 0);
    update_until(&mut server, &mut client, |_, client| {
        let arrived = received(client, &mut sent);
        if chunks > 0 || !arrived.is_empty() {
            frames += 1;
        }
        chunks += arrived.len();
        bytes += arrived
            .iter()
            .map(|edits| postcard::to_stdvec(edits).unwrap().len())
            .sum::<usize>();
        chunks == edited_chunks
    });
    println!("50,000 edits in {chunks} chunks: {bytes} bytes over {frames} frames");
    update_until(&mut server, &mut client, |_, client| {
        edits(client) == Some(50_000)
    });
    assert!(bytes < 200_000, "{bytes} bytes");
    assert!(frames > 1 && frames < 30, "{frames} frames");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_a_chunks_edits_and_a_change_to_it_overtaking_each_other_leave_the_newer() {
    let dir = temp_dir("overtaking");
    let (mut server, mut client) = server_and_client(&dir, None);
    let spawn = server
        .world()
        .resource::<World>()
        .spawn()
        .floor()
        .as_i64vec3();
    let block = spawn + I64Vec3::X * 2;
    let (coord, local) = (
        ChunkCoordinate::containing(block),
        ChunkCoordinate::local_block(block),
    );
    server
        .world_mut()
        .resource_mut::<World>()
        .edit(block, BlockType::Bricks);
    update_until(&mut server, &mut client, |server, client| {
        let world = server.world().resource::<World>();
        world.is_chunk_generated(coord) && client_has_edit(client, block, BlockType::Bricks)
    });
    // the host breaks it, the client's first change
    server.world_mut().write_message(RequestEdit {
        block,
        to: BlockType::Air,
        slot: 0,
        seq: 1,
    });
    update_until(&mut server, &mut client, |_, client| {
        client_has_edit(client, block, BlockType::Air)
    });
    let shown = |client: &mut App| {
        generate_at(client, block);
        client.world().resource::<World>().get_block_at(block)
    };
    assert_eq!(BlockType::Air, shown(&mut client));

    // the chunk's edits from before the change, held up by a lost packet, don't undo it
    let before = ChunkEdits::new(coord, 0, [(local, BlockType::Bricks)]);
    client.world_mut().write_message(before);
    client.update();
    assert_eq!(BlockType::Air, shown(&mut client));

    // nor does a second change held up behind the chunk's edits sent after it
    let after = ChunkEdits::new(coord, 2, [(local, BlockType::Stone)]);
    client.world_mut().write_message(after);
    client.update();
    client.world_mut().write_message(BlockChanged {
        block,
        to: BlockType::Bricks,
        by: None,
    });
    client.update();
    assert_eq!(BlockType::Stone, shown(&mut client));
    std::fs::remove_dir_all(&dir).unwrap();
}

// The names of the players `app` has, as the server has them or a client's been shown them.
fn names(app: &mut App) -> Vec<String> {
    let mut names = app.world_mut().query::<&PlayerName>();
    let mut names: Vec<_> = names
        .iter(app.world())
        .map(|name| name.as_str().to_owned())
        .collect();
    names.sort_unstable();
    names
}

// Where the client draws the other players.
fn seen(client: &mut App) -> Vec<(Entity, Vec3)> {
    let mut seen = client.world_mut().query::<(&SeenPlayer, &Transform)>();
    seen.iter(client.world())
        .map(|(seen, body)| (seen.0, body.translation))
        .collect()
}

fn printed(client: &App, cursor: &mut MessageCursor<CommandOutput>) -> Vec<String> {
    let lines = received(client, cursor);
    lines.into_iter().map(|line| line.text).collect()
}

#[test]
fn test_players_get_names_of_their_own_and_see_everyone_else() {
    let dir = temp_dir("seeing");
    let (server, mut sockets) = host(&dir, 2, false);
    let checksum = GenerationChecksum::default();
    let second = joiner(&server, sockets.pop().unwrap(), "guest", None, checksum);
    let first = joiner(&server, sockets.pop().unwrap(), "guest", None, checksum);
    let mut apps = [server, first, second];
    let mut said = MessageCursor::<CommandOutput>::default();
    let mut lines = Vec::new();
    update_all_until(&mut apps, |apps| {
        lines.extend(printed(&apps[1], &mut said));
        apps[1..].iter_mut().all(|client| seen(client).len() == 2)
    });
    let everyone = names(&mut apps[0]);
    assert_eq!(["guest", "guest2", "host"], everyone[..]);
    // each sees the host and the other, but not themselves, whom they draw where they are
    let mut own = Vec::new();
    for client in &mut apps[1..] {
        let shown = names(client);
        assert!(
            shown.len() == 2 && shown.contains(&"host".into()),
            "{shown:?}"
        );
        own.extend(
            everyone
                .iter()
                .filter(|name| !shown.contains(name))
                .cloned(),
        );
        let local = local_player(client);
        assert!(seen(client).iter().all(|&(player, _)| player != local));
    }
    assert_eq!(2, own.len());
    assert_ne!(own[0], own[1]);
    let second = &own[1];
    assert!(lines.contains(&format!("{second} joined the game")));

    // the second leaves, and goes from the first's screen
    apps[2]
        .world_mut()
        .resource_mut::<RenetClient>()
        .disconnect();
    update_all_until(&mut apps, |apps| {
        lines.extend(printed(&apps[1], &mut said));
        seen(&mut apps[1]).len() == 1 && lines.contains(&format!("{second} left the game"))
    });
    assert_eq!(["host"], names(&mut apps[1])[..]);
    std::fs::remove_dir_all(&dir).unwrap();
}

// Where the client draws the player called `name`, once it's been told where they are.
fn seen_at(client: &mut App, name: &str) -> Option<Vec3> {
    let mut seen = client
        .world_mut()
        .query::<(&SeenPlayer, &Transform, &Visibility)>();
    let mut names = client.world_mut().query::<&PlayerName>();
    let world = client.world();
    seen.iter(world)
        .find(|(seen, ..)| names.get(world, seen.0).unwrap().as_str() == name)
        .filter(|(.., shown)| **shown != Visibility::Hidden)
        .map(|(_, body, _)| body.translation)
}

#[test]
fn test_players_are_seen_moving_smoothly_between_their_poses() {
    let dir = temp_dir("poses");
    let (server, mut sockets) = host(&dir, 2, false);
    let checksum = GenerationChecksum::default();
    let watcher = joiner(&server, sockets.pop().unwrap(), "watcher", None, checksum);
    let walker = joiner(&server, sockets.pop().unwrap(), "walker", None, checksum);
    let mut apps = [server, walker, watcher];
    update_all_until(&mut apps, |apps| {
        seen_at(&mut apps[2], "walker").is_some() && seen_at(&mut apps[2], "host").is_some()
    });
    let start = seen_at(&mut apps[2], "walker").unwrap();
    let mut players = apps[2].world_mut().query::<(Entity, &PlayerName)>();
    let walkers = players
        .iter(apps[2].world())
        .find(|(_, name)| name.as_str() == "walker")
        .unwrap()
        .0;
    let host_start = seen_at(&mut apps[2], "host").unwrap();
    let mut poses = MessageCursor::<PlayerMoved>::default();
    let (mut sent, mut shown) = (Vec::new(), Vec::new());
    // both walking 2 blocks a second, the walker along x and the host along z
    let step = 2.0 * FRAME.as_secs_f32();
    update_all_until(&mut apps, |apps| {
        let walker = local_player(&mut apps[1]);
        let at = apps[1]
            .world()
            .get::<Transform>(walker)
            .unwrap()
            .translation;
        move_player(&mut apps[1], walker, at + Vec3::X * step);
        let host = player_of(&mut apps[0], ClientId::Server);
        let at = apps[0].world().get::<Transform>(host).unwrap().translation;
        move_player(&mut apps[0], host, at + Vec3::Z * step);
        let arrived = received(&apps[2], &mut poses);
        let walkers_poses = arrived.iter().filter(|moved| moved.player == walkers);
        sent.extend(walkers_poses.map(|moved| moved.position.x));
        shown.push(seen_at(&mut apps[2], "walker").unwrap().x);
        shown.last().unwrap() - start.x > 2.0
            && seen_at(&mut apps[2], "host").unwrap().z - host_start.z > 2.0
    });
    // drawn behind the poses, and in between them, not jumping from one to the next
    let between = shown.iter().filter(|&&x| {
        let (before, after) = (
            sent.iter().rfind(|&&s| s < x),
            sent.iter().find(|&&s| s > x),
        );
        before.is_some() && after.is_some() && !sent.iter().any(|&s| (s - x).abs() < 1e-4)
    });
    assert!(between.count() > shown.len() / 2, "{shown:?} from {sent:?}");
    assert!(shown.windows(2).all(|pair| pair[1] >= pair[0]), "{shown:?}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_a_pose_too_far_or_into_the_ground_is_refused_and_the_client_put_back() {
    let dir = temp_dir("too-fast");
    let (mut server, mut client) = server_and_client(&dir, None);
    update_until(&mut server, &mut client, |_, client| joined(client));
    let spawn = server.world().resource::<World>().spawn();
    let guest = guests_player(&mut server);
    let pose = |position, teleports| PlayerPose {
        at: 0,
        position,
        yaw: 0.0,
        pitch: 0.0,
        movement: Movement::Walk,
        slot: 0,
        teleports,
    };
    let mut teleports = MessageCursor::<Teleport>::default();
    // a hundred blocks in a moment, walking
    client
        .world_mut()
        .write_message(pose(spawn + Vec3::X * 100.0, 0));
    let put = update_until_sent(&mut server, &mut client, &mut teleports, |_| true);
    assert_eq!(spawn, put.position);
    let on_server = |server: &App| server.world().get::<Transform>(guest).unwrap().translation;
    assert_eq!(spawn, on_server(&server));
    let me = local_player(&mut client);
    assert_eq!(
        spawn,
        client.world().get::<Transform>(me).unwrap().translation
    );

    // into the ground under it, once the server has it
    let below = |down| spawn.floor().as_i64vec3() - I64Vec3::Y * down;
    update_until(&mut server, &mut client, |server, _| {
        let world = server.world().resource::<World>();
        (1..40).all(|down| world.is_chunk_generated(ChunkCoordinate::containing(below(down))))
    });
    let world = server.world().resource::<World>();
    let ground = (1..40)
        .map(below)
        .find(|&block| world.get_block_at(block).is_solid())
        .unwrap();
    let in_ground = ground.as_vec3() + Vec3::new(0.5, 0.0, 0.5);
    client.world_mut().write_message(pose(in_ground, 1));
    update_until_sent(&mut server, &mut client, &mut teleports, |_| true);
    assert_eq!(spawn, on_server(&server));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_tp_and_gamemode_move_and_change_a_joiner_on_its_own_screen() {
    let dir = temp_dir("tp");
    let (mut server, mut client) = server_and_client(&dir, None);
    update_until(&mut server, &mut client, |_, client| joined(client));
    let me = local_player(&mut client);
    let to = Vec3::new(10.5, 500.0, -20.0);
    teleport(&mut client, to);
    update_until(&mut server, &mut client, |_, client| {
        client.world().get::<Transform>(me).unwrap().translation == to
    });
    // and it stays there, its poses from there taken
    let guest = guests_player(&mut server);
    for _ in 0..20 {
        client.update();
        server.update();
    }
    assert_eq!(
        to,
        server.world().get::<Transform>(guest).unwrap().translation
    );

    let mode = |client: &App| *client.world().get::<GameMode>(me).unwrap();
    assert_eq!(GameMode::Survival, mode(&client));
    client
        .world_mut()
        .write_message(RunCommand("/gamemode creative".into()));
    update_until(&mut server, &mut client, |_, client| {
        mode(client) == GameMode::Creative
    });
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_a_joiner_comes_back_where_they_left_with_what_they_had() {
    let dir = temp_dir("own-saves");
    let place = Vec3::new(40.5, 480.0, -20.25);
    let looking = PlayerLook {
        yaw: 1.0,
        pitch: -0.3,
    };
    let gift = ItemStack::new(Item::Block(BlockType::Bricks), 5);
    {
        let (server, mut sockets) = host(&dir, 1, true);
        let client = joiner(
            &server,
            sockets.pop().unwrap(),
            "guest",
            None,
            Default::default(),
        );
        let (mut server, mut client) = (server, client);
        update_until(&mut server, &mut client, |_, client| joined(client));
        teleport(&mut client, place);
        let me = local_player(&mut client);
        client.world_mut().entity_mut(me).insert(looking);
        let guest = guests_player(&mut server);
        let mut inventory = server.world_mut().get_mut::<Inventory>(guest).unwrap();
        assert_eq!(None, inventory.add(gift));
        // hurt, and too hungry to heal
        let hurt = Vitals {
            health: 9,
            food: 10,
            ..Vitals::default()
        };
        server.world_mut().entity_mut(guest).insert(hurt);
        update_until(&mut server, &mut client, |server, _| {
            let world = server.world();
            world.get::<Transform>(guest).unwrap().translation == place
                && *world.get::<PlayerLook>(guest).unwrap() == looking
        });
        client
            .world_mut()
            .resource_mut::<RenetClient>()
            .disconnect();
        let saved = dir.join("players").join("guest.toml");
        update_until(&mut server, &mut client, |_, _| saved.exists());
    }

    // in a server opened from the save again
    let (server, mut sockets) = host(&dir, 1, true);
    let client = joiner(
        &server,
        sockets.pop().unwrap(),
        "guest",
        None,
        Default::default(),
    );
    let (mut server, mut client) = (server, client);
    update_until(&mut server, &mut client, |_, client| joined(client));
    let me = local_player(&mut client);
    let world = client.world();
    assert_eq!(place, world.get::<Transform>(me).unwrap().translation);
    assert_eq!(looking, *world.get::<PlayerLook>(me).unwrap());
    assert!(world
        .get::<Inventory>(me)
        .unwrap()
        .slots
        .contains(&Some(gift)));
    assert_eq!(9, world.get::<Vitals>(me).unwrap().health);
    assert_eq!(10, world.get::<Vitals>(me).unwrap().food);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_a_client_is_told_its_players_health_and_respawns_where_the_server_puts_it() {
    let dir = temp_dir("respawning");
    let (mut server, mut client) = server_and_client(&dir, None);
    update_until(&mut server, &mut client, |_, client| joined(client));
    let guest = owners(&mut server)
        .into_iter()
        .find(|&owner| owner != ClientId::Server)
        .unwrap();
    let guests = player_of(&mut server, guest);
    let local = local_player(&mut client);
    let vitals = |client: &App| *client.world().get::<Vitals>(local).unwrap();

    // dying, as far as the client's concerned, once the server says so
    server.world_mut().get_mut::<Vitals>(guests).unwrap().health = 0;
    update_until(&mut server, &mut client, |_, client| {
        vitals(client).is_dead()
    });
    // having moved a little, respawning at the world's spawn, as it's told
    let at = client.world().get::<Transform>(local).unwrap().translation;
    move_player(&mut client, local, at + Vec3::X);
    client.world_mut().write_message(Respawn);
    update_until(&mut server, &mut client, |_, client| {
        !vitals(client).is_dead()
    });
    assert_eq!(Vitals::default(), vitals(&client));
    let spawn = server.world().resource::<World>().spawn();
    update_until(&mut server, &mut client, |_, client| {
        client.world().get::<Transform>(local).unwrap().translation == spawn
    });
    let _ = std::fs::remove_dir_all(&dir);
}
