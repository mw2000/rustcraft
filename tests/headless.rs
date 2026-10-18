//! The guard that gameplay doesn't depend on rendering: what a dedicated server will run, on
//! `MinimalPlugins` with no window, camera or renderer.

use std::path::Path;
use std::time::{Duration, Instant};

use bevy::ecs::message::Messages;
use bevy::math::I64Vec3;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimeUpdateStrategy;
use bevy_replicon::prelude::*;
use rustcraft::block::BlockType;
use rustcraft::chunks::chunk::{ChunkCoordinate, CHUNK_SIZE};
use rustcraft::clock::TimeOfDay;
use rustcraft::game_mode::{GameMode, GameModePlugin};
use rustcraft::inventory::Inventory;
use rustcraft::items::{Item, ItemStack};
use rustcraft::player::{Movement, Player, PlayerLook, PlayerOwner, EYE_HEIGHT};
use rustcraft::protocol::{BlockChanged, ProtocolPlugin, RequestEdit};
use rustcraft::save::{SavePlugin, WorldSave};
use rustcraft::server::{ServerPlugin, WorldToOpen};
use rustcraft::world::World;

// A server playing the world saved in `dir`, or a new one with seed 3, started.
fn server(dir: &Path) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        StatesPlugin,
        RepliconPlugins,
        ProtocolPlugin,
        ServerPlugin,
        SavePlugin,
        GameModePlugin,
    ))
    .insert_resource(WorldToOpen {
        dir: dir.to_owned(),
        seed: Some(3),
        time_of_day: None,
        region_distance: 16,
    })
    // a second a frame, so the clock visibly moves
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(1)));
    app.finish();
    app.update();
    app
}

// Runs the server until `done`, giving it time to generate chunks.
fn update_until(app: &mut App, done: impl Fn(&World) -> bool) {
    let start = Instant::now();
    while !done(app.world().resource::<World>()) {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "the server never got there"
        );
        std::thread::sleep(Duration::from_millis(5));
        app.update();
    }
}

// A directory for a world, emptied.
fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rustcraft-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

// The one player, the host's.
fn hosts_player(app: &mut App) -> Entity {
    let (player, owner) = app
        .world_mut()
        .query::<(Entity, &PlayerOwner)>()
        .single(app.world())
        .unwrap();
    assert_eq!(ClientId::Server, owner.0);
    player
}

#[test]
fn test_gameplay_runs_and_saves_with_no_rendering() {
    let dir = temp_dir("headless");
    let mut app = server(&dir);
    let player = hosts_player(&mut app);
    let spawn = app.world().resource::<World>().spawn();
    assert_eq!(
        spawn,
        app.world().get::<Transform>(player).unwrap().translation
    );
    let started = app.world().resource::<TimeOfDay>().0;

    // a block under the player's feet, in a chunk the server loads itself, with no client
    let below = spawn.floor().as_i64vec3() - I64Vec3::Y;
    update_until(&mut app, |world| {
        world.is_chunk_generated(ChunkCoordinate::containing(below))
    });
    let look = PlayerLook {
        yaw: 1.0,
        pitch: 0.3,
    };
    app.world_mut().entity_mut(player).insert(look);
    for _ in 0..5 {
        app.update();
    }
    assert!(app.world().resource::<TimeOfDay>().0 > started);
    app.world_mut()
        .entity_mut(player)
        .insert(GameMode::Survival);
    app.update();
    let movement = app.world().get::<Player>(player).unwrap().movement;
    assert_eq!(Movement::Walk, movement);

    // a player's edit, checked and made by the server, and sent on to the clients, taking the
    // block from what they carry
    let bricks = Item::Block(BlockType::Bricks);
    app.world_mut().get_mut::<Inventory>(player).unwrap().slots[0] =
        Some(ItemStack::new(bricks, 2));
    app.world_mut().write_message(RequestEdit {
        block: below,
        to: BlockType::Bricks,
        slot: 0,
        seq: 1,
    });
    app.update();
    assert_eq!(
        BlockType::Bricks,
        app.world().resource::<World>().get_block_at(below)
    );
    assert_eq!(1, app.world().resource::<Messages<BlockChanged>>().len());
    let carried = app.world().get::<Inventory>(player).unwrap().clone();
    assert_eq!(Some(ItemStack::new(bricks, 1)), carried.slots[0]);

    app.world_mut().write_message(AppExit::Success);
    app.update();
    drop(app);
    let (_, saved, saved_edits) = WorldSave::open(dir.clone(), None).unwrap();
    let coord = ChunkCoordinate::containing(below);
    let local = ChunkCoordinate::local_block(below);
    let saved_edits: Vec<_> = saved_edits.in_chunk(coord).collect();
    assert_eq!(vec![(local, BlockType::Bricks)], saved_edits);
    assert!(saved.time_of_day > started);

    // and the next server spawns the player as they were saved
    let mut app = server(&dir);
    let player = hosts_player(&mut app);
    assert_eq!(look, *app.world().get::<PlayerLook>(player).unwrap());
    assert_eq!(
        GameMode::Survival,
        *app.world().get::<GameMode>(player).unwrap()
    );
    assert_eq!(carried, *app.world().get::<Inventory>(player).unwrap());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn test_a_server_alone_keeps_the_chunks_near_its_player_loaded() {
    let dir = temp_dir("residency");
    let mut app = server(&dir);
    let player = hosts_player(&mut app);
    let feet = app.world().get::<Transform>(player).unwrap().translation;
    let eye = ChunkCoordinate::containing((feet + Vec3::Y * EYE_HEIGHT).floor().as_i64vec3());
    let near: Vec<_> = eye.neighbors_26().into_iter().chain([eye]).collect();
    update_until(&mut app, |world| {
        near.iter().all(|&chunk| world.is_chunk_generated(chunk))
    });
    let far = ChunkCoordinate(eye.0 + I64Vec3::new(10, 0, 0));
    assert!(!app.world().resource::<World>().is_chunk_generated(far));

    // a block placed in the air under the player roofs the chunk under it, which with no
    // client to draw it isn't recorded for redrawing
    let below = feet.floor().as_i64vec3() - I64Vec3::Y;
    let under = below - I64Vec3::Y * CHUNK_SIZE as i64;
    assert!(!app.world().resource::<World>().is_under_ground(under));
    app.world_mut().write_message(RequestEdit {
        block: below,
        to: BlockType::Bricks,
        seq: 1,
        slot: 0,
    });
    app.update();
    let mut world = app.world_mut().resource_mut::<World>();
    assert!(world.is_under_ground(under));
    assert!(world.take_reroofed().is_empty());

    // and the chunks it left are let go of as it goes
    let away = feet + Vec3::X * 100.0 * CHUNK_SIZE as f32;
    app.world_mut()
        .get_mut::<Transform>(player)
        .unwrap()
        .translation = away;
    app.update();
    let world = app.world().resource::<World>();
    assert!(near.iter().all(|&chunk| !world.is_chunk_generated(chunk)));
    let there = ChunkCoordinate::containing(away.floor().as_i64vec3());
    update_until(&mut app, |world| world.is_chunk_generated(there));
    std::fs::remove_dir_all(&dir).unwrap();
}
