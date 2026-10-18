//! Monsters hunting players (M3.3): noticing them in sight, going after them and striking at
//! them once they've wound up a blow, leaping at them, or throwing stones from afar, which fly
//! under gravity until they hit something. And their going: despawned once nobody's near, and
//! fading by day out under the sky.

use bevy::prelude::*;
use bevy_replicon::prelude::*;

use super::{cell_of, view_cell, Body, Doing, Mind, Whereabouts};
use crate::chunks::chunk::ChunkCoordinate;
use crate::clock::{is_night, TimeOfDay};
use crate::collision::Aabb;
use crate::creatures::{Attack, Creature, CreatureKind, Missile, Stance, Vanishing};
use crate::game_mode::GameMode;
use crate::items::Item;
use crate::player::{Player, BODY};
use crate::server::{Blow, HeldItem};
use crate::survival::Vitals;
use crate::world::World;

/// How often a monster looks about for prey, and for light it shuns, in seconds.
pub(super) const LOOK_INTERVAL: f32 = 0.5;
// How much further than it notices them a monster keeps after its prey before it gives up.
const LEASH: f32 = 1.5;
// How near another of its kind going after someone a monster joins in, in blocks.
pub(super) const PACK: f32 = 10.0;
// How far across from under it, in blocks, a player sets off a monster hanging from the ceiling.
pub(super) const AMBUSH: f32 = 3.0;
// How near its prey a monster runs rather than stalks, in blocks.
pub(super) const RUNS_FROM: f32 = 8.0;
// How much further than its reach a blow still lands, as its prey moves while it swings.
const STRIKE_SLACK: f32 = 0.6;
// A leap: how fast across and up, in blocks a second.
const LEAP: f32 = 9.0;
const LEAP_LIFT: f32 = 6.0;
// Where a monster throws from, as a fraction of its height, and aims at, in blocks over the
// player's feet.
const HAND: f32 = 0.8;
pub(super) const CHEST: f32 = 1.2;
// How far a missile looks along its way for something to hit each step, in blocks, and how big
// it is, and how long it flies at most, in seconds.
const MISSILE_STEP: f32 = 0.25;
const MISSILE_SIZE: f32 = 0.15;
const MISSILE_LIFE: f32 = 5.0;
/// Monsters further than this from every player, in blocks, despawn.
pub const DESPAWN_DISTANCE: f32 = 64.0;
// How far apart a line of sight's checked, in blocks.
const SIGHT_STEP: f32 = 0.3;
// How bright block light must be for a monster that shuns it to run from it, and how near a
// player must be holding something that gives that much, in blocks.
const BRIGHT: u8 = 8;
const HELD_LIGHT: f32 = 6.0;

/// How a monster's fight is going: when its wound-up blow lands, if it's winding one up, when it
/// can wind up another, when it next looks about, whether it saw its prey then, and where the
/// light it shuns was if it was too bright, all by the server's clock in seconds.
#[derive(Component, Debug, Default)]
pub struct Fight {
    pub(super) strikes_at: Option<f32>,
    pub(super) ready_at: f32,
    pub(super) looks_at: f32,
    pub(super) sees: bool,
    pub(super) glare: Option<Vec3>,
}

/// A missile in flight: what threw it and when, and how hard it hits.
#[derive(Component, Debug, Clone, Copy)]
pub struct Flight {
    by: CreatureKind,
    thrown_at: f32,
    damage: u8,
    knockback: f32,
}

/// Whether nothing that keeps light out is on the straight line from `from` to `to`, so one
/// can see the other.
pub(super) fn sees(world: &World, from: Vec3, to: Vec3) -> bool {
    let steps = (from.distance(to) / SIGHT_STEP).ceil() as usize;
    (1..steps).all(|step| {
        let at = from.lerp(to, step as f32 / steps as f32);
        !world.get_block_at(at.floor().as_i64vec3()).blocks_light()
    })
}

/// How far apart two boxes are, 0 if they touch.
pub(super) fn gap(a: Aabb, b: Aabb) -> f32 {
    let apart = (a.min - b.max).max(b.min - a.max).max(Vec3::ZERO);
    apart.length()
}

/// Where a creature's eyes are, near enough, with its feet at `at`.
pub(super) fn eye(kind: CreatureKind, at: Vec3) -> Vec3 {
    at + Vec3::Y * kind.properties().neck.y
}

/// `aabb` moved to `at`.
pub(super) fn placed(aabb: Aabb, at: Vec3) -> Aabb {
    Aabb {
        min: at + aabb.min,
        max: at + aabb.max,
    }
}

/// The player a monster at `at` that sees as far as `sight` would go after, if any: the nearest
/// in survival and alive it can see.
pub(super) fn notice<'a>(
    world: &World,
    kind: CreatureKind,
    at: Vec3,
    sight: f32,
    players: impl Iterator<Item = (Entity, Vec3, &'a Vitals, &'a GameMode)>,
) -> Option<Entity> {
    let eyes = eye(kind, at);
    players
        .filter(|&(_, feet, vitals, mode)| is_prey(vitals, mode) && feet.distance(at) <= sight)
        .filter(|&(_, feet, ..)| sees(world, eyes, feet + Vec3::Y * CHEST))
        .min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at)))
        .map(|(player, ..)| player)
}

/// Where the light a monster at `at` shuns comes from, if it's too bright there: the nearest
/// block lighting it, or a player near holding something that gives light.
pub(super) fn glare<'a>(
    world: &World,
    at: Vec3,
    mut holding: impl Iterator<Item = (&'a Transform, &'a HeldItem)>,
) -> Option<Vec3> {
    let cell = cell_of(at);
    if world.block_light(cell) >= BRIGHT {
        let nearest = world.emitters_near(cell).first().copied();
        return nearest.map(|(from, _)| from.as_vec3() + Vec3::splat(0.5));
    }
    let lights = |held: &HeldItem| {
        let block = held.0.and_then(Item::places);
        block.is_some_and(|block| block.emits() >= BRIGHT)
    };
    holding
        .find(|(body, held)| lights(held) && body.translation.distance(at) <= HELD_LIGHT)
        .map(|(body, _)| body.translation)
}

/// Whether a player can be hunted: alive, and in survival, as nothing hurts them otherwise.
pub(super) fn is_prey(vitals: &Vitals, mode: &GameMode) -> bool {
    !vitals.is_dead() && *mode == GameMode::Survival
}

/// Whether a monster that notices players as far as `sight` keeps after one at `prey`.
pub(super) fn keeps_after(at: Vec3, prey: Vec3, sight: f32) -> bool {
    prey.distance(at) <= sight * LEASH
}

type Fighting<'a> = (
    &'a Creature,
    &'a Transform,
    &'a Mind,
    &'a mut Fight,
    &'a mut Stance,
    &'a mut Body,
);

/// Each monster going after a player fighting them: winding up a blow once they're in reach,
/// or in range to throw at, and its cooldown's over, leaping at them first if its kind leaps,
/// and landing it once it's wound up if they're still in reach, or throwing a stone at them.
pub(in crate::server) fn fight(
    time: Res<Time>,
    world: Res<World>,
    players: Query<&Transform, With<Player>>,
    mut creatures: Query<Fighting>,
    mut blows: MessageWriter<Blow>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    for (creature, body, mind, mut fight, mut stance, mut motion) in &mut creatures {
        let kind = creature.0;
        let Some(attack) = &kind.properties().attack else {
            continue;
        };
        let at = body.translation;
        let prey = match mind.doing {
            Doing::Hunt(player) => players.get(player).ok().map(|p| (player, p.translation)),
            _ => None,
        };
        let Some((player, feet)) = prey else {
            // a blow wound up at nobody goes nowhere
            fight.strikes_at = None;
            stance.winding = false;
            continue;
        };
        let reach = gap(placed(kind.body(false), at), placed(BODY, feet));
        let across = (feet - at).xz().length();
        match fight.strikes_at {
            Some(when) if now >= when => {
                fight.strikes_at = None;
                fight.ready_at = now + attack.cooldown;
                stance.winding = false;
                let eyes = eye(kind, at);
                if !sees(&world, eyes, feet + Vec3::Y * CHEST) {
                    continue;
                }
                match &attack.throws {
                    Some(throw) => {
                        let hand = at + Vec3::Y * kind.properties().height * HAND;
                        if let Some(velocity) = aim(hand, feet + Vec3::Y * CHEST, throw.speed) {
                            commands.spawn(missile(kind, attack, hand, velocity, now));
                        }
                    }
                    None if reach <= attack.reach + STRIKE_SLACK => {
                        blows.write(Blow {
                            player,
                            damage: attack.damage,
                            from: eyes,
                            knockback: attack.knockback,
                            by: kind,
                        });
                    }
                    None => {}
                }
            }
            Some(_) => {}
            None if now >= fight.ready_at && fight.sees => {
                let in_range = match &attack.throws {
                    Some(throw) => across <= throw.keep.end,
                    None => reach <= attack.reach,
                };
                let leaps = attack.lunge > 0.0
                    && reach > attack.reach
                    && across <= attack.lunge
                    && motion.momentum.grounded;
                if leaps {
                    let towards = (feet - at).xz().normalize_or_zero() * LEAP;
                    motion.knockback = Vec3::new(towards.x, 0.0, towards.y);
                    motion.momentum.velocity.y = LEAP_LIFT;
                }
                if in_range || leaps {
                    fight.strikes_at = Some(now + attack.wind_up);
                    stance.winding = true;
                }
            }
            None => {}
        }
    }
}

// A missile thrown by a `kind` from `from` at `velocity`, at `now` by the server's clock.
fn missile(
    kind: CreatureKind,
    attack: &Attack,
    from: Vec3,
    velocity: Vec3,
    now: f32,
) -> impl Bundle {
    (
        Missile { from, velocity },
        Flight {
            by: kind,
            thrown_at: now,
            damage: attack.damage,
            knockback: attack.knockback,
        },
        Transform::from_translation(from),
        Whereabouts(view_cell(from)),
        Replicated,
    )
}

/// How to throw something from `from` at `speed` so it falls on `to`, on the lower of the two
/// arcs that reach it, or none if it's too far to reach.
pub(super) fn aim(from: Vec3, to: Vec3, speed: f32) -> Option<Vec3> {
    let g = crate::creatures::MISSILE_GRAVITY;
    let across = (to - from).xz();
    let (d, h) = (across.length(), to.y - from.y);
    let s2 = speed * speed;
    let root = s2 * s2 - g * (g * d * d + 2.0 * h * s2);
    if root < 0.0 || d < 1e-3 {
        return None;
    }
    let angle = ((s2 - root.sqrt()) / (g * d)).atan();
    let flat = across / d * speed * angle.cos();
    Some(Vec3::new(flat.x, speed * angle.sin(), flat.y))
}

type Flying<'a> = (Entity, &'a Missile, &'a Flight, &'a mut Transform);

/// Flies each missile along its arc, a step at a time: one that hits a player strikes them,
/// and one that hits a block, flies too long or leaves the ground loaded is gone.
pub(in crate::server) fn fly_missiles(
    time: Res<Time>,
    world: Res<World>,
    players: Query<(Entity, &Transform), With<Player>>,
    mut missiles: Query<Flying, Without<Player>>,
    mut blows: MessageWriter<Blow>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let dt = time.delta_secs();
    for (entity, missile, flight, mut body) in &mut missiles {
        let t = now - flight.thrown_at;
        let (was, to) = (missile.at((t - dt).max(0.0)), missile.at(t));
        let steps = (was.distance(to) / MISSILE_STEP).ceil().max(1.0) as usize;
        let mut gone = t > MISSILE_LIFE;
        for step in 1..=steps {
            let at = was.lerp(to, step as f32 / steps as f32);
            let near = Vec3::splat(MISSILE_SIZE);
            let hit = players.iter().find(|(_, player)| {
                let body = placed(BODY, player.translation);
                at.cmpge(body.min - near).all() && at.cmple(body.max + near).all()
            });
            if let Some((player, _)) = hit {
                blows.write(Blow {
                    player,
                    damage: flight.damage,
                    from: at - missile.velocity.normalize_or_zero(),
                    knockback: flight.knockback,
                    by: flight.by,
                });
                gone = true;
                break;
            }
            let block = at.floor().as_i64vec3();
            if !world.is_chunk_generated(ChunkCoordinate::containing(block))
                || world.get_block_at(block).is_solid()
            {
                gone = true;
                break;
            }
        }
        if gone {
            commands.entity(entity).despawn();
        } else {
            body.translation = to;
        }
    }
}

/// Takes away monsters nobody's near, which never see anyone die as no one's near enough to,
/// and has those that fade by day vanish once it's day where they're under the sky.
pub(in crate::server) fn despawn(
    clock: Res<TimeOfDay>,
    world: Res<World>,
    players: Query<&Transform, With<Player>>,
    creatures: Query<(Entity, &Creature, &Transform), Without<Vanishing>>,
    mut commands: Commands,
) {
    let day = !is_night(clock.0);
    for (entity, creature, body) in &creatures {
        let properties = creature.0.properties();
        if !properties.despawns {
            continue;
        }
        let at = body.translation;
        let near = players
            .iter()
            .any(|player| player.translation.distance(at) <= DESPAWN_DISTANCE);
        if !near {
            commands.entity(entity).despawn();
        } else if properties.fades_by_day && day && !world.is_under_ground(cell_of(at)) {
            commands.entity(entity).insert(Vanishing);
        }
    }
}

/// Despawns the creatures vanishing, the frame after they started to, so clients are told
/// they're vanishing first.
pub(in crate::server) fn vanish(
    vanishing: Query<(Entity, Ref<Vanishing>)>,
    mut commands: Commands,
) {
    for (entity, vanishing) in &vanishing {
        if !vanishing.is_added() {
            commands.entity(entity).despawn();
        }
    }
}
