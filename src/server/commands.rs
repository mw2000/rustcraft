use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_replicon::prelude::*;

use super::chat::{tidy, ChatAllowances, TOO_QUICK};
use super::creatures::creature;
use super::players::PoseCheck;
use super::structures::{list, Authoring};
use crate::chunks::generate::WATER_LEVEL;
use crate::commands::{help, parse, Command, Destination};
use crate::creatures::SavedCreature;
use crate::game_mode::GameMode;
use crate::player::name::PlayerName;
use crate::player::PlayerOwner;
use crate::protocol::{CommandOutput, RunCommand, Teleport};
use crate::save::WorldSave;
use crate::world::World;

// How far above the ground `/tp x z` puts the player.
const SURFACE_CLEARANCE: f32 = 2.0;
/// Who a dedicated server's console speaks as, as it has no player of its own.
pub(super) const CONSOLE_NAME: &str = "Server";

/// The players who may run the commands that change the world or other players besides the
/// host, who always may, by name, as `world.toml` keeps them. Names aren't checked as players
/// join (§4.4), so this is only as safe as who the host lets join.
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct Operators(Vec<String>);

impl Operators {
    pub fn new(names: Vec<String>) -> Self {
        Self(names)
    }

    pub fn names(&self) -> &[String] {
        &self.0
    }

    /// Whether the client whose player is called `name` may run every command.
    pub(super) fn allow(&self, client: ClientId, name: &PlayerName) -> bool {
        client == ClientId::Server || self.contains(name.as_str())
    }

    fn contains(&self, name: &str) -> bool {
        self.position(name).is_some()
    }

    // Ignoring case, as names are told apart (`PlayerName::unique_among`).
    fn position(&self, name: &str) -> Option<usize> {
        self.0.iter().position(|op| op.eq_ignore_ascii_case(name))
    }
}

type Players<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static PlayerOwner,
        &'static PlayerName,
        &'static mut Transform,
        &'static mut GameMode,
        Option<&'static mut PoseCheck>,
    ),
>;

/// What running commands reads and changes.
#[derive(SystemParam)]
pub(super) struct Running<'w, 's> {
    world: ResMut<'w, World>,
    operators: ResMut<'w, Operators>,
    // none in a test that builds its own world
    save: Option<ResMut<'w, WorldSave>>,
    allowances: ResMut<'w, ChatAllowances>,
    time: Res<'w, Time>,
    players: Players<'w, 's>,
    output: MessageWriter<'w, ToClients<CommandOutput>>,
    teleports: MessageWriter<'w, ToClients<Teleport>>,
    authoring: Authoring<'w, 's>,
    commands: Commands<'w, 's>,
}

/// A player here, as a command finds them.
struct Found {
    player: Entity,
    owner: ClientId,
    name: PlayerName,
    at: Vec3,
}

/// Runs the commands clients type, each for the player who typed it, and tells only them what
/// it printed. Those that change the world or other players are only for operators. A player
/// from elsewhere `/tp` moves is told where they've been put, as they decide their own movement,
/// and a player another changes is told who did it. A dedicated server's console runs them as
/// the server (`ClientId::Server`) with no player, so only on others.
pub(super) fn run_commands(mut lines: MessageReader<FromClient<RunCommand>>, mut running: Running) {
    for FromClient {
        client_id: client,
        message: RunCommand(line),
    } in lines.read()
    {
        let (text, error) = match running.run(*client, line) {
            Ok(text) => (text, false),
            Err(text) => (text, true),
        };
        running.tell(*client, text, error);
    }
}

impl Running<'_, '_> {
    // Runs `line` for the client's player, or for the console, returning what it prints.
    fn run(&mut self, client: ClientId, line: &str) -> Result<String, String> {
        let sender = self.players.iter().find(|(_, owner, ..)| owner.0 == client);
        let sender = sender.map(|(player, ..)| self.found(player));
        let (player, name, at) = match &sender {
            Some(found) => (Some(found.player), found.name.clone(), found.at),
            // `~` is from the world's spawn
            None if client == ClientId::Server => {
                (None, PlayerName::new(CONSOLE_NAME), self.world.spawn())
            }
            None => return Err("You have no player to run commands as".into()),
        };
        let op = self.operators.allow(client, &name);
        // the sender's own player, for a command that names no other
        let own = || sender.ok_or_else(|| "Name a player, as the console has none".to_owned());
        let author =
            || player.ok_or("Structures are built in the game, as the console can't select");
        match parse(line, at, op)? {
            Command::Teleport { who, to } => {
                let target = match who {
                    Some(who) => self.find(&who)?,
                    None => own()?,
                };
                let to = match to {
                    Destination::Exact(to) => to,
                    Destination::Surface { x, z } => {
                        let ground = self.world.region_store.noise().sample(x as f64, z as f64);
                        let top = ground.height.max(WATER_LEVEL as f64) as f32;
                        Vec3::new(x, top + SURFACE_CLEARANCE, z)
                    }
                    Destination::Player(other) => self.find(&other)?.at,
                };
                self.teleport(target.player, to);
                let at = format!("{:.1}, {:.1}, {:.1}", to.x, to.y, to.z);
                if Some(target.player) == player {
                    return Ok(format!("Teleported to {at}"));
                }
                let by = format!("{} teleported you to {at}", name.as_str());
                self.tell(target.owner, by, false);
                Ok(format!("Teleported {} to {at}", target.name.as_str()))
            }
            Command::Seed => Ok(format!("Seed: {}", self.world.seed())),
            Command::Summon { kind, at } => {
                self.commands
                    .spawn(creature(SavedCreature::grown(kind, at), 0.0));
                Ok(format!("Summoned a {} at {at}", kind.name()))
            }
            Command::GameMode { mode, who } => {
                let target = match who {
                    Some(who) => self.find(&who)?,
                    None => own()?,
                };
                let (.., mut game_mode, _) = self.players.get_mut(target.player).unwrap();
                *game_mode = mode;
                if Some(target.player) == player {
                    return Ok(format!("Game mode set to {}", mode.name()));
                }
                let by = format!("{} set your game mode to {}", name.as_str(), mode.name());
                self.tell(target.owner, by, false);
                let target = target.name.as_str();
                Ok(format!("Set {target}'s game mode to {}", mode.name()))
            }
            Command::Help => Ok(help(op)),
            Command::List => {
                let names = self.players.iter().map(|(_, _, name, ..)| name.as_str());
                let mut names: Vec<_> = names.collect();
                names.sort_unstable_by_key(|name| name.to_lowercase());
                let count = match names.len() {
                    0 => return Ok("Nobody's here".into()),
                    1 => "1 player".into(),
                    n => format!("{n} players"),
                };
                Ok(format!("{count} here: {}", names.join(", ")))
            }
            Command::Message { to, text } => {
                let target = self.find(&to)?;
                if !self.allowances.take(client, self.time.elapsed()) {
                    return Err(TOO_QUICK.into());
                }
                let text = tidy(&text);
                if text.is_empty() {
                    return Err("Usage: /msg <player> <text>".into());
                }
                let whisper = format!("{} whispers to you: {text}", name.as_str());
                self.tell(target.owner, whisper, false);
                Ok(format!("You whisper to {}: {text}", target.name.as_str()))
            }
            Command::Op(who) => {
                let named = PlayerName::new(&who);
                if named.as_str() != who {
                    return Err(format!("{who} isn't a name anyone could have"));
                }
                if self.is_host(&who) || self.operators.contains(&who) {
                    return Err(format!("{who} is already an operator"));
                }
                self.operators.0.push(who.clone());
                self.save_soon();
                if let Ok(target) = self.find(&who) {
                    let made = format!("{} made you an operator", name.as_str());
                    self.tell(target.owner, made, false);
                }
                Ok(format!("Made {who} an operator"))
            }
            Command::Deop(who) => {
                if self.is_host(&who) {
                    return Err(format!("{who} is the host, who's always an operator"));
                }
                let Some(index) = self.operators.position(&who) else {
                    return Err(format!("{who} isn't an operator"));
                };
                self.operators.0.remove(index);
                self.save_soon();
                if let Ok(target) = self.find(&who) {
                    let unmade = format!("{} made you no longer an operator", name.as_str());
                    self.tell(target.owner, unmade, false);
                }
                Ok(format!("{who} is no longer an operator"))
            }
            Command::Corner { corner, at: typed } => {
                Ok(self
                    .authoring
                    .corner(&self.world, author()?, at, corner, typed))
            }
            Command::SaveStructure { name, floor } => {
                let dir = self.save.as_ref().map(|save| save.dir());
                self.authoring
                    .save(&self.world, dir, author()?, &name, floor)
            }
            Command::PlaceStructure { name, rotation } => {
                let dir = self.save.as_ref().map(|save| save.dir());
                let world = &mut self.world;
                self.authoring
                    .place(world, dir, author()?, at, &name, rotation)
            }
            Command::ListStructures => Ok(list(self.save.as_ref().map(|save| save.dir()))),
        }
    }

    // The player here called `name`, ignoring case as names are told apart.
    fn find(&self, name: &str) -> Result<Found, String> {
        let mut players = self.players.iter();
        let player = players.find(|(_, _, other, ..)| other.as_str().eq_ignore_ascii_case(name));
        let (player, ..) = player.ok_or_else(|| format!("No player called {name} is here"))?;
        Ok(self.found(player))
    }

    fn found(&self, player: Entity) -> Found {
        let (player, owner, name, body, ..) = self.players.get(player).unwrap();
        Found {
            player,
            owner: owner.0,
            name: name.clone(),
            at: body.translation,
        }
    }

    fn is_host(&self, name: &str) -> bool {
        self.find(name)
            .is_ok_and(|found| found.owner == ClientId::Server)
    }

    // Puts `player` at `to`. In one App this is the local player's own body, so it moves at
    // once. A remote player decides their own movement, so they're sent a `Teleport`.
    fn teleport(&mut self, player: Entity, to: Vec3) {
        let (_, owner, _, mut body, _, check) = self.players.get_mut(player).unwrap();
        body.translation = to;
        if let Some(mut check) = check {
            check.teleported();
            self.teleports.write(ToClients {
                targets: SendTargets::Single(owner.0),
                message: Teleport { position: to },
            });
        }
    }

    // `world.toml` keeps who's an operator, which shouldn't wait for the next autosave.
    fn save_soon(&mut self) {
        if let Some(save) = &mut self.save {
            save.save_soon();
        }
    }

    fn tell(&mut self, client: ClientId, text: String, error: bool) {
        self.output.write(ToClients {
            targets: SendTargets::Single(client),
            message: CommandOutput { text, error },
        });
    }
}
