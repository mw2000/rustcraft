//! A dedicated server's console: lines typed into its terminal run as commands, as the server
//! itself, with every operator's right, and what they print goes to its log.

use std::io::BufRead;
use std::sync::mpsc::{self, Receiver};

use bevy::platform::cell::SyncCell;
use bevy::prelude::*;
use bevy_replicon::prelude::ServerSystems;

use super::commands::CONSOLE_NAME;
use crate::commands::usages;
use crate::protocol::{Chat, CommandOutput, RunCommand};

/// The lines typed into a dedicated server's console, as a thread reading them sends them.
#[derive(Resource)]
pub struct Console(SyncCell<Receiver<String>>);

impl Console {
    /// The lines typed into the terminal, read on a thread of their own, as reading waits for
    /// them. A server with nothing to read from, such as one run as a service, gets none.
    pub fn stdin() -> Self {
        let (send, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        Self::new(lines)
    }

    /// The lines sent on a channel.
    pub fn new(lines: Receiver<String>) -> Self {
        Self(SyncCell::new(lines))
    }
}

/// Runs what's typed into the `Console`, which it needs, and logs what the server tells it.
/// Needs `ServerPlugin`.
pub struct ConsolePlugin;

impl Plugin for ConsolePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, read_console)
            // as replicon hands the server's own messages over, as it does the host's
            .add_systems(PostUpdate, log_output.after(ServerSystems::Send));
    }
}

/// Runs each line typed: `say <text>` says it to everyone, `stop` saves and stops the server,
/// `help` lists what can be typed, and anything else is a command, typed with or without its
/// `/`, which the server runs as it does a player's.
fn read_console(
    mut console: ResMut<Console>,
    mut commands: MessageWriter<RunCommand>,
    mut chat: MessageWriter<Chat>,
    mut exit: MessageWriter<AppExit>,
) {
    while let Ok(line) = console.0.get().try_recv() {
        let line = line.trim().trim_start_matches('/');
        let (word, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        match word {
            "" => {}
            "say" => {
                chat.write(Chat(rest.into()));
            }
            "stop" => {
                info!("stopping the server");
                exit.write(AppExit::Success);
            }
            "help" => info!(
                "{}\nsay <text>  say something to everyone, as {CONSOLE_NAME}\n\
                 stop  save the world and stop the server\n\
                 Commands can be typed without the /",
                usages(true)
            ),
            _ => {
                commands.write(RunCommand(format!("/{line}")));
            }
        }
    }
}

fn log_output(mut output: MessageReader<CommandOutput>) {
    for CommandOutput { text, error } in output.read() {
        if *error {
            warn!("{text}");
        } else {
            info!("{text}");
        }
    }
}
