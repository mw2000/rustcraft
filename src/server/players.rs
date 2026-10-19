use std::time::Duration;

use bevy::math::I64Vec3;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::tasks::{block_on, AsyncComputeTaskPool, Task};
use bevy_replicon::prelude::*;
use bevy_replicon::shared::backend::connected_client::NetworkId;
use bevy_replicon_renet2::netcode::NetcodeServerTransport;
use serde::{Deserialize, Serialize};

use super::chat::ChatAllowances;
use super::commands::CONSOLE_NAME;
use super::edits::EditAllowances;
use super::sync::SyncedArea;
use crate::chunks::chunk::ChunkCoordinate;
use crate::chunks::generate::checksum::world_checksum;
use crate::collision::overlaps;
use crate::game_mode::GameMode;
use crate::inventory::Hotbar;
use crate::inventory::{Inventory, HOTBAR_SLOTS};
use crate::items::Item;
use crate::player::name::PlayerName;
use crate::player::physics::top_speed;
use crate::player::{arriving, Movement, Player, PlayerLook, PlayerOwner, BODY};
use crate::protocol::{
    name_from_user_data, CommandOutput, GameModeChanged, InventoryChanged, PlayerMoved, PlayerPose,
    PlayerSwung, SetSkin, Swing, Teleport, Welcome,
};
use crate::save::{PlayerState, WorldSave};
use crate::skin::Skin;
use crate::survival::{SpawnBed, Vitals};
use crate::world::World;

// How far ahead of the server's clock a client's movement can get, in seconds of moving at its
// top speed: a pose held up on its way by this long, then arriving with the next, is still let
// through, which covers a network's jitter and a frame's hitch on either side. Over this, the
// player's put back. It never lets a client go faster than its top speed for longer.
const POSE_SLACK: f32 = 0.5;
// And a little over its top speed, for the rounding in its clock and steps.
const SPEED_SLACK: f32 = 1.1;

/// What a player holds in their hand, for others to see.
#[derive(Component, Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeldItem(pub Option<Item>);

/// The armour a player wears, helmet first, for others to see.
#[derive(Component, Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Worn(pub [Option<Item>; 4]);

/// A player as the server spawns them, to everyone but their owner a replicated entity, in
/// the skin they were saved wearing until their client tells it theirs.
pub(super) fn spawn_player(
    commands: &mut Commands,
    save: &WorldSave,
    state: &PlayerState,
    owner: ClientId,
    name: PlayerName,
) -> Entity {
    let mut player = commands.spawn((
        arriving(state),
        PlayerOwner(owner),
        name,
        HeldItem::default(),
        Worn::default(),
        Replicated,
    ));
    if let Some(skin) = save.skin(state) {
        player.insert(skin);
    }
    player.id()
}

// A player's shown to every client but its owner's, which moves its own.
impl VisibilityFilter for PlayerOwner {
    type ClientComponent = AuthorizedClient;
    type Scope = Entity;

    fn is_visible(&self, client: Entity, _: Option<&AuthorizedClient>) -> bool {
        self.0 != ClientId::Client(client)
    }
}

/// How a client's player's poses are checked: how far ahead of the server's clock its movement
/// has got, in seconds at its top speed, as of when, and how many times it's been teleported.
#[derive(Component)]
pub(super) struct PoseCheck {
    ahead: f32,
    at: Duration,
    teleports: u32,
}

impl PoseCheck {
    /// Counts a teleport, after which poses sent before the client heard of it are ignored.
    pub(super) fn teleported(&mut self) {
        self.teleports += 1;
    }
}

/// The checksum of what the world's seed generates, for clients to check theirs against (D-3).
/// Worked out on a task as the world opens, which is done long before anyone joins.
#[derive(Resource)]
pub(super) enum WorldChecksum {
    Working(Task<u64>),
    Done(u64),
}

impl WorldChecksum {
    pub(super) fn of(seed: u32) -> Self {
        Self::Working(AsyncComputeTaskPool::get().spawn(async move { world_checksum(seed) }))
    }

    // Waits for the task if it's still going, which it only is for a client joining just as
    // the world's opened, as in tests.
    fn get(&mut self) -> u64 {
        let checksum = match self {
            Self::Working(task) => block_on(task),
            Self::Done(checksum) => *checksum,
        };
        *self = Self::Done(checksum);
        checksum
    }
}

/// Welcomes a client the server has let in, its versions matching: spawns its player by the
/// name it sent, made unique, where they were saved or else at the world's spawn, tells it the
/// seed, its checksum and where its player is, starts keeping its edits in step round its
/// player, and tells everyone they've joined.
#[allow(clippy::too_many_arguments)]
pub(super) fn welcome(
    authorized: On<Add<AuthorizedClient>>,
    world: Res<World>,
    save: Res<WorldSave>,
    time: Res<Time>,
    mut checksum: ResMut<WorldChecksum>,
    transport: Option<Res<NetcodeServerTransport>>,
    clients: Query<&NetworkId>,
    names: Query<&PlayerName>,
    mut commands: Commands,
    mut welcomes: MessageWriter<ToClients<Welcome>>,
    mut output: MessageWriter<ToClients<CommandOutput>>,
) {
    let client = ClientId::Client(authorized.entity);
    let sent = transport
        .zip(clients.get(authorized.entity).ok())
        .and_then(|(transport, id)| transport.user_data(id.get()));
    // and never the console's, so no one can speak as the server
    let console = PlayerName::new(CONSOLE_NAME);
    let name = sent
        .map_or_else(|| PlayerName::new(""), |data| name_from_user_data(&data))
        .unique_among(names.iter().chain([&console]));
    let player = save
        .player(&name)
        .unwrap_or_else(|| PlayerState::arriving_at(world.spawn(), save.new_player_mode()));
    let check = PoseCheck {
        ahead: 0.0,
        at: time.elapsed(),
        teleports: 0,
    };
    let spawned = spawn_player(&mut commands, &save, &player, client, name.clone());
    commands.entity(spawned).insert(check);
    commands
        .entity(authorized.entity)
        .insert(SyncedArea::default());
    welcomes.write(ToClients {
        targets: SendTargets::Single(client),
        message: Welcome {
            seed: world.seed(),
            checksum: checksum.get(),
            player,
            name: name.clone(),
            you: client,
        },
    });
    announce(&mut output, format!("{} joined the game", name.as_str()));
    info!("{client} joined as {}", name.as_str());
}

type Leaving<'a> = (
    Entity,
    &'a PlayerOwner,
    &'a PlayerName,
    &'a Transform,
    &'a PlayerLook,
    &'a Player,
    &'a GameMode,
    &'a Hotbar,
    &'a Inventory,
    &'a Vitals,
    &'a SpawnBed,
    Option<&'a Skin>,
);

/// Sees a client off as it disconnects: keeps its player's state to save, despawns them, which
/// takes them off everyone's screen, forgets its allowances and tells everyone they've left.
pub(super) fn see_off(
    gone: On<Remove<ConnectedClient>>,
    players: Query<Leaving>,
    mut save: ResMut<WorldSave>,
    mut allowances: ResMut<EditAllowances>,
    mut chat: ResMut<ChatAllowances>,
    mut output: MessageWriter<ToClients<CommandOutput>>,
    mut commands: Commands,
) {
    let client = ClientId::Client(gone.entity);
    allowances.forget(client);
    chat.forget(client);
    let leaving = players.iter().find(|(_, owner, ..)| owner.0 == client);
    if let Some((player, _, name, body, look, state, mode, hotbar, inventory, vitals, bed, skin)) =
        leaving
    {
        let state = PlayerState::of(body, *look, state.movement, *mode, *hotbar, inventory)
            .surviving(*vitals, *bed)
            .wearing(skin);
        save.left(name, state, skin);
        commands.entity(player).despawn();
        announce(&mut output, format!("{} left the game", name.as_str()));
        info!("{client} left");
    }
}

/// Tells everyone `text` in the command bar.
pub(super) fn announce(output: &mut MessageWriter<ToClients<CommandOutput>>, text: String) {
    output.write(ToClients {
        targets: SendTargets::All,
        message: CommandOutput { text, error: false },
    });
}

type Moving<'a> = (
    Entity,
    &'a PlayerOwner,
    &'a GameMode,
    &'a mut Player,
    &'a mut Transform,
    &'a mut PlayerLook,
    &'a mut Hotbar,
    &'a mut PoseCheck,
);

/// Moves players where their clients say they've gone, as each decides its own movement, and
/// tells everyone else. A pose further than the player could have gone since the last, or
/// walking into a solid block, is refused, and the client told to put its player back where
/// the server has them.
pub(super) fn move_players(
    mut poses: MessageReader<FromClient<PlayerPose>>,
    world: Res<World>,
    time: Res<Time>,
    mut players: Query<Moving>,
    mut teleports: MessageWriter<ToClients<Teleport>>,
    mut moved: MessageWriter<ToClients<PlayerMoved>>,
) {
    for &FromClient {
        client_id: client,
        message: pose,
    } in poses.read()
    {
        let player = players.iter_mut().find(|(_, owner, ..)| owner.0 == client);
        let Some((player, _, mode, mut state, mut body, mut look, mut hotbar, mut check)) = player
        else {
            continue;
        };
        // sent before the client heard where it was last put, so from where it no longer is
        if pose.teleports < check.teleports {
            continue;
        }
        let now = time.elapsed();
        check.ahead = (check.ahead - (now - check.at).as_secs_f32()).max(0.0);
        check.at = now;
        // only flying where the game mode allows it, as a client may not have heard it's
        // changed yet
        let movement = match pose.movement {
            Movement::Fly if mode.can_fly() => Movement::Fly,
            _ => Movement::Walk,
        };
        let took = time_to_move(body.translation, pose.position, movement);
        let into_solid = movement == Movement::Walk
            && in_solid(&world, pose.position)
            && !in_solid(&world, body.translation);
        if check.ahead + took > POSE_SLACK || into_solid {
            debug!("put {client} back, as it moved further than it could");
            check.teleported();
            teleports.write(ToClients {
                targets: SendTargets::Single(client),
                message: Teleport {
                    position: body.translation,
                },
            });
            continue;
        }
        check.ahead += took;
        body.translation = pose.position;
        *look = PlayerLook {
            yaw: pose.yaw,
            pitch: pose.pitch,
        };
        state.movement = movement;
        let selected = usize::from(pose.slot).min(HOTBAR_SLOTS - 1);
        hotbar.set_if_neq(Hotbar { selected });
        moved.write(ToClients {
            targets: SendTargets::AllExcept(client),
            message: PlayerMoved {
                player,
                at: pose.at,
                position: pose.position,
                yaw: pose.yaw,
                pitch: pose.pitch,
            },
        });
    }
}

// How long, in seconds, a player moving with `movement` takes at the least to go from `from` to
// `to`, each way they can go at its own top speed.
fn time_to_move(from: Vec3, to: Vec3, movement: Movement) -> f32 {
    let (across, up, down) = top_speed(movement);
    let moved = to - from;
    let times = [
        moved.xz().length() / across,
        moved.y.max(0.0) / up,
        (-moved.y).max(0.0) / down,
    ];
    times.into_iter().fold(0.0, f32::max) / SPEED_SLACK
}

// Whether a body with its feet at `feet` is in a solid block, of those loaded.
fn in_solid(world: &World, feet: Vec3) -> bool {
    let (min, max) = (feet + BODY.min, feet + BODY.max);
    let (min, max) = (min.floor().as_i64vec3(), max.floor().as_i64vec3());
    (min.x..=max.x).any(|x| {
        (min.y..=max.y).any(|y| {
            (min.z..=max.z).any(|z| {
                let block = I64Vec3::new(x, y, z);
                world.is_chunk_generated(ChunkCoordinate::containing(block))
                    && world.get_block_at(block).is_solid()
                    && overlaps(BODY, feet, block)
            })
        })
    })
}

// How often a player's swings are passed on at most, as an arm takes about this long to
// swing.
const SWING_INTERVAL: Duration = Duration::from_millis(150);

/// Passes each player's swings on to everyone else, no more often than an arm swings.
pub(super) fn relay_swings(
    mut swings: MessageReader<FromClient<Swing>>,
    time: Res<Time>,
    players: Query<(Entity, &PlayerOwner)>,
    mut last: Local<HashMap<ClientId, Duration>>,
    mut swung: MessageWriter<ToClients<PlayerSwung>>,
) {
    let now = time.elapsed();
    for &FromClient { client_id, .. } in swings.read() {
        let Some((player, _)) = players.iter().find(|(_, owner)| owner.0 == client_id) else {
            continue;
        };
        if last
            .get(&client_id)
            .is_some_and(|&at| now < at + SWING_INTERVAL)
        {
            continue;
        }
        last.insert(client_id, now);
        swung.write(ToClients {
            targets: SendTargets::AllExcept(client_id),
            message: PlayerSwung { player },
        });
    }
}

/// Puts on each player the skin their client's chosen, which was checked as it was read, or
/// takes theirs off for the default skin, and so shows it to everyone else.
pub(super) fn receive_skins(
    mut skins: MessageReader<FromClient<SetSkin>>,
    players: Query<(Entity, &PlayerOwner, Option<&Skin>)>,
    mut commands: Commands,
) {
    for FromClient { client_id, message } in skins.read() {
        let Some((player, _, worn)) = players.iter().find(|(_, owner, _)| owner.0 == *client_id)
        else {
            continue;
        };
        match &message.0 {
            Some(skin) if worn != Some(skin) => {
                commands.entity(player).insert(skin.clone());
            }
            None if worn.is_some() => {
                commands.entity(player).remove::<Skin>();
            }
            _ => {}
        }
    }
}

/// Tells everyone but the host where the host's player is, as often as clients tell theirs.
pub(super) fn send_host_pose(
    time: Res<Time>,
    players: Query<(Entity, &PlayerOwner, &Transform, &PlayerLook)>,
    mut moved: MessageWriter<ToClients<PlayerMoved>>,
) {
    for (player, owner, body, look) in &players {
        if owner.0 == ClientId::Server {
            moved.write(ToClients {
                targets: SendTargets::CLIENTS_ONLY,
                message: PlayerMoved {
                    player,
                    at: time.elapsed().as_millis() as u32,
                    position: body.translation,
                    yaw: look.yaw,
                    pitch: look.pitch,
                },
            });
        }
    }
}

/// Keeps what each player holds and wears up to date for others to see, as their inventory or
/// the slot they've selected changes.
#[allow(clippy::type_complexity)]
pub(super) fn hold_items(
    mut players: Query<
        (&Inventory, &Hotbar, &mut HeldItem, &mut Worn),
        Or<(Changed<Inventory>, Changed<Hotbar>)>,
    >,
) {
    for (inventory, hotbar, mut held, mut worn) in &mut players {
        held.set_if_neq(HeldItem(hotbar.held(inventory).map(|stack| stack.item)));
        worn.set_if_neq(Worn(inventory.armour.map(|stack| stack.map(|s| s.item))));
    }
}

/// Tells each client its player's inventory whenever the server changes it.
pub(super) fn send_inventories(
    players: Query<(&PlayerOwner, &Inventory), Changed<Inventory>>,
    mut changes: MessageWriter<ToClients<InventoryChanged>>,
) {
    for (owner, inventory) in &players {
        if owner.0 != ClientId::Server {
            changes.write(ToClients {
                targets: SendTargets::Single(owner.0),
                message: InventoryChanged(inventory.clone()),
            });
        }
    }
}

/// Tells each client its player's game mode whenever the server changes it.
pub(super) fn send_game_modes(
    players: Query<(&PlayerOwner, &GameMode), Changed<GameMode>>,
    mut changes: MessageWriter<ToClients<GameModeChanged>>,
) {
    for (owner, mode) in &players {
        if owner.0 != ClientId::Server {
            changes.write(ToClients {
                targets: SendTargets::Single(owner.0),
                message: GameModeChanged(*mode),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::prelude::*;
    use bevy::time::TimeUpdateStrategy;
    use bevy_replicon::prelude::{ClientId, FromClient, SendTargets, ToClients};

    use super::{relay_swings, time_to_move};
    use crate::player::{Movement, PlayerOwner};
    use crate::protocol::{PlayerSwung, Swing};

    #[test]
    fn test_swings_are_passed_on_to_everyone_else_as_often_as_an_arm_swings() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                50,
            )))
            .add_message::<FromClient<Swing>>()
            .add_message::<ToClients<PlayerSwung>>()
            .add_systems(Update, relay_swings);
        let guest = ClientId::Client(Entity::from_raw_u32(9).unwrap());
        let player = app.world_mut().spawn(PlayerOwner(guest)).id();
        let mut relayed = Vec::new();
        for _ in 0..10 {
            for _ in 0..3 {
                app.world_mut().write_message(FromClient {
                    client_id: guest,
                    message: Swing,
                });
            }
            app.update();
            let mut sent = app
                .world_mut()
                .resource_mut::<Messages<ToClients<PlayerSwung>>>();
            relayed.extend(sent.drain());
        }
        // half a second's worth, one in every 150 ms
        assert!((3..=4).contains(&relayed.len()), "{}", relayed.len());
        for swung in relayed {
            assert_eq!(PlayerSwung { player }, swung.message);
            assert!(matches!(swung.targets, SendTargets::AllExcept(client) if client == guest));
        }
    }

    #[test]
    fn test_walking_is_held_to_walking_speed_and_flying_to_flying() {
        let second = |to: Vec3, movement| time_to_move(Vec3::ZERO, to, movement);
        // a second's sprint either way is let through, and much more isn't
        assert!(second(Vec3::new(5.0, 0.0, 0.0), Movement::Walk) < 1.0);
        assert!(second(Vec3::new(10.0, 0.0, 0.0), Movement::Walk) > 1.5);
        assert!(second(Vec3::new(80.0, 80.0, 80.0), Movement::Fly) < 1.0);
        assert!(second(Vec3::new(160.0, 0.0, 0.0), Movement::Fly) > 1.2);
        // falling is quicker than jumping
        assert!(second(Vec3::new(0.0, -40.0, 0.0), Movement::Walk) < 1.0);
        assert!(second(Vec3::new(0.0, 20.0, 0.0), Movement::Walk) > 1.5);
    }
}
