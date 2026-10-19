//! Keeping each client's copy of the edit layer in step with the server's near its player
//! (§4.4): the edits of chunks coming near as `ChunkEdits`, and changes there as `BlockChanged`.
//! Further off, as far as it draws, it's kept in step with them as LOD nodes draw them, as
//! `EditSummary`s of the chunks coming into view and of the cells changed since.

use bevy::ecs::system::SystemParam;
use bevy::math::I64Vec3;
use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use bevy_replicon::prelude::*;

use crate::block::Block;
use crate::chunks::chunk::ChunkCoordinate;
use crate::edits::Edits;
use crate::player::PlayerOwner;
use crate::protocol::{BlockChanged, ChunkEdits, EditReach, EditSummary};
use crate::world::World;

// About a full chunk's worth of edits a frame for each client, a dozen kilobytes, so a client
// joining among a big build gets it over a few frames, nearest first, rather than in one.
const EDITS_PER_FRAME: usize = 4096;
// About as many bytes again of 2-block cells of LOD edits, 3 or 4 bytes each.
const SUMMARY_CELLS_PER_FRAME: usize = 4096;
// A chunk's sent edits are forgotten only this many chunks beyond a client's reach, so walking
// to and fro over the edge of it doesn't send the same chunks again and again.
const FORGET_SLACK: f32 = 4.0;

/// The chunks around a client's player whose edits it's kept in step with: those it's been sent,
/// and those in its reach it's still to be, nearest first. It's told of every change in either.
#[derive(Component, Default)]
pub struct SyncedArea {
    // the player's chunk as last seen, or none to look again
    centre: Option<ChunkCoordinate>,
    // in chunks, and 0, the player's own chunk, until the client says
    reach: f32,
    sent: HashSet<ChunkCoordinate>,
    // the farthest first, to pop the nearest
    unsent: Vec<ChunkCoordinate>,
    // how many `BlockChanged`s the client's been sent, see `ChunkEdits`
    changes_sent: u64,
    // in chunks, how far the client draws, 0 until it says
    view: f32,
    // the chunks in view whose LOD edits the client's been sent or is to be, those it's to be,
    // the farthest first, and the 2-block cells changed in them since
    summarised: HashSet<ChunkCoordinate>,
    unsummarised: Vec<ChunkCoordinate>,
    resummarised: HashSet<I64Vec3>,
}

impl SyncedArea {
    fn reaches(&self, coord: ChunkCoordinate) -> bool {
        self.centre
            .is_some_and(|centre| distance(centre, coord) <= self.reach)
    }

    /// Whether a change in the chunk at `coord` is to be sent: the client's been sent the
    /// chunk's edits, or it's in reach, so either it's to be or the chunk had none.
    fn takes_in(&self, coord: ChunkCoordinate) -> bool {
        self.sent.contains(&coord) || self.reaches(coord)
    }

    fn sees(&self, coord: ChunkCoordinate) -> bool {
        self.centre
            .is_some_and(|centre| distance(centre, coord) <= self.view)
    }

    // Moves the area to the player's chunk at `centre`, forgetting what's been sent well beyond
    // it, as the client would be sent changes there for nothing, and lining up the chunks with
    // edits that have come into reach, and into view. Looks at every chunk with edits, which is
    // quick even for a big world, and done only as the player crosses into another chunk.
    fn move_to(&mut self, centre: ChunkCoordinate, edits: &Edits) {
        self.centre = Some(centre);
        let forget = self.reach + FORGET_SLACK;
        self.sent.retain(|&coord| distance(centre, coord) <= forget);
        self.unsent = edits
            .chunks()
            .filter(|&coord| self.reaches(coord) && !self.sent.contains(&coord))
            .collect();
        let forget = self.view + FORGET_SLACK;
        self.summarised
            .retain(|&coord| distance(centre, coord) <= forget);
        let coming: Vec<ChunkCoordinate> = edits
            .chunks()
            .filter(|&coord| self.sees(coord) && !self.summarised.contains(&coord))
            .collect();
        self.summarised.extend(coming.iter().copied());
        let summarised = &self.summarised;
        self.unsummarised.retain(|coord| summarised.contains(coord));
        self.unsummarised.extend(coming);
        let farthest_first = |a: &ChunkCoordinate, b: &ChunkCoordinate| {
            distance(centre, *b).total_cmp(&distance(centre, *a))
        };
        self.unsent.sort_unstable_by(farthest_first);
        self.unsummarised.sort_unstable_by(farthest_first);
    }

    // Notes a change to `block` for the client's LOD edits, if it's in view or was. A chunk in
    // view not summarised had no edits as the area last moved, so this cell is all it has.
    fn resummarise(&mut self, block: I64Vec3) {
        let coord = ChunkCoordinate::containing(block);
        if self.sees(coord) || self.summarised.contains(&coord) {
            self.summarised.insert(coord);
            self.resummarised
                .insert(block.div_euclid(I64Vec3::splat(2)));
        }
    }
}

fn distance(a: ChunkCoordinate, b: ChunkCoordinate) -> f32 {
    (a.0 - b.0).as_vec3().length()
}

/// Sets how far each client wants edits from, as it says.
pub(super) fn receive_reaches(
    mut reaches: MessageReader<FromClient<EditReach>>,
    mut areas: Query<&mut SyncedArea>,
) {
    for FromClient { client_id, message } in reaches.read() {
        if let Some(mut area) = client_id
            .entity()
            .and_then(|client| areas.get_mut(client).ok())
        {
            area.reach = message.full_detail as f32;
            area.view = message.view as f32;
            area.centre = None;
        }
    }
}

/// Moves each client's area with its player.
pub(super) fn follow_players(
    world: Res<World>,
    players: Query<(&PlayerOwner, &Transform)>,
    mut areas: Query<&mut SyncedArea>,
) {
    for (owner, body) in &players {
        let Some(Ok(mut area)) = owner.0.entity().map(|client| areas.get_mut(client)) else {
            continue;
        };
        let centre = ChunkCoordinate::containing(body.translation.floor().as_i64vec3());
        if area.centre != Some(centre) {
            area.move_to(centre, world.edits());
        }
    }
}

/// Sends each client the edits of the chunks come into its reach, a few frames' worth at a time.
pub(super) fn send_chunk_edits(
    world: Res<World>,
    mut areas: Query<(Entity, &mut SyncedArea)>,
    mut sent: MessageWriter<ToClients<ChunkEdits>>,
) {
    for (client, mut area) in &mut areas {
        let mut budget = EDITS_PER_FRAME;
        while budget > 0 {
            let Some(coord) = area.unsent.pop() else {
                break;
            };
            let edits = ChunkEdits::new(coord, area.changes_sent, world.edits().in_chunk(coord));
            budget = budget.saturating_sub(edits.edit_count());
            area.sent.insert(coord);
            sent.write(ToClients {
                targets: SendTargets::Single(ClientId::Client(client)),
                message: edits,
            });
        }
    }
}

/// Sends each client the LOD edits of the cells changed in view, and of the chunks come into
/// view, a few frames' worth at a time.
pub(super) fn send_edit_summaries(
    world: Res<World>,
    mut areas: Query<(Entity, &mut SyncedArea)>,
    mut sent: MessageWriter<ToClients<EditSummary>>,
) {
    let lod_edits = world.lod_edits();
    for (client, mut area) in &mut areas {
        let area = &mut *area;
        let mut cells: Vec<_> = area
            .resummarised
            .drain()
            .filter_map(|cell| Some((cell, lod_edits.cell(1, cell)?)))
            .collect();
        while cells.len() < SUMMARY_CELLS_PER_FRAME {
            let Some(coord) = area.unsummarised.pop() else {
                break;
            };
            cells.extend(lod_edits.chunk_cells(coord));
        }
        if !cells.is_empty() {
            sent.write(ToClients {
                targets: SendTargets::Single(ClientId::Client(client)),
                message: EditSummary::new(cells),
            });
        }
    }
}

/// Tells of the server's changes to blocks: the host, whose client shares the server's world,
/// and the clients whose areas take in the block's chunk. The rest are sent it with the chunk's
/// edits as they come near.
#[derive(SystemParam)]
pub struct BlockChanges<'w, 's> {
    areas: Query<'w, 's, (Entity, &'static mut SyncedArea)>,
    changes: MessageWriter<'w, ToClients<BlockChanged>>,
    chunks: MessageWriter<'w, ToClients<ChunkEdits>>,
}

impl BlockChanges<'_, '_> {
    pub fn write(&mut self, change: BlockChanged) {
        self.changes.write(ToClients {
            targets: SendTargets::Single(ClientId::Server),
            message: change,
        });
        let coord = ChunkCoordinate::containing(change.block);
        for (client, mut area) in &mut self.areas {
            area.resummarise(change.block);
            if area.takes_in(coord) {
                area.changes_sent += 1;
                self.changes.write(ToClients {
                    targets: SendTargets::Single(ClientId::Client(client)),
                    message: change,
                });
            }
        }
    }

    /// Tells of many changes in the chunk at `coord` at once, as fluids make them (M4.3): the
    /// host change by change, as its client shares the server's world and sends nothing, and
    /// each client whose area takes in the chunk all together as a partial `ChunkEdits`, 3
    /// bytes a block rather than a message each.
    pub fn write_chunk(&mut self, coord: ChunkCoordinate, changes: &[(I64Vec3, Block)]) {
        for &(block, to) in changes {
            self.changes.write(ToClients {
                targets: SendTargets::Single(ClientId::Server),
                message: BlockChanged {
                    block,
                    to,
                    by: None,
                },
            });
        }
        for (client, mut area) in &mut self.areas {
            for &(block, _) in changes {
                area.resummarise(block);
            }
            if area.takes_in(coord) {
                let local = changes
                    .iter()
                    .map(|&(block, to)| (ChunkCoordinate::local_block(block), to));
                self.chunks.write(ToClients {
                    targets: SendTargets::Single(ClientId::Client(client)),
                    message: ChunkEdits::changed(coord, area.changes_sent, local),
                });
            }
        }
    }
}
