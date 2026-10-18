use bevy::prelude::*;
use bevy::tasks::{block_on, AsyncComputeTaskPool, Task};
use bevy_replicon::prelude::*;

use super::edits::EditAllowances;
use super::sync::SyncedArea;
use crate::chunks::generate::checksum::world_checksum;
use crate::inventory::Inventory;
use crate::player::{arriving, PlayerLook, PlayerOwner};
use crate::protocol::{InventoryChanged, PlayerPose, Welcome};
use crate::save::PlayerState;
use crate::world::World;

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

/// Welcomes a client the server has let in, its versions matching: spawns its player at the
/// world's spawn, tells it the seed, its checksum and where its player is, and starts keeping
/// its edits in step round its player.
pub(super) fn welcome(
    authorized: On<Add<AuthorizedClient>>,
    world: Res<World>,
    mut checksum: ResMut<WorldChecksum>,
    mut commands: Commands,
    mut welcomes: MessageWriter<ToClients<Welcome>>,
) {
    let client = ClientId::Client(authorized.entity);
    // where everyone joining arrives until M0.6d saves each player
    let player = PlayerState::arriving_at(world.spawn());
    commands.spawn((arriving(&player), PlayerOwner(client)));
    commands
        .entity(authorized.entity)
        .insert(SyncedArea::default());
    welcomes.write(ToClients {
        targets: SendTargets::Single(client),
        message: Welcome {
            seed: world.seed(),
            checksum: checksum.get(),
            player,
            you: client,
        },
    });
    info!("{client} joined");
}

/// Sees a client off as it disconnects, despawning its player and forgetting its edits.
pub(super) fn see_off(
    gone: On<Remove<ConnectedClient>>,
    players: Query<(Entity, &PlayerOwner)>,
    mut allowances: ResMut<EditAllowances>,
    mut commands: Commands,
) {
    let client = ClientId::Client(gone.entity);
    allowances.forget(client);
    for (player, owner) in &players {
        if owner.0 == client {
            commands.entity(player).despawn();
            info!("{client} left");
        }
    }
}

/// Moves players where their clients say they've gone, as each decides its own movement.
pub(super) fn move_players(
    mut poses: MessageReader<FromClient<PlayerPose>>,
    mut players: Query<(&PlayerOwner, &mut Transform, &mut PlayerLook)>,
) {
    for FromClient { client_id, message } in poses.read() {
        let player = players.iter_mut().find(|(owner, ..)| owner.0 == *client_id);
        if let Some((_, mut body, mut look)) = player {
            body.translation = message.position;
            *look = PlayerLook {
                yaw: message.yaw,
                pitch: message.pitch,
            };
        }
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
