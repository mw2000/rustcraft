use bevy::prelude::*;
use bevy_replicon::prelude::*;

use super::players::PoseCheck;
use crate::chunks::generate::WATER_LEVEL;
use crate::commands::{parse, Command, Destination, HELP};
use crate::game_mode::GameMode;
use crate::player::PlayerOwner;
use crate::protocol::{CommandOutput, RunCommand, Teleport};
use crate::world::World;

// How far above the ground `/tp x z` puts the player.
const SURFACE_CLEARANCE: f32 = 2.0;

/// Runs the commands clients type, each for the player who typed it, and tells only them what
/// it printed. Anyone may change their game mode until operators come in M0.6. A player from
/// elsewhere `/tp` moves is told where they've been put, as they decide their own movement.
pub(super) fn run_commands(
    mut lines: MessageReader<FromClient<RunCommand>>,
    world: Res<World>,
    mut players: Query<(
        &PlayerOwner,
        &mut Transform,
        &mut GameMode,
        Option<&mut PoseCheck>,
    )>,
    mut output: MessageWriter<ToClients<CommandOutput>>,
    mut teleports: MessageWriter<ToClients<Teleport>>,
) {
    for FromClient {
        client_id: client,
        message: RunCommand(line),
    } in lines.read()
    {
        let player = players.iter_mut().find(|(owner, ..)| owner.0 == *client);
        let result = match player {
            Some((_, mut transform, mut mode, check)) => {
                let was = transform.translation;
                let result =
                    parse(line, was).map(|command| run(command, &world, &mut transform, &mut mode));
                if let Some(mut check) = check.filter(|_| transform.translation != was) {
                    check.teleported();
                    teleports.write(ToClients {
                        targets: SendTargets::Single(*client),
                        message: Teleport {
                            position: transform.translation,
                        },
                    });
                }
                result
            }
            None => Err("You have no player to run commands as".into()),
        };
        let (text, error) = match result {
            Ok(text) => (text, false),
            Err(text) => (text, true),
        };
        output.write(ToClients {
            targets: SendTargets::Single(*client),
            message: CommandOutput { text, error },
        });
    }
}

// Runs `command` for the player at `transform`, returning what it prints.
fn run(command: Command, world: &World, transform: &mut Transform, mode: &mut GameMode) -> String {
    match command {
        Command::Teleport(destination) => {
            let to = match destination {
                Destination::Exact(to) => to,
                Destination::Surface { x, z } => {
                    let ground = world.region_store.noise().sample(x as f64, z as f64).height;
                    let top = ground.max(WATER_LEVEL as f64) as f32;
                    Vec3::new(x, top + SURFACE_CLEARANCE, z)
                }
            };
            // In one App this is the local player's own body, so it moves at once. A remote
            // player decides their own movement, so they're sent a `Teleport`.
            transform.translation = to;
            format!("Teleported to {:.1}, {:.1}, {:.1}", to.x, to.y, to.z)
        }
        Command::Seed => format!("Seed: {}", world.seed()),
        Command::GameMode(to) => {
            *mode = to;
            format!("Game mode set to {}", to.name())
        }
        Command::Help => HELP.into(),
    }
}
