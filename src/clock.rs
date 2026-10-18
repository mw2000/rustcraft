use bevy::ecs::resource::Resource;

/// Seconds in a game day.
pub const DAY_SECONDS: f32 = 600.0;
/// Days of the clock the world starts at: 0 is midnight, 0.5 is noon.
pub const START_TIME: f32 = 0.4;

/// The time of day, in days: the whole part counts days, the rest is the clock, see `START_TIME`.
/// The server runs it (`server::run_clock`) and clients keep to it (`client::clock`); the sky
/// follows it, see `sky::move_sky`.
#[derive(Resource)]
pub struct TimeOfDay(pub f32);

impl Default for TimeOfDay {
    fn default() -> Self {
        Self(START_TIME)
    }
}
