use bevy::{
    app::{App, Plugin, Update},
    ecs::{
        resource::Resource,
        system::{Res, ResMut},
    },
    input::{keyboard::KeyCode, ButtonInput},
    time::Time,
};

/// Seconds in a game day.
const DAY_SECONDS: f32 = 600.0;
/// Days of the clock the world starts at: 0 is midnight, 0.5 is noon.
pub const START_TIME: f32 = 0.4;
// How fast `[` and `]` wind the clock, in days a second.
const WIND_SPEED: f32 = 0.1;
const WIND_BACK_KEY: KeyCode = KeyCode::BracketLeft;
const WIND_FORWARD_KEY: KeyCode = KeyCode::BracketRight;

/// The time of day, in days: the whole part counts days, the rest is the clock, see `START_TIME`.
#[derive(Resource)]
pub struct TimeOfDay(pub f32);

/// Runs the clock, a day every `DAY_SECONDS`. The sky follows it, see `sky::move_sky`.
pub struct ClockPlugin;

impl Plugin for ClockPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, run_clock);
    }
}

/// Moves the clock on with the game's time.
pub fn run_clock(time: Res<Time>, mut clock: ResMut<TimeOfDay>) {
    clock.0 += time.delta_secs() / DAY_SECONDS;
}

/// Winds the clock back or forward while `[` or `]` is held, for looking at the sky at any hour.
pub fn wind_clock(keys: Res<ButtonInput<KeyCode>>, time: Res<Time>, mut clock: ResMut<TimeOfDay>) {
    let wound = keys.pressed(WIND_FORWARD_KEY) as i32 - keys.pressed(WIND_BACK_KEY) as i32;
    if wound != 0 {
        clock.0 += time.delta_secs() * wound as f32 * WIND_SPEED;
    }
}
