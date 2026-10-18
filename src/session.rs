//! Playing a world, from the menus or as `RC_WORLD` and `RC_JOIN` ask, and leaving it again
//! (M-U1): opening a world of this game's own or joining a server's, the loading screen until
//! there's ground under the player, and Save and Quit to Title, which puts away everything the
//! world had so another can be opened in the same game.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use bevy::math::I64Vec3;
use bevy::prelude::*;
use bevy_replicon::prelude::RepliconChannels;

use crate::chunks::chunk::ChunkCoordinate;
use crate::chunks::chunk_loader::{ChunkLoader, Unsettled};
use crate::client::join::{join, server_address, Checking, LeftServer};
use crate::client::player::LocalPlayer;
use crate::map::explored::MapDir;
use crate::menu::{GameState, Menu, Page};
use crate::preferences::Preferences;
use crate::protocol::DEFAULT_PORT;
use crate::server::{host, WorldToOpen, WorldUnopened};
use crate::settings::Settings;
use crate::world::{LeaveWorld, World};

// How far round the player's chunk the loading screen waits for terrain, across and down, and
// up: the spawn is 20 blocks over the ground, so this always has the ground under it.
const READY_ACROSS: i64 = 2;
const READY_DOWN: i64 = 2;
const READY_UP: i64 = 1;

/// The world being played, or to be once it's loaded.
#[derive(Resource, Debug, Clone, PartialEq)]
pub enum Session {
    /// A world of this game's own, saved in `dir`, with `seed` if it's new, starting at
    /// `time_of_day` if one's given, and opened to friends on `host` if that's given.
    Singleplayer {
        dir: PathBuf,
        seed: Option<u32>,
        time_of_day: Option<f64>,
        host: Option<u16>,
    },
    /// A world on the server at `address`, `host:port` or `host`.
    Join { address: String },
}

impl Session {
    /// The world to play straight away, skipping the title screen, as environment variables
    /// ask for development, `var` reading them: `RC_JOIN`'s server, or the world in `worlds`
    /// that `RC_WORLD` names (`world` if it's unset), made with `RC_SEED` if it's new, starting
    /// at `RC_TIME`, in days, and opened to friends on `RC_HOST`'s port, or the default one if
    /// it's not a port, if `RC_HOST` is set. None if none of `RC_JOIN`, `RC_WORLD` or `RC_HOST`
    /// is set, for the title screen.
    pub fn from_env(var: impl Fn(&str) -> Option<String>, worlds: &Path) -> Option<Self> {
        if let Some(address) = var("RC_JOIN") {
            return Some(Session::Join { address });
        }
        let host = var("RC_HOST");
        let name = var("RC_WORLD");
        if name.is_none() && host.is_none() {
            return None;
        }
        Some(Session::Singleplayer {
            dir: worlds.join(name.as_deref().unwrap_or("world")),
            seed: var("RC_SEED").and_then(|seed| seed.parse().ok()),
            time_of_day: var("RC_TIME").and_then(|time| time.parse().ok()),
            host: host.map(|port| port.parse().unwrap_or(DEFAULT_PORT)),
        })
    }

    /// Starts playing this: loading it, then into it.
    pub fn start(self, commands: &mut Commands) {
        commands.insert_resource(self);
        commands.set_state(GameState::Loading);
    }
}

/// Where the world's open to friends, as the pause menu shows, or why it couldn't be.
#[derive(Resource, Debug, Clone)]
pub struct Hosting(pub Result<SocketAddr, String>);

/// How far loading's got, for the loading screen.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct Loading {
    /// What it's doing.
    pub doing: String,
    /// How much of the terrain round the player is there, from 0 to 1, once there's terrain.
    pub terrain: Option<f32>,
}

/// Opens the world asked for as loading starts, follows it loading until the player can play,
/// and goes back to the title screen, saying why, if it can't be opened or the server's lost.
pub struct SessionPlugin;

impl Plugin for SessionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Loading>()
            .add_systems(OnEnter(GameState::Loading), begin)
            .add_systems(
                Update,
                (
                    follow_loading.run_if(in_state(GameState::Loading)),
                    (leave_unopened, leave_when_disconnected).run_if(resource_exists::<Session>),
                ),
            );
    }
}

// Opens the world, or starts joining the server, that's been asked for.
fn begin(
    session: Option<Res<Session>>,
    settings: Res<Settings>,
    preferences: Res<Preferences>,
    channels: Res<RepliconChannels>,
    mut commands: Commands,
) {
    let Some(session) = session else {
        return;
    };
    let name = preferences.player_name();
    match session.clone() {
        Session::Singleplayer {
            dir,
            seed,
            time_of_day,
            host: port,
        } => {
            commands.insert_resource(MapDir(dir.clone()));
            commands.insert_resource(WorldToOpen {
                dir,
                seed,
                time_of_day,
                region_distance: settings.renderer.lod_layout().region_reach() as u32,
                host: Some(name),
            });
            if let Some(port) = port {
                let hosted = host(&mut commands, &channels, port).map_err(|e| e.to_string());
                match &hosted {
                    Ok(address) => info!("open to friends at {address}"),
                    Err(error) => error!("couldn't open the world to friends on {port}: {error}"),
                }
                commands.insert_resource(Hosting(hosted));
            }
        }
        Session::Join { address } => {
            commands.insert_resource(MapDir::joined(&address));
            let joined = server_address(&address)
                .map_err(|error| format!("Couldn't find the server {address}: {error}"))
                .and_then(|server| {
                    join(&mut commands, &channels, server, &name)
                        .map(|()| info!("joining {server}"))
                        .map_err(|error| format!("Couldn't join {server}: {error}"))
                });
            if let Err(text) = joined {
                error!("{text}");
                commands.run_system_cached_with(leave_saying, notice("Couldn't join", text));
            }
        }
    }
}

// Says how far loading's got, and plays once the terrain round the player's drawn.
#[allow(clippy::too_many_arguments)]
fn follow_loading(
    session: Option<Res<Session>>,
    world: Option<Res<World>>,
    checking: Option<Res<Checking>>,
    player: Query<&Transform, With<LocalPlayer>>,
    loader: Res<ChunkLoader>,
    unsettled: Query<(), Unsettled>,
    mut loading: ResMut<Loading>,
    mut next: ResMut<NextState<GameState>>,
) {
    let joining = matches!(session.as_deref(), Some(Session::Join { .. }));
    let (Some(_), Ok(body)) = (world, player.single()) else {
        let doing = match (joining, checking) {
            (true, Some(_)) => "Checking the world is the server's...",
            (true, None) => "Joining the server...",
            (false, _) => "Opening the world...",
        };
        loading.set_if_neq(Loading {
            doing: doing.into(),
            terrain: None,
        });
        return;
    };
    let middle = ChunkCoordinate::containing(body.translation.floor().as_i64vec3());
    let around = (-READY_ACROSS..=READY_ACROSS).flat_map(|x| {
        (-READY_DOWN..=READY_UP).flat_map(move |y| {
            (-READY_ACROSS..=READY_ACROSS)
                .map(move |z| ChunkCoordinate(middle.0 + I64Vec3::new(x, y, z)))
        })
    });
    let (mut drawn, mut all) = (0, 0);
    for chunk in around {
        all += 1;
        drawn += usize::from(loader.is_drawn(chunk, &unsettled));
    }
    loading.set_if_neq(Loading {
        doing: "Loading the terrain...".into(),
        terrain: Some(drawn as f32 / all as f32),
    });
    if drawn == all {
        next.set(GameState::Playing);
    }
}

// Back to the title screen if the world couldn't be opened, saying why.
fn leave_unopened(mut unopened: MessageReader<WorldUnopened>, mut commands: Commands) {
    if let Some(WorldUnopened(text)) = unopened.read().last() {
        commands.run_system_cached_with(
            leave_saying,
            notice("Couldn't open the world", text.clone()),
        );
    }
}

// Back to the title screen if the server's refused this client or gone, saying why: only the
// first reason given counts, as one leads to another.
fn leave_when_disconnected(
    mut left: MessageReader<LeftServer>,
    state: Res<State<GameState>>,
    mut commands: Commands,
) {
    if let Some(LeftServer(text)) = left.read().next() {
        let title = if *state.get() == GameState::Loading {
            "Couldn't join"
        } else {
            "Disconnected"
        };
        commands.run_system_cached_with(leave_saying, notice(title, text.clone()));
    }
}

fn notice(title: &str, text: String) -> Page {
    Page::Notice {
        title: title.into(),
        text,
    }
}

/// Leaves the world being played, saving it, for the title screen.
pub fn leave(world: &mut bevy::ecs::world::World) {
    if world.remove_resource::<Session>().is_none() {
        return;
    }
    world.remove_resource::<WorldToOpen>();
    world.remove_resource::<Hosting>();
    world.run_schedule(LeaveWorld);
    world.insert_resource(Loading::default());
    world
        .resource_mut::<NextState<GameState>>()
        .set(GameState::Title);
}

// The same, with `notice` over the title screen.
fn leave_saying(In(notice): In<Page>, world: &mut bevy::ecs::world::World) {
    leave(world);
    world.resource_mut::<Menu>().show(vec![Page::Title, notice]);
}
