use bevy::ecs::resource::Resource;

/// Seconds in a game day.
pub const DAY_SECONDS: f32 = 600.0;
/// Days of the clock the world starts at: 0 is midnight, 0.5 is noon.
pub const START_TIME: f64 = 0.4;
// When night's begun and ended, as far as beds go: from just before the sun sets, at 0.75,
// until just before it rises, at 0.25. Sleeping through it wakes just after.
const DUSK: f64 = 0.74;
const DAWN: f64 = 0.24;
const MORNING: f64 = 0.26;

/// The time of day, in days: the whole part counts days, the rest is the clock, see `START_TIME`.
/// The server runs it (`server::run_clock`) and clients keep to it (`client::clock`); the sky
/// follows it, see `sky::move_sky`. It's an `f64`, which can add a frame's worth of time to
/// days beyond counting, where an `f32` stops moving past about a thousand. Anything that needs
/// it as an `f32`, such as a shader, takes `day_fraction` of it first.
#[derive(Resource)]
pub struct TimeOfDay(pub f64);

impl Default for TimeOfDay {
    fn default() -> Self {
        Self(START_TIME)
    }
}

/// Whether it's night at `time`, in days, when beds can be slept in.
pub fn is_night(time: f64) -> bool {
    !(DAWN..DUSK).contains(&time.rem_euclid(1.0))
}

/// The first morning after `time`, which sleeping through the night wakes to.
pub fn morning_after(time: f64) -> f64 {
    let morning = time.floor() + MORNING;
    if morning > time {
        morning
    } else {
        morning + 1.0
    }
}

/// How far through its day `time` is, from 0 at midnight, which is precise whatever day it is,
/// as the days are taken off in an `f64` before it's narrowed.
pub fn day_fraction(time: f64) -> f32 {
    time.rem_euclid(1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::{day_fraction, is_night, morning_after};

    #[test]
    fn test_the_fraction_of_the_day_stays_precise_however_many_days_have_passed() {
        for days in [0.0, 1_000.0, 1_000_000.0, 100_000_000.0] {
            assert!((day_fraction(days + 0.4) - 0.4).abs() < 1e-6, "{days}");
        }
        assert!((day_fraction(-0.25) - 0.75).abs() < 1e-6);
    }

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
