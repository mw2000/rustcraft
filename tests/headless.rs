//! The guard that gameplay doesn't depend on rendering: what a dedicated server will run, on
//! `MinimalPlugins` with no window, camera or renderer.

use std::path::Path;
use std::time::Duration;

use bevy::ecs::message::Messages;
use bevy::math::I64Vec3;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimeUpdateStrategy;
use bevy_replicon::prelude::*;
use rustcraft::block::BlockType;
use rustcraft::chunks::chunk::{ChunkCoordinate, ChunkData};
use rustcraft::clock::TimeOfDay;
use rustcraft::game_mode::{GameMode, GameModePlugin};
use rustcraft::player::{Movement, Player, PlayerLook, PlayerOwner};
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
    let dir = std::env::temp_dir().join(format!("rustcraft-headless-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut app = server(&dir);
    let player = hosts_player(&mut app);
    let spawn = app.world().resource::<World>().spawn();
    assert_eq!(
        spawn,
        app.world().get::<Transform>(player).unwrap().translation
    );
    let started = app.world().resource::<TimeOfDay>().0;

    // a block under the player's feet, in a chunk loaded empty
    let below = spawn.floor().as_i64vec3() - I64Vec3::Y;
    app.world_mut()
        .resource_mut::<World>()
        .insert_chunk(ChunkCoordinate::containing(below), ChunkData::default());
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

    // a player's edit, checked and made by the server, and sent on to the clients
    app.world_mut().write_message(RequestEdit {
        block: below,
        to: BlockType::Bricks,
        seq: 1,
    });
    app.update();
    assert_eq!(
        BlockType::Bricks,
        app.world().resource::<World>().get_block_at(below)
    );
    assert_eq!(1, app.world().resource::<Messages<BlockChanged>>().len());

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
    std::fs::remove_dir_all(&dir).unwrap();
}
