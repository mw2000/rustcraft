//! Creatures appearing by themselves near players, where and when their kind's row says, in
//! its biomes and band of heights (M4.10), and the ground the server holds round them: out of a
//! player's sight where it's
//! cheap to tell, never too near, and only while there are fewer of the kind round them than its
//! cap. Monsters come only in the dark, out in the night or in the caves, never where a torch or
//! anything else gives light, and from the spawners of dungeons while someone's close, which are
//! built as someone first comes near them.

use std::f32::consts::TAU;
use std::ops::Range;

use bevy::math::{I64Vec2, I64Vec3};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use rand::seq::IndexedRandom;
use rand::{Rng, RngExt};

use super::{chunk_of, creature, walker};
use crate::block::BlockType;
use crate::chunks::chunk::{ChunkCoordinate, CHUNK_SIZE};
use crate::chunks::generate::structures::SPAWNER;
use crate::clock::TimeOfDay;
use crate::creatures::path::Walker;
use crate::creatures::{Creature, CreatureKind, Daytime, SavedCreature, Sky, Spawns, Stance};
use crate::player::{Player, PlayerLook};
use crate::protocol::BlockChanged;
use crate::server::drops::Chance;
use crate::server::sync::BlockChanges;
use crate::weather::{is_dark_out, Weather};
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
// Spawners: how near a player must be, in blocks, for one to bring monsters out, how often it
// does, in seconds, how many of its kind there can be round it, within how many blocks, before
// it brings out no more, and how far from it across, and up and down, they appear.
const SPAWNER_RANGE: f32 = 16.0;
const SPAWNER_EVERY: Range<f32> = 10.0..20.0;
const SPAWNER_CROWD: usize = 3;
const SPAWNER_AREA: f32 = 12.0;
const SPAWNER_SPREAD: i64 = 4;
const SPAWNER_RISE: i64 = 2;
// How near a player comes, in blocks, before a structure's spawner is built: further than it
// works from, so it's there before it's seen.
const SPAWNER_BUILT: f32 = 32.0;

/// How often, in seconds, a creature's tried for round each player, of each kind with room
/// for more there, or none to have creatures appear by themselves at all.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct SpawnRate(pub Option<f32>);

impl Default for SpawnRate {
    fn default() -> Self {
        Self(Some(2.0))
    }
}

/// The spawners near players, by where they are, each with when it next brings monsters out by
/// the server's clock in seconds.
#[derive(Resource, Debug, Default)]
pub struct Spawners(HashMap<I64Vec3, f32>);

/// Every so often, tries round each player for a group of each kind that's under its cap
/// there, in a place its row allows, and has the spawners near them bring out monsters.
#[allow(clippy::too_many_arguments)]
pub(in crate::server) fn spawn(
    time: Res<Time>,
    rate: Res<SpawnRate>,
    clock: Res<TimeOfDay>,
    weather: Res<Weather>,
    world: Res<World>,
    players: Query<(&Transform, &PlayerLook), With<Player>>,
    creatures: Query<(&Creature, &Transform)>,
    mut spawners: ResMut<Spawners>,
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
    // dark under the sky, at night or in a storm's gloom at dusk and dawn
    let night = is_dark_out(*clock, &weather);
    let rng = &mut chance.0;
    let count = |kind: CreatureKind, at: Vec3, within: f32| {
        creatures
            .iter()
            .filter(|(creature, body)| {
                creature.0 == kind && (body.translation - at).xz().length() <= within
            })
            .count()
    };
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
            let round = count(kind, feet, AREA);
            if !time_suits || round >= spawns.cap {
                continue;
            }
            let angle = rng.random_range(0.0..TAU);
            let ahead = look.forward().xz().normalize_or_zero();
            if Vec2::from_angle(angle).dot(ahead) > SIGHT.cos() {
                continue;
            }
            let distance = rng.random_range(NEAREST..FURTHEST);
            let column = feet.xz() + Vec2::from_angle(angle) * distance;
            let biomes = spawns.biomes;
            if !biomes.is_empty() && !biomes.contains(&biome_at(&world, column)) {
                continue;
            }
            let walker = walker(kind);
            let height = kind.properties().height;
            let Some(first) = place(&world, spawns, walker, height, column, feet.y, night, rng)
            else {
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
                let column = first.xz() + offset;
                let at = place(&world, spawns, walker, height, column, first.y, night, rng);
                if let Some(at) = at.filter(|at| !placed.contains(at)) {
                    placed.push(at);
                }
            }
            for at in placed {
                let yaw = rng.random_range(0.0..TAU);
                let mut spawned = commands.spawn(creature(SavedCreature::grown(kind, at), yaw));
                if spawns.ceiling || spawns.buried {
                    spawned.insert(Stance {
                        clinging: spawns.ceiling,
                        buried: spawns.buried,
                        ..default()
                    });
                }
            }
        }
    }

    // the spawners near players now, each keeping when it's next due, or due a while from now
    // if it's newly near
    let now = time.elapsed_secs();
    let near: Vec<I64Vec3> = players
        .iter()
        .flat_map(|(body, _)| spawners_near(&world, body.translation))
        .collect();
    let mut due = HashMap::new();
    for at in near {
        let next = spawners.0.get(&at).copied();
        due.insert(
            at,
            next.unwrap_or_else(|| now + rng.random_range(SPAWNER_EVERY)),
        );
    }
    spawners.0 = due;
    for (&at, next) in &mut spawners.0 {
        if now < *next {
            continue;
        }
        *next = now + rng.random_range(SPAWNER_EVERY);
        let kind = CreatureKind::guarding(world.seed(), at);
        let middle = at.as_vec3() + Vec3::splat(0.5);
        let lit = world.block_light(at + I64Vec3::Y) > 0;
        if lit || count(kind, middle, SPAWNER_AREA) >= SPAWNER_CROWD {
            continue;
        }
        for _ in 0..rng.random_range(1..=2) {
            if let Some(feet) = beside(&world, walker(kind), at, rng) {
                let yaw = rng.random_range(0.0..TAU);
                commands.spawn(creature(SavedCreature::grown(kind, feet), yaw));
            }
        }
    }
}

/// Builds the spawners structures have at their `spawner` markers, as edits, the first time a
/// player comes near one whose ground is loaded, telling the clients near it. Generation leaves
/// the place for it, as a chest's loot is rolled only when it's first opened (§5.6), so the
/// world's generation is as it was; an edit there already, such as the air of one broken, means
/// it's been built or taken.
pub(in crate::server) fn build_spawners(
    mut world: ResMut<World>,
    players: Query<&Transform, With<Player>>,
    mut changed: BlockChanges,
) {
    for body in &players {
        let feet = body.translation;
        let column = feet.floor().as_i64vec3().xz();
        let reach = I64Vec2::splat(SPAWNER_BUILT as i64);
        for placed in world
            .region_store
            .structures_near(column - reach, column + reach)
        {
            for (kind, at) in placed.markers() {
                let unbuilt = kind == SPAWNER
                    && at.as_vec3().distance(feet) <= SPAWNER_BUILT
                    && world.edits().get(at).is_none()
                    && world.is_chunk_generated(ChunkCoordinate::containing(at));
                if unbuilt && world.edit(at, BlockType::Spawner) {
                    changed.write(BlockChanged {
                        block: at,
                        to: BlockType::Spawner.into(),
                        by: None,
                    });
                }
            }
        }
    }
}

// The spawners within `SPAWNER_RANGE` of a player's feet at `feet`, in the chunks loaded.
fn spawners_near(world: &World, feet: Vec3) -> Vec<I64Vec3> {
    let reach = Vec3::splat(SPAWNER_RANGE);
    let first = ChunkCoordinate::containing((feet - reach).floor().as_i64vec3()).0;
    let last = ChunkCoordinate::containing((feet + reach).floor().as_i64vec3()).0;
    let mut near = Vec::new();
    for x in first.x..=last.x {
        for y in first.y..=last.y {
            for z in first.z..=last.z {
                let coord = I64Vec3::new(x, y, z);
                let Some(data) = world.get_chunk_data(ChunkCoordinate(coord)) else {
                    continue;
                };
                let origin = coord * CHUNK_SIZE as i64;
                near.extend(
                    data.positions_of(BlockType::Spawner)
                        .map(|local| origin + local.as_i64vec3())
                        .filter(|at| at.as_vec3().distance(feet) <= SPAWNER_RANGE),
                );
            }
        }
    }
    near
}

// Somewhere for a body of `walker`'s size to appear beside the spawner at `spawner`: a floor
// with room over it, a few blocks off, tried a few times.
fn beside(world: &World, walker: Walker, spawner: I64Vec3, rng: &mut impl Rng) -> Option<Vec3> {
    for _ in 0..8 {
        let x = rng.random_range(-SPAWNER_SPREAD..=SPAWNER_SPREAD);
        let z = rng.random_range(-SPAWNER_SPREAD..=SPAWNER_SPREAD);
        for y in (-SPAWNER_RISE..=SPAWNER_RISE).rev() {
            let feet = spawner + I64Vec3::new(x, y, z);
            let floor = world.get_block_at(feet - I64Vec3::Y).is_solid();
            if floor && has_room(world, walker, feet, 1) {
                return Some(feet.as_vec3() + Vec3::new(0.5, 0.0, 0.5));
            }
        }
    }
    None
}

/// The name of the biome owning the column at `column`, `base` for the base terrain's.
pub(in crate::server) fn biome_at(world: &World, column: Vec2) -> &'static str {
    let noise = world.region_store.noise();
    let sample = noise.sample(column.x.into(), column.y.into());
    noise.biomes().name(sample.biome).unwrap_or("base")
}

// Whether a body of `walker`'s size has room with its feet's cell at `feet`, its cells all
// loaded, clear and out of water, up from there if `up` is 1 or down if it's -1.
fn has_room(world: &World, walker: Walker, feet: I64Vec3, up: i64) -> bool {
    let half = walker.across / 2;
    (-half..=half).all(|x| {
        (-half..=half).all(|z| {
            (0..walker.tall).all(|y| {
                let cell = feet + I64Vec3::new(x, y * up, z);
                let block = world.get_block_at(cell);
                world.is_chunk_generated(ChunkCoordinate::containing(cell))
                    && block.is_replaceable()
                    && block.fluid().is_none()
            })
        })
    })
}

/// Where a creature of `walker`'s size and `height` can appear in the column at `column`,
/// within `SURFACE_RANGE` of `y`, as `spawns` allows: on one of its blocks, or hanging under
/// one, with room for its body. One that must be under the sky only on the highest, and the
/// rest in any of the places, chosen at random, under ground if it must be, in the dark if it
/// must be (by `night` or out of the sky, and with no block giving light near it), and in its
/// band of heights. At its feet.
#[allow(clippy::too_many_arguments)]
fn place(
    world: &World,
    spawns: &Spawns,
    walker: Walker,
    height: f32,
    column: Vec2,
    y: f32,
    night: bool,
    rng: &mut impl Rng,
) -> Option<Vec3> {
    let column = column.floor().as_i64vec2();
    let top = y.floor() as i64 + SURFACE_RANGE;
    let mut places = Vec::new();
    for y in (top - 2 * SURFACE_RANGE..=top).rev() {
        let at = I64Vec3::new(column.x, y, column.y);
        if !world.is_chunk_generated(ChunkCoordinate::containing(at)) {
            continue;
        }
        let block = world.get_block_at(at);
        if !block.is_solid() {
            continue;
        }
        let ours = spawns.on.contains(&block);
        if spawns.ceiling {
            let under = at - I64Vec3::Y;
            if ours && has_room(world, walker, under, -1) {
                places.push((under, at.as_vec3() - Vec3::Y * height));
            }
        } else {
            let feet = at + I64Vec3::Y;
            if ours && has_room(world, walker, feet, 1) {
                places.push((feet, feet.as_vec3()));
            }
            // the highest solid block's the only one under the sky
            if spawns.sky == Sky::Open {
                break;
            }
        }
    }
    let &(cell, feet) = places.choose(rng)?;
    let dark = || world.block_light(cell) == 0 && (night || world.is_under_ground(cell));
    let sky_suits = match spawns.sky {
        Sky::Open => !world.is_under_ground(cell),
        Sky::Under => world.is_under_ground(cell),
        Sky::Either => true,
    };
    let suits = sky_suits
        && spawns.heights.contains(&cell.y)
        && (!spawns.dark || dark())
        && world.is_chunk_generated(chunk_of(cell.as_vec3()));
    suits.then_some(feet + Vec3::new(0.5, 0.0, 0.5))
}
