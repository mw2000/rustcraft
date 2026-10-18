//! What clients and the server send each other (§4.4). Blocks travel by name (§4.1).
//!
//! A player's inventory changes only on the server (INV-4): clients ask with `InventoryClick`,
//! `CloseInventory`, `DropItem` and `PickBlock`, and placing a block in survival takes it from
//! the slot `RequestEdit` names. In one App the change is made in the frame it's asked for, to
//! the same `Inventory`; from M0.6 the server replicates each player's to its owner.

use bevy::math::I64Vec3;
use bevy::prelude::*;
use bevy_replicon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::block::BlockType;
use crate::inventory::{Click, Inventory};
use crate::save::PlayerState;
use crate::server::drops::DroppedItem;

/// The port a world is hosted on unless another's asked for.
pub const DEFAULT_PORT: u16 = 25600;

/// The version of what's sent, raised whenever a change to a message would mislead an older
/// game. A client joins only a server with the same one and the same game version.
pub const PROTOCOL_VERSION: u32 = 2;

/// What netcode tells rustcraft's connections apart from other games' by.
pub const NETCODE_PROTOCOL_ID: u64 = u64::from_be_bytes(*b"rustcraf");

/// Registers the messages, on the server and clients alike. Replicon tells messages apart by the
/// order they're registered in, so it's all done here, in one order. Needs `RepliconPlugins`.
pub struct ProtocolPlugin;

impl Plugin for ProtocolPlugin {
    fn build(&self, app: &mut App) {
        // ordered, as edits to one block only make sense in the order they're made, commands
        // and what they print in the order they're typed, and the clock's set in the order
        // it's told
        app.add_client_message::<RequestEdit>(Channel::Ordered)
            .add_server_message::<BlockChanged>(Channel::Ordered)
            .add_server_message::<EditRejected>(Channel::Ordered)
            .add_client_message::<RunCommand>(Channel::Ordered)
            .add_server_message::<CommandOutput>(Channel::Ordered)
            .add_client_message::<WindClock>(Channel::Ordered)
            .add_server_message::<TimeSync>(Channel::Ordered)
            // in order, as each click starts from where the last left the inventory
            .add_client_message::<InventoryClick>(Channel::Ordered)
            .add_client_message::<CloseInventory>(Channel::Ordered)
            .add_client_message::<DropItem>(Channel::Ordered)
            .add_client_message::<PickBlock>(Channel::Ordered)
            .add_server_message::<Welcome>(Channel::Ordered)
            // unreliable, as a pose is soon out of date and the next one follows it
            .add_client_message::<PlayerPose>(Channel::Unreliable)
            .add_server_message::<InventoryChanged>(Channel::Ordered)
            // to every client, which draw them where the server has them
            .replicate::<DroppedItem>()
            .replicate_filtered_as::<Transform, Position, With<DroppedItem>>();
        // A client whose versions differ is refused as it connects, in replicon's check that
        // both sides registered the same messages, and told so with a `ProtocolMismatch`.
        app.world_mut()
            .resource_mut::<ProtocolHasher>()
            .add_custom((env!("CARGO_PKG_VERSION"), PROTOCOL_VERSION));
    }
}

/// A client asking for the block at `block` to become `to`, air to break it, placing it from
/// hotbar slot `slot` in survival. `seq` counts the client's requests, so it can tell which one
/// an answer is to.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestEdit {
    pub block: I64Vec3,
    pub to: BlockType,
    pub slot: u8,
    pub seq: u32,
}

/// The block at `block` has become `to`, by the client and request `by` if a player asked for
/// it.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockChanged {
    pub block: I64Vec3,
    pub to: BlockType,
    #[serde(with = "requester")]
    pub by: Option<(ClientId, u32)>,
}

/// The server's refusal of the request `seq` to change `block`, which is `actual`.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditRejected {
    pub seq: u32,
    pub block: I64Vec3,
    pub actual: BlockType,
}

/// A line a player typed into the command bar, for the server to run as theirs.
#[derive(Message, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RunCommand(pub String);

/// What a command printed, for the player who ran it, and whether it's an error.
#[derive(Message, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub text: String,
    pub error: bool,
}

/// A player winding the clock back (-1) or forward (1), or letting it run at its own pace
/// again (0).
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindClock(pub i8);

/// The server's time of day, in days, and how fast it's running, in days a second.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct TimeSync {
    pub time: f32,
    pub rate: f32,
}

/// A click on inventory slot `slot` in the inventory screen, see `Inventory::click`.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryClick {
    pub slot: u8,
    pub click: Click,
}

/// The inventory screen closing, which puts the stack on the cursor back, or drops what
/// doesn't fit.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseInventory;

/// Throwing one item from inventory slot `slot`, or `all` of its stack.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropItem {
    pub slot: u8,
    pub all: bool,
}

/// A player with every block to hand (creative) putting a stack of `block` in hotbar slot
/// `slot`, from the block picker or by middle-clicking it.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickBlock {
    pub slot: u8,
    pub block: BlockType,
}

/// What a client joining is told as the server lets it in: the world's seed, to generate its
/// terrain from (INV-1), the checksum of what the seed generates, which the client checks its
/// own against (`world_checksum`, D-3), and where its player is.
#[derive(Message, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Welcome {
    pub seed: u32,
    pub checksum: u64,
    pub player: PlayerState,
}

/// A client's player's inventory, as the server has changed it. The host's needs no telling,
/// as it's the server's own.
#[derive(Message, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct InventoryChanged(pub Inventory);

/// Where a client has moved its player to, by its feet, and which way they look, which the
/// client decides (§4.4).
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct PlayerPose {
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
}

// A dropped item's `Transform`, which has no serde of its own, as only its translation: the
// spin is the client's.
#[derive(Serialize, Deserialize)]
struct Position(Vec3);

impl From<Transform> for Position {
    fn from(transform: Transform) -> Self {
        Self(transform.translation)
    }
}

impl From<Position> for Transform {
    fn from(Position(at): Position) -> Self {
        Transform::from_translation(at)
    }
}

// `ClientId` has no serde of its own. A client is its entity on the server, or the server itself
// when it plays too, which goes as no entity.
mod requester {
    use bevy::prelude::Entity;
    use bevy_replicon::prelude::ClientId;
    use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(
        by: &Option<(ClientId, u32)>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        by.map(|(client, seq)| (client.entity().map(Entity::to_bits), seq))
            .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<(ClientId, u32)>, D::Error> {
        let Some((entity, seq)) = Option::<(Option<u64>, u32)>::deserialize(deserializer)? else {
            return Ok(None);
        };
        let client = match entity {
            None => ClientId::Server,
            Some(bits) => ClientId::Client(
                Entity::try_from_bits(bits).ok_or_else(|| D::Error::custom("not an entity"))?,
            ),
        };
        Ok(Some((client, seq)))
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::I64Vec3;
    use bevy::prelude::Entity;
    use bevy_replicon::prelude::ClientId;

    use super::BlockChanged;
    use crate::block::BlockType;

    #[test]
    fn test_block_changes_round_trip_with_whoever_made_them() {
        let client = ClientId::Client(Entity::from_raw_u32(7).unwrap());
        for by in [None, Some((ClientId::Server, 3)), Some((client, 4))] {
            let change = BlockChanged {
                block: I64Vec3::new(-5, 70, 1 << 40),
                to: BlockType::Planks,
                by,
            };
            let bytes = postcard::to_stdvec(&change).unwrap();
            assert_eq!(change, postcard::from_bytes(&bytes).unwrap());
        }
    }
}
