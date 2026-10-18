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
//! game mode and what they hold, with where they are and look relayed as `PlayerMoved`, and
//! their arm's swings as they break and place as `PlayerSwung` (M-A2).
//!
//! Creatures are replicated to the clients whose players are near them (M3.1): their kind,
//! their pose, which the server sets 20 times a second as they move, how many times they've
//! been hurt, and whether they're young, shorn or in love. Clients hit them with `HitCreature`
//! and feed or shear them with `UseCreature` (M3.2).

use std::time::Duration;

use bevy::ecs::entity::MapEntities;
use bevy::math::{I64Vec3, U16Vec3};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy_replicon::prelude::*;
use bevy_replicon_renet2::netcode::NETCODE_USER_DATA_BYTES;
use serde::{Deserialize, Serialize};

use crate::authoring::Selection;
use crate::block::{Block, BlockType};
use crate::chunks::chunk::{ChunkCoordinate, CELLS, CHUNK_SIZE};
use crate::chunks::lod::CellEdits;
use crate::containers::Container;
use crate::crafting::Recipe;
use crate::creatures::{Condition, Creature, CreaturePose, Flinches};
use crate::game_mode::GameMode;
use crate::inventory::{Click, Inventory};
use crate::items::Item;
use crate::player::name::PlayerName;
use crate::player::{Movement, PlayerOwner};
use crate::save::region::{local_block, local_index};
use crate::save::PlayerState;
use crate::server::creatures::Whereabouts;
use crate::server::drops::DroppedItem;
use crate::server::HeldItem;
use crate::survival::Vitals;

/// The port a world is hosted on unless another's asked for.
pub const DEFAULT_PORT: u16 = 25600;

/// The version of what's sent, raised whenever a change to a message would mislead an older
/// game. A client joins only a server with the same one and the same game version.
pub const PROTOCOL_VERSION: u32 = 18;

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
            // in order, as each cell is told as it is now, so the last told is the server's
            .add_server_message::<EditSummary>(Channel::Ordered)
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
            .add_server_message::<VitalsChanged>(Channel::Ordered)
            // in the order they're said
            .add_client_message::<Chat>(Channel::Ordered)
            .add_server_message::<ChatLine>(Channel::Ordered)
            // in the order the commands changing it are run
            .add_server_message::<SelectionChanged>(Channel::Ordered)
            // to the clients near them, which draw them between their poses (§5.8)
            .replicate::<Creature>()
            .replicate::<CreaturePose>()
            .replicate::<Flinches>()
            .replicate::<Condition>()
            .add_visibility_filter::<Whereabouts>()
            .add_mapped_client_message::<HitCreature>(Channel::Ordered)
            .add_mapped_client_message::<UseCreature>(Channel::Ordered)
            // unreliable, as a swing missed is soon followed by another
            .add_client_message::<Swing>(Channel::Unreliable)
            .add_mapped_server_message::<PlayerSwung>(Channel::Unreliable);
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
    pub to: Block,
    pub slot: u8,
    pub seq: u32,
}

/// The block at `block` has become `to`, by the client and request `by` if a player asked for
/// it.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockChanged {
    pub block: I64Vec3,
    pub to: Block,
    #[serde(with = "requester")]
    pub by: Option<(ClientId, u32)>,
}

/// The server's refusal of the request `seq` to change `block`, which is `actual`.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditRejected {
    pub seq: u32,
    pub block: I64Vec3,
    pub actual: Block,
}

/// A line a player typed into the command bar, for the server to run as theirs.
#[derive(Message, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct RunCommand(pub String);

/// A line for a player's command bar, and whether it's an error: what a command they ran
/// printed, a private message, or news for everyone, such as someone joining.
#[derive(Message, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub text: String,
    pub error: bool,
}

/// The longest a chat line can be, in characters. The server cuts a longer one short, and the
/// command bar takes no more.
pub const MAX_CHAT_LENGTH: usize = 256;

/// A line a player said in chat, for the server to pass on to everyone.
#[derive(Message, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Chat(pub String);

/// What a player said in chat, as the server passes it on to everyone.
#[derive(Message, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ChatLine {
    pub from: PlayerName,
    pub text: String,
}

/// A player winding the clock back (-1) or forward (1), or letting it run at its own pace
/// again (0).
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindClock(pub i8);

/// The server's time of day, in days, and how fast it's running, in days a second.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct TimeSync {
    pub time: f64,
    pub rate: f64,
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

/// A player with every block to hand (creative) putting a stack of `item` in hotbar slot
/// `slot`, from the block picker or by middle-clicking a block: a block, or seeds, which place
/// one. Sent by name, so it's as it was when it was only ever a block.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickBlock {
    pub slot: u8,
    pub item: Item,
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
/// edits of the chunks that far, and how far it draws at all, and so wants their LOD edits
/// (`EditSummary`), told as it's welcomed.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditReach {
    pub full_detail: u32,
    pub view: u32,
}

/// Edits as LOD nodes draw them (§4.2), for a client to draw the builds it sees but hasn't the
/// edits of: some of the 2-block cells edits have changed, by chunk, each as where it is in its
/// chunk, an index into a palette of its commonest block built, and how many of its 8 blocks are
/// built and cleared, a nibble each. Every cell goes as it is when it's sent, so a client
/// putting them in as they come in order ends up with the server's.
#[derive(Message, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct EditSummary {
    palette: Vec<BlockType>,
    chunks: Vec<(I64Vec3, Vec<SummaryCell>)>,
}

// A cell of an `EditSummary`: where it is in its chunk, its block's index in the palette, and its
// counts.
type SummaryCell = (u16, u8, u8);

// A chunk is this many 2-block cells across.
const SUMMARY_CELLS: i64 = CHUNK_SIZE as i64 / 2;

impl EditSummary {
    /// `cells`, each 2-block cell as level 1 counts them, and what edits have made of it.
    pub fn new(cells: impl IntoIterator<Item = (I64Vec3, CellEdits)>) -> Self {
        let mut palette = Vec::new();
        let mut indices = [None; BlockType::ALL.len()];
        let mut chunks: Vec<(I64Vec3, Vec<SummaryCell>)> = Vec::new();
        let mut chunk_index = HashMap::new();
        for (cell, edits) in cells {
            let block = *indices[edits.block as usize].get_or_insert_with(|| {
                palette.push(edits.block);
                palette.len() as u8 - 1
            });
            let coord = cell.div_euclid(I64Vec3::splat(SUMMARY_CELLS));
            let local = cell - coord * SUMMARY_CELLS;
            let at = ((local.x * SUMMARY_CELLS + local.y) * SUMMARY_CELLS + local.z) as u16;
            let counts = (edits.built as u8) << 4 | edits.cleared as u8;
            let chunk = *chunk_index.entry(coord).or_insert_with(|| {
                chunks.push((coord, Vec::new()));
                chunks.len() - 1
            });
            chunks[chunk].1.push((at, block, counts));
        }
        Self { palette, chunks }
    }

    pub fn cell_count(&self) -> usize {
        self.chunks.iter().map(|(_, cells)| cells.len()).sum()
    }

    /// Each cell, as level 1 counts them, and what edits have made of it. Any a server never
    /// sends, out of its chunk or the palette or with more than 8 blocks, are left out.
    pub fn cells(&self) -> impl Iterator<Item = (I64Vec3, CellEdits)> + '_ {
        self.chunks.iter().flat_map(move |(coord, cells)| {
            cells.iter().filter_map(move |&(at, block, counts)| {
                let (built, cleared) = ((counts >> 4) as u32, (counts & 15) as u32);
                let at = at as i64;
                if at >= SUMMARY_CELLS.pow(3) || built + cleared > 8 {
                    return None;
                }
                let local = I64Vec3::new(
                    at / SUMMARY_CELLS.pow(2),
                    at / SUMMARY_CELLS % SUMMARY_CELLS,
                    at % SUMMARY_CELLS,
                );
                let edits = CellEdits {
                    block: *self.palette.get(block as usize)?,
                    built,
                    cleared,
                };
                Some((coord * SUMMARY_CELLS + local, edits))
            })
        })
    }
}

/// The edits made in a chunk, for a client coming near it, as a region file has them: each
/// block as where it is in the chunk and an index into a palette of the blocks edited to, with
/// their states, so each is named once. With how many `BlockChanged`s the client had been sent
/// before, as either can overtake the other on its own channel, so the client can tell which
/// is newer.
#[derive(Message, Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ChunkEdits {
    coord: I64Vec3,
    changes_before: u64,
    palette: Vec<Block>,
    edits: Vec<(u16, u16)>,
}

impl ChunkEdits {
    pub fn new(
        coord: ChunkCoordinate,
        changes_before: u64,
        edits: impl IntoIterator<Item = (U16Vec3, Block)>,
    ) -> Self {
        let mut palette = Vec::new();
        let edits = edits
            .into_iter()
            .map(|(local, block)| {
                let index = palette.iter().position(|&b| b == block).unwrap_or_else(|| {
                    palette.push(block);
                    palette.len() - 1
                });
                (local_index(local), index as u16)
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
    pub fn edits(&self) -> impl Iterator<Item = (I64Vec3, Block)> + '_ {
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

/// A client's player's selection, as a command has changed it, to draw. The host's needs no
/// telling.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionChanged(pub Selection);

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

/// A client's player swinging their arm, as they hit, break, place or use something.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Swing;

/// Another player swinging their arm, for clients to draw.
#[derive(Message, MapEntities, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerSwung {
    #[entities]
    pub player: Entity,
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

/// A player hitting the creature `creature` with what they hold, which the server checks is in
/// their reach.
#[derive(Message, MapEntities, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitCreature {
    #[entities]
    pub creature: Entity,
}

/// A player using what they hold on the creature `creature`, as right click does: shearing it
/// or feeding it. The server checks it's in their reach.
#[derive(Message, MapEntities, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct UseCreature {
    #[entities]
    pub creature: Entity,
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

    use super::{name_from_user_data, name_to_user_data, BlockChanged, ChunkEdits, EditSummary};
    use crate::block::{Block, BlockType};
    use crate::chunks::chunk::ChunkCoordinate;
    use crate::player::name::PlayerName;

    #[test]
    fn test_block_changes_round_trip_with_whoever_made_them() {
        let client = ClientId::Client(Entity::from_raw_u32(7).unwrap());
        for by in [None, Some((ClientId::Server, 3)), Some((client, 4))] {
            let change = BlockChanged {
                block: I64Vec3::new(-5, 70, 1 << 40),
                to: "door[facing=south,half=top,open=true]".parse().unwrap(),
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
        let stairs = "planks_stairs[facing=west]".parse().unwrap();
        let edits = [
            (U16Vec3::new(0, 0, 0), Block::AIR),
            (U16Vec3::new(15, 15, 15), BlockType::Planks.into()),
            (U16Vec3::new(1, 2, 3), Block::AIR),
            (U16Vec3::new(1, 2, 4), stairs),
        ];
        let sent = ChunkEdits::new(coord, 7, edits);
        let bytes = postcard::to_stdvec(&sent).unwrap();
        let received: ChunkEdits = postcard::from_bytes(&bytes).unwrap();
        let origin = coord.0 * 16;
        let expected = edits.map(|(local, block)| (origin + local.as_i64vec3(), block));
        assert!(received.edits().eq(expected));
        assert_eq!((coord, 7), (received.coord(), received.changes_before()));
        // a few bytes an edit, and a name a block
        assert!(bytes.len() < 70, "{} bytes", bytes.len());
    }

    #[test]
    fn test_a_houses_lod_edits_round_trip_in_a_few_hundred_bytes() {
        use crate::chunks::lod::LodEdits;
        use crate::edits::Edits;
        // a house 7 by 7, with walls 4 high round a floor, a roof and a doorway
        let corner = I64Vec3::new(1234, 470, -5678);
        let mut edits = Edits::default();
        for x in 0..7 {
            for z in 0..7 {
                for y in 0..6 {
                    let wall = x == 0 || x == 6 || z == 0 || z == 6;
                    let door = x == 3 && z == 0 && (1..=2).contains(&y);
                    if (y == 0 || y == 5 || wall) && !door {
                        edits.set(corner + I64Vec3::new(x, y, z), BlockType::Planks);
                    }
                }
            }
        }
        let lod_edits = LodEdits::of(&edits);
        let cells: Vec<_> = edits
            .chunks()
            .flat_map(|coord| lod_edits.chunk_cells(coord).collect::<Vec<_>>())
            .collect();
        let sent = EditSummary::new(cells.iter().copied());
        let bytes = postcard::to_stdvec(&sent).unwrap();
        let received: EditSummary = postcard::from_bytes(&bytes).unwrap();
        let mut got: Vec<_> = received.cells().collect();
        let mut expected = cells.clone();
        got.sort_by_key(|(cell, _)| cell.to_array());
        expected.sort_by_key(|(cell, _)| cell.to_array());
        assert_eq!(expected, got);
        println!(
            "a house of 172 blocks in {} cells: {} bytes",
            cells.len(),
            bytes.len()
        );
        assert!(bytes.len() < 400, "{} bytes", bytes.len());
    }
}
