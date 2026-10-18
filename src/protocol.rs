//! What clients and the server send each other (§4.4). Blocks travel by name (§4.1).

use bevy::math::I64Vec3;
use bevy::prelude::*;
use bevy_replicon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::block::BlockType;

/// Registers the messages, on the server and clients alike. Replicon tells messages apart by the
/// order they're registered in, so it's all done here, in one order. Needs `RepliconPlugins`.
pub struct ProtocolPlugin;

impl Plugin for ProtocolPlugin {
    fn build(&self, app: &mut App) {
        // ordered, as edits to one block only make sense in the order they're made
        app.add_client_message::<RequestEdit>(Channel::Ordered)
            .add_server_message::<BlockChanged>(Channel::Ordered)
            .add_server_message::<EditRejected>(Channel::Ordered);
    }
}

/// A client asking for the block at `block` to become `to`, air to break it. `seq` counts the
/// client's requests, so it can tell which one an answer is to.
#[derive(Message, Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestEdit {
    pub block: I64Vec3,
    pub to: BlockType,
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
