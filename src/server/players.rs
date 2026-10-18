use bevy::prelude::*;
use bevy_replicon::prelude::*;

use super::edits::EditAllowances;
use crate::inventory::Inventory;
use crate::player::{arriving, PlayerLook, PlayerOwner};
use crate::protocol::{InventoryChanged, PlayerPose, Welcome};
use crate::save::PlayerState;
use crate::world::World;

/// Welcomes a client the server has let in, its versions matching: spawns its player at the
/// world's spawn, and tells it the seed and where its player is.
pub(super) fn welcome(
    authorized: On<Add<AuthorizedClient>>,
    world: Res<World>,
    mut commands: Commands,
    mut welcomes: MessageWriter<ToClients<Welcome>>,
) {
    let client = ClientId::Client(authorized.entity);
    // where everyone joining arrives until M0.6d saves each player
    let player = PlayerState::arriving_at(world.spawn());
    commands.spawn((arriving(&player), PlayerOwner(client)));
    welcomes.write(ToClients {
        targets: SendTargets::Single(client),
        message: Welcome {
            seed: world.seed(),
            player,
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
