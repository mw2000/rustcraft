//! What clients and the server send each other (§4.4). Blocks travel by name (§4.1).
//!
//! A player's inventory changes only on the server (INV-4): clients ask with `InventoryClick`,
//! `CloseInventory`, `DropItem` and `PickBlock`, and placing a block in survival takes it from
//! the slot `RequestEdit` names. In one App the change is made in the frame it's asked for, to
//! the same `Inventory`; from M0.6 the server replicates each player's to its owner.

use bevy::math::{I64Vec3, U16Vec3};
use bevy::prelude::*;
use bevy_replicon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::block::BlockType;
use crate::chunks::chunk::{ChunkCoordinate, CELLS, CHUNK_SIZE};
use crate::inventory::{Click, Inventory};
use crate::save::region::{local_block, local_index};
use crate::save::PlayerState;
use crate::server::drops::DroppedItem;

/// The port a world is hosted on unless another's asked for.
pub const DEFAULT_PORT: u16 = 25600;

/// The version of what's sent, raised whenever a change to a message would mislead an older
/// game. A client joins only a server with the same one and the same game version.
pub const PROTOCOL_VERSION: u32 = 3;

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
            .add_server_message::<ChunkEdits>(Channel::Ordered)
            .add_client_message::<EditReach>(Channel::Ordered)
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
/// own against (`world_checksum`, D-3), where its player is, and who it is to the server, to
/// tell its own edits by.
#[derive(Message, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Welcome {
    pub seed: u32,
    pub checksum: u64,
    pub player: PlayerState,
    #[serde(with = "client_id")]
    pub you: ClientId,
}

/// How far from its player, in chunks, a client draws blocks at full detail, and so wants the
/// edits of the chunks that far, told as it's welcomed.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditReach(pub u32);

/// The edits made in a chunk, for a client coming near it, as a region file has them: each
/// block as where it is in the chunk and an index into a palette of the blocks edited to, so
/// each block's name is sent once. With how many `BlockChanged`s the client had been sent
/// before, as either can overtake the other on its own channel, so the client can tell which
/// is newer.
#[derive(Message, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ChunkEdits {
    coord: I64Vec3,
    changes_before: u64,
    palette: Vec<BlockType>,
    edits: Vec<(u16, u16)>,
}

impl ChunkEdits {
    pub fn new(
        coord: ChunkCoordinate,
        changes_before: u64,
        edits: impl IntoIterator<Item = (U16Vec3, BlockType)>,
    ) -> Self {
        let mut palette = Vec::new();
        let mut indices = [None; BlockType::ALL.len()];
        let edits = edits
            .into_iter()
            .map(|(local, block)| {
                let index = *indices[block as usize].get_or_insert_with(|| {
                    palette.push(block);
                    palette.len() as u16 - 1
                });
                (local_index(local), index)
            })
            .collect();
        Self {
            coord: coord.0,
            changes_before,
            palette,
            edits,
        }
    }

    pub fn coord(&self) -> ChunkCoordinate {
        ChunkCoordinate(self.coord)
    }

    pub fn changes_before(&self) -> u64 {
        self.changes_before
    }

    pub fn edit_count(&self) -> usize {
        self.edits.len()
    }

    /// Each edit, as the block edited and what it's become. Any that aren't in a chunk or the
    /// palette, which a server never sends, are left out.
    pub fn edits(&self) -> impl Iterator<Item = (I64Vec3, BlockType)> + '_ {
        let origin = self.coord * CHUNK_SIZE as i64;
        self.edits.iter().filter_map(move |&(local, index)| {
            let block = *self.palette.get(index as usize)?;
            let local = (usize::from(local) < CELLS).then(|| local_block(local))?;
            Some((origin + local.as_i64vec3(), block))
        })
    }
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
mod client_id {
    use bevy::prelude::Entity;
    use bevy_replicon::prelude::ClientId;
    use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(client: &ClientId, serializer: S) -> Result<S::Ok, S::Error> {
        client.entity().map(Entity::to_bits).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<ClientId, D::Error> {
        Ok(match Option::<u64>::deserialize(deserializer)? {
            None => ClientId::Server,
            Some(bits) => ClientId::Client(
                Entity::try_from_bits(bits).ok_or_else(|| D::Error::custom("not an entity"))?,
            ),
        })
    }
}

// A request, as the client making it and its `seq`.
mod requester {
    use bevy_replicon::prelude::ClientId;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Requester(#[serde(with = "super::client_id")] ClientId, u32);

    pub fn serialize<S: Serializer>(
        by: &Option<(ClientId, u32)>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        by.map(|(client, seq)| Requester(client, seq))
            .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<(ClientId, u32)>, D::Error> {
        let by = Option::<Requester>::deserialize(deserializer)?;
        Ok(by.map(|Requester(client, seq)| (client, seq)))
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::{I64Vec3, U16Vec3};
    use bevy::prelude::Entity;
    use bevy_replicon::prelude::ClientId;

    use super::{BlockChanged, ChunkEdits};
    use crate::block::BlockType;
    use crate::chunks::chunk::ChunkCoordinate;

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

    #[test]
    fn test_a_chunks_edits_round_trip_naming_each_block_once() {
        let coord = ChunkCoordinate(I64Vec3::new(-3, 28, 1 << 40));
        let edits = [
            (U16Vec3::new(0, 0, 0), BlockType::Air),
            (U16Vec3::new(15, 15, 15), BlockType::Planks),
            (U16Vec3::new(1, 2, 3), BlockType::Air),
        ];
        let sent = ChunkEdits::new(coord, 7, edits);
        let bytes = postcard::to_stdvec(&sent).unwrap();
        let received: ChunkEdits = postcard::from_bytes(&bytes).unwrap();
        let origin = coord.0 * 16;
        let expected = edits.map(|(local, block)| (origin + local.as_i64vec3(), block));
        assert!(received.edits().eq(expected));
        assert_eq!((coord, 7), (received.coord(), received.changes_before()));
        // a few bytes an edit, and a name a block
        assert!(bytes.len() < 40, "{} bytes", bytes.len());
    }
}
