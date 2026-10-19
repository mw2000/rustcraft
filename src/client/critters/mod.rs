//! Ambient life: butterflies over the flowers by day, fireflies blinking by water and in the woods
//! at night, a murmuration of starlings wheeling over open land at dusk, and skeins of geese
//! passing high over in a V by day (ported from rornic/rustcraft #79, by Aymen Qader, §5.8).
//!
//! Unlike creatures they're the client's alone, decoration round the camera: the server knows
//! nothing of them and nothing is sent, so each player sees their own, which is natural enough
//! for things this small and fleeting. They're never hit, aimed at or collided with, as the
//! raycast and collision look only at blocks and at the creatures the server replicates.

mod birds;
mod glow;
mod insects;
mod wings;

use crate::client::player::WorldCamera;
use std::f32::consts::TAU;

use bevy::prelude::*;
use rand::rngs::StdRng;
use rand::RngExt;

use crate::clock::TimeOfDay;
use crate::menu::GameState;
use crate::preferences::Preferences;
use crate::sky::sun_direction;
use crate::world::{LeaveWorld, LeaveWorldSystems, World};

pub use glow::MAX_FIREFLY_LIGHTS;

/// Butterflies, fireflies and birds about the camera, for the game's App. Needs the renderer's
/// assets (meshes, materials and images), which tests can give it with `AssetPlugin`.
pub struct AmbientLifePlugin;

impl Plugin for AmbientLifePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AmbientLife>()
            .init_resource::<CritterDice>()
            .add_plugins((
                wings::WingPlugin,
                insects::InsectPlugin,
                birds::BirdPlugin,
                glow::GlowPlugin,
            ))
            .add_systems(Update, follow_preferences)
            .add_systems(LeaveWorld, clear_critters.in_set(LeaveWorldSystems::Clear));
    }
}

/// Whether there's ambient life about, from the Video settings, and the weather that sends it to
/// shelter.
#[derive(Resource, Debug, Clone, Copy)]
pub struct AmbientLife {
    /// The player's "Ambient Life" setting: with it off none appear, and those about go at once.
    pub on: bool,
    /// How hard it's raining, 0 to 1: the hook for weather (M3.5) to set. Past `SHELTER_RAIN`
    /// butterflies, fireflies and starlings leave and no geese fly over.
    pub rain: f32,
}

impl Default for AmbientLife {
    fn default() -> Self {
        Self {
            on: true,
            rain: 0.0,
        }
    }
}

// How hard it rains before they shelter.
const SHELTER_RAIN: f32 = 0.3;

impl AmbientLife {
    /// Whether critters should be out at all, whatever the time and place.
    pub fn wanted(&self) -> bool {
        self.on && self.rain < SHELTER_RAIN
    }
}

/// The dice critters are spawned with: seeded from the system, or by a test.
#[derive(Resource)]
pub struct CritterDice(pub StdRng);

impl Default for CritterDice {
    fn default() -> Self {
        Self(rand::make_rng())
    }
}

impl CritterDice {
    fn range(&mut self, range: std::ops::Range<f32>) -> f32 {
        self.0.random_range(range)
    }

    fn angle(&mut self) -> f32 {
        self.range(0.0..TAU)
    }
}

/// Every critter's root, whatever kind it is, so they can all be put away together.
#[derive(Component)]
pub struct AmbientCritter;

// Turned off, every critter goes at once, rather than fading as they do at the wrong time of
// day, as whoever turned them off may want their frames back.
fn follow_preferences(
    preferences: Option<Res<Preferences>>,
    mut life: ResMut<AmbientLife>,
    critters: Query<Entity, With<AmbientCritter>>,
    commands: Commands,
) {
    let Some(preferences) = preferences.filter(|p| p.is_changed()) else {
        return;
    };
    let on = preferences.video.ambient_life;
    if life.on != on {
        life.on = on;
        if !on {
            clear_critters(critters, commands);
        }
    }
}

// Critters go with the world they were in.
fn clear_critters(critters: Query<Entity, With<AmbientCritter>>, mut commands: Commands) {
    for critter in &critters {
        commands.entity(critter).try_despawn();
    }
}

/// Whether there's a world being played round the camera, which the title screen's isn't.
fn in_world(state: Option<Res<State<GameState>>>, world: Option<Res<World>>) -> bool {
    world.is_some() && state.is_some_and(|state| state.in_world())
}

/// How much it's day, from 0 at night to 1 once the sun's well up, for what's about by day.
pub fn daylight(clock: &TimeOfDay) -> f32 {
    ((sun_direction(clock.days()).y + 0.05) / 0.25).clamp(0.0, 1.0)
}

// From the late afternoon until the sun's just down, as a fraction of the day: the sun sets at
// 0.75, see `clock`.
const DUSK: (f32, f32) = (0.64, 0.77);

/// How far into dusk it is, 0 outside it and 1 through the heart of it, easing at either end.
pub fn dusk(clock: &TimeOfDay) -> f32 {
    let at = clock.days();
    let ease_in = smoothstep(DUSK.0, DUSK.0 + 0.02, at);
    let ease_out = 1.0 - smoothstep(DUSK.1 - 0.02, DUSK.1, at);
    ease_in.min(ease_out)
}

/// Eases from 0 at `from` to 1 at `to`, smoothly at both ends, as the shaders' `smoothstep`.
fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    ease(((x - from) / (to - from)).clamp(0.0, 1.0))
}

/// Eases `t`, 0..1, smoothly in and out.
fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Where the camera is, if there's one.
fn eye(camera: &Query<&GlobalTransform, WorldCamera>) -> Option<GlobalTransform> {
    camera.single().ok().copied()
}

#[cfg(test)]
mod tests;
