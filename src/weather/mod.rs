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
//!
//! On top of upstream's: what falls goes by the climate where it falls (`precipitation`), snow
//! where it's cold and little in deserts; an operator can set the weather (`/weather`, `set`);
//! beds can be slept in through a storm, which waking clears (`wake`); and a storm's gloom
//! counts as dark for monsters round dusk and dawn (`is_dark_out`).

#[cfg(feature = "client")]
pub mod look;

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

use crate::chunks::generate::biome::Climate;
use crate::chunks::generate::generator::{
    snow_line, SNOWY_GRASS_TEMPERATURE, SNOW_MAX_TEMPERATURE,
};
use crate::chunks::generate::trees::hash;
use crate::clock::{daylight, TimeOfDay, TICKS_PER_SECOND};

// The first storm comes a game day after the world's made, in the same part of the day, so
// the sun's still up for a rainbow after it; later ones come and go at random, well apart. How
// long spells last, in seconds of game time.
const FIRST_STORM_IN: f64 = 600.0;
const FIRST_STORM_SECONDS: f64 = 95.0;
const CLEAR_SECONDS: (f64, f64) = (1800.0, 3600.0);
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
// How hard `/weather rain` and `/weather storm` set it raining: a shower under the lightning's
// `STRIKE_RAIN`, and a storm's.
const SHOWER_RAIN: f32 = 0.45;
const SET_STORM_RAIN: f32 = 0.9;
/// How much of the sun's and moon's light gets through the clouds in the heaviest rain.
pub const STORM_LIGHT: f32 = 0.3;
// How little daylight, through the clouds, counts as dark for monsters under the sky, as night
// does: in the heaviest storm, the hour or so either side of night.
const DARK_DAYLIGHT: f32 = 0.35;
// How much of the rain still falls in the driest deserts, and the humidity under which hot lands
// get less, down to that below the first.
const DESERT_RAIN: f32 = 0.3;
const DRY: (f32, f32) = (0.25, 0.42);
// The temperature over which dry lands are deserts, rather than cold lands, which snow.
const HOT: (f32, f32) = (0.45, 0.6);
// How far below the snow line snow starts falling instead of rain, in blocks.
const SLEET_BAND: f32 = 30.0;

/// What `/weather` sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sky {
    Clear,
    /// Rain, under the lightning's.
    Rain,
    /// Rain with lightning and thunder.
    Storm,
}

impl Sky {
    pub const ALL: [Sky; 3] = [Sky::Clear, Sky::Rain, Sky::Storm];

    pub fn name(self) -> &'static str {
        match self {
            Sky::Clear => "clear",
            Sky::Rain => "rain",
            Sky::Storm => "storm",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|sky| sky.name() == name)
    }

    // How hard it rains, and how long it lasts unless it's said, in seconds: as long as the
    // seed's spells of it last at most.
    fn spell(self) -> (f32, f64) {
        match self {
            Sky::Clear => (0.0, CLEAR_SECONDS.1),
            Sky::Rain => (SHOWER_RAIN, STORM_SECONDS.1),
            Sky::Storm => (SET_STORM_RAIN, STORM_SECONDS.1),
        }
    }
}

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

    /// Whether it's raining hard enough at `now` for lightning: a storm, which beds can be
    /// slept through as nights can.
    pub fn is_stormy(&self, now: TimeOfDay) -> bool {
        self.rain(now) > STRIKE_RAIN
    }

    /// How much of the sun's and moon's light gets through the clouds at `now`, 0 to 1.
    pub fn shade(&self, now: TimeOfDay) -> f32 {
        1.0 - (1.0 - STORM_LIGHT) * self.rain(now)
    }

    /// Turns the weather to `sky` from `now`, for `seconds` or the longest of the seed's spells
    /// of it, easing from how it is; then the seed's spells go on, the next of the other kind.
    pub fn set(&mut self, sky: Sky, now: TimeOfDay, seconds: Option<f64>) {
        let (to, longest) = sky.spell();
        let rain = self.rain(now);
        *self = Self {
            spell: self.spell,
            since: now.0,
            until: now.0.saturating_add(ticks(seconds.unwrap_or(longest))),
            from: rain,
            to,
            after_rain: to == 0.0 && (rain > RAINBOW_RAIN || self.to > 0.0),
        };
    }

    /// Wakes sleepers at `now` to a clear sky, if it was raining, with a rainbow if the sun's
    /// up: sleeping skips a storm as it does the night.
    pub fn wake(&mut self, now: TimeOfDay) {
        if self.to == 0.0 && self.rain(now) <= RAINBOW_RAIN {
            return;
        }
        self.set(Sky::Clear, now, None);
        // the rain's over while they slept
        self.from = 0.0;
        self.after_rain = true;
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

/// Whether it's dark enough under the sky at `now` for monsters: at night, as ever, and in a
/// storm's gloom while the sun's low, which in the heaviest is the hour or so either side.
pub fn is_dark_out(now: TimeOfDay, weather: &Weather) -> bool {
    now.is_night() || daylight(now.days()) * weather.shade(now) < DARK_DAYLIGHT
}

/// What falls, each from 0 to 1.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Precipitation {
    pub rain: f32,
    pub snow: f32,
}

impl Precipitation {
    pub fn total(self) -> f32 {
        self.rain + self.snow
    }
}

/// What falls of rain `rain` hard, 0 to 1, where the climate is `climate`, `above_sea` blocks
/// up: snow where it's cold enough for snow to lie, as generation has it (the snowy grass of
/// the taiga and glaciers, and the heights above the snow line), rain elsewhere, and in hot,
/// dry lands only a little of either.
pub fn precipitation(rain: f32, climate: Climate, above_sea: f32) -> Precipitation {
    let wet = smoothstep(DRY.0, DRY.1, climate.humidity);
    let hot = smoothstep(HOT.0, HOT.1, climate.temperature);
    let wet = 1.0 - hot * (1.0 - wet);
    let falling = rain * (wet + (1.0 - wet) * DESERT_RAIN);
    let snow = snowing(climate.temperature, above_sea);
    Precipitation {
        rain: falling * (1.0 - snow),
        snow: falling * snow,
    }
}

// How much of what falls is snow, 0 to 1, at `temperature` `above_sea` blocks up: all of it
// where even the grass is under snow, and on the heights above the snow line, which it turns to
// over the band below.
fn snowing(temperature: f32, above_sea: f32) -> f32 {
    let cold = 1.0
        - smoothstep(
            SNOWY_GRASS_TEMPERATURE - 0.03,
            SNOWY_GRASS_TEMPERATURE + 0.03,
            temperature,
        );
    let high = if temperature < SNOW_MAX_TEMPERATURE {
        let line = snow_line(temperature) as f32;
        smoothstep(line - SLEET_BAND, line, above_sea)
    } else {
        0.0
    };
    cold.max(high)
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
    fn test_an_operators_weather_eases_in_and_the_seeds_go_on_after() {
        let mut weather = Weather::new(SEED, TimeOfDay(0));
        weather.set(Sky::Storm, TimeOfDay(100), Some(60.0));
        assert_eq!(
            (0, 100, 1_300),
            (weather.spell, weather.since, weather.until)
        );
        assert_eq!(0.0, weather.rain(TimeOfDay(100)));
        assert!(weather.is_stormy(TimeOfDay(1_000)));
        // a shower never lightens
        let mut shower = weather;
        shower.set(Sky::Rain, TimeOfDay(1_000), None);
        assert!(!shower.is_stormy(TimeOfDay(5_000)) && shower.rain(TimeOfDay(5_000)) > 0.4);
        // after the storm the seed's next clear spell, and a rainbow
        weather.run(SEED, TimeOfDay(1_300));
        assert_eq!((2, 0.0), (weather.spell, weather.to));
        assert!(weather.rainbow(TimeOfDay(1_300 + ticks(30.0))) > 0.9);
        // clearing a storm leaves a rainbow, and the seed's next storm comes after
        let mut cleared = Weather::new(SEED, TimeOfDay(0));
        cleared.run(SEED, TimeOfDay(cleared.until + 400));
        assert_eq!(1, cleared.spell);
        cleared.set(Sky::Clear, TimeOfDay(cleared.until - 1), Some(10.0));
        assert!(cleared.after_rain);
        cleared.run(SEED, TimeOfDay(cleared.until));
        assert_eq!(3, cleared.spell);
        assert!(cleared.to >= STORM_RAIN.0);
        for sky in Sky::ALL {
            assert_eq!(Some(sky), Sky::from_name(sky.name()));
        }
        assert_eq!(None, Sky::from_name("hail"));
    }

    #[test]
    fn test_waking_clears_rain_and_storms_but_leaves_a_clear_sky_alone() {
        let mut weather = Weather::new(SEED, TimeOfDay(0));
        weather.set(Sky::Storm, TimeOfDay(100), None);
        let woke = TimeOfDay(2_000);
        weather.wake(woke);
        assert_eq!((0.0, 0.0), (weather.to, weather.rain(woke)));
        assert!(weather.rainbow(TimeOfDay(2_000 + ticks(20.0))) > 0.9);
        let clear = Weather::new(SEED, TimeOfDay(0));
        let mut woken = clear;
        woken.wake(TimeOfDay(50));
        assert_eq!(clear, woken);
    }

    #[test]
    fn test_a_storms_gloom_is_dark_round_dusk_but_not_at_noon() {
        let clear = Weather::new(SEED, TimeOfDay(0));
        let mut storm = clear;
        storm.set(Sky::Storm, TimeOfDay(0), Some(1e9));
        let at = |days: f64| TimeOfDay::from_days(days);
        let late = at(10.72);
        assert!(!is_dark_out(late, &clear) && is_dark_out(late, &storm));
        let noon = at(10.5);
        assert!(!is_dark_out(noon, &clear) && !is_dark_out(noon, &storm));
        for night in [10.0, 10.9] {
            assert!(is_dark_out(at(night), &clear));
        }
        // and only modestly: an hour or so of game time either side, not the afternoon
        assert!(!is_dark_out(at(10.62), &storm));
    }

    fn climate(temperature: f32, humidity: f32) -> Climate {
        Climate {
            temperature,
            humidity,
        }
    }

    #[test]
    fn test_snow_falls_in_the_cold_and_up_high_and_deserts_get_little_rain() {
        let meadow = precipitation(0.9, climate(0.5, 0.5), 10.0);
        assert_eq!((0.9, 0.0), (meadow.rain, meadow.snow));
        // the taiga's snowy half and glaciers
        for temperature in [0.05, 0.2] {
            let cold = precipitation(0.9, climate(temperature, 0.5), 10.0);
            assert_eq!((0.0, 0.9), (cold.rain, cold.snow), "{temperature}");
        }
        // on a temperate mountain above the snow line, and rain lower down it
        let line = snow_line(0.5) as f32;
        assert_eq!(0.0, precipitation(0.9, climate(0.5, 0.5), line + 5.0).rain);
        assert_eq!(0.0, precipitation(0.9, climate(0.5, 0.5), line - 40.0).snow);
        // too hot for snow however high
        assert_eq!(0.0, precipitation(0.9, climate(0.7, 0.5), 600.0).snow);
        let desert = precipitation(0.9, climate(0.9, 0.1), 10.0);
        assert!(desert.snow == 0.0 && desert.rain > 0.0 && desert.rain < 0.3);
        // a cold, dry land isn't a desert: it snows
        assert_eq!(0.9, precipitation(0.9, climate(0.1, 0.1), 10.0).snow);
        assert_eq!(0.0, precipitation(0.0, climate(0.1, 0.5), 0.0).total());
    }

    #[test]
    fn test_the_weather_goes_over_the_wire() {
        let mut weather = Weather::new(SEED, TimeOfDay(5));
        weather.run(SEED, TimeOfDay(weather.until));
        let bytes = postcard::to_stdvec(&weather).unwrap();
        assert_eq!(weather, postcard::from_bytes(&bytes).unwrap());
    }
}
