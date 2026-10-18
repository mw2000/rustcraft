//! The sounds of what happens: blocks broken and placed, by this player and others, the blows
//! while breaking, picking things up, eating, being hurt and dying, chests and buttons.

use bevy::math::I64Vec3;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use bevy_replicon::prelude::{ClientId, ClientSystems, FromClient};

use super::{PlaySound, SoundKind, Surface};
use crate::block::BlockType;
use crate::client::edits::{apply_block_changes, Predictions};
use crate::client::others::SeenPlayer;
use crate::client::player::LocalPlayer;
use crate::farming::tilled;
use crate::interaction::Breaking;
use crate::menu::GameState;
use crate::player::BODY;
use crate::protocol::{BlockChanged, Eat, RequestEdit, UseBlock};
use crate::server::drops::DroppedItem;
use crate::server::is_authority;
use crate::survival::Vitals;
use crate::world::World;

// Seconds between blows at a block being broken, and between bites eating.
const HIT_INTERVAL: f32 = 0.25;
const BITE_INTERVAL: f32 = 0.22;
// How near a player, in blocks, an item that goes is taken to have been picked up rather than
// gathered into another or left to vanish.
const PICKED_UP: f32 = 2.0;

pub(super) struct SoundEventsPlugin;

impl Plugin for SoundEventsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(pick_up)
            .add_observer(click)
            .add_systems(
                Update,
                (
                    hit_while_breaking,
                    eat,
                    hurt_and_die,
                    open_and_close_chests,
                    // a joiner's edits, before the host's server makes them
                    hear_joiners_edits.run_if(is_authority),
                )
                    .run_if(resource_exists::<World>),
            )
            .add_systems(
                PostUpdate,
                (
                    // before the host's server or a joiner's prediction makes them
                    hear_own_edits.before(ClientSystems::Send),
                    // before they're put in the world, which a joiner's are after replicon's
                    // handed them over
                    hear_others_edits
                        .before(apply_block_changes)
                        .run_if(not(is_authority)),
                )
                    .run_if(resource_exists::<World>),
            );
    }
}

/// The sound of the block at `block` being `was` becoming `to`, if it makes one: breaking what
/// was there, or placing or tilling what's put there.
fn edit_sound(was: BlockType, to: BlockType) -> Option<SoundKind> {
    if to == BlockType::Air {
        Surface::of(was).map(SoundKind::Break)
    } else if tilled(was) == Some(to) {
        Some(SoundKind::Hit(Surface::Dirt))
    } else {
        Surface::of(to).map(SoundKind::Place)
    }
}

fn edit_played(world: &World, block: I64Vec3, to: BlockType) -> Option<PlaySound> {
    edit_sound(world.get_block_at(block), to).map(|kind| PlaySound {
        kind,
        at: Some(block.as_vec3() + 0.5),
    })
}

// The local player's edits, heard as they're asked for, from the world as it is before them.
fn hear_own_edits(
    world: Res<World>,
    mut requests: MessageReader<RequestEdit>,
    mut sounds: MessageWriter<PlaySound>,
) {
    for request in requests.read() {
        sounds.write_batch(edit_played(&world, request.block, request.to));
    }
}

// On the host, the edits joiners ask for, which the server makes later in the frame.
fn hear_joiners_edits(
    world: Res<World>,
    mut requests: MessageReader<FromClient<RequestEdit>>,
    mut sounds: MessageWriter<PlaySound>,
) {
    for request in requests.read() {
        if request.client_id != ClientId::Server {
            sounds.write_batch(edit_played(&world, request.block, request.to));
        }
    }
}

// On a joiner, the edits other players made, which the server's told it of. Those it made
// without a player asking, such as plants going with the ground under them, make no sound.
fn hear_others_edits(
    world: Res<World>,
    predictions: Res<Predictions>,
    mut changes: MessageReader<BlockChanged>,
    mut sounds: MessageWriter<PlaySound>,
) {
    for change in changes.read() {
        if change
            .by
            .is_some_and(|(client, _)| Some(client) != predictions.me)
        {
            sounds.write_batch(edit_played(&world, change.block, change.to));
        }
    }
}

fn hit_while_breaking(
    world: Res<World>,
    breaking: Res<Breaking>,
    time: Res<Time>,
    mut until: Local<f32>,
    mut sounds: MessageWriter<PlaySound>,
) {
    let Some((block, _)) = breaking.cracks() else {
        *until = 0.0;
        return;
    };
    *until -= time.delta_secs();
    if *until > 0.0 {
        return;
    }
    *until = HIT_INTERVAL;
    if let Some(surface) = Surface::of(world.get_block_at(block)) {
        sounds.write(PlaySound {
            kind: SoundKind::Hit(surface),
            at: Some(block.as_vec3() + 0.5),
        });
    }
}

// Chewing for as long as this player's eating, as `hud::eat` tells the server.
fn eat(
    time: Res<Time>,
    mut eats: MessageReader<Eat>,
    mut eating: Local<bool>,
    mut until: Local<f32>,
    mut sounds: MessageWriter<PlaySound>,
) {
    if let Some(Eat(slot)) = eats.read().last() {
        *eating = slot.is_some();
        *until = 0.0;
    }
    if !*eating {
        return;
    }
    *until -= time.delta_secs();
    if *until <= 0.0 {
        *until = BITE_INTERVAL;
        sounds.write(PlaySound {
            kind: SoundKind::Eat,
            at: None,
        });
    }
}

fn hurt_and_die(
    player: Query<&Vitals, (With<LocalPlayer>, Changed<Vitals>)>,
    mut last_health: Local<Option<u8>>,
    mut sounds: MessageWriter<PlaySound>,
) {
    let Ok(vitals) = player.single() else {
        return;
    };
    if last_health.is_some_and(|was| vitals.health < was) {
        let kind = if vitals.is_dead() {
            SoundKind::Death
        } else {
            SoundKind::Hurt
        };
        sounds.write(PlaySound { kind, at: None });
    }
    *last_health = Some(vitals.health);
}

// An item lying near a player that goes has been picked up, by them or someone else, and is
// heard where it was.
fn pick_up(
    gone: On<Remove<DroppedItem>>,
    items: Query<&Transform>,
    players: Query<&GlobalTransform, With<LocalPlayer>>,
    others: Query<&GlobalTransform, With<SeenPlayer>>,
    mut sounds: MessageWriter<PlaySound>,
) {
    let Ok(item) = items.get(gone.entity) else {
        return;
    };
    let at = item.translation;
    let near = |body: &GlobalTransform| {
        let feet = body.translation();
        at.distance(at.clamp(feet, feet + Vec3::Y * BODY.max.y)) <= PICKED_UP
    };
    if players.iter().chain(&others).any(near) {
        sounds.write(PlaySound {
            kind: SoundKind::Pickup,
            at: Some(at),
        });
    }
}

// A chest creaks open as it's used, and shuts once the inventory screen over it closes. Shutting
// is looked for first, as the screen opens the frame after the chest's used.
fn open_and_close_chests(
    world: Res<World>,
    state: Res<State<GameState>>,
    mut uses: MessageReader<UseBlock>,
    mut open: Local<Option<I64Vec3>>,
    mut sounds: MessageWriter<PlaySound>,
) {
    let at = |block: I64Vec3| Some(block.as_vec3() + 0.5);
    if *state.get() != GameState::Inventory {
        if let Some(chest) = open.take() {
            sounds.write(PlaySound {
                kind: SoundKind::ChestClose,
                at: at(chest),
            });
        }
    }
    for used in uses.read() {
        if world.get_block_at(used.block) == BlockType::Chest {
            *open = Some(used.block);
            sounds.write(PlaySound {
                kind: SoundKind::ChestOpen,
                at: at(used.block),
            });
        }
    }
}

fn click(_: On<Activate>, mut sounds: MessageWriter<PlaySound>) {
    sounds.write(PlaySound {
        kind: SoundKind::Click,
        at: None,
    });
}

#[cfg(test)]
mod tests {
    use super::{edit_sound, SoundKind, Surface};
    use crate::block::BlockType::*;

    #[test]
    fn test_edits_sound_like_what_breaks_or_is_put_down() {
        assert_eq!(
            Some(SoundKind::Break(Surface::Stone)),
            edit_sound(Stone, Air)
        );
        assert_eq!(
            Some(SoundKind::Break(Surface::Leaves)),
            edit_sound(TallGrass, Air)
        );
        assert_eq!(
            Some(SoundKind::Place(Surface::Wood)),
            edit_sound(Air, Planks)
        );
        assert_eq!(
            Some(SoundKind::Place(Surface::Sand)),
            edit_sound(TallGrass, Sand)
        );
        assert_eq!(
            Some(SoundKind::Hit(Surface::Dirt)),
            edit_sound(Grass, Farmland)
        );
        // nothing there to break
        assert_eq!(None, edit_sound(Air, Air));
        assert_eq!(None, edit_sound(Water, Air));
    }
}
