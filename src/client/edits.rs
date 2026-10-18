use bevy::math::I64Vec3;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy_replicon::prelude::ClientId;

use crate::block::Block;
use crate::chunks::chunk::ChunkCoordinate;
use crate::chunks::chunk_loader::ChunkLoader;
use crate::protocol::{BlockChanged, ChunkEdits, EditRejected, EditSummary, RequestEdit};
use crate::world::World;

/// The edits a client joined to a server has made in its own world without waiting for the
/// server's answer, so its player sees them at once (§4.4). The host makes none, as the server
/// answers it in the same frame.
#[derive(Resource, Default)]
pub struct Predictions {
    /// This client, as the server knows it, to tell its own edits by.
    pub(super) me: Option<ClientId>,
    // the block each request unanswered is for
    requests: HashMap<u32, I64Vec3>,
    // each block with requests unanswered, what the server has there as last told, and how
    // many of them there are
    blocks: HashMap<I64Vec3, (Block, u32)>,
}

impl Predictions {
    // Makes the edit requested in `world`, remembering what the server has there in case it's
    // refused. Returns whether the world changed.
    fn predict(&mut self, world: &mut World, request: &RequestEdit) -> bool {
        let server = world.get(request.block);
        self.requests.insert(request.seq, request.block);
        self.blocks.entry(request.block).or_insert((server, 0)).1 += 1;
        world.edit(request.block, request.to)
    }

    // Puts what the server says is at `block` into `world`, unless this client's asked for it
    // to change and not yet heard back, when it's kept for if the server refuses. Returns
    // whether the world changed.
    fn told(&mut self, world: &mut World, block: I64Vec3, to: Block) -> bool {
        match self.blocks.get_mut(&block) {
            Some((server, _)) => {
                *server = to;
                false
            }
            None => world.edit(block, to),
        }
    }

    // The server's answered request `seq`, allowing or refusing it. Once it's answered every
    // request for the block, puts back what it has there, which is what was predicted unless
    // it refused or someone else changed it since. Returns whether the world changed.
    fn answered(&mut self, world: &mut World, seq: u32) -> bool {
        let Some(block) = self.requests.remove(&seq) else {
            return false;
        };
        let Some((server, unanswered)) = self.blocks.get_mut(&block) else {
            return false;
        };
        *unanswered -= 1;
        if *unanswered > 0 {
            return false;
        }
        let server = *server;
        self.blocks.remove(&block);
        world.edit(block, server)
    }
}

/// Which of the server's `ChunkEdits` and `BlockChanged`s about a block is newer, for a client
/// joined to a server: each comes on a channel of its own, so either can overtake the other.
/// `BlockChanged`s are counted as they come, as the server counts them as it sends them, and
/// each `ChunkEdits` says how many were sent before it.
#[derive(Resource, Default)]
pub struct EditOrder {
    changes: u64,
    // each chunk's latest edits, by the changes sent before them
    chunks: HashMap<ChunkCoordinate, u64>,
    // each block changed since the latest chunk's edits came, by its change's count, which
    // only a chunk's edits sent earlier still on their way can be older than
    blocks: HashMap<I64Vec3, u64>,
}

impl EditOrder {
    // Counts a `BlockChanged` for `block`, and returns whether it's newer than its chunk's
    // edits.
    fn change(&mut self, block: I64Vec3) -> bool {
        self.changes += 1;
        let synced = self.chunks.get(&ChunkCoordinate::containing(block));
        let newer = synced.is_none_or(|&before| self.changes > before);
        if newer {
            self.blocks.insert(block, self.changes);
        }
        newer
    }

    // Takes in a chunk's edits, returning which of their blocks have changed since.
    fn chunk(&mut self, chunk: &ChunkEdits) -> impl Fn(I64Vec3) -> bool + '_ {
        let before = chunk.changes_before();
        self.chunks.insert(chunk.coord(), before);
        // chunks' edits come in the order they're sent, so any still on their way were sent
        // after these and nothing's older than them
        self.blocks.retain(|_, &mut change| change > before);
        move |block| self.blocks.contains_key(&block)
    }
}

/// What the server's sent about edits before this client's world is made, while it's checked,
/// waiting for it.
#[derive(Resource, Default)]
pub struct WaitingEdits {
    chunks: Vec<ChunkEdits>,
    summaries: Vec<EditSummary>,
    changes: Vec<BlockChanged>,
}

/// Makes this client's requests to change blocks in its own world at once, and has the meshes
/// they reach rebuilt.
pub fn predict_edits(
    mut requests: MessageReader<RequestEdit>,
    mut world: ResMut<World>,
    mut predictions: ResMut<Predictions>,
    mut chunk_loader: ResMut<ChunkLoader>,
) {
    for request in requests.read() {
        if predictions.predict(&mut world, request) {
            chunk_loader.rebuild_edited(&mut world, request.block);
        }
    }
}

/// Puts the edits of the chunks this client's player comes near into its world, but for blocks
/// changed since: a chunk not loaded yet takes them as it's generated, and a loaded one is
/// redrawn. Those that come before the world's made, while it's checked, wait for it.
pub fn apply_chunk_edits(
    mut sent: MessageReader<ChunkEdits>,
    world: Option<ResMut<World>>,
    order: Option<ResMut<EditOrder>>,
    mut predictions: ResMut<Predictions>,
    mut chunk_loader: ResMut<ChunkLoader>,
    mut waiting: ResMut<WaitingEdits>,
) {
    let (Some(mut world), Some(mut order)) = (world, order) else {
        waiting.chunks.extend(sent.read().cloned());
        return;
    };
    for chunk in std::mem::take(&mut waiting.chunks)
        .iter()
        .chain(sent.read())
    {
        let changed_since = order.chunk(chunk);
        for (block, to) in chunk.edits() {
            if !changed_since(block) && predictions.told(&mut world, block, to) {
                chunk_loader.rebuild_edited(&mut world, block);
            }
        }
    }
}

/// Puts the LOD edits the server sends into this client's world, for its LOD nodes to draw,
/// which redraws those they change. Those that come before the world's made wait for it.
pub fn apply_edit_summaries(
    mut summaries: MessageReader<EditSummary>,
    world: Option<ResMut<World>>,
    mut waiting: ResMut<WaitingEdits>,
) {
    let Some(mut world) = world else {
        waiting.summaries.extend(summaries.read().cloned());
        return;
    };
    let lod_edits = world.lod_edits_mut();
    for summary in std::mem::take(&mut waiting.summaries)
        .iter()
        .chain(summaries.read())
    {
        lod_edits.set(summary.cells());
    }
}

/// Puts the server's changes to blocks into the client's world, but for those older than their
/// chunk's edits, and has the meshes they reach rebuilt, but for those that only confirm what
/// this client predicted. Changes that come before the world's made wait for it.
pub fn apply_block_changes(
    mut changes: MessageReader<BlockChanged>,
    world: Option<ResMut<World>>,
    mut order: Option<ResMut<EditOrder>>,
    mut predictions: ResMut<Predictions>,
    mut chunk_loader: ResMut<ChunkLoader>,
    mut waiting: ResMut<WaitingEdits>,
) {
    let Some(mut world) = world else {
        waiting.changes.extend(changes.read().copied());
        return;
    };
    for change in std::mem::take(&mut waiting.changes)
        .into_iter()
        .chain(changes.read().copied())
    {
        // only a client joined to a server keeps the order, as the host is sent no chunks
        let newer = order
            .as_mut()
            .is_none_or(|order| order.change(change.block));
        let changed = newer && predictions.told(&mut world, change.block, change.to);
        let rebuild = match change.by {
            Some((client, seq)) if Some(client) == predictions.me => {
                changed | predictions.answered(&mut world, seq)
            }
            // In singleplayer and on the host the server's world is this one, and it's made
            // the change already, so this changes nothing and its meshes are rebuilt all the
            // same.
            _ => true,
        };
        if rebuild {
            chunk_loader.rebuild_edited(&mut world, change.block);
        }
    }
}

/// Puts back what the server has where it's refused this client's requests to change blocks.
pub fn roll_back_refused(
    mut rejections: MessageReader<EditRejected>,
    mut world: ResMut<World>,
    mut predictions: ResMut<Predictions>,
    mut chunk_loader: ResMut<ChunkLoader>,
) {
    for rejection in rejections.read() {
        if predictions.answered(&mut world, rejection.seq) {
            chunk_loader.rebuild_edited(&mut world, rejection.block);
        }
    }
}
