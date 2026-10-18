//! The guard that gameplay doesn't depend on rendering: what a dedicated server will run, on
//! `MinimalPlugins` with no window, camera or renderer.

use std::time::Duration;

use bevy::ecs::message::Messages;
use bevy::math::I64Vec3;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimeUpdateStrategy;
use bevy_replicon::RepliconPlugins;
use rustcraft::block::BlockType;
use rustcraft::chunks::chunk::{ChunkCoordinate, ChunkData};
use rustcraft::clock::{ClockPlugin, TimeOfDay};
use rustcraft::game_mode::{GameMode, GameModePlugin};
use rustcraft::player::{Movement, Player, PlayerLook};
use rustcraft::protocol::{BlockChanged, ProtocolPlugin, RequestEdit};
use rustcraft::save::{PlayerState, SavePlugin, WorldSave};
use rustcraft::server::ServerPlugin;
use rustcraft::world::World;

#[test]
fn test_gameplay_runs_and_saves_with_no_rendering() {
    let dir = std::env::temp_dir().join(format!("rustcraft-headless-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (save, file, edits) = WorldSave::open(dir.clone(), Some(3)).unwrap();
    let mut world = World::from_save(file.seed, edits, 16);
    let state = PlayerState::arriving_at(world.spawn());
    // a block under the player's feet, in a chunk loaded empty
    let below = world.spawn().floor().as_i64vec3() - I64Vec3::Y;
    world.insert_chunk(ChunkCoordinate::containing(below), ChunkData::default());
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        StatesPlugin,
        RepliconPlugins,
        ProtocolPlugin,
        ServerPlugin,
        SavePlugin,
        GameModePlugin,
        ClockPlugin,
    ))
    .insert_resource(save)
    .insert_resource(world)
    .insert_resource(TimeOfDay(file.time_of_day))
    // a second a frame, so the clock visibly moves
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(1)));
    let look = PlayerLook {
        yaw: 1.0,
        pitch: 0.3,
    };
    let player = app
        .world_mut()
        .spawn((Player::default(), look, state.body()))
        .id();
    app.finish();
    for _ in 0..5 {
        app.update();
    }
    assert!(app.world().resource::<TimeOfDay>().0 > file.time_of_day);
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
    let (_, saved, saved_edits) = WorldSave::open(dir.clone(), None).unwrap();
    let coord = ChunkCoordinate::containing(below);
    let local = ChunkCoordinate::local_block(below);
    let saved_edits: Vec<_> = saved_edits.in_chunk(coord).collect();
    assert_eq!(vec![(local, BlockType::Bricks)], saved_edits);
    let saved_player = saved.player.unwrap();
    assert_eq!(look, saved_player.look());
    assert_eq!(GameMode::Survival, saved_player.game_mode);
    assert!(saved.time_of_day > file.time_of_day);
    std::fs::remove_dir_all(&dir).unwrap();
}
