//! Creatures heard where they're drawn: calling now and then, and crying out as they're hurt.

use bevy::prelude::*;
use rand::RngExt;

use super::{CreatureSound, PlaySound, SoundKind};
use crate::client::creatures::SeenCreature;
use crate::creatures::{Creature, Flinches};

// How often a creature calls, on average, in seconds: now and then, so a flock isn't a din.
const CALL_EVERY: f32 = 25.0;

pub(super) struct CreatureSoundsPlugin;

impl Plugin for CreatureSoundsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<CreatureSound>()
            .add_systems(Update, ((call, cry_out), hear_creatures).chain());
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
                    hurt: false,
                    at: body.translation,
                });
            }
        }
    }
}

// As the server counts another flinch, which it does as a creature's hit.
fn cry_out(
    seen: Query<(&SeenCreature, &Transform)>,
    creatures: Query<(&Creature, Ref<Flinches>)>,
    mut sounds: MessageWriter<CreatureSound>,
) {
    for (seen, body) in &seen {
        let Ok((&Creature(kind), flinches)) = creatures.get(seen.0) else {
            continue;
        };
        if flinches.is_changed() && !flinches.is_added() {
            sounds.write(CreatureSound {
                kind,
                hurt: true,
                at: body.translation,
            });
        }
    }
}

fn hear_creatures(
    mut creatures: MessageReader<CreatureSound>,
    mut sounds: MessageWriter<PlaySound>,
) {
    for &CreatureSound { kind, hurt, at } in creatures.read() {
        let kind = if hurt {
            SoundKind::Yelp(kind)
        } else {
            SoundKind::Call(kind)
        };
        sounds.write(PlaySound { kind, at: Some(at) });
    }
}
