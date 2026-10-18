//! What clients and the server send each other (§4.4). Blocks travel by name (§4.1).
//!
//! A player's inventory changes only on the server (INV-4): clients ask with `InventoryClick`,
//! `CloseInventory`, `DropItem`, `PickBlock` and `Craft`, and placing a block in survival takes
//! it from the slot `RequestEdit` names. In one App the change is made in the frame it's asked
//! for, to the same `Inventory`; from M0.6 the server replicates each player's to its owner.
//! Containers too (§5.2): a player opens one with `UseBlock`, clicks on its slots are
//! `InventoryClick`s naming it, and it's sent to them as `ContainerChanged` while it's open.
//!
//! Players are replicated to everyone but their owner, who moves its own (M0.6d): their name,
//! game mode and what they hold, with where they are and look relayed as `PlayerMoved`.

use std::time::Duration;

use bevy::ecs::entity::MapEntities;
use bevy::math::{I64Vec3, U16Vec3};
use bevy::prelude::*;
use bevy_replicon::prelude::*;
use bevy_replicon_renet2::netcode::NETCODE_USER_DATA_BYTES;
use serde::{Deserialize, Serialize};

use crate::block::BlockType;
use crate::chunks::chunk::{ChunkCoordinate, CELLS, CHUNK_SIZE};
use crate::containers::Container;
use crate::crafting::Recipe;
use crate::game_mode::GameMode;
use crate::inventory::{Click, Inventory};
use crate::player::name::PlayerName;
use crate::player::{Movement, PlayerOwner};
use crate::save::region::{local_block, local_index};
use crate::save::PlayerState;
use crate::server::drops::DroppedItem;
use crate::server::HeldItem;
use crate::survival::Vitals;

/// The port a world is hosted on unless another's asked for.
pub const DEFAULT_PORT: u16 = 25600;

/// The version of what's sent, raised whenever a change to a message would mislead an older
/// game. A client joins only a server with the same one and the same game version.
pub const PROTOCOL_VERSION: u32 = 7;

/// How often a client tells the server where its player is, and the server tells the others
/// where the host's is: about as often as reaching and keeping chunks near them needs, and
/// often enough to draw them moving smoothly.
pub const POSE_INTERVAL: Duration = Duration::from_millis(50);

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
            // after the station it's made at's opened
            .add_client_message::<UseBlock>(Channel::Ordered)
            .add_client_message::<Craft>(Channel::Ordered)
            // before the edit that finishes it
            .add_client_message::<StartBreaking>(Channel::Ordered)
            .add_server_message::<ContainerChanged>(Channel::Ordered)
            // to every client, which draw them where the server has them
            .replicate::<DroppedItem>()
            .replicate_filtered_as::<Transform, Position, With<DroppedItem>>()
            // to the client whose player it is, as the server puts them somewhere and changes
            // how they play
            .add_server_message::<Teleport>(Channel::Ordered)
            .add_server_message::<GameModeChanged>(Channel::Ordered)
            // unreliable, as the poses it's relayed from are
            .add_mapped_server_message::<PlayerMoved>(Channel::Unreliable)
            .replicate::<PlayerName>()
            .replicate::<GameMode>()
            .replicate::<HeldItem>()
            .add_visibility_filter::<PlayerOwner>()
            // in order, as stopping eating only makes sense after starting, and a client's
            // player's health after the damage before it
            .add_client_message::<Eat>(Channel::Ordered)
            .add_client_message::<Respawn>(Channel::Ordered)
            .add_server_message::<VitalsChanged>(Channel::Ordered);
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

/// A click in the inventory screen on slot `slot` of the container at `container`, which the
/// player has open, or of their inventory if it's none, see `Container::click` and
/// `Inventory::click`.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryClick {
    pub container: Option<I64Vec3>,
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

/// Where a client has moved its player to, by its feet, which way they look, how they're
/// moving and which hotbar slot they hold, which the client decides (§4.4) and the server
/// checks. `at` is when, in milliseconds by the client's clock, and `teleports` how many
/// `Teleport`s it had been sent by then, so the server can tell a pose from before its last.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct PlayerPose {
    pub at: u32,
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub movement: Movement,
    pub slot: u8,
    pub teleports: u32,
}

/// Where the server has put a client's player, by its feet, for the client to move them to:
/// somewhere `/tp` sent them, where they respawn, or back where they were if they moved further
/// than they could.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Teleport {
    pub position: Vec3,
}

/// A client's player's game mode, as the server has changed it. The host's needs no telling.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameModeChanged(pub GameMode);

/// Where another player is, by their feet, and which way they look, at `at` milliseconds by
/// their own clock, which only tells their poses apart and how far apart they are.
#[derive(Message, MapEntities, Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct PlayerMoved {
    #[entities]
    pub player: Entity,
    pub at: u32,
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
}

/// A player's name as a client sends it on connecting, in netcode's user data.
pub fn name_to_user_data(name: &PlayerName) -> [u8; NETCODE_USER_DATA_BYTES] {
    let mut data = [0; NETCODE_USER_DATA_BYTES];
    let bytes = name.as_str().as_bytes();
    data[..bytes.len()].copy_from_slice(bytes);
    data
}

/// The name a client sent on connecting, made a name whatever it sent.
pub fn name_from_user_data(data: &[u8; NETCODE_USER_DATA_BYTES]) -> PlayerName {
    let end = data
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(data.len());
    PlayerName::new(&String::from_utf8_lossy(&data[..end]))
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

/// A player using the block at `block` by right-clicking it, as opening a crafting table or a
/// furnace does, which stays open for them until they close the inventory screen.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct UseBlock {
    pub block: I64Vec3,
}

/// Making `recipe` once, or `all` the times the ingredients carried allow, into the inventory.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Craft {
    pub recipe: Recipe,
    pub all: bool,
}

/// What the container at `at`, which the player has open, holds now, or none once it's gone or
/// out of their reach, which closes it.
#[derive(Message, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ContainerChanged {
    pub at: I64Vec3,
    pub container: Option<Container>,
}

/// A player in survival starting to break the block at `block`, which the server times, so it
/// can refuse a `RequestEdit` breaking it sooner than it takes.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartBreaking {
    pub block: I64Vec3,
}

/// A player starting to eat what's in hotbar slot `slot`, as they hold right click with food
/// in it, or stopping, as `None`, as they let go. The server times it, as it does breaking.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Eat(pub Option<u8>);

/// A dead player asking to live again, from the death screen.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Respawn;

/// A client's player's health, food and breath, as the server has changed them, and whether
/// they're asleep. The host's needs no telling, as they're the server's own.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct VitalsChanged {
    pub vitals: Vitals,
    pub asleep: bool,
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

    use super::{name_from_user_data, name_to_user_data, BlockChanged, ChunkEdits};
    use crate::block::BlockType;
    use crate::chunks::chunk::ChunkCoordinate;
    use crate::player::name::PlayerName;

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
    fn test_names_go_in_the_user_data_and_come_out_names() {
        let name = PlayerName::new("Zoë");
        assert_eq!(name, name_from_user_data(&name_to_user_data(&name)));
        let mut sent = [b'/'; 256];
        sent[..4].copy_from_slice(b"..ab");
        assert_eq!("ab", name_from_user_data(&sent).as_str());
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
