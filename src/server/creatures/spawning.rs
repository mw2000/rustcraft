//! Creatures appearing by themselves near players, where and when their kind's row says, in
//! its biomes and the ground the server holds round them: out of a player's sight where it's cheap to tell,
//! never too near, and only while there are fewer of the kind round them than its cap.

use std::f32::consts::TAU;

use bevy::math::I64Vec3;
use bevy::prelude::*;
use rand::RngExt;

use super::{chunk_of, creature, walker};
use crate::block::BlockType;
use crate::chunks::chunk::ChunkCoordinate;
use crate::clock::{is_night, TimeOfDay};
use crate::creatures::path::Walker;
use crate::creatures::{Creature, CreatureKind, Daytime, SavedCreature, Spawns};
use crate::player::{Player, PlayerLook};
use crate::server::drops::Chance;
use crate::world::World;

// How near a player, in blocks across, creatures appear: not so near they're seen popping up,
// and within the ground a server holds round a player.
const NEAREST: f32 = 24.0;
const FURTHEST: f32 = 48.0;
// How far round a player, in blocks, creatures count towards a kind's cap.
const AREA: f32 = 64.0;
// Ahead of a player, within this angle either side of where they look, in radians, they'd see
// one appear.
const SIGHT: f32 = 1.0;
// How far up and down from a player's feet the ground's looked for.
const SURFACE_RANGE: i64 = 16;
// How far from the first of a group the rest appear, in blocks across.
const GROUP_SPREAD: i64 = 3;

/// How often, in seconds, a creature's tried for round each player, of each kind with room
/// for more there, or none to have creatures appear by themselves at all.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct SpawnRate(pub Option<f32>);

impl Default for SpawnRate {
    fn default() -> Self {
        Self(Some(2.0))
    }
}

/// Every so often, tries round each player for a group of each kind that's under its cap
/// there, in a place its row allows.
#[allow(clippy::too_many_arguments)]
pub(in crate::server) fn spawn(
    time: Res<Time>,
    rate: Res<SpawnRate>,
    clock: Res<TimeOfDay>,
    world: Res<World>,
    players: Query<(&Transform, &PlayerLook), With<Player>>,
    creatures: Query<(&Creature, &Transform)>,
    mut chance: ResMut<Chance>,
    mut since: Local<f32>,
    mut commands: Commands,
) {
    let Some(every) = rate.0 else {
        return;
    };
    *since += time.delta_secs();
    if *since < every {
        return;
    }
    *since = 0.0;
    let night = is_night(clock.0);
    for (body, look) in &players {
        let feet = body.translation;
        for &kind in CreatureKind::ALL {
            let Some(spawns) = &kind.properties().spawns else {
                continue;
            };
            let time_suits = match spawns.by {
                Daytime::Day => !night,
                Daytime::Night => night,
                Daytime::Always => true,
            };
            let round = creatures
                .iter()
                .filter(|(creature, at)| {
                    creature.0 == kind && (at.translation - feet).xz().length() <= AREA
                })
                .count();
            if !time_suits || round >= spawns.cap {
                continue;
            }
            let rng = &mut chance.0;
            let angle = rng.random_range(0.0..TAU);
            let ahead = look.forward().xz().normalize_or_zero();
            if Vec2::from_angle(angle).dot(ahead) > SIGHT.cos() {
                continue;
            }
            let distance = rng.random_range(NEAREST..FURTHEST);
            let column = feet.xz() + Vec2::from_angle(angle) * distance;
            if !spawns.biomes.contains(&biome_at(&world, column)) {
                continue;
            }
            let walker = walker(kind);
            let Some(first) = ground(&world, spawns, walker, column, feet.y) else {
                continue;
            };
            let room = spawns.cap - round;
            let count = usize::from(rng.random_range(spawns.group.clone())).min(room);
            let mut placed = vec![first];
            for _ in 1..count * 3 {
                if placed.len() == count {
                    break;
                }
                let offset = Vec2::new(
                    rng.random_range(-GROUP_SPREAD..=GROUP_SPREAD) as f32,
                    rng.random_range(-GROUP_SPREAD..=GROUP_SPREAD) as f32,
                );
                let at = ground(&world, spawns, walker, first.xz() + offset, first.y);
                if let Some(at) = at.filter(|at| !placed.contains(at)) {
                    placed.push(at);
                }
            }
            for at in placed {
                let yaw = rng.random_range(0.0..TAU);
                commands.spawn(creature(SavedCreature::grown(kind, at), yaw));
            }
        }
    }
}

/// The name of the biome owning the column at `column`, `base` for the base terrain's.
pub(in crate::server) fn biome_at(world: &World, column: Vec2) -> &'static str {
    let noise = world.region_store.noise();
    let sample = noise.sample(column.x.into(), column.y.into());
    noise.biomes().name(sample.biome).unwrap_or("base")
}

/// Where a creature of `walker`'s size can appear in the column at `column`, the highest place
/// within `SURFACE_RANGE` of `y` that `spawns` allows: on one of its blocks, with room for its
/// body, under the sky if it must be, in ground that's loaded. At the middle of the block's top.
fn ground(world: &World, spawns: &Spawns, walker: Walker, column: Vec2, y: f32) -> Option<Vec3> {
    let column = column.floor().as_i64vec2();
    let top = y.floor() as i64 + SURFACE_RANGE;
    let half = walker.across / 2;
    let generated = |at: I64Vec3| world.is_chunk_generated(ChunkCoordinate::containing(at));
    let mut clear = 0;
    for y in (top - 2 * SURFACE_RANGE..=top).rev() {
        let at = I64Vec3::new(column.x, y, column.y);
        if !generated(at) {
            clear = 0;
            continue;
        }
        let block = world.get_block_at(at);
        if !block.is_solid() {
            // and never in water, which isn't solid but holds no one up
            clear = if block.is_replaceable() && block != BlockType::Water {
                clear + 1
            } else {
                0
            };
            continue;
        }
        let fits = clear >= walker.tall
            && (-half..=half).all(|x| {
                (-half..=half).all(|z| {
                    (1..=walker.tall).all(|up| {
                        let cell = at + I64Vec3::new(x, up, z);
                        generated(cell) && !world.get_block_at(cell).is_solid()
                    })
                })
            });
        let feet = at + I64Vec3::Y;
        if fits
            && spawns.on.contains(&block)
            && (!spawns.under_sky || !world.is_under_ground(feet))
            && world.is_chunk_generated(chunk_of(feet.as_vec3()))
        {
            return Some(feet.as_vec3() + Vec3::new(0.5, 0.0, 0.5));
        }
        return None;
    }
    None
}
