//! The dedicated server (§4.4, §5.9): a world hosted with no window, GPU or player of its own,
//! for an always-on machine to keep open for friends, who join it as they would a game opened
//! to them. Its console takes commands typed into the terminal.

use std::io::IsTerminal;
use std::time::Duration;

use bevy::app::{ScheduleRunnerPlugin, TerminalCtrlCHandlerPlugin};
use bevy::log::tracing_subscriber::fmt;
use bevy::log::{BoxedFmtLayer, LogPlugin};
use bevy::prelude::*;
use bevy_replicon::prelude::RepliconChannels;
use rustcraft::chunks::task_pool_plugin;
use rustcraft::protocol::DEFAULT_PORT;
use rustcraft::save::{world_name, worlds_dir};
use rustcraft::server::{host, Console, DedicatedServerPlugins, WorldToOpen};
use rustcraft::world::World;

// Frames a second. Clients send where they are 20 times a second and others see them 100 ms
// behind, so passing poses on within a 30th of a second is soon enough, and nothing the server
// runs, falling items, furnaces or hunger, needs more. Half the usual 60 halves what an idle
// server costs the machine it's left running on.
const TICK_RATE: f64 = 30.0;

const USAGE: &str = "\
Usage: rustcraft-server [--world <name>] [--port <port>] [--seed <seed>]

Hosts a rustcraft world with no window, for players to join over the network.

Options:
  --world <name>  the world to host, a name in the worlds directory or a path
                  (default: RC_WORLD, or \"world\")
  --port <port>   the UDP port to take players on (default: RC_HOST, or 25600)
  --seed <seed>   the seed a new world takes (default: RC_SEED, or a random one)
  --help          print this

Type `help` once it's running for the console's commands, and `stop` to save and stop.";

/// What the server's asked to do, by its arguments or else the environment.
struct Options {
    world: String,
    port: u16,
    seed: Option<u32>,
}

impl Options {
    // None if asked for help.
    fn read(mut args: impl Iterator<Item = String>) -> Result<Option<Self>, String> {
        let port = std::env::var("RC_HOST")
            .ok()
            .and_then(|port| port.parse().ok());
        let mut options = Self {
            world: world_name(),
            port: port.unwrap_or(DEFAULT_PORT),
            seed: World::requested_seed(),
        };
        while let Some(arg) = args.next() {
            let mut value = || args.next().ok_or(format!("{arg} needs a value"));
            match arg.as_str() {
                "--world" => options.world = value()?,
                "--port" => options.port = number(&value()?)?,
                "--seed" => options.seed = Some(number(&value()?)?),
                "--help" | "-h" => return Ok(None),
                _ => return Err(format!("unknown argument {arg}")),
            }
        }
        Ok(Some(options))
    }
}

fn number<T: std::str::FromStr>(text: &str) -> Result<T, String> {
    text.parse().map_err(|_| format!("{text} isn't a number"))
}

fn main() -> AppExit {
    let options = match Options::read(std::env::args().skip(1)) {
        Ok(Some(options)) => options,
        Ok(None) => {
            println!("{USAGE}");
            return AppExit::Success;
        }
        Err(error) => {
            eprintln!("{error}\n\n{USAGE}");
            return AppExit::error();
        }
    };
    let frame = Duration::from_secs_f64(1.0 / TICK_RATE);
    App::new()
        .add_plugins((
            MinimalPlugins
                .set(ScheduleRunnerPlugin::run_loop(frame))
                .set(task_pool_plugin()),
            LogPlugin {
                fmt_layer: log_to_stdout,
                ..default()
            },
            // which saves as it stops, as `stop` does
            TerminalCtrlCHandlerPlugin,
            DedicatedServerPlugins,
        ))
        // replicon sends what's changed once a fixed step, so once a frame
        .insert_resource(Time::<Fixed>::from_hz(TICK_RATE))
        .insert_resource(WorldToOpen {
            dir: worlds_dir().join(&options.world),
            seed: options.seed,
            time_of_day: None,
            // only the chunks round each player are generated, so the fewest regions are kept
            region_distance: 0,
            host: None,
        })
        .insert_resource(Console::stdin())
        .add_systems(Startup, serve(options.port))
        .run()
}

// The log, to stdout as a service's is, coloured only in a terminal.
fn log_to_stdout(_: &mut App) -> Option<BoxedFmtLayer> {
    let terminal = std::io::stdout().is_terminal();
    let layer = fmt::Layer::default()
        .with_writer(std::io::stdout)
        .with_ansi(terminal);
    Some(Box::new(layer))
}

// Takes players on `port`, or stops the server if it can't.
fn serve(port: u16) -> impl FnMut(Commands, Res<RepliconChannels>, MessageWriter<AppExit>) {
    move |mut commands, channels, mut exit| match host(&mut commands, &channels, port) {
        Ok(address) => info!("taking players at {address}; type `help` for commands"),
        Err(error) => {
            error!("couldn't take players on port {port}: {error}");
            exit.write(AppExit::error());
        }
    }
}
