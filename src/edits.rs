use bevy::{
    ecs::message::Message,
    math::{I64Vec3, U16Vec3},
    platform::collections::HashMap,
};

use crate::block::BlockType;
use crate::chunks::chunk::{ChunkCoordinate, ChunkData};

/// A request to change the block at `block` to `to`, air to break it. Every change to the world
/// goes through one of these, so whatever owns the world, now `apply_edits` and later the
/// server (INV-4), decides what happens.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditBlock {
    pub block: I64Vec3,
    pub to: BlockType,
}

/// The edit layer (§4.2): every block players have changed, air included, kept by chunk apart
/// from the generated world. The world is its generated blocks with these on top, and nothing
/// generated is kept here (INV-1), so a chunk can always be regenerated and its edits put back.
/// Keeping them by chunk means an area's edits are found by looking up its chunks.
#[derive(Default)]
pub struct Edits {
    chunks: HashMap<ChunkCoordinate, HashMap<U16Vec3, BlockType>>,
}

impl Edits {
    pub fn set(&mut self, block: I64Vec3, to: BlockType) {
        self.chunks
            .entry(ChunkCoordinate::containing(block))
            .or_default()
            .insert(ChunkCoordinate::local_block(block), to);
    }

    /// Puts the edits made in the chunk at `coord` onto its generated `data`.
    pub fn apply(&self, coord: ChunkCoordinate, data: &mut ChunkData) {
        for (&local, &block) in self.chunks.get(&coord).into_iter().flatten() {
            data.set_block_at(local, block);
        }
    }
}
