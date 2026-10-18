//! Creatures on the server (§5.8, INV-4): their bodies walking under gravity as players' do,
//! their brains choosing what to do from their kind's behaviours, the ways they find
//! (`creatures::path`), on the compute pool, and follow, being hit and dying, and sleeping,
//! saved, while nobody's near them. Clients draw them from the poses the server sends
//! (`client::creatures`), only those near their player.

mod spawning;
#[cfg(test)]
mod tests;

pub(super) use spawning::spawn;
pub use spawning::SpawnRate;

use std::collections::VecDeque;
use std::f32::consts::TAU;

use bevy::math::{I64Vec2, I64Vec3};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::tasks::{futures, AsyncComputeTaskPool, Task};
use bevy_replicon::prelude::*;
use rand::RngExt;

use super::drops::{Chance, DroppedItem};
use super::edits::REACH_SLACK;
use super::HeldItem;
use crate::block::BlockType;
use crate::chunks::chunk::ChunkCoordinate;
use crate::creatures::path::{find_path, Blocks, Limits, Path, Walker};
use crate::creatures::{Behaviour, Creature, CreatureKind, CreaturePose, Flinches, SavedCreature};
use crate::inventory::{Hotbar, Inventory};
use crate::items::ItemStack;
use crate::player::physics::{walk, Momentum};
use crate::player::{Player, PlayerOwner, EYE_HEIGHT, REACH};
use crate::protocol::HitCreature;
use crate::save::region::Region;
use crate::survival::{fall_damage, Vitals};
use crate::world::World;

// Creatures further than this from every player, in blocks, sleep, and those asleep nearer
// than `WAKE_DISTANCE` wake, if the ground under them is loaded: the gap keeps one at the edge
// from waking and sleeping over and over.
const SLEEP_DISTANCE: f32 = 128.0;
const WAKE_DISTANCE: f32 = 96.0;
// How far a creature drops down in one go when it finds its way, which a fall of hurts nobody.
const DROP: i64 = 3;
// How far a way's looked for, and through how many cells at most, which a few hundred
// microseconds covers.
const PATH_LIMITS: Limits = Limits {
    cells: 1500,
    reach: 24,
};
// How long a creature waits between asking for ways, in seconds, at least.
const REPLAN_INTERVAL: f32 = 0.5;
// How near the middle of the next cell of its way, across, a creature must come to go on to
// the one after.
const ARRIVED: f32 = 0.35;
// How long it can go without getting any nearer to it, in seconds, before it's taken to be
// stuck and finds another way.
const STUCK_SECONDS: f32 = 1.5;
// Turning, in radians a second.
const TURN_SPEED: f32 = 8.0;
// Standing about between wanders, in seconds, and how far a wander goes, in blocks.
const IDLE: std::ops::Range<f32> = 2.0..8.0;
const WANDER: i64 = 8;
// Running from what hurt it, for how long and how far ahead it looks for somewhere to run to.
const FLEE_SECONDS: f32 = 5.0;
const FLEE_DISTANCE: f32 = 10.0;
// How near a player holding a lure must be, in blocks, to be followed, and how near it's
// followed.
const LURE_RANGE: f32 = 8.0;
const FOLLOW_NEAR: i64 = 2;
// Seconds a creature can't be hurt again after it's hurt, which also keeps a flurry of clicks
// from killing it at once.
const HURT_COOLDOWN: f32 = 0.5;
// How hard a hit knocks it back, and up, in blocks a second, and how quickly that dies away,
// as the fraction left after a second.
const KNOCKBACK: f32 = 6.0;
const KNOCKBACK_LIFT: f32 = 5.0;
const KNOCKBACK_LEFT: f32 = 0.002;
// How big a cell of the world a creature's seen by clients in, in blocks, and how many cells
// off it can be, across, from a client's player and still be seen: within 96 to 128 blocks.
const VIEW_CELL: f32 = 32.0;
const VIEW_CELLS: i64 = 3;

/// A creature's health, and until when, by the server's clock in seconds, it can't be hurt.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Health {
    pub points: u8,
    immune_until: f32,
}

/// A creature's body: how it's moving, which way it faces, the highest it's been since it was
/// last on the ground, to hurt it by when it lands, and the push of the last hit.
#[derive(Component, Debug, Default)]
pub struct Body {
    momentum: Momentum,
    yaw: f32,
    peak: Option<f32>,
    knockback: Vec3,
}

/// What a creature's brain is doing, and until when, by the server's clock in seconds, it's
/// fleeing or standing about.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Mind {
    pub doing: Doing,
    until: f32,
}

/// What a creature is doing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Doing {
    /// Standing about.
    Idle,
    /// Walking somewhere near, for nothing in particular.
    Wander,
    /// Running from where it was hurt from.
    Flee { from: Vec3 },
    /// Going after a player: one with something it wants, or, for a monster (M3.3), its prey.
    Follow(Entity),
}

/// The way a creature's walking: the cells still to go through, where it's going, the way
/// being found if one's asked for, and how it's getting on.
#[derive(Component, Debug, Default)]
pub struct Route {
    cells: VecDeque<I64Vec3>,
    goal: Option<I64Vec3>,
    planning: Option<Task<Option<Path>>>,
    planned_at: f32,
    // how near it's been to the next cell lately, and when it last got nearer
    best: f32,
    progressed_at: f32,
}

impl Route {
    /// Whether it has a way to walk or one being found.
    pub fn is_going(&self) -> bool {
        !self.cells.is_empty() || self.planning.is_some()
    }

    fn stop(&mut self) {
        self.cells.clear();
        self.goal = None;
        self.planning = None;
    }

    // Starts finding a way from `from` to within `near` of `goal` for `walker`, on the compute
    // pool, from a copy of the chunks round it.
    fn plan(
        &mut self,
        world: &World,
        from: I64Vec3,
        goal: I64Vec3,
        near: i64,
        walker: Walker,
        now: f32,
    ) {
        let mut blocks = Blocks::around(world, from, PATH_LIMITS.reach + 1);
        self.goal = Some(goal);
        self.planned_at = now;
        self.planning = Some(AsyncComputeTaskPool::get().spawn(async move {
            find_path(from, goal, near, walker, PATH_LIMITS, |at| blocks.get(at))
        }));
    }
}

/// Which cell of the world a creature's in, as clients are shown it by (`VIEW_CELL`).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(immutable)]
pub struct Whereabouts(I64Vec2);

/// On a client, the cell of the world its player's in, which it's shown creatures near.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(immutable)]
pub struct Viewpoint(I64Vec2);

fn view_cell(at: Vec3) -> I64Vec2 {
    (at.xz() / VIEW_CELL).floor().as_i64vec2()
}

impl VisibilityFilter for Whereabouts {
    type ClientComponent = Viewpoint;
    type Scope = Entity;

    fn is_visible(&self, _: Entity, viewpoint: Option<&Viewpoint>) -> bool {
        viewpoint.is_some_and(|view| (view.0 - self.0).abs().max_element() <= VIEW_CELLS)
    }
}

/// A creature as the server spawns it: of `kind`, at `at` with `health`, replicated.
pub fn creature(kind: CreatureKind, at: Vec3, health: u8, yaw: f32) -> impl Bundle {
    (
        Creature(kind),
        Transform::from_translation(at),
        Health {
            points: health,
            immune_until: 0.0,
        },
        Body {
            yaw,
            ..Body::default()
        },
        Mind {
            doing: Doing::Idle,
            until: 0.0,
        },
        Route::default(),
        CreaturePose {
            at: 0,
            position: at,
            yaw,
        },
        Flinches::default(),
        Whereabouts(view_cell(at)),
        Replicated,
    )
}

/// The creatures asleep while nobody's near them, by region, as they're saved.
#[derive(Resource, Debug, Default)]
pub struct Dormant(pub HashMap<Region, Vec<SavedCreature>>);

impl Dormant {
    /// Those saved, asleep until someone comes near.
    pub fn of(saved: impl IntoIterator<Item = SavedCreature>) -> Self {
        let mut dormant = Self::default();
        for creature in saved {
            dormant.keep(creature);
        }
        dormant
    }

    fn keep(&mut self, creature: SavedCreature) {
        let region = Region::containing(chunk_of(creature.position));
        self.0.entry(region).or_default().push(creature);
    }
}

fn chunk_of(at: Vec3) -> ChunkCoordinate {
    ChunkCoordinate::containing(at.floor().as_i64vec3())
}

fn walker(kind: CreatureKind) -> Walker {
    let properties = kind.properties();
    Walker::sized(
        properties.width,
        properties.height,
        DROP,
        !properties.avoids_water,
    )
}

// The cell a body's feet are in.
fn cell_of(feet: Vec3) -> I64Vec3 {
    // a hair up, as feet resting on a block's top are in the cell over it
    (feet + Vec3::Y * 0.01).floor().as_i64vec3()
}

/// Wakes the creatures asleep near players, where the ground under them is loaded.
pub(super) fn wake(
    world: Res<World>,
    mut dormant: ResMut<Dormant>,
    mut chance: ResMut<Chance>,
    players: Query<&Transform, With<Player>>,
    mut commands: Commands,
) {
    if dormant.0.is_empty() {
        return;
    }
    let near = |at: Vec3| {
        players
            .iter()
            .any(|body| body.translation.distance(at) <= WAKE_DISTANCE)
    };
    for body in &players {
        let here = Region::containing(chunk_of(body.translation));
        for x in -1..=1 {
            for z in -1..=1 {
                let region = Region {
                    x: here.x + x,
                    z: here.z + z,
                };
                let Some(asleep) = dormant.0.get_mut(&region) else {
                    continue;
                };
                asleep.retain(|creature| {
                    let wakes = near(creature.position)
                        && world.is_chunk_generated(chunk_of(creature.position));
                    if wakes {
                        let yaw = chance.0.random_range(0.0..TAU);
                        commands.spawn(self::creature(
                            creature.kind,
                            creature.position,
                            creature.health,
                            yaw,
                        ));
                    }
                    !wakes
                });
                if asleep.is_empty() {
                    dormant.0.remove(&region);
                }
            }
        }
    }
}

/// Puts to sleep the creatures far from every player, or whose ground is no longer loaded, to
/// be saved and woken when someone comes near again.
pub(super) fn sleep(
    world: Res<World>,
    mut dormant: ResMut<Dormant>,
    players: Query<&Transform, With<Player>>,
    creatures: Query<(Entity, &Creature, &Transform, &Health)>,
    mut commands: Commands,
) {
    for (entity, creature, body, health) in &creatures {
        let at = body.translation;
        let near = players
            .iter()
            .any(|player| player.translation.distance(at) <= SLEEP_DISTANCE);
        if near && world.is_chunk_generated(chunk_of(at)) {
            continue;
        }
        dormant.keep(SavedCreature {
            kind: creature.0,
            position: at,
            health: health.points,
        });
        commands.entity(entity).despawn();
    }
}

type Players<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Transform,
        &'static HeldItem,
        &'static Vitals,
    ),
    With<Player>,
>;

/// Each creature's brain choosing what to do, the first of its kind's behaviours that has
/// something to do, and asking for a way to go if it needs one.
pub(super) fn think(
    time: Res<Time>,
    world: Res<World>,
    players: Players,
    mut chance: ResMut<Chance>,
    mut creatures: Query<(&Creature, &Transform, &mut Mind, &mut Route)>,
) {
    let now = time.elapsed_secs();
    for (creature, body, mut mind, mut route) in &mut creatures {
        let at = body.translation;
        let mut doing = Doing::Idle;
        for behaviour in creature.0.properties().behaviours {
            let chosen = match *behaviour {
                Behaviour::Flee => match mind.doing {
                    Doing::Flee { from } if now < mind.until => Some(Doing::Flee { from }),
                    _ => None,
                },
                Behaviour::FollowLure(lure) => players
                    .iter()
                    .filter(|(_, player, held, vitals)| {
                        held.0 == Some(lure)
                            && !vitals.is_dead()
                            && player.translation.distance(at) <= LURE_RANGE
                    })
                    .min_by(|a, b| {
                        let distance = |p: &Transform| p.translation.distance_squared(at);
                        distance(a.1).total_cmp(&distance(b.1))
                    })
                    .map(|(player, ..)| Doing::Follow(player)),
                Behaviour::Wander => Some(match mind.doing {
                    Doing::Wander if route.is_going() => Doing::Wander,
                    Doing::Idle if now < mind.until => Doing::Idle,
                    Doing::Idle => Doing::Wander,
                    _ => Doing::Idle,
                }),
            };
            if let Some(chosen) = chosen {
                doing = chosen;
                break;
            }
        }
        if doing != mind.doing {
            route.stop();
            if doing == Doing::Idle {
                mind.until = now + chance.0.random_range(IDLE);
            }
            mind.doing = doing;
        }

        let from = cell_of(at);
        let walker = walker(creature.0);
        let ready = route.planning.is_none() && now >= route.planned_at + REPLAN_INTERVAL;
        match mind.doing {
            Doing::Idle => {}
            Doing::Wander if ready && !route.is_going() => {
                let rng = &mut chance.0;
                let offset = I64Vec3::new(
                    rng.random_range(-WANDER..=WANDER),
                    0,
                    rng.random_range(-WANDER..=WANDER),
                );
                route.plan(&world, from, from + offset, 0, walker, now);
            }
            Doing::Wander => {}
            Doing::Flee { from: danger } if ready && route.cells.is_empty() => {
                let away = (at - danger).xz().normalize_or(Vec2::X);
                let swerve = chance.0.random_range(-0.6..0.6);
                let away = Vec2::from_angle(swerve).rotate(away) * FLEE_DISTANCE;
                let goal = (at + Vec3::new(away.x, 0.0, away.y)).floor().as_i64vec3();
                route.plan(&world, from, goal, 1, walker, now);
            }
            Doing::Flee { .. } => {}
            Doing::Follow(player) => {
                let Ok((_, target, ..)) = players.get(player) else {
                    continue;
                };
                let goal = cell_of(target.translation);
                // again as they move away from where it was going
                let moved = route
                    .goal
                    .is_none_or(|was| (was - goal).abs().max_element() > FOLLOW_NEAR);
                let close = (goal - from).xz().abs().max_element() <= FOLLOW_NEAR;
                if close {
                    route.stop();
                } else if ready && (moved || route.cells.is_empty()) {
                    route.plan(&world, from, goal, FOLLOW_NEAR, walker, now);
                }
            }
        }
    }
}

/// Takes the ways found for creatures as they're ready.
pub(super) fn take_paths(time: Res<Time>, mut creatures: Query<&mut Route>) {
    for mut route in &mut creatures {
        let Some(task) = route.planning.as_mut() else {
            continue;
        };
        let Some(path) = futures::check_ready(task) else {
            continue;
        };
        route.planning = None;
        route.best = f32::INFINITY;
        route.progressed_at = time.elapsed_secs();
        route.cells = path.map(|path| path.cells.into()).unwrap_or_default();
    }
}

type Moving<'a> = (
    Entity,
    &'a Creature,
    &'a mut Transform,
    &'a mut Body,
    &'a mut Route,
    &'a Mind,
    &'a mut Health,
);

/// Walks each creature along its way under gravity, as players walk: towards the middle of
/// its next cell, jumping up a block where the way climbs or it's walked into one, and kept
/// afloat in water, then hurts it for any fall it's landed from. Creatures whose ground isn't
/// loaded wait where they are, as they're soon put to sleep.
pub(super) fn move_creatures(
    time: Res<Time>,
    world: Res<World>,
    players: Query<&Transform, (With<Player>, Without<Creature>)>,
    mut creatures: Query<Moving>,
    mut chance: ResMut<Chance>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    let block = |at: I64Vec3| {
        world
            .is_chunk_generated(ChunkCoordinate::containing(at))
            .then(|| world.get(at))
    };
    for (entity, creature, mut transform, mut body, mut route, mind, mut health) in &mut creatures {
        let mut at = transform.translation;
        if block(at.floor().as_i64vec3() - I64Vec3::Y).is_none() {
            continue;
        }
        let properties = creature.0.properties();
        let speed = match mind.doing {
            Doing::Flee { .. } => properties.run_speed,
            _ => properties.walk_speed,
        };

        // towards the next cell, passing those it's come to
        let mut wanted = Vec3::ZERO;
        let mut climb = false;
        while let Some(&next) = route.cells.front() {
            let target = next.as_vec3() + Vec3::new(0.5, 0.0, 0.5);
            let to = (target - at).xz();
            if to.length() < ARRIVED && (target.y - at.y).abs() < 1.0 {
                route.cells.pop_front();
                route.best = f32::INFINITY;
                continue;
            }
            // walking slower as it nears the end, so it doesn't overshoot
            let last = route.cells.len() == 1;
            let pace = if last {
                speed.min(to.length() * 4.0)
            } else {
                speed
            };
            let direction = to.normalize_or_zero() * pace;
            wanted = Vec3::new(direction.x, 0.0, direction.y);
            climb = target.y > at.y + 0.5;
            if to.length() < route.best - 0.05 {
                route.best = to.length();
                route.progressed_at = now;
            } else if now - route.progressed_at > STUCK_SECONDS {
                // let the brain find another way
                route.stop();
            }
            break;
        }

        let middle = at + Vec3::Y * properties.height / 2.0;
        let in_water = block(middle.floor().as_i64vec3()).map(|b| b.kind) == Some(BlockType::Water);
        let jump = in_water || ((climb || body.momentum.against_wall) && wanted != Vec3::ZERO);
        let push = body.knockback;
        body.knockback *= KNOCKBACK_LEFT.powf(dt);
        at = walk(
            at,
            &mut body.momentum,
            creature.0.body(),
            wanted + push,
            jump,
            dt,
            block,
        );
        if transform.translation != at {
            transform.translation = at;
        }
        // facing the way it's going, or the player it follows when it's caught them up
        let facing = match mind.doing {
            Doing::Follow(player) if wanted == Vec3::ZERO => players
                .get(player)
                .map_or(Vec3::ZERO, |player| player.translation - at),
            _ => wanted,
        };
        if facing.xz() != Vec2::ZERO {
            let yaw = (-facing.x).atan2(-facing.z);
            let turn =
                (yaw - body.yaw + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI;
            let most = TURN_SPEED * dt;
            body.yaw = (body.yaw + turn.clamp(-most, most)).rem_euclid(TAU);
        }

        // falls, as players' hurt (§5.5)
        if body.momentum.grounded || in_water {
            if let Some(peak) = body.peak.take().filter(|_| !in_water) {
                let damage = fall_damage(peak - at.y);
                if damage > 0 {
                    health.points = health.points.saturating_sub(damage);
                    if health.points == 0 {
                        die(entity, creature.0, at, &mut chance, &mut commands);
                    }
                }
            }
        } else {
            body.peak = Some(body.peak.map_or(at.y, |peak| peak.max(at.y)));
        }
    }
}

// A creature dying where its feet are at `at`, dropping what its kind drops.
fn die(entity: Entity, kind: CreatureKind, at: Vec3, chance: &mut Chance, commands: &mut Commands) {
    for (item, counts) in kind.properties().drops {
        let count = chance.0.random_range(counts.clone());
        let stack = ItemStack::new(*item, count);
        let (dropped, transform) = DroppedItem::scattered(stack, at, &mut chance.0);
        commands.spawn((dropped, transform, Replicated));
    }
    commands.entity(entity).despawn();
}

/// Hurts the creatures players hit, if they're alive and it's in their reach: as hard as
/// what they hold hits, knocking it back away from them, and frightening it if it flees.
/// What's held wears as it does breaking a block. One killed drops what its kind drops.
#[allow(clippy::too_many_arguments)]
pub(super) fn receive_hits(
    mut hits: MessageReader<FromClient<HitCreature>>,
    time: Res<Time>,
    mut players: Query<(&PlayerOwner, &Transform, &Vitals, &Hotbar, &mut Inventory)>,
    mut creatures: Query<(
        &Creature,
        &Transform,
        &mut Health,
        &mut Body,
        &mut Mind,
        &mut Flinches,
        &mut Route,
    )>,
    mut chance: ResMut<Chance>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    for FromClient { client_id, message } in hits.read() {
        let Some((_, player, vitals, hotbar, mut inventory)) =
            players.iter_mut().find(|(owner, ..)| owner.0 == *client_id)
        else {
            continue;
        };
        let Ok((creature, body, mut health, mut motion, mut mind, mut flinches, mut route)) =
            creatures.get_mut(message.creature)
        else {
            continue;
        };
        let eye = player.translation + Vec3::Y * EYE_HEIGHT;
        let reach = creature.0.body();
        let nearest = eye.clamp(body.translation + reach.min, body.translation + reach.max);
        if vitals.is_dead()
            || eye.distance(nearest) > REACH + REACH_SLACK
            || now < health.immune_until
        {
            continue;
        }
        let held = hotbar.held(&inventory);
        let damage = held.map_or(1, |stack| stack.item.damage());
        if let Some(stack) = held.filter(|stack| stack.item.tool().is_some()) {
            inventory.slots[hotbar.selected] = stack.worn();
        }
        health.points = health.points.saturating_sub(damage);
        health.immune_until = now + HURT_COOLDOWN;
        flinches.0 = flinches.0.wrapping_add(1);
        let away = (body.translation - player.translation)
            .xz()
            .normalize_or(Vec2::X)
            * KNOCKBACK;
        motion.knockback = Vec3::new(away.x, 0.0, away.y);
        if motion.momentum.grounded {
            motion.momentum.velocity.y = KNOCKBACK_LIFT;
        }
        if creature
            .0
            .properties()
            .behaviours
            .contains(&Behaviour::Flee)
        {
            route.stop();
            *mind = Mind {
                doing: Doing::Flee {
                    from: player.translation,
                },
                until: now + FLEE_SECONDS,
            };
        }
        if health.points == 0 {
            die(
                message.creature,
                creature.0,
                body.translation,
                &mut chance,
                &mut commands,
            );
        }
    }
}

/// Sets each creature's pose to where it is now, for clients to draw, if it's moved or turned
/// since the last.
pub(super) fn send_poses(
    time: Res<Time>,
    mut creatures: Query<(&Transform, &Body, &mut CreaturePose)>,
) {
    let at = time.elapsed().as_millis() as u32;
    for (body, motion, mut pose) in &mut creatures {
        if pose.position != body.translation || pose.yaw != motion.yaw {
            *pose = CreaturePose {
                at,
                position: body.translation,
                yaw: motion.yaw,
            };
        }
    }
}

/// Keeps which cell of the world each creature's in, and each client's player's, up to date,
/// so clients are shown the creatures near them alone.
pub(super) fn follow_whereabouts(
    creatures: Query<(Entity, &Transform, &Whereabouts), Changed<Transform>>,
    players: Query<(&PlayerOwner, &Transform), With<Player>>,
    viewpoints: Query<Option<&Viewpoint>, With<AuthorizedClient>>,
    mut commands: Commands,
) {
    for (entity, body, whereabouts) in &creatures {
        let cell = view_cell(body.translation);
        if cell != whereabouts.0 {
            commands.entity(entity).insert(Whereabouts(cell));
        }
    }
    for (owner, body) in &players {
        let ClientId::Client(client) = owner.0 else {
            continue;
        };
        let cell = view_cell(body.translation);
        if let Ok(viewpoint) = viewpoints.get(client) {
            if viewpoint.is_none_or(|view| view.0 != cell) {
                commands.entity(client).insert(Viewpoint(cell));
            }
        }
    }
}
