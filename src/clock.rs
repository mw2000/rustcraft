use bevy::ecs::resource::Resource;

/// Seconds in a game day.
pub const DAY_SECONDS: f32 = 600.0;
/// The clock counts whole ticks, so it stays exact however long the game runs, and two machines
/// at the same tick agree on everything that follows from it.
pub const TICKS_PER_SECOND: u64 = 20;
/// Ticks in a game day.
pub const DAY_TICKS: u64 = DAY_SECONDS as u64 * TICKS_PER_SECOND;
/// Days of the clock the world starts at: 0 is midnight, 0.5 is noon.
pub const START_TIME: f64 = 0.4;
// When night's begun and ended, as far as beds go: from just before the sun sets, at 0.75,
// until just before it rises, at 0.25. Sleeping through it wakes just after. In ticks into
// the day.
const DUSK: u64 = DAY_TICKS * 74 / 100;
const DAWN: u64 = DAY_TICKS * 24 / 100;
const MORNING: u64 = DAY_TICKS * 26 / 100;

/// The time in ticks since midnight of day 0, see `TICKS_PER_SECOND`, `DAY_TICKS` and
/// `START_TIME`. The server runs it (`server::run_clock`) and clients keep to it
/// (`client::clock`); the sky follows it, see `sky::move_sky`. It's a whole number, the same
/// on every machine however many days have passed: what's drawn between two ticks takes its
/// fraction from `SubTick`.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimeOfDay(pub u64);

impl Default for TimeOfDay {
    fn default() -> Self {
        Self::from_days(START_TIME)
    }
}

impl TimeOfDay {
    /// The tick nearest `days` days since midnight of day 0, or day 0's midnight before it, as
    /// the clock was counted until it counted ticks.
    pub fn from_days(days: f64) -> Self {
        Self((days.max(0.0) * DAY_TICKS as f64).round() as u64)
    }

    /// How far through the day it is, 0..1: 0 is midnight, 0.5 is noon. Precise whatever day
    /// it is, as the days are taken off before it's narrowed.
    pub fn days(&self) -> f32 {
        (self.0 % DAY_TICKS) as f32 / DAY_TICKS as f32
    }

    /// The same, `sub` of the way on to the next tick, for drawing the sky smoothly between
    /// ticks. It reaches 1 just before midnight, which is the same as 0 to anything that
    /// turns with the day.
    pub fn drawn_days(&self, sub: &SubTick) -> f32 {
        ((self.0 % DAY_TICKS) as f32 + sub.0) / DAY_TICKS as f32
    }

    /// Which day it is, counting from 0.
    pub fn day(&self) -> u64 {
        self.0 / DAY_TICKS
    }

    /// Whether it's night, when beds can be slept in.
    pub fn is_night(&self) -> bool {
        !(DAWN..DUSK).contains(&(self.0 % DAY_TICKS))
    }

    /// The first morning after now, which sleeping through the night wakes to.
    pub fn morning_after(&self) -> Self {
        let morning = self.day() * DAY_TICKS + MORNING;
        Self(if morning > self.0 {
            morning
        } else {
            morning + DAY_TICKS
        })
    }

    /// Runs the clock on by `ticks`, which may be fractional, or negative to wind it back,
    /// counting whole ticks and carrying the rest in `sub`. It stops at day 0's midnight.
    pub fn run(&mut self, sub: &mut SubTick, ticks: f64) {
        let carried = f64::from(sub.0) + ticks;
        let whole = carried.floor();
        let ran = self.0.checked_add_signed(whole as i64);
        self.0 = ran.unwrap_or(if whole < 0.0 { 0 } else { u64::MAX });
        sub.0 = if ran.is_some() {
            (carried - whole) as f32
        } else {
            0.0
        };
    }
}

/// How far the clock's got toward its next tick, 0..1: the part of a tick not yet counted.
/// It's only for drawing, so the sky moves on every frame rather than twenty times a second;
/// nothing the game decides reads it.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct SubTick(pub f32);

#[cfg(test)]
mod tests {
    use super::{SubTick, TimeOfDay, DAY_TICKS, TICKS_PER_SECOND};

    #[test]
    fn test_a_day_is_twelve_thousand_ticks_and_its_fraction_wraps_at_midnight() {
        assert_eq!(12_000, DAY_TICKS);
        assert_eq!(0.0, TimeOfDay(0).days());
        assert_eq!(0.5, TimeOfDay(DAY_TICKS / 2).days());
        assert_eq!(0.0, TimeOfDay(3 * DAY_TICKS).days());
        assert_eq!(3, TimeOfDay(4 * DAY_TICKS - 1).day());
        assert_eq!(4, TimeOfDay(4 * DAY_TICKS).day());
        // however many days have passed, exactly
        for days in [0, 1_000, 1_000_000, 100_000_000_000] {
            let clock = TimeOfDay(days * DAY_TICKS + DAY_TICKS * 2 / 5);
            assert_eq!(0.4, clock.days(), "{days}");
            assert_eq!(days, clock.day());
        }
        assert_eq!(TimeOfDay(4_800), TimeOfDay::default());
    }

    #[test]
    fn test_days_convert_to_the_nearest_tick() {
        assert_eq!(TimeOfDay(DAY_TICKS * 25 / 2), TimeOfDay::from_days(12.5));
        assert_eq!(TimeOfDay(0), TimeOfDay::from_days(-0.25));
        // a tick is a 12,000th of a day, so a millionth of one rounds away even a million in
        assert_eq!(
            TimeOfDay(1_000_000 * DAY_TICKS + 1_481),
            TimeOfDay::from_days(1_000_000.123_456)
        );
    }

    #[test]
    fn test_the_clock_counts_whole_ticks_and_carries_the_rest() {
        let (mut clock, mut sub) = (TimeOfDay(100), SubTick::default());
        // a 60th of a second at a time is a third of a tick each
        for _ in 0..60 {
            clock.run(&mut sub, TICKS_PER_SECOND as f64 / 60.0);
        }
        assert!(
            clock == TimeOfDay(119) || clock == TimeOfDay(120),
            "{clock:?}"
        );
        assert!((clock.0 as f32 + sub.0 - 120.0).abs() < 1e-3);
        // wound back past its start, it stops there
        clock.run(&mut sub, -1_000.5);
        assert_eq!((TimeOfDay(0), SubTick(0.0)), (clock, sub));
        clock.run(&mut sub, 2.25);
        assert_eq!((TimeOfDay(2), SubTick(0.25)), (clock, sub));
        clock.run(&mut sub, -0.5);
        assert_eq!((TimeOfDay(1), SubTick(0.75)), (clock, sub));
        assert_eq!(1.75 / DAY_TICKS as f32, clock.drawn_days(&sub));
    }

    #[test]
    fn test_night_is_while_the_sun_is_down_and_ends_in_the_morning() {
        let at = |days: f64| TimeOfDay::from_days(days);
        for night in [0.0, 0.1, 0.8, 0.99, 3.9] {
            assert!(at(night).is_night(), "{night}");
        }
        for day in [0.3, 0.5, 0.7, 2.5] {
            assert!(!at(day).is_night(), "{day}");
        }
        for time in [2.8, 3.1] {
            assert_eq!(at(3.26), at(time).morning_after(), "{time}");
        }
    }
}
