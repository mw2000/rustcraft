//! The guard that gameplay doesn't depend on rendering: what a dedicated server will run, on
//! `MinimalPlugins` with no window, camera or renderer.

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use rustcraft::clock::{ClockPlugin, TimeOfDay};
use rustcraft::game_mode::{GameMode, GameModePlugin};
use rustcraft::player::{Movement, Player, PlayerLook};
use rustcraft::save::{PlayerState, SavePlugin, WorldSave};
use rustcraft::world::World;

#[test]
fn test_gameplay_runs_and_saves_with_no_rendering() {
    let dir = std::env::temp_dir().join(format!("rustcraft-headless-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (save, file, edits) = WorldSave::open(dir.clone(), Some(3)).unwrap();
    let world = World::from_save(file.seed, edits, 16);
    let state = PlayerState::arriving_at(world.spawn());
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, SavePlugin, GameModePlugin, ClockPlugin))
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

    app.world_mut().write_message(AppExit::Success);
    app.update();
    let (_, saved, _) = WorldSave::open(dir.clone(), None).unwrap();
    let saved_player = saved.player.unwrap();
    assert_eq!(look, saved_player.look());
    assert_eq!(GameMode::Survival, saved_player.game_mode);
    assert!(saved.time_of_day > file.time_of_day);
    std::fs::remove_dir_all(&dir).unwrap();
}
