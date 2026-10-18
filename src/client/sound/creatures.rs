//! Creatures heard where they're drawn: calling now and then, crying out as they're hurt, as a
//! monster winds up a blow, and as they die.

use bevy::prelude::*;
use rand::RngExt;

use super::{CreatureSound, Cry, PlaySound, SoundKind};
use crate::client::creatures::{Dying, Fading, SeenCreature, SeenKind};
use crate::creatures::{Creature, Flinches, Stance};

// How often a creature calls, on average, in seconds: now and then, so a flock isn't a din.
const CALL_EVERY: f32 = 25.0;

pub(super) struct CreatureSoundsPlugin;

impl Plugin for CreatureSoundsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<CreatureSound>()
            .add_systems(Update, ((call, cry_out, die), hear_creatures).chain());
    }
}

fn call(
    time: Res<Time>,
    seen: Query<(&SeenCreature, &Transform)>,
    creatures: Query<&Creature>,
    mut sounds: MessageWriter<CreatureSound>,
) {
    let chance = time.delta_secs() / CALL_EVERY;
    let mut rng = rand::rng();
    for (seen, body) in &seen {
        if let Ok(&Creature(kind)) = creatures.get(seen.0) {
            if rng.random::<f32>() < chance {
                sounds.write(CreatureSound {
                    kind,
                    cry: Cry::Call,
                    at: body.translation,
                });
            }
        }
    }
}

// As the server counts another flinch, which it does as a creature's hit, and as a monster
// starts winding up a blow.
fn cry_out(
    seen: Query<(&SeenCreature, &Transform)>,
    creatures: Query<(&Creature, Ref<Flinches>, Ref<Stance>)>,
    mut sounds: MessageWriter<CreatureSound>,
) {
    for (seen, body) in &seen {
        let Ok((&Creature(kind), flinches, stance)) = creatures.get(seen.0) else {
            continue;
        };
        let at = body.translation;
        if flinches.is_changed() && !flinches.is_added() {
            sounds.write(CreatureSound {
                kind,
                cry: Cry::Hurt,
                at,
            });
        }
        if stance.is_changed() && stance.winding {
            sounds.write(CreatureSound {
                kind,
                cry: Cry::Attack,
                at,
            });
        }
    }
}

// What's seen of creatures just starting to die, rather than vanishing.
type Dead = (Added<Dying>, Without<Fading>);

// As what's seen of one starts dying, rather than vanishing.
fn die(dying: Query<(&SeenKind, &Transform), Dead>, mut sounds: MessageWriter<CreatureSound>) {
    for (&SeenKind(kind), body) in &dying {
        sounds.write(CreatureSound {
            kind,
            cry: Cry::Death,
            at: body.translation,
        });
    }
}

fn hear_creatures(
    mut creatures: MessageReader<CreatureSound>,
    mut sounds: MessageWriter<PlaySound>,
) {
    for &CreatureSound { kind, cry, at } in creatures.read() {
        let kind = match cry {
            Cry::Call => SoundKind::Call(kind),
            Cry::Hurt => SoundKind::Yelp(kind),
            Cry::Attack => SoundKind::Strike(kind),
            Cry::Death => SoundKind::Perish(kind),
        };
        sounds.write(PlaySound { kind, at: Some(at) });
    }
}
