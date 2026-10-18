//! Keeping each client's copy of the edit layer in step with the server's near its player
//! (§4.4): the edits of chunks coming near as `ChunkEdits`, and changes there as `BlockChanged`.

use bevy::ecs::system::SystemParam;
use bevy::platform::collections::HashSet;
use bevy::prelude::*;
use bevy_replicon::prelude::*;

use crate::chunks::chunk::ChunkCoordinate;
use crate::edits::Edits;
use crate::player::PlayerOwner;
use crate::protocol::{BlockChanged, ChunkEdits, EditReach};
use crate::world::World;

// About a full chunk's worth of edits a frame for each client, a dozen kilobytes, so a client
// joining among a big build gets it over a few frames, nearest first, rather than in one.
const EDITS_PER_FRAME: usize = 4096;
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

    // Moves the area to the player's chunk at `centre`, forgetting what's been sent well beyond
    // it, as the client would be sent changes there for nothing, and lining up the chunks with
    // edits that have come into reach. Looks at every chunk with edits, which is quick even
    // for a big world, and done only as the player crosses into another chunk.
    fn move_to(&mut self, centre: ChunkCoordinate, edits: &Edits) {
        self.centre = Some(centre);
        let forget = self.reach + FORGET_SLACK;
        self.sent.retain(|&coord| distance(centre, coord) <= forget);
        self.unsent = edits
            .chunks()
            .filter(|&coord| self.reaches(coord) && !self.sent.contains(&coord))
            .collect();
        self.unsent
            .sort_unstable_by(|&a, &b| distance(centre, b).total_cmp(&distance(centre, a)));
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
            area.reach = message.0 as f32;
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

/// Tells of the server's changes to blocks: the host, whose client shares the server's world,
/// and the clients whose areas take in the block's chunk. The rest are sent it with the chunk's
/// edits as they come near.
#[derive(SystemParam)]
pub struct BlockChanges<'w, 's> {
    areas: Query<'w, 's, (Entity, &'static mut SyncedArea)>,
    changes: MessageWriter<'w, ToClients<BlockChanged>>,
}

impl BlockChanges<'_, '_> {
    pub fn write(&mut self, change: BlockChanged) {
        self.changes.write(ToClients {
            targets: SendTargets::Single(ClientId::Server),
            message: change,
        });
        let coord = ChunkCoordinate::containing(change.block);
        for (client, mut area) in &mut self.areas {
            if area.takes_in(coord) {
                area.changes_sent += 1;
                self.changes.write(ToClients {
                    targets: SendTargets::Single(ClientId::Client(client)),
                    message: change,
                });
            }
        }
    }
}
