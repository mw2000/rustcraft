use bevy::ecs::resource::Resource;

/// Seconds in a game day.
pub const DAY_SECONDS: f32 = 600.0;
/// Days of the clock the world starts at: 0 is midnight, 0.5 is noon.
pub const START_TIME: f32 = 0.4;
// When night's begun and ended, as far as beds go: from just before the sun sets, at 0.75,
// until just before it rises, at 0.25. Sleeping through it wakes just after.
const DUSK: f32 = 0.74;
const DAWN: f32 = 0.24;
const MORNING: f32 = 0.26;

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

/// Whether it's night at `time`, in days, when beds can be slept in.
pub fn is_night(time: f32) -> bool {
    !(DAWN..DUSK).contains(&time.rem_euclid(1.0))
}

/// The first morning after `time`, which sleeping through the night wakes to.
pub fn morning_after(time: f32) -> f32 {
    let morning = time.floor() + MORNING;
    if morning > time {
        morning
    } else {
        morning + 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::{is_night, morning_after};

    #[test]
    fn test_night_is_while_the_sun_is_down_and_ends_in_the_morning() {
        for night in [0.0, 0.1, 0.8, 0.99, 3.9, -0.1] {
            assert!(is_night(night), "{night}");
        }
        for day in [0.3, 0.5, 0.7, 2.5, -0.5] {
            assert!(!is_night(day), "{day}");
        }
        for time in [2.8, 3.1] {
            assert!((morning_after(time) - 3.26).abs() < 1e-5, "{time}");
        }
    }
}
