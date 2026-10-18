//! Tending animals (§5.8): shearing them, feeding them so they breed, and their young growing
//! up. A creature's `Life` counts down how long each of those lasts, and its `Condition`, which
//! clients draw it by, follows.

use std::ops::Range;

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use rand::RngExt;

use super::{creature, in_reach, Doing, Mind};
use crate::creatures::{Condition, Creature, CreatureKind, SavedCreature};
use crate::game_mode::GameMode;
use crate::inventory::{Hotbar, Inventory};
use crate::items::{ItemStack, ToolKind};
use crate::player::PlayerOwner;
use crate::protocol::UseCreature;
use crate::server::drops::{Chance, DroppedItem};
use crate::survival::Vitals;

// How long a creature fed stays in love, looking for a mate, how long a young one takes to grow
// up, and how long one that's bred rests before it can again, in seconds.
const LOVE: f32 = 30.0;
pub(super) const GROWING_UP: f32 = 240.0;
const RESTING: f32 = 300.0;
// How long a fleece takes to grow back, in seconds.
const REGROWING: Range<f32> = 240.0..360.0;
// How far a creature in love looks for a mate, and how near, across, two come to breed, in
// blocks.
pub(super) const MATE_RANGE: f32 = 8.0;
const MATE_NEAR: f32 = 1.6;
// How many of a kind there can be within `CROWD_RADIUS` blocks before they take no more food
// and breed no more: plenty for a farm, and few enough that one can't grow without end.
pub(super) const CROWD: usize = 24;
const CROWD_RADIUS: f32 = 16.0;

/// How long, in seconds, a creature has left to grow up, for its fleece to grow back, to be in
/// love, and to rest before it can breed again, each 0 once it's over.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq)]
pub struct Life {
    pub grows_up_in: f32,
    pub regrows_in: f32,
    pub love: f32,
    pub rests: f32,
}

impl Life {
    /// A creature's life as it was saved: love and rest aren't, as they're soon over.
    pub fn of(saved: &SavedCreature) -> Self {
        Self {
            grows_up_in: saved.grows_up_in,
            regrows_in: saved.regrows_in,
            ..Self::default()
        }
    }

    pub fn is_grown(&self) -> bool {
        self.grows_up_in == 0.0
    }

    /// How clients are to draw it.
    pub fn condition(&self) -> Condition {
        Condition {
            young: !self.is_grown(),
            shorn: self.regrows_in > 0.0,
            in_love: self.love > 0.0,
        }
    }
}

/// Counts down how long each creature has left to grow up, regrow its fleece, be in love and
/// rest, keeping how it's drawn in step.
pub(in crate::server) fn grow(time: Res<Time>, mut creatures: Query<(&mut Life, &mut Condition)>) {
    let dt = time.delta_secs();
    for (mut life, mut condition) in &mut creatures {
        let Life {
            grows_up_in,
            regrows_in,
            love,
            rests,
        } = &mut *life;
        for left in [grows_up_in, regrows_in, love, rests] {
            *left = (*left - dt).max(0.0);
        }
        condition.set_if_neq(life.condition());
    }
}

// How many creatures of `kind` there are within `CROWD_RADIUS` of `at`.
fn crowd(kind: CreatureKind, at: Vec3, creatures: &Query<(&Creature, &Transform)>) -> usize {
    creatures
        .iter()
        .filter(|(creature, body)| {
            creature.0 == kind && body.translation.distance(at) <= CROWD_RADIUS
        })
        .count()
}

/// Uses what players hold on the creatures they right-click, if they're alive and it's in their
/// reach: shears shear a grown creature with a fleece that's grown, wearing them a use, and its
/// food puts a grown one that isn't resting in love, taking one in survival, unless there are
/// too many of its kind about.
pub(in crate::server) fn receive_uses(
    mut uses: MessageReader<FromClient<UseCreature>>,
    mut players: Query<(
        &PlayerOwner,
        &Transform,
        &Vitals,
        &GameMode,
        &Hotbar,
        &mut Inventory,
    )>,
    mut creatures: Query<(&Creature, &Transform, &mut Life)>,
    near: Query<(&Creature, &Transform)>,
    mut chance: ResMut<Chance>,
    mut commands: Commands,
) {
    for FromClient { client_id, message } in uses.read() {
        let Some((_, player, vitals, mode, hotbar, mut inventory)) =
            players.iter_mut().find(|(owner, ..)| owner.0 == *client_id)
        else {
            continue;
        };
        let Ok((creature, body, mut life)) = creatures.get_mut(message.creature) else {
            continue;
        };
        let kind = creature.0;
        let at = body.translation;
        if vitals.is_dead() || !life.is_grown() || !in_reach(player, at, kind.body(false)) {
            continue;
        }
        let Some(held) = hotbar.held(&inventory) else {
            continue;
        };
        let properties = kind.properties();
        let shears = held.item.tool().is_some_and(|t| t.kind == ToolKind::Shears);
        if shears {
            let Some((wool, counts)) = properties.fleece.clone() else {
                continue;
            };
            if life.regrows_in > 0.0 {
                continue;
            }
            life.regrows_in = chance.0.random_range(REGROWING);
            let stack = ItemStack::new(wool, chance.0.random_range(counts));
            let (dropped, transform) = DroppedItem::scattered(stack, at, &mut chance.0);
            commands.spawn((dropped, transform, Replicated));
            inventory.wear(hotbar.selected);
        } else if properties.food.contains(&held.item)
            && life.love == 0.0
            && life.rests == 0.0
            && crowd(kind, at, &near) < CROWD
        {
            life.love = LOVE;
            if !mode.has_every_block() {
                inventory.use_one(hotbar.selected, held.item);
            }
        }
    }
}

/// Makes a young one of each pair of creatures in love that have found each other, unless there
/// are too many of their kind about already, and rests them.
pub(in crate::server) fn breed(
    mut creatures: Query<(Entity, &Creature, &Transform, &Mind, &mut Life)>,
    near: Query<(&Creature, &Transform)>,
    mut chance: ResMut<Chance>,
    mut commands: Commands,
) {
    let mut pairs = Vec::new();
    for (entity, creature, body, mind, _) in &creatures {
        // each pair once, from the one that comes first
        let Doing::Mate(partner) = mind.doing else {
            continue;
        };
        if partner < entity {
            continue;
        }
        let Ok((_, _, other, other_mind, _)) = creatures.get(partner) else {
            continue;
        };
        let close = (other.translation - body.translation).xz().length() <= MATE_NEAR;
        if other_mind.doing == Doing::Mate(entity) && close {
            let between = body.translation.lerp(other.translation, 0.5);
            pairs.push((creature.0, entity, partner, between));
        }
    }
    for (kind, a, b, at) in pairs {
        for parent in [a, b] {
            if let Ok((.., mut life)) = creatures.get_mut(parent) {
                life.love = 0.0;
                life.rests = RESTING;
            }
        }
        if crowd(kind, at, &near) >= CROWD {
            continue;
        }
        let young = SavedCreature {
            grows_up_in: GROWING_UP,
            ..SavedCreature::grown(kind, at)
        };
        let yaw = chance.0.random_range(0.0..std::f32::consts::TAU);
        commands.spawn(creature(young, yaw));
    }
}
