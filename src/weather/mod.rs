//! The weather (M3.5): now and then a storm, which darkens the sun and clouds, rains round the
//! camera and blows the wind harder, with lightning and thunder in the heaviest rain, and a
//! rainbow opposite the sun once it clears. Ported from rornic/rustcraft #80, by Aymen Qader.
//!
//! The server decides it (INV-4): a world's weather is a run of spells, clear and stormy in
//! turn, each rolled from the seed and its number, so it's the same however the clock gets
//! there, and kept on the game clock, so winding the clock on winds the weather on too. Clients
//! are told each spell as it starts (`WeatherSync`) and work out how far its rain has got from
//! their own clock, as the server does. How it looks and sounds is the client's (`look`, and the
//! rain and thunder in `client::sound`).

#[cfg(feature = "client")]
pub mod look;

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

use crate::chunks::generate::trees::hash;
use crate::clock::{TimeOfDay, TICKS_PER_SECOND};

// The first storm comes soon after the world's made, while the sun's still up for a rainbow
// after it; later ones come and go at random. How long spells last, in seconds of game time.
const FIRST_STORM_IN: f64 = 25.0;
const FIRST_STORM_SECONDS: f64 = 95.0;
const CLEAR_SECONDS: (f64, f64) = (240.0, 480.0);
const STORM_SECONDS: (f64, f64) = (90.0, 180.0);
// How hard a storm rains, at the least and most, see `Weather::rain`.
const STORM_RAIN: (f32, f32) = (0.75, 1.0);
// How long rain takes to come on or clear, in seconds: most of the way in this time.
const RAIN_EASE_SECONDS: f64 = 12.0;
// How long a rainbow stays after a storm, in seconds, and how long it takes to come and go,
// once the rain's eased under `RAINBOW_RAIN`.
const RAINBOW_SECONDS: f64 = 70.0;
const RAINBOW_FADE_SECONDS: f64 = 10.0;
const RAINBOW_RAIN: f32 = 0.15;
/// How hard it rains before lightning strikes: a storm's, not a shower's.
pub const STRIKE_RAIN: f32 = 0.6;
// What the seed's spells are rolled with, so they aren't the same rolls as anything else's.
const SALT: u64 = 0x0057_4541_5448_4552;

/// The world's weather: the spell it's in, from tick `since` to `until`, in which the rain eases
/// from `from` toward `to`, 0 for a clear spell. Clear and stormy spells come in turn, each
/// rolled from the seed and its number (`spell`). Sent to clients as each starts.
#[derive(Resource, Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Weather {
    /// Which of the world's spells this is, from 0, its first, which is clear.
    pub spell: u64,
    pub since: u64,
    pub until: u64,
    /// How hard it was raining as the spell began, 0 to 1.
    pub from: f32,
    /// How hard it's set to rain, which the rain eases toward, 0 to 1.
    pub to: f32,
    /// Whether it came after rain, so a rainbow shows as it clears.
    pub after_rain: bool,
}

impl Default for Weather {
    /// A clear sky that never changes, for where there's no world, such as the title screen.
    fn default() -> Self {
        Self {
            spell: 0,
            since: 0,
            until: u64::MAX,
            from: 0.0,
            to: 0.0,
            after_rain: false,
        }
    }
}

impl Weather {
    /// The weather of a world of `seed` first played at `now`: its first spell, clear.
    pub fn new(seed: u32, now: TimeOfDay) -> Self {
        let (to, seconds) = spell(seed, 0);
        Self {
            since: now.0,
            until: now.0.saturating_add(ticks(seconds)),
            to,
            ..Self::default()
        }
    }

    /// How hard it's raining at `now`, 0 to 1, eased from how it was as the spell began
    /// toward how it's set, as it was until the clock counted ticks: a step at a time, each
    /// most of the way there in `RAIN_EASE_SECONDS`. Before the spell began, as it began, so
    /// winding the clock back holds it.
    pub fn rain(&self, now: TimeOfDay) -> f32 {
        let eased = 1.0 - (-3.0 * self.seconds_in(now) / RAIN_EASE_SECONDS).exp();
        self.from + (self.to - self.from) * eased as f32
    }

    /// How strong the rainbow is at `now`, 0 to 1: after rain, once it's eased off, until
    /// `RAINBOW_SECONDS` into the clear, fading in and out.
    pub fn rainbow(&self, now: TimeOfDay) -> f32 {
        if !self.after_rain || self.to > 0.0 {
            return 0.0;
        }
        // when the rain eases under `RAINBOW_RAIN`, solving `rain` for it
        let cleared = if self.from > RAINBOW_RAIN {
            (f64::from(self.from) / f64::from(RAINBOW_RAIN)).ln() * RAIN_EASE_SECONDS / 3.0
        } else {
            0.0
        };
        let t = self.seconds_in(now);
        let rising = (t - cleared) / RAINBOW_FADE_SECONDS;
        let going = (RAINBOW_SECONDS - t) / RAINBOW_FADE_SECONDS;
        rising.min(going).clamp(0.0, 1.0) as f32
    }

    /// Whether it's raining hard enough at `now` for lightning: a storm.
    pub fn is_stormy(&self, now: TimeOfDay) -> bool {
        self.rain(now) > STRIKE_RAIN
    }

    /// Moves on to the seed's next spells, as many as have ended by `now`, each starting as the
    /// last ended. Whether it changed.
    pub fn run(&mut self, seed: u32, now: TimeOfDay) -> bool {
        let mut changed = false;
        while now.0 >= self.until {
            let next = self.next_spell();
            let (to, seconds) = spell(seed, next);
            let ended = TimeOfDay(self.until);
            *self = Self {
                spell: next,
                since: self.until,
                until: self.until.saturating_add(ticks(seconds)),
                from: self.rain(ended),
                to,
                after_rain: self.to > 0.0,
            };
            changed = true;
        }
        changed
    }

    // The seed's spell after this one: the next of the other kind, rain after clear and clear
    // after rain.
    fn next_spell(&self) -> u64 {
        let raining = self.to > 0.0;
        let mut next = self.spell + 1;
        if is_storm(next) == raining {
            next += 1;
        }
        next
    }

    // How far into the spell `now` is, in seconds, or 0 before it.
    fn seconds_in(&self, now: TimeOfDay) -> f64 {
        now.0.saturating_sub(self.since) as f64 / TICKS_PER_SECOND as f64
    }
}

// Whether the seed's spell `n` is a storm: every other one, from the second.
fn is_storm(n: u64) -> bool {
    n % 2 == 1
}

// How hard the seed's spell `n` rains and how long it lasts, in seconds.
fn spell(seed: u32, n: u64) -> (f32, f64) {
    let roll = |salt: i64| {
        let bits = hash(u64::from(seed) ^ SALT, n as i64, salt) >> 11;
        bits as f64 / (1u64 << 53) as f64
    };
    let between = |(least, most): (f64, f64), roll: f64| least + (most - least) * roll;
    match n {
        0 => (0.0, FIRST_STORM_IN),
        1 => (storm_rain(roll(0)), FIRST_STORM_SECONDS),
        n if is_storm(n) => (storm_rain(roll(0)), between(STORM_SECONDS, roll(1))),
        _ => (0.0, between(CLEAR_SECONDS, roll(1))),
    }
}

fn storm_rain(roll: f64) -> f32 {
    STORM_RAIN.0 + (STORM_RAIN.1 - STORM_RAIN.0) * roll as f32
}

// Seconds of game time in ticks.
fn ticks(seconds: f64) -> u64 {
    (seconds * TICKS_PER_SECOND as f64).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::DAY_TICKS;

    const SEED: u32 = 7;

    // The world's spells from tick 0 on, as many as `count`.
    fn spells(seed: u32, count: usize) -> Vec<Weather> {
        let mut weather = Weather::new(seed, TimeOfDay(0));
        let mut seen = vec![weather];
        while seen.len() < count {
            weather.run(seed, TimeOfDay(weather.until));
            seen.push(weather);
        }
        seen
    }

    #[test]
    fn test_a_seed_has_the_same_weather_every_time_clear_and_stormy_in_turn() {
        let sequence = spells(SEED, 100);
        assert_eq!(sequence, spells(SEED, 100));
        assert_ne!(sequence, spells(SEED + 1, 100));
        for (n, spell) in sequence.iter().enumerate() {
            assert_eq!(n as u64, spell.spell);
            let storm = n % 2 == 1;
            assert_eq!(storm, spell.to >= STORM_RAIN.0, "{n}");
            assert_eq!(!storm, spell.to == 0.0, "{n}");
            let seconds = (spell.until - spell.since) as f64 / TICKS_PER_SECOND as f64;
            let (least, most) = match n {
                0 => (FIRST_STORM_IN, FIRST_STORM_IN),
                1 => (FIRST_STORM_SECONDS, FIRST_STORM_SECONDS),
                _ if storm => STORM_SECONDS,
                _ => CLEAR_SECONDS,
            };
            assert!(
                (least - 0.05..=most + 0.05).contains(&seconds),
                "{n}: {seconds}"
            );
        }
        // each starting where the last ended, from the rain it ended on
        for pair in sequence.windows(2) {
            assert_eq!(pair[0].until, pair[1].since);
            assert_eq!(pair[0].rain(TimeOfDay(pair[0].until)), pair[1].from);
        }
    }

    #[test]
    fn test_however_the_clock_gets_there_the_weather_is_the_same() {
        // run a frame at a time, or all at once after a long sleep
        let mut stepped = Weather::new(SEED, TimeOfDay(0));
        let mut tick = 0;
        while tick <= 3 * DAY_TICKS {
            stepped.run(SEED, TimeOfDay(tick));
            tick += 7;
        }
        let mut jumped = Weather::new(SEED, TimeOfDay(0));
        assert!(jumped.run(SEED, TimeOfDay(tick - 7)));
        assert_eq!(stepped, jumped);
        assert!(!jumped.run(SEED, TimeOfDay(tick - 7)));
    }

    #[test]
    fn test_the_first_storm_comes_clears_and_leaves_a_rainbow() {
        let mut weather = Weather::new(SEED, TimeOfDay(1_000));
        let (mut peak, mut bow) = (0.0f32, 0.0f32);
        let end = 1_000 + ticks(FIRST_STORM_IN + FIRST_STORM_SECONDS + 60.0);
        for tick in (1_000..end).step_by(2) {
            let now = TimeOfDay(tick);
            weather.run(SEED, now);
            peak = peak.max(weather.rain(now));
            bow = bow.max(weather.rainbow(now));
        }
        let now = TimeOfDay(end);
        assert_eq!(2, weather.spell);
        assert!(peak > 0.7, "{peak}");
        assert!(weather.rain(now) < 0.05, "{}", weather.rain(now));
        assert!(bow > 0.9, "{bow}");
        // and the rainbow goes again
        let later = TimeOfDay(weather.since + ticks(RAINBOW_SECONDS));
        assert_eq!(0.0, weather.rainbow(later));
    }

    #[test]
    fn test_the_rain_eases_on_and_off_and_winding_back_holds_it() {
        let weather = Weather {
            since: 100,
            until: 100_000,
            from: 0.0,
            to: 0.9,
            ..Weather::default()
        };
        assert_eq!(0.0, weather.rain(TimeOfDay(100)));
        assert_eq!(0.0, weather.rain(TimeOfDay(50)));
        let after = |seconds: f64| weather.rain(TimeOfDay(100 + ticks(seconds)));
        assert!(
            after(RAIN_EASE_SECONDS) > 0.85,
            "{}",
            after(RAIN_EASE_SECONDS)
        );
        // and never a jump
        let mut last = 0.0;
        for tick in 100..1_000 {
            let rain = weather.rain(TimeOfDay(tick));
            assert!((rain - last).abs() < 0.02);
            last = rain;
        }
        assert!(weather.is_stormy(TimeOfDay(1_000)) && !weather.is_stormy(TimeOfDay(110)));
        // no rainbow while it rains, nor after a clear spell
        assert_eq!(0.0, weather.rainbow(TimeOfDay(10_000)));
        let clear = Weather::new(SEED, TimeOfDay(0));
        assert_eq!(0.0, clear.rainbow(TimeOfDay(200)));
    }

    #[test]
    fn test_the_weather_goes_over_the_wire() {
        let mut weather = Weather::new(SEED, TimeOfDay(5));
        weather.run(SEED, TimeOfDay(weather.until));
        let bytes = postcard::to_stdvec(&weather).unwrap();
        assert_eq!(weather, postcard::from_bytes(&bytes).unwrap());
    }
}
