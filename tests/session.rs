//! Playing worlds from the menus (M-U1): opening one, saving and quitting to the title screen,
//! and opening another in the same game, with nothing of the first left behind.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bevy::asset::AssetPlugin;
use bevy::input::InputPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::time::TimeUpdateStrategy;
use bevy::window::WindowFocused;
use bevy_replicon::prelude::*;
use bevy_replicon_renet2::RepliconRenetPlugins;
use rustcraft::chunks::chunk_loader::ChunkLoader;
use rustcraft::chunks::generate::WORLD_HEIGHT;
use rustcraft::client::player::LocalPlayer;
use rustcraft::client::servers::ServerList;
use rustcraft::client::ClientPlugin;
use rustcraft::game_mode::GameModePlugin;
use rustcraft::menu::{GameState, Menu, MenuPlugin, Page};
use rustcraft::player::Player;
use rustcraft::protocol::{ProtocolPlugin, DEFAULT_PORT};
use rustcraft::save::worlds::WorldsDir;
use rustcraft::save::{SavePlugin, WorldSave};
use rustcraft::server::ServerPlugin;
use rustcraft::session::{leave, Session, SessionPlugin};
use rustcraft::settings::Settings;
use rustcraft::world::World;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rustcraft-session-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

// The game with its menus, its client and its server, headless, saving worlds in `worlds`, but
// for drawing anything.
fn game(worlds: &Path) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        StatesPlugin,
        InputPlugin,
        AssetPlugin::default(),
        RepliconPlugins,
        RepliconRenetPlugins,
        ProtocolPlugin,
        ClientPlugin,
        ServerPlugin,
        SavePlugin,
        GameModePlugin,
        SessionPlugin,
        MenuPlugin,
    ))
    .add_message::<WindowFocused>()
    .insert_resource(Settings::default())
    .insert_resource(WorldsDir(worlds.to_owned()))
    .insert_resource(ServerList::read(worlds.join("servers.toml")))
    .insert_resource(ChunkLoader::new(
        Settings::default().renderer.lod_layout(),
        WORLD_HEIGHT,
        false,
        Handle::default(),
        Handle::default(),
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        50,
    )));
    app.finish();
    app.update();
    app
}

fn state(app: &App) -> GameState {
    app.world().resource::<State<GameState>>().get().clone()
}

fn update_until(app: &mut App, done: impl Fn(&mut App) -> bool) {
    let start = Instant::now();
    while !done(app) {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "it never got there"
        );
        app.update();
    }
}

// Opens the world in `dir` as the menus would, and plays once its player's there, as the
// loading screen would once its terrain's drawn, which this game doesn't draw.
fn play(app: &mut App, dir: &Path) -> Entity {
    app.world_mut().commands().queue({
        let dir = dir.to_owned();
        move |world: &mut bevy::ecs::world::World| {
            Session::Singleplayer {
                dir,
                seed: Some(7),
                time_of_day: None,
                host: None,
            }
            .start(&mut world.commands());
        }
    });
    update_until(app, |app| {
        app.world_mut()
            .query_filtered::<Entity, With<LocalPlayer>>()
            .single(app.world())
            .is_ok()
    });
    assert_eq!(GameState::Loading, state(app));
    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::Playing);
    app.update();
    app.world_mut()
        .query_filtered::<Entity, With<LocalPlayer>>()
        .single(app.world())
        .unwrap()
}

fn quit_to_title(app: &mut App) {
    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::Paused);
    app.update();
    app.world_mut().run_system_cached(leave).unwrap();
    app.update();
    app.update();
    assert_eq!(GameState::Title, state(app));
    assert_eq!(Some(&Page::Title), app.world().resource::<Menu>().top());
}

fn entities(app: &mut App) -> usize {
    app.world_mut().query::<Entity>().iter(app.world()).count()
}

#[test]
fn test_quitting_to_title_saves_and_leaves_nothing_behind_for_the_next_world() {
    let worlds = temp_dir("quit");
    let mut app = game(&worlds);
    assert_eq!(GameState::Title, state(&app));

    // the first time round makes what's kept between worlds, such as the systems the menus run
    let first = worlds.join("First");
    let player = play(&mut app, &first);
    app.world_mut()
        .get_mut::<Transform>(player)
        .unwrap()
        .translation = Vec3::new(5.0, 600.0, 9.0);
    quit_to_title(&mut app);
    let between = entities(&mut app);
    for gone in [
        app.world().contains_resource::<World>(),
        app.world().contains_resource::<WorldSave>(),
        app.world().contains_resource::<Session>(),
    ] {
        assert!(!gone);
    }
    assert_eq!(
        0,
        app.world_mut().query::<&Player>().iter(app.world()).count()
    );
    // and it was saved as it was left
    let saved = std::fs::read_to_string(first.join("world.toml")).unwrap();
    assert!(saved.contains("600.0"), "{saved}");

    // another world, played and left the same way, leaves the game as the first did
    let second = worlds.join("Second");
    play(&mut app, &second);
    assert!(app.world().resource::<World>().is_served_here());
    quit_to_title(&mut app);
    assert_eq!(between, entities(&mut app));

    // and the first opens again where it was left
    let player = play(&mut app, &first);
    let at = app.world().get::<Transform>(player).unwrap().translation;
    assert_eq!(Vec3::new(5.0, 600.0, 9.0), at);
    quit_to_title(&mut app);
    assert_eq!(between, entities(&mut app));
    std::fs::remove_dir_all(&worlds).unwrap();
}

#[test]
fn test_a_world_that_cant_be_opened_goes_back_to_the_title_saying_why() {
    let worlds = temp_dir("unopened");
    let broken = worlds.join("Broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("world.toml"), "format = 99\nseed = 1").unwrap();
    let mut app = game(&worlds);
    app.world_mut()
        .commands()
        .queue(move |world: &mut bevy::ecs::world::World| {
            Session::Singleplayer {
                dir: broken,
                seed: None,
                time_of_day: None,
                host: None,
            }
            .start(&mut world.commands());
        });
    update_until(&mut app, |app| {
        matches!(
            app.world().resource::<Menu>().top(),
            Some(Page::Notice { .. })
        )
    });
    app.update();
    assert_eq!(GameState::Title, state(&app));
    let Some(Page::Notice { text, .. }) = app.world().resource::<Menu>().top() else {
        unreachable!();
    };
    assert!(text.contains("newer version"), "{text}");
    std::fs::remove_dir_all(&worlds).unwrap();
}

#[test]
fn test_a_server_that_cant_be_reached_goes_back_to_the_title_saying_why() {
    let worlds = temp_dir("unreachable");
    let mut app = game(&worlds);
    app.world_mut()
        .commands()
        .queue(|world: &mut bevy::ecs::world::World| {
            Session::Join {
                address: "no.such.host.invalid".into(),
            }
            .start(&mut world.commands());
        });
    update_until(&mut app, |app| {
        matches!(
            app.world().resource::<Menu>().top(),
            Some(Page::Notice { .. })
        )
    });
    app.update();
    assert_eq!(GameState::Title, state(&app));
    assert!(!app.world().contains_resource::<Session>());
}

#[test]
fn test_environment_variables_skip_the_title_screen_for_development() {
    let worlds = Path::new("/worlds");
    let vars = |set: &'static [(&str, &str)]| {
        move |name: &str| {
            set.iter()
                .find(|(var, _)| *var == name)
                .map(|(_, value)| value.to_string())
        }
    };
    assert_eq!(None, Session::from_env(vars(&[("RC_SEED", "7")]), worlds));
    assert_eq!(
        Some(Session::Singleplayer {
            dir: worlds.join("test"),
            seed: Some(7),
            time_of_day: Some(0.5),
            host: None,
        }),
        Session::from_env(
            vars(&[("RC_WORLD", "test"), ("RC_SEED", "7"), ("RC_TIME", "0.5")]),
            worlds
        )
    );
    assert_eq!(
        Some(Session::Join {
            address: "example.org".into()
        }),
        Session::from_env(
            vars(&[("RC_JOIN", "example.org"), ("RC_WORLD", "x")]),
            worlds
        )
    );
    let Some(Session::Singleplayer { dir, host, .. }) =
        Session::from_env(vars(&[("RC_HOST", "")]), worlds)
    else {
        panic!("hosting plays the default world");
    };
    assert_eq!((worlds.join("world"), Some(DEFAULT_PORT)), (dir, host));

    // and a game started with one goes straight to loading it, never showing the title
    let dir = temp_dir("dev-path");
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        StatesPlugin,
        InputPlugin,
        AssetPlugin::default(),
        RepliconPlugins,
        RepliconRenetPlugins,
        ProtocolPlugin,
        ClientPlugin,
        ServerPlugin,
        SavePlugin,
        GameModePlugin,
        SessionPlugin,
        MenuPlugin,
    ))
    .add_message::<WindowFocused>()
    .insert_resource(Settings::default())
    .insert_resource(ChunkLoader::new(
        Settings::default().renderer.lod_layout(),
        WORLD_HEIGHT,
        false,
        Handle::default(),
        Handle::default(),
    ))
    .insert_resource(Session::Singleplayer {
        dir: dir.clone(),
        seed: Some(3),
        time_of_day: None,
        host: None,
    })
    .insert_state(GameState::Loading);
    app.finish();
    app.update();
    app.update();
    assert_eq!(GameState::Loading, state(&app));
    assert_eq!(Some(&Page::Loading), app.world().resource::<Menu>().top());
    assert_eq!(3, app.world().resource::<World>().seed());
    std::fs::remove_dir_all(&dir).unwrap();
}
