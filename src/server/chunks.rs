use bevy::math::I64Vec3;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use bevy::tasks::{futures, AsyncComputeTaskPool, Task};

use crate::chunks::chunk::{ChunkCoordinate, ChunkData};
use crate::chunks::generate::generator::generate_chunk;
use crate::player::{Player, EYE_HEIGHT};
use crate::world::{Holder, World};

// How near a player's eye, in chunks, the server keeps chunks loaded. Everything an edit can
// reach (`REACH` and its slack, 6 blocks) and the whole body (2 blocks tall) is in the eye's
// chunk or one against it, at most sqrt(3) chunks off, so 2 covers those and a chunk more
// along each axis, for the player to move into while it's generated.
const LOAD_RADIUS: f32 = 2.0;
// Chunks are let go only further off, so walking to and fro over a chunk's border doesn't
// generate the same ones over and over.
const RELEASE_RADIUS: f32 = 3.0;

/// The chunks the server holds for its players, and those of them it's generating itself.
#[derive(Resource, Default)]
pub struct ServerChunks {
    held: HashSet<ChunkCoordinate>,
    generating: HashMap<ChunkCoordinate, Task<ChunkData>>,
}

/// Keeps the chunks near every player loaded, for checking edits and, later, movement against,
/// and lets go of those no player is near. Nothing is meshed. A chunk already loaded, or on its
/// way for the client, isn't generated again, so in singleplayer, where the client loads far
/// more, this almost never generates anything.
pub fn keep_chunks_near_players(
    mut world: ResMut<World>,
    mut chunks: ResMut<ServerChunks>,
    players: Query<&Transform, With<Player>>,
) {
    let chunks = &mut *chunks;
    chunks
        .generating
        .retain(|&coord, task| match futures::check_ready(task) {
            Some(data) => {
                world.insert_chunk(coord, data);
                false
            }
            None => true,
        });

    let eyes: Vec<ChunkCoordinate> = players
        .iter()
        .map(|body| {
            let eye = body.translation + Vec3::Y * EYE_HEIGHT;
            ChunkCoordinate::containing(eye.floor().as_i64vec3())
        })
        .collect();
    let near = |coord: ChunkCoordinate, radius| {
        eyes.iter()
            .any(|eye| (coord.0 - eye.0).as_vec3().length() <= radius)
    };
    chunks.held.retain(|&coord| {
        let keep = near(coord, RELEASE_RADIUS);
        if !keep {
            // dropping a task cancels it
            chunks.generating.remove(&coord);
            world.release(coord, Holder::Server);
        }
        keep
    });

    let layers = World::layers(world.height);
    let r = LOAD_RADIUS as i64;
    for eye in &eyes {
        for x in -r..=r {
            for y in -r..=r {
                for z in -r..=r {
                    let offset = I64Vec3::new(x, y, z);
                    let coord = ChunkCoordinate(eye.0 + offset);
                    if offset.as_vec3().length() > LOAD_RADIUS
                        || !layers.contains(&coord.0.y)
                        || !chunks.held.insert(coord)
                    {
                        continue;
                    }
                    let held_already = world.hold(coord, Holder::Server);
                    if !held_already && !world.is_chunk_generated(coord) {
                        let regions = world.region_store.clone();
                        let task = AsyncComputeTaskPool::get()
                            .spawn(async move { generate_chunk(regions, coord) });
                        chunks.generating.insert(coord, task);
                    }
                }
            }
        }
    }
}
