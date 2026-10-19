//! Survival on the server (§5.5, INV-4): falling, drowning, blows from creatures through the
//! armour worn, hunger, healing, eating, dying and respawning, for players in survival.
//! Creative takes no damage and needs no food.
//!
//! Movement is the client's to decide (§4.4), so falls are judged from where the server sees a
//! player frame by frame, whether by their own client's poses or, for the host, their body
//! itself, against the ground in the server's own world (`Footing`). The client says nothing
//! about falling, so it can't leave a fall out: a player is hurt when the server finds them
//! standing again, by how far they've come down from the highest it saw them since they last
//! stood or swam. Only moving further in a frame than anyone can (`TELEPORT_DISTANCE`) goes
//! unjudged, as being put somewhere, which plausibility checks will catch as a cheat (M0.6d),
//! and so does falling before a player's first stood since arriving, respawning or being put
//! somewhere, which is them dropping onto ground that's only just loaded under them.

use bevy::math::I64Vec3;
use bevy::prelude::*;
use bevy_replicon::prelude::*;

use super::drops::{Chance, DroppedItem};
use super::players::{announce, PoseCheck};
use crate::block::shape::Use;
use crate::block::BlockType;
use crate::chunks::chunk::ChunkCoordinate;
use crate::collision::boxes_at;
use crate::creatures::CreatureKind;
use crate::game_mode::GameMode;
use crate::inventory::{Inventory, HOTBAR_SLOTS};
use crate::player::name::PlayerName;
use crate::player::{PlayerOwner, BODY, EYE_HEIGHT};
use crate::protocol::{CommandOutput, Eat, Respawn, Struck, Teleport, VitalsChanged};
use crate::survival::{
    fall_damage, on_bed, Asleep, Bruise, SpawnBed, Vitals, MAX_AIR, MAX_FOOD, MAX_HEALTH,
};
use crate::world::World;

// Seconds of breath under water, which the bubbles count down, and how many seconds of it
// come back a second above water.
const BREATH_SECONDS: f32 = 15.0;
const BREATH_REFILL: f32 = 7.5;
// Out of breath, how much drowning hurts and how often, in seconds.
const DROWNING_DAMAGE: u8 = 2;
const DROWNING_INTERVAL: f32 = 1.0;
// In lava, how much it burns and how often, in seconds, and how long a player burns on after
// climbing out, a point a second, unless they put it out in water (M4.3).
const LAVA_DAMAGE: u8 = 4;
const LAVA_INTERVAL: f32 = 0.5;
const ALIGHT_SECONDS: f32 = 8.0;
const FIRE_DAMAGE: u8 = 1;
const FIRE_INTERVAL: f32 = 1.0;
// Food used up, in points of the bar: just living, a point every 35 seconds; sprinting, twice
// as much again; and a little a jump and half a point for each point healed.
const FOOD_PER_SECOND: f32 = 1.0 / 35.0;
const SPRINTING_FOOD_PER_SECOND: f32 = 2.0 / 35.0;
const JUMP_FOOD: f32 = 1.0 / 40.0;
const HEAL_FOOD: f32 = 0.5;
// With more food than this a player heals a point every `HEAL_INTERVAL` seconds, and with none
// hurts a point every `STARVE_INTERVAL`, though never below `STARVING_HEALTH`, half a heart.
const HEAL_ABOVE: u8 = 17;
const HEAL_INTERVAL: f32 = 4.0;
const STARVE_INTERVAL: f32 = 4.0;
const STARVING_HEALTH: u8 = 1;
/// How long eating one takes, in seconds, holding right click.
pub const EATING_SECONDS: f32 = 1.6;
// Faster than this across the ground, in blocks a second over `SPRINT_WINDOW` seconds, a
// player's taken to be sprinting: walking is 4.3 and sprinting 5.6. Over a window, as a
// client's poses come only 20 times a second.
const SPRINT_SPEED: f32 = 5.0;
const SPRINT_WINDOW: f32 = 0.5;
// Further than this from one frame to the next, in blocks, a player's been put somewhere
// (teleported, or respawned) rather than moved: falling at full speed or flying sprinting
// covers 3 or 4 between a client's poses.
const TELEPORT_DISTANCE: f32 = 8.0;
// How close to a block's top, in blocks, feet must be to stand on it.
const STANDING: f32 = 0.05;
// How long after a blow lands, in seconds, another can't, so a crowd's blows at once land as
// one and a player has a moment to get away.
const GRACE: f32 = 0.5;
// How hard a blow lifts a player standing, in blocks a second.
const KNOCK_LIFT: f32 = 4.0;

/// What the server has seen of how a player moves, to judge their falls, jumps and sprinting by.
#[derive(Component, Debug, Default)]
pub(super) struct Footing {
    // where their feet were last frame, and whether they stood on the ground
    last: Option<Vec3>,
    grounded: bool,
    // whether they've stood or swum since they arrived or were put where they are: a fall
    // before then started before the ground under them had loaded, as a player waits in the air
    // for it on arriving, and doesn't count
    stood: bool,
    // the highest their feet have been since they last stood or swam
    peak: Option<f32>,
    // how far they've gone across the ground, and in how long, in this sprint window
    window: (f32, f32),
}

/// How a player stands in a fight: until when, by the server's clock in seconds, no blow can
/// land on them, and when they can next hit, after their last.
#[derive(Component, Debug, Default)]
pub(super) struct Combat {
    pub(super) safe_until: f32,
    pub(super) ready_at: f32,
}

/// A blow at a player by a creature (M3.3): how hard, from where, how hard it knocks them back,
/// in blocks a second, and what struck it.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct Blow {
    pub player: Entity,
    pub damage: u8,
    pub from: Vec3,
    pub knockback: f32,
    pub by: CreatureKind,
}

/// Where a player's body is, as far as falling and drowning go.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Surroundings {
    /// Standing on the top of a solid block, or of a slab, stairs or anything else solid.
    pub on_ground: bool,
    /// Feet or middle in water, which breaks a fall.
    pub in_water: bool,
    /// Feet or middle in lava, which burns, and breaks a fall too.
    pub in_lava: bool,
    /// On a ladder or vines, which is no fall either.
    pub climbing: bool,
    /// Eyes under water, holding their breath.
    pub head_under: bool,
}

/// What a frame's movement came to.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(super) struct Moved {
    /// How far they fell, if they've landed.
    pub landed: Option<f32>,
    pub jumped: bool,
    /// How many seconds they're found to have sprinted, at the end of a window.
    pub sprinted: f32,
}

impl Footing {
    /// Whether they're coming down from a jump or a fall, as of their last step, which makes
    /// their hits land harder.
    pub(super) fn is_falling(&self) -> bool {
        let below_peak = self
            .last
            .zip(self.peak)
            .is_some_and(|(at, peak)| at.y < peak - 0.1);
        !self.grounded && below_peak
    }

    /// Follows a player to `at` over `dt` seconds, `around` them there.
    pub(super) fn step(&mut self, at: Vec3, around: Surroundings, dt: f32) -> Moved {
        let mut moved = Moved::default();
        let last = self.last.replace(at);
        let Some(last) = last.filter(|last| last.distance(at) <= TELEPORT_DISTANCE) else {
            // just arrived or put here, which is no fall
            *self = Self {
                last: Some(at),
                grounded: around.on_ground,
                stood: around.on_ground || around.in_water || around.in_lava || around.climbing,
                ..Self::default()
            };
            return moved;
        };
        self.window.0 += (at - last).xz().length();
        self.window.1 += dt;
        if self.window.1 >= SPRINT_WINDOW {
            let (distance, time) = std::mem::take(&mut self.window);
            if distance / time > SPRINT_SPEED {
                moved.sprinted = time;
            }
        }
        if around.in_water || around.in_lava || around.climbing {
            self.peak = None;
            self.stood = true;
        } else if around.on_ground {
            moved.landed = self.peak.take().map(|peak| peak - at.y);
            self.stood = true;
        } else if self.stood {
            // from as high as the ground they left, however late the first pose in the air
            let from = if self.grounded { last.y } else { at.y };
            self.peak = Some(self.peak.map_or(from, |peak| peak.max(from)).max(at.y));
            moved.jumped = self.grounded && at.y > last.y;
        }
        self.grounded = around.on_ground;
        moved
    }
}

/// What a body with its feet at `feet` is in, in `world`. Ground that isn't loaded holds nobody
/// up here, unlike in movement: they're judged by what the server knows.
pub(super) fn surroundings(world: &World, feet: Vec3) -> Surroundings {
    let loaded = |at: I64Vec3| world.is_chunk_generated(ChunkCoordinate::containing(at));
    let block = |at: Vec3| {
        let at = at.floor().as_i64vec3();
        loaded(at).then(|| world.get_block_at(at))
    };
    // the corners of the body's footprint, just inside it, as touching a block's side isn't
    // standing on it
    let (min, max) = (BODY.min + 1e-3, BODY.max - 1e-3);
    let corners = [
        (min.x, min.z),
        (min.x, max.z),
        (max.x, min.z),
        (max.x, max.z),
    ];
    // the top of a box of what's under a corner, at most a cell down, or two for a fence's
    let stands_on = |x: f32, z: f32| {
        let corner = Vec3::new(feet.x + x, feet.y - STANDING, feet.z + z)
            .floor()
            .as_i64vec3();
        (0..2).any(|down| {
            let cell = corner - I64Vec3::Y * down;
            loaded(cell)
                && boxes_at(cell, Use::Collision, |at| world.get(at))
                    .iter()
                    .any(|part| {
                        let (low, high) = (cell.as_vec3() + part.min, cell.as_vec3() + part.max);
                        let (x, z) = (feet.x + x, feet.z + z);
                        (low.x..=high.x).contains(&x)
                            && (low.z..=high.z).contains(&z)
                            && (feet.y - high.y).abs() < STANDING
                    })
        })
    };
    let on_ground = corners.into_iter().any(|(x, z)| stands_on(x, z));
    let water = |at: Vec3| block(at) == Some(BlockType::Water);
    let lava = |at: Vec3| block(at) == Some(BlockType::Lava);
    let climbable = |at: Vec3| block(at).is_some_and(|block| block.is_climbable());
    let middle = feet + Vec3::Y * BODY.max.y / 2.0;
    Surroundings {
        on_ground,
        in_water: water(feet) || water(middle),
        in_lava: lava(feet) || lava(middle),
        climbing: climbable(feet) || climbable(middle),
        head_under: water(feet + Vec3::Y * EYE_HEIGHT),
    }
}

/// How a death came about, for the message everyone's sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Death {
    Fell,
    Drowned,
    Burned,
    Slain(CreatureKind),
}

impl Death {
    fn message(self, name: &str) -> String {
        match self {
            Death::Fell => format!("{name} fell from a high place"),
            Death::Drowned => format!("{name} drowned"),
            Death::Burned => format!("{name} tried to swim in lava"),
            Death::Slain(kind) => match &kind.properties().attack {
                Some(attack) => format!("{name} {}", attack.slays),
                None => format!("{name} was slain by a {}", kind.name().replace('_', " ")),
            },
        }
    }
}

/// A player's breath, hunger and healing, as they go on between the whole points `Vitals` has.
#[derive(Component, Debug)]
pub(super) struct Metabolism {
    // seconds of breath left
    breath: f32,
    // seconds towards the next hurt from drowning, healing and starving
    drowning: f32,
    // seconds left alight, and before the next hurt from burning
    alight: f32,
    burning: f32,
    healing: f32,
    starving: f32,
    // food used up towards the next point off the bar
    used: f32,
}

impl Metabolism {
    pub(super) fn of(vitals: Vitals) -> Self {
        Self {
            breath: f32::from(vitals.air) / f32::from(MAX_AIR) * BREATH_SECONDS,
            drowning: 0.0,
            alight: 0.0,
            burning: 0.0,
            healing: 0.0,
            starving: 0.0,
            used: 0.0,
        }
    }

    /// Burns `dt` seconds in lava if they're in it, and alight for a while after they climb
    /// out, unless they're in water, which puts it out. Returns how they died, if they did.
    pub(super) fn burn(
        &mut self,
        vitals: &mut Vitals,
        around: Surroundings,
        dt: f32,
    ) -> Option<Death> {
        if around.in_lava {
            self.alight = ALIGHT_SECONDS;
        } else if around.in_water {
            self.alight = 0.0;
        }
        let (damage, interval) = if around.in_lava {
            (LAVA_DAMAGE, LAVA_INTERVAL)
        } else {
            (FIRE_DAMAGE, FIRE_INTERVAL)
        };
        if self.alight <= 0.0 {
            self.burning = 0.0;
            return None;
        }
        self.alight = (self.alight - dt).max(0.0);
        // counting down to the next burn, the first at once, on stepping in
        self.burning -= dt;
        if self.burning > 0.0 {
            return None;
        }
        self.burning += interval;
        hurt(vitals, damage, Death::Burned)
    }

    /// Lives `dt` seconds of having `moved` with their head under water or not, changing
    /// `vitals`, and returns how they died, if they did.
    pub(super) fn live(
        &mut self,
        vitals: &mut Vitals,
        head_under: bool,
        moved: Moved,
        dt: f32,
    ) -> Option<Death> {
        let mut died = None;
        if head_under {
            self.breath = (self.breath - dt).max(0.0);
            if self.breath == 0.0 {
                self.drowning += dt;
                if self.drowning >= DROWNING_INTERVAL {
                    self.drowning -= DROWNING_INTERVAL;
                    died = hurt(vitals, DROWNING_DAMAGE, Death::Drowned);
                }
            }
        } else {
            self.breath = (self.breath + BREATH_REFILL * dt).min(BREATH_SECONDS);
            self.drowning = 0.0;
        }
        vitals.air = (self.breath / BREATH_SECONDS * f32::from(MAX_AIR)).ceil() as u8;
        if let Some(fallen) = moved.landed {
            died = died.or(hurt(vitals, fall_damage(fallen), Death::Fell));
        }
        if died.is_some() {
            return died;
        }

        self.used += FOOD_PER_SECOND * dt
            + SPRINTING_FOOD_PER_SECOND * moved.sprinted
            + if moved.jumped { JUMP_FOOD } else { 0.0 };
        self.healing = tick(
            self.healing,
            dt,
            vitals.food > HEAL_ABOVE && vitals.health < MAX_HEALTH,
        );
        if self.healing >= HEAL_INTERVAL {
            self.healing -= HEAL_INTERVAL;
            vitals.health += 1;
            self.used += HEAL_FOOD;
        }
        self.starving = tick(
            self.starving,
            dt,
            vitals.food == 0 && vitals.health > STARVING_HEALTH,
        );
        if self.starving >= STARVE_INTERVAL {
            self.starving -= STARVE_INTERVAL;
            vitals.health -= 1;
        }
        while self.used >= 1.0 {
            self.used -= 1.0;
            vitals.food = vitals.food.saturating_sub(1);
        }
        None
    }
}

// A timer run on by `dt` while `running`, and back to nothing while it isn't.
fn tick(timer: f32, dt: f32, running: bool) -> f32 {
    if running {
        timer + dt
    } else {
        0.0
    }
}

// Takes `damage` from `vitals`, returning `death` if it's the end of them.
fn hurt(vitals: &mut Vitals, damage: u8, death: Death) -> Option<Death> {
    let was = vitals.health;
    vitals.health = was.saturating_sub(damage);
    (was > 0 && vitals.health == 0).then_some(death)
}

/// A player holding right click to eat the food in hotbar slot `slot`, and how long it has to go.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(super) struct Eating {
    slot: usize,
    left: f32,
}

/// Starts each player following their survival as they arrive, from how they were saved.
pub(super) fn start_living(add: On<Add<Vitals>>, vitals: Query<&Vitals>, mut commands: Commands) {
    if let Ok(&vitals) = vitals.get(add.entity) {
        commands.entity(add.entity).insert((
            Footing::default(),
            Metabolism::of(vitals),
            Combat::default(),
            Told::default(),
        ));
    }
}

/// Starts and stops players eating as they ask: one in survival, alive, hungry and holding food.
pub(super) fn receive_eating(
    mut requests: MessageReader<FromClient<Eat>>,
    players: Query<(Entity, &PlayerOwner, &GameMode, &Vitals, &Inventory)>,
    mut commands: Commands,
) {
    for &FromClient {
        client_id,
        message: Eat(slot),
    } in requests.read()
    {
        let Some((player, _, mode, vitals, inventory)) =
            players.iter().find(|(_, owner, ..)| owner.0 == client_id)
        else {
            continue;
        };
        let slot = slot.map(usize::from).filter(|&slot| slot < HOTBAR_SLOTS);
        match slot.filter(|&slot| can_eat(*mode, *vitals, inventory, slot)) {
            Some(slot) => commands.entity(player).insert(Eating {
                slot,
                left: EATING_SECONDS,
            }),
            None => commands.entity(player).remove::<Eating>(),
        };
    }
}

// Whether a player can eat what's in `slot`: they're in survival, alive and not full, and it's
// food.
fn can_eat(mode: GameMode, vitals: Vitals, inventory: &Inventory, slot: usize) -> bool {
    mode == GameMode::Survival
        && !vitals.is_dead()
        && vitals.food < MAX_FOOD
        && inventory.slots[slot].is_some_and(|stack| stack.item.fills().is_some())
}

type Living<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        Option<&'static PlayerName>,
        &'static Transform,
        &'static GameMode,
        &'static mut Vitals,
        &'static mut Footing,
        &'static mut Metabolism,
        &'static mut Inventory,
        Option<&'static mut Eating>,
        &'static mut Combat,
        &'static mut Bruise,
    ),
>;

/// Each frame of survival for every player: falling, the blows that land on them, breathing,
/// hunger, healing, starving and eating, and dying of it. Everyone's told of a death in the
/// command bar.
#[allow(clippy::too_many_arguments)]
pub(super) fn live(
    time: Res<Time>,
    world: Res<World>,
    mut blows: MessageReader<Blow>,
    mut players: Living,
    mut chance: ResMut<Chance>,
    mut output: MessageWriter<ToClients<CommandOutput>>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    let blows: Vec<Blow> = blows.read().copied().collect();
    for (
        player,
        name,
        body,
        mode,
        mut vitals,
        mut footing,
        mut metabolism,
        mut inventory,
        eating,
        mut combat,
        mut bruise,
    ) in &mut players
    {
        let around = surroundings(&world, body.translation);
        let moved = footing.step(body.translation, around, dt);
        if *mode != GameMode::Survival || vitals.is_dead() {
            continue;
        }
        let mut now = *vitals;
        let mut died = None;
        for blow in blows.iter().filter(|blow| blow.player == player) {
            if time.elapsed_secs() < combat.safe_until {
                break;
            }
            combat.safe_until = time.elapsed_secs() + GRACE;
            died = died.or(hurt(
                &mut now,
                inventory.blunt(blow.damage),
                Death::Slain(blow.by),
            ));
            inventory.wear_armour();
            let away = (body.translation - blow.from).xz().normalize_or_zero() * blow.knockback;
            let lift = if blow.knockback > 0.0 {
                KNOCK_LIFT
            } else {
                0.0
            };
            *bruise = Bruise {
                from: blow.from,
                push: Vec3::new(away.x, lift, away.y),
                count: bruise.count.wrapping_add(1),
            };
        }
        let died = died
            .or_else(|| metabolism.burn(&mut now, around, dt))
            .or_else(|| metabolism.live(&mut now, around.head_under, moved, dt));
        if let Some(mut eating) = eating {
            eating.left -= dt;
            if !can_eat(*mode, now, &inventory, eating.slot) {
                commands.entity(player).remove::<Eating>();
            } else if eating.left <= 0.0 {
                let stack = inventory.slots[eating.slot].expect("food to eat");
                let fills = stack.item.fills().expect("food to eat");
                inventory.use_one(eating.slot, stack.item);
                now.food = (now.food + fills).min(MAX_FOOD);
                // and on to the next, as long as the mouse is held
                eating.left = EATING_SECONDS;
            }
        }
        vitals.set_if_neq(now);
        if let Some(death) = died {
            let said = death.message(name.map_or("Player", PlayerName::as_str));
            info!("{said}");
            announce(&mut output, said);
            // everything they carried, where they fell
            for stack in inventory.empty() {
                commands.spawn((
                    DroppedItem::scattered(stack, body.translation, &mut chance.0),
                    Replicated,
                ));
            }
            commands.entity(player).remove::<(Eating, Asleep)>();
        }
    }
}

type Respawning<'a> = (
    &'a PlayerOwner,
    &'a mut Transform,
    &'a mut Vitals,
    &'a mut SpawnBed,
    &'a mut Footing,
    &'a mut Metabolism,
    Option<&'a mut PoseCheck>,
);

/// Brings dead players back as they ask, whole and fed, on the bed they last used if it's
/// still there, and otherwise at the world's spawn.
pub(super) fn receive_respawns(
    mut requests: MessageReader<FromClient<Respawn>>,
    world: Res<World>,
    mut players: Query<Respawning>,
    mut teleports: MessageWriter<ToClients<Teleport>>,
    mut output: MessageWriter<ToClients<CommandOutput>>,
) {
    for &FromClient { client_id, .. } in requests.read() {
        let Some((_, mut body, mut vitals, mut bed, mut footing, mut metabolism, check)) =
            players.iter_mut().find(|(owner, ..)| owner.0 == client_id)
        else {
            continue;
        };
        if !vitals.is_dead() {
            continue;
        }
        let at = match bed.0 {
            Some(at) if world.get_block_at(at) == BlockType::Bed => on_bed(at),
            Some(_) => {
                bed.0 = None;
                output.write(ToClients {
                    targets: SendTargets::Single(client_id),
                    message: CommandOutput {
                        text: "Your bed was missing, so you're back at the world's spawn".into(),
                        error: false,
                    },
                });
                world.spawn()
            }
            None => world.spawn(),
        };
        body.translation = at;
        *vitals = Vitals::default();
        *footing = Footing::default();
        *metabolism = Metabolism::of(*vitals);
        // the host's body is the server's own, and a client's moves where it's told, its
        // poses from before then ignored
        if let Some(mut check) = check {
            check.teleported();
            teleports.write(ToClients {
                targets: SendTargets::Single(client_id),
                message: Teleport { position: at },
            });
        }
    }
}

/// What a client was last told of its player, to tell it again only when that's changed.
#[derive(Component, Debug, Default)]
pub(super) struct Told(Option<VitalsChanged>);

/// Tells each client of the blows its player takes, to feel them, as the host feels its own.
pub(super) fn send_bruises(
    players: Query<(&PlayerOwner, &Bruise), Changed<Bruise>>,
    mut struck: MessageWriter<ToClients<Struck>>,
) {
    for (owner, &bruise) in &players {
        if owner.0 != ClientId::Server && bruise.count > 0 {
            struck.write(ToClients {
                targets: SendTargets::Single(owner.0),
                message: Struck(bruise),
            });
        }
    }
}

/// Tells each client of its player's vitals, and whether they're asleep, when they change.
pub(super) fn send_vitals(
    mut players: Query<(&PlayerOwner, &Vitals, Has<Asleep>, &mut Told)>,
    mut changes: MessageWriter<ToClients<VitalsChanged>>,
) {
    for (owner, &vitals, asleep, mut told) in &mut players {
        let now = VitalsChanged { vitals, asleep };
        if owner.0 == ClientId::Server || told.0 == Some(now) {
            continue;
        }
        told.0 = Some(now);
        changes.write(ToClients {
            targets: SendTargets::Single(owner.0),
            message: now,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::math::I64Vec3;
    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;

    use super::*;
    use crate::chunks::chunk::ChunkData;
    use crate::items::{Item, ItemStack};
    use crate::player::Player;
    use crate::protocol::ProtocolPlugin;
    use crate::server::ServerPlugin;
    use crate::world::World;

    const FRAME: f32 = 0.05;
    const TEST_BED: I64Vec3 = I64Vec3::new(3, 4, 0);
    const FLOOR: Vec3 = Vec3::new(0.5, 4.0, 0.5);
    const STILL: Surroundings = Surroundings {
        on_ground: true,
        in_water: false,
        in_lava: false,
        climbing: false,
        head_under: false,
    };
    const AIR: Surroundings = Surroundings {
        on_ground: false,
        ..STILL
    };

    // Follows feet down from `from` to `to` at 20 blocks a second, then standing there, in
    // `around` as they land, returning how far they're found to have fallen.
    fn drop(footing: &mut Footing, from: f32, to: f32, around: Surroundings) -> Option<f32> {
        footing.step(Vec3::new(0.5, from, 0.5), STILL, FRAME);
        let mut y = from;
        let mut landed = None;
        while landed.is_none() && y > to - 1.0 {
            y = (y - 20.0 * FRAME).max(to);
            let here = if y == to { around } else { AIR };
            landed = footing.step(Vec3::new(0.5, y, 0.5), here, FRAME).landed;
            if y == to {
                break;
            }
        }
        landed
    }

    #[test]
    fn test_a_fall_is_measured_from_the_ground_left_to_the_ground_landed_on() {
        let mut footing = Footing::default();
        let fallen = drop(&mut footing, 20.0, 10.0, STILL).unwrap();
        assert!((fallen - 10.0).abs() < 1e-5, "{fallen}");
        assert_eq!(7, fall_damage(fallen));
        // and a little drop or a jump is no fall to speak of
        assert_eq!(
            0,
            fall_damage(drop(&mut footing, 13.0, 10.0, STILL).unwrap())
        );
    }

    #[test]
    fn test_a_fall_into_water_is_no_fall() {
        let mut footing = Footing::default();
        let water = Surroundings {
            on_ground: false,
            in_water: true,
            climbing: false,
            head_under: true,
            ..Surroundings::default()
        };
        assert_eq!(None, drop(&mut footing, 40.0, 10.0, water));
        // nor is climbing out of it onto the bank
        let landed = footing.step(Vec3::new(0.5, 11.0, 0.5), STILL, FRAME).landed;
        assert_eq!(None, landed);
    }

    #[test]
    fn test_falls_are_judged_from_poses_however_sparse() {
        // a client's poses, the highest seen in the air, then on the ground
        let mut footing = Footing::default();
        footing.step(Vec3::new(0.0, 30.0, 0.0), STILL, FRAME);
        footing.step(Vec3::new(1.0, 30.8, 0.0), AIR, FRAME);
        footing.step(Vec3::new(2.0, 28.0, 0.0), AIR, FRAME);
        footing.step(Vec3::new(2.0, 25.0, 0.0), AIR, FRAME);
        let landed = footing.step(Vec3::new(2.0, 24.0, 0.0), STILL, FRAME).landed;
        assert!((landed.unwrap() - 6.8).abs() < 1e-5, "{landed:?}");
        // a jump off a ledge counts from the ledge, though the first pose in the air is lower
        footing.step(Vec3::new(2.0, 24.0, 0.0), STILL, FRAME);
        footing.step(Vec3::new(2.0, 23.0, 0.0), AIR, FRAME);
        let landed = footing.step(Vec3::new(2.0, 19.0, 0.0), STILL, FRAME).landed;
        assert_eq!(Some(5.0), landed);
        // and being put somewhere far below isn't a fall
        footing.step(Vec3::new(2.0, 19.0, 0.0), AIR, FRAME);
        let landed = footing.step(Vec3::new(2.0, 5.0, 0.0), STILL, FRAME).landed;
        assert_eq!(None, landed);
    }

    #[test]
    fn test_jumps_and_sprints_are_seen_from_movement() {
        let mut footing = Footing::default();
        footing.step(Vec3::ZERO, STILL, FRAME);
        let jump = footing.step(Vec3::Y * 0.4, AIR, FRAME);
        assert!(jump.jumped);
        // walking a second, then sprinting one
        let mut sprinted = 0.0;
        for (speed, frames) in [(4.3, 20), (5.6, 20)] {
            for _ in 0..frames {
                let at = footing.last.unwrap() + Vec3::X * speed * FRAME;
                let moved = footing.step(Vec3::new(at.x, 0.0, 0.0), STILL, FRAME);
                assert!(!moved.jumped);
                sprinted += moved.sprinted;
            }
        }
        assert!((sprinted - 1.0).abs() < 1e-4, "{sprinted}");
    }

    #[test]
    fn test_the_ground_is_the_top_of_a_solid_block_under_the_body() {
        let mut world = World::new(16);
        world.insert_chunk(ChunkCoordinate(I64Vec3::ZERO), ChunkData::default());
        world.edit(I64Vec3::new(0, 4, 0), BlockType::Stone);
        world.edit(I64Vec3::new(5, 4, 0), BlockType::Water);
        world.edit(I64Vec3::new(5, 5, 0), BlockType::Water);
        world.edit(I64Vec3::new(5, 6, 0), BlockType::Water);
        let on = |x: f32, y: f32| surroundings(&world, Vec3::new(x, y, 0.5));
        assert!(on(0.5, 5.0).on_ground);
        // a corner over it is enough
        assert!(on(1.25, 5.0).on_ground);
        assert!(!on(1.35, 5.0).on_ground);
        assert!(!on(0.5, 5.3).on_ground);
        assert!(!on(0.5, 6.0).on_ground);
        let swimming = on(5.5, 4.2);
        assert!(swimming.in_water && swimming.head_under && !swimming.on_ground);
        let wading = on(5.5, 6.0);
        assert!(wading.in_water && !wading.head_under);
        // and nothing not loaded holds anyone up
        assert_eq!(Surroundings::default(), on(40.5, 5.0));
    }

    // Lives `seconds` from `vitals`, with the head under water or not, returning how they are.
    fn live_for(
        vitals: &mut Vitals,
        head_under: bool,
        moved: Moved,
        seconds: f32,
    ) -> Option<Death> {
        let mut metabolism = Metabolism::of(*vitals);
        (0..(seconds / FRAME).round() as usize)
            .find_map(|_| metabolism.live(vitals, head_under, moved, FRAME))
    }

    #[test]
    fn test_drowning_hurts_every_second_once_breath_runs_out() {
        // too hungry to heal
        let mut vitals = Vitals {
            food: HEAL_ABOVE,
            ..Vitals::default()
        };
        assert_eq!(None, live_for(&mut vitals, true, Moved::default(), 7.5));
        assert_eq!((MAX_HEALTH, MAX_AIR / 2), (vitals.health, vitals.air));
        let mut metabolism = Metabolism::of(vitals);
        let mut hurt_at = vec![];
        for frame in 1..=400 {
            let was = vitals.health;
            metabolism.live(&mut vitals, true, Moved::default(), FRAME);
            if vitals.health < was {
                hurt_at.push(frame as f32 * FRAME);
            }
        }
        assert_eq!(0, vitals.air);
        // out of breath 7.5 seconds on, and hurt each second after
        assert!((hurt_at[0] - 8.5).abs() < 0.06, "{hurt_at:?}");
        assert!((hurt_at[1] - hurt_at[0] - 1.0).abs() < 0.06, "{hurt_at:?}");
        // and dead ten seconds after that
        assert!(vitals.is_dead());
        assert!((hurt_at.last().unwrap() - 17.5).abs() < 0.06, "{hurt_at:?}");

        // while breath comes back in a couple of seconds
        let mut vitals = Vitals {
            air: 0,
            ..Vitals::default()
        };
        live_for(&mut vitals, false, Moved::default(), 2.0);
        assert_eq!(MAX_AIR, vitals.air);
    }

    #[test]
    fn test_lava_burns_fast_and_on_after_climbing_out_unless_water_puts_it_out() {
        let lava = Surroundings {
            in_lava: true,
            ..Surroundings::default()
        };
        let wet = Surroundings {
            in_water: true,
            ..Surroundings::default()
        };
        let mut vitals = Vitals::default();
        let mut metabolism = Metabolism::of(vitals);
        let mut burn = |vitals: &mut Vitals, around, seconds: f32| {
            let was = vitals.health;
            for _ in 0..(seconds / FRAME).round() as usize {
                metabolism.burn(vitals, around, FRAME);
            }
            was - vitals.health
        };
        // at once and every half second in it
        assert_eq!(2 * LAVA_DAMAGE, burn(&mut vitals, lava, 0.9));
        // a point a second out of it
        let burnt = burn(&mut vitals, Surroundings::default(), 3.0);
        assert!((2..=3).contains(&burnt), "{burnt}");
        // until water puts it out
        assert_eq!(0, burn(&mut vitals, wet, 0.1));
        assert_eq!(0, burn(&mut vitals, Surroundings::default(), 3.0));
        // and long enough in it kills
        let mut vitals = Vitals::default();
        let mut metabolism = Metabolism::of(vitals);
        let died =
            (0..(5.0 / FRAME) as usize).find_map(|_| metabolism.burn(&mut vitals, lava, FRAME));
        assert_eq!(Some(Death::Burned), died);
        assert_eq!("Ann tried to swim in lava", Death::Burned.message("Ann"));
    }

    #[test]
    fn test_drowning_and_landing_say_how_they_killed() {
        let mut vitals = Vitals::default();
        assert_eq!(
            Some(Death::Drowned),
            live_for(&mut vitals, true, Moved::default(), 60.0)
        );
        let mut vitals = Vitals {
            health: 4,
            ..Vitals::default()
        };
        let landed = Moved {
            landed: Some(7.0),
            ..Moved::default()
        };
        let mut metabolism = Metabolism::of(vitals);
        assert_eq!(
            Some(Death::Fell),
            metabolism.live(&mut vitals, false, landed, FRAME)
        );
        assert_eq!("Player 3 drowned", Death::Drowned.message("Player 3"));
    }

    // How long it takes to use up a point of food, standing still, sprinting or jumping.
    fn seconds_a_point(moved: impl Fn(usize) -> Moved) -> f32 {
        let mut vitals = Vitals {
            // too hungry to heal
            food: 10,
            ..Vitals::default()
        };
        let mut metabolism = Metabolism::of(vitals);
        let frames = (0..).find(|&frame| {
            metabolism.live(&mut vitals, false, moved(frame), FRAME);
            vitals.food < 10
        });
        frames.unwrap() as f32 * FRAME
    }

    #[test]
    fn test_hunger_drains_slowly_and_faster_sprinting_or_jumping() {
        let still = seconds_a_point(|_| Moved::default());
        assert!((30.0..=40.0).contains(&still), "{still}");
        let sprinting = seconds_a_point(|frame| Moved {
            sprinted: if frame % 10 == 9 { 10.0 * FRAME } else { 0.0 },
            ..Moved::default()
        });
        assert!(sprinting < still / 2.5, "{sprinting}");
        // jumping once a second
        let jumping = seconds_a_point(|frame| Moved {
            jumped: frame % 20 == 0,
            ..Moved::default()
        });
        assert!(jumping < still * 0.7 && jumping > sprinting, "{jumping}");
    }

    #[test]
    fn test_a_full_bar_heals_using_food_and_an_empty_one_hurts_down_to_half_a_heart() {
        let mut vitals = Vitals {
            health: 10,
            ..Vitals::default()
        };
        // healing a point every 4 seconds, until the food's down to 17 a few points later
        live_for(&mut vitals, false, Moved::default(), 30.0);
        assert!((14..=15).contains(&vitals.health), "{vitals:?}");
        assert_eq!(HEAL_ABOVE, vitals.food);
        let healed = vitals.health;
        live_for(&mut vitals, false, Moved::default(), 30.0);
        assert_eq!(healed, vitals.health);

        let mut vitals = Vitals {
            health: 5,
            food: 0,
            ..Vitals::default()
        };
        assert_eq!(None, live_for(&mut vitals, false, Moved::default(), 9.0));
        assert_eq!(3, vitals.health);
        assert_eq!(None, live_for(&mut vitals, false, Moved::default(), 60.0));
        assert_eq!((STARVING_HEALTH, 0), (vitals.health, vitals.food));
    }

    // A server alone with its player in survival, standing on stone at `FLOOR` in chunks loaded
    // round it, with a bed on the floor and a pool of water three deep at x 8.
    fn server() -> (App, Entity) {
        let mut world = World::new(16);
        for x in -1..=1 {
            for y in 0..=1 {
                for z in -1..=1 {
                    let coord = ChunkCoordinate(I64Vec3::new(x, y, z));
                    world.insert_chunk(coord, ChunkData::default());
                }
            }
        }
        for x in -8..16 {
            for z in -8..8 {
                world.edit(I64Vec3::new(x, 3, z), BlockType::Stone);
            }
        }
        for y in 3..6 {
            world.edit(I64Vec3::new(8, y, 0), BlockType::Water);
        }
        world.edit(TEST_BED, BlockType::Bed);
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins,
            ProtocolPlugin,
            ServerPlugin,
        ))
        .insert_resource(world)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            FRAME,
        )));
        let player = app
            .world_mut()
            .spawn((
                Player::default(),
                PlayerOwner(ClientId::Server),
                GameMode::Survival,
                Transform::from_translation(FLOOR),
            ))
            .id();
        app.finish();
        app.update();
        (app, player)
    }

    fn run(app: &mut App, seconds: f32) {
        for _ in 0..(seconds / FRAME).round() as usize {
            app.update();
        }
    }

    fn vitals(app: &App, player: Entity) -> Vitals {
        *app.world().get::<Vitals>(player).unwrap()
    }

    fn put(app: &mut App, player: Entity, at: Vec3) {
        app.world_mut()
            .get_mut::<Transform>(player)
            .unwrap()
            .translation = at;
    }

    // Takes the player up from the floor to `height`, as if they'd climbed, and drops them back
    // onto it at 20 blocks a second.
    fn fall(app: &mut App, player: Entity, height: f32) {
        let mut y = 0.0;
        while y < height {
            put(app, player, FLOOR + Vec3::Y * y);
            app.update();
            y += 5.0;
        }
        let mut y = height;
        while y > 0.0 {
            put(app, player, FLOOR + Vec3::Y * y);
            app.update();
            y -= 20.0 * FRAME;
        }
        put(app, player, FLOOR);
        app.update();
    }

    fn inventory(app: &mut App, player: Entity) -> Mut<'_, Inventory> {
        app.world_mut().get_mut::<Inventory>(player).unwrap()
    }

    fn messages<M: Message + Clone>(app: &mut App) -> Vec<M> {
        app.world_mut()
            .resource_mut::<bevy::ecs::message::Messages<M>>()
            .drain()
            .collect()
    }

    #[test]
    fn test_a_fall_hurts_in_survival_and_not_in_creative() {
        let (mut app, player) = server();
        fall(&mut app, player, 7.0);
        assert_eq!(MAX_HEALTH - 4, vitals(&app, player).health);
        app.world_mut()
            .entity_mut(player)
            .insert(GameMode::Creative);
        fall(&mut app, player, 30.0);
        assert_eq!(MAX_HEALTH - 4, vitals(&app, player).health);
        // nor does creative drown, or go hungry
        put(&mut app, player, Vec3::new(8.5, 3.0, 0.5));
        run(&mut app, 60.0);
        assert_eq!(MAX_HEALTH - 4, vitals(&app, player).health);
        assert_eq!(MAX_FOOD, vitals(&app, player).food);
    }

    #[test]
    fn test_arriving_in_the_air_over_ground_still_loading_is_no_fall() {
        let (mut app, player) = server();
        // as a new player arrives, waiting high in the air for the ground to load
        put(&mut app, player, FLOOR + Vec3::Y * 40.0);
        run(&mut app, 1.0);
        let mut y = 20.0;
        while y > 0.0 {
            put(&mut app, player, FLOOR + Vec3::Y * y);
            app.update();
            y -= 1.0;
        }
        put(&mut app, player, FLOOR);
        app.update();
        assert_eq!(Vitals::default(), vitals(&app, player));
        // though once they've stood, a fall counts
        fall(&mut app, player, 10.0);
        assert_eq!(MAX_HEALTH - 7, vitals(&app, player).health);
    }

    #[test]
    fn test_dying_drops_everything_and_respawning_brings_the_player_back_whole() {
        let (mut app, player) = server();
        let apples = ItemStack::new(Item::Apple, 5);
        inventory(&mut app, player).slots[3] = Some(apples);
        inventory(&mut app, player).cursor = Some(ItemStack::new(Item::Stick, 2));
        messages::<CommandOutput>(&mut app);
        fall(&mut app, player, 30.0);
        assert!(vitals(&app, player).is_dead());
        assert_eq!(Inventory::default(), *inventory(&mut app, player));
        let mut dropped = app.world_mut().query::<&DroppedItem>();
        let mut stacks: Vec<_> = dropped.iter(app.world()).map(|item| item.stack).collect();
        stacks.sort_by_key(|stack| stack.count);
        assert_eq!(vec![ItemStack::new(Item::Stick, 2), apples], stacks);
        let said = messages::<CommandOutput>(&mut app);
        assert_eq!(1, said.len(), "{said:?}");
        assert_eq!("Player fell from a high place", said[0].text);
        // the dead pick nothing up, so it's still there to come back for
        run(&mut app, 3.0);
        assert_eq!(2, dropped.iter(app.world()).count());

        // with no bed, at the world's spawn
        app.world_mut().write_message(Respawn);
        app.update();
        assert_eq!(Vitals::default(), vitals(&app, player));
        let spawn = app.world().resource::<World>().spawn();
        let body = app.world().get::<Transform>(player).unwrap().translation;
        assert_eq!(spawn, body);
    }

    #[test]
    fn test_respawning_is_on_the_bed_used_while_its_there() {
        let (mut app, player) = server();
        app.world_mut()
            .entity_mut(player)
            .insert(SpawnBed(Some(TEST_BED)));
        let die = |app: &mut App| {
            app.world_mut().get_mut::<Vitals>(player).unwrap().health = 0;
            app.world_mut().write_message(Respawn);
            app.update();
            app.world().get::<Transform>(player).unwrap().translation
        };
        assert_eq!(on_bed(TEST_BED), die(&mut app));
        // and the living can't respawn
        put(&mut app, player, FLOOR);
        app.world_mut().write_message(Respawn);
        app.update();
        let body = app.world().get::<Transform>(player).unwrap().translation;
        assert_eq!(FLOOR, body);

        app.world_mut()
            .resource_mut::<World>()
            .edit(TEST_BED, BlockType::Air);
        let spawn = app.world().resource::<World>().spawn();
        assert_eq!(spawn, die(&mut app));
        assert_eq!(
            SpawnBed(None),
            *app.world().get::<SpawnBed>(player).unwrap()
        );
    }

    // A creature's blow at the player from 3 blocks along -x.
    fn strike(app: &mut App, player: Entity, damage: u8) {
        app.world_mut().write_message(Blow {
            player,
            damage,
            from: FLOOR - Vec3::X * 3.0,
            knockback: 6.0,
            by: CreatureKind::Sheep,
        });
        app.update();
    }

    #[test]
    fn test_blows_land_once_in_a_while_through_armour_which_wears_and_knock_back() {
        let (mut app, player) = server();
        strike(&mut app, player, 4);
        assert_eq!(MAX_HEALTH - 4, vitals(&app, player).health);
        let bruise = *app.world().get::<Bruise>(player).unwrap();
        assert_eq!(1, bruise.count);
        // away from where it came from, and up
        assert!(bruise.push.x > 5.0 && bruise.push.y > 0.0, "{bruise:?}");
        // another straight after doesn't land, but one after a moment does
        strike(&mut app, player, 4);
        assert_eq!(MAX_HEALTH - 4, vitals(&app, player).health);
        run(&mut app, 0.5);
        // iron chestplate and leggings take 36% of it, and wear
        inventory(&mut app, player).armour[1] = Some(ItemStack::new(Item::IronChestplate, 1));
        inventory(&mut app, player).armour[2] = Some(ItemStack::new(Item::IronLeggings, 1));
        strike(&mut app, player, 10);
        assert_eq!(MAX_HEALTH - 4 - 6, vitals(&app, player).health);
        let worn = inventory(&mut app, player)
            .armour
            .map(|s| s.map(|s| s.wear));
        assert_eq!([None, Some(1), Some(1), None], worn);
        // in creative, nothing lands
        run(&mut app, 0.5);
        app.world_mut()
            .entity_mut(player)
            .insert(GameMode::Creative);
        strike(&mut app, player, 10);
        assert_eq!(MAX_HEALTH - 10, vitals(&app, player).health);
    }

    #[test]
    fn test_a_killing_blow_drops_the_armour_too_and_says_what_struck() {
        let (mut app, player) = server();
        inventory(&mut app, player).armour[0] = Some(ItemStack::new(Item::LeatherHelmet, 1));
        messages::<CommandOutput>(&mut app);
        app.world_mut().get_mut::<Vitals>(player).unwrap().health = 2;
        strike(&mut app, player, 5);
        assert!(vitals(&app, player).is_dead());
        assert_eq!(Inventory::default(), *inventory(&mut app, player));
        let said = messages::<CommandOutput>(&mut app);
        assert_eq!("Player was slain by a sheep", said[0].text);
        let mut dropped = app.world_mut().query::<&DroppedItem>();
        assert_eq!(1, dropped.iter(app.world()).count());
    }

    #[test]
    fn test_drowning_in_a_pool_kills_and_says_so() {
        let (mut app, player) = server();
        messages::<CommandOutput>(&mut app);
        put(&mut app, player, Vec3::new(8.5, 3.0, 0.5));
        run(&mut app, 14.0);
        assert!(vitals(&app, player).air < MAX_AIR / 2);
        assert_eq!(MAX_HEALTH, vitals(&app, player).health);
        // dead within 15 seconds of breath and 10 of drowning, give or take healing meanwhile
        let mut said = vec![];
        for _ in 0..(20.0 / FRAME) as usize {
            app.update();
            said.extend(messages::<CommandOutput>(&mut app));
        }
        assert!(vitals(&app, player).is_dead());
        assert_eq!(1, said.len(), "{said:?}");
        assert_eq!("Player drowned", said[0].text);
    }

    #[test]
    fn test_eating_takes_its_time_fills_the_bar_and_is_refused_when_full() {
        let (mut app, player) = server();
        inventory(&mut app, player).slots[2] = Some(ItemStack::new(Item::Apple, 3));
        inventory(&mut app, player).slots[3] = Some(ItemStack::new(Item::Stick, 3));
        let apples = |app: &mut App| inventory(app, player).count(Item::Apple);
        // full, so it's refused
        app.world_mut().write_message(Eat(Some(2)));
        run(&mut app, 2.0);
        assert_eq!(3, apples(&mut app));

        app.world_mut().get_mut::<Vitals>(player).unwrap().food = 10;
        // sticks aren't food
        app.world_mut().write_message(Eat(Some(3)));
        run(&mut app, 2.0);
        assert_eq!(3, inventory(&mut app, player).count(Item::Stick));
        app.world_mut().write_message(Eat(Some(2)));
        run(&mut app, EATING_SECONDS - 0.2);
        assert_eq!((3, 10), (apples(&mut app), vitals(&app, player).food));
        run(&mut app, 0.3);
        assert_eq!((2, 14), (apples(&mut app), vitals(&app, player).food));
        // held on, the next, until it's full, or let go
        run(&mut app, EATING_SECONDS);
        assert_eq!((1, 18), (apples(&mut app), vitals(&app, player).food));
        app.world_mut().write_message(Eat(None));
        run(&mut app, EATING_SECONDS * 2.0);
        assert_eq!(1, apples(&mut app));
    }

    #[test]
    fn test_bread_fills_five() {
        let (mut app, player) = server();
        inventory(&mut app, player).slots[0] = Some(ItemStack::new(Item::Bread, 2));
        app.world_mut().get_mut::<Vitals>(player).unwrap().food = 10;
        app.world_mut().write_message(Eat(Some(0)));
        run(&mut app, EATING_SECONDS + 0.1);
        assert_eq!(15, vitals(&app, player).food);
        assert_eq!(1, inventory(&mut app, player).count(Item::Bread));
    }
}
