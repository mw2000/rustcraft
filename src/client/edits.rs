use bevy::prelude::*;

use crate::chunks::chunk_loader::ChunkLoader;
use crate::protocol::BlockChanged;
use crate::world::World;

/// Puts the server's changes to blocks into the client's world, and has the meshes they reach
/// rebuilt.
pub fn apply_block_changes(
    mut changes: MessageReader<BlockChanged>,
    mut world: ResMut<World>,
    mut chunk_loader: ResMut<ChunkLoader>,
) {
    for change in changes.read() {
        // In singleplayer the server's world is this one and it's made the change already, so
        // this changes nothing and its meshes are rebuilt all the same.
        world.edit(change.block, change.to);
        chunk_loader.rebuild_edited(&mut world, change.block);
    }
}
