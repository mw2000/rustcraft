//! Weather (§5.11): one front over the whole world, which turns from one spell of clear sky,
//! cloud, rain or storm to the next as the clock runs. The server decides it (INV-4), from the
//! world's seed unless an operator says otherwise, and tells clients as it changes
//! (`WeatherSync`), who work out how far each change has got from the time of day, as the
//! server does. What falls where a player is, rain or snow or nothing, goes by the climate
//! there (`precipitation`). The rest of this module is how it looks and sounds, on the client.

#[cfg(feature = "client")]
pub mod curtains;
#[cfg(feature = "client")]
pub mod lightning;
#[cfg(feature = "client")]
pub mod look;
#[cfg(feature = "client")]
pub mod precipitation;

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

use crate::chunks::generate::biome::Climate;
use crate::chunks::generate::generator::{
    snow_line, SNOWY_GRASS_TEMPERATURE, SNOW_MAX_TEMPERATURE,
};
use crate::chunks::generate::trees::hash;

/// How long a change of weather takes, in days: clouds gather over all of it, and rain starts
/// in its second half, or stops in its first as they clear.
pub const CHANGE_DAYS: f64 = 0.04;
// What the seed's spells are rolled with, so they're not the same rolls as anything else's.
const SALT: u64 = 0x5745_4154_4845_52;
// How likely each sky is to be the next spell's, in the order of `Sky::ALL`.
const CHANCES: [f64; 4] = [0.45, 0.25, 0.2, 0.1];
// How much of a storm's rain still falls in a desert, where little else does.
const DESERT_STORM_RAIN: f32 = 0.3;
// The humidity under which hot lands get less rain, down to none but storms' below the first.
const DRY: (f32, f32) = (0.25, 0.42);
// The temperature over which dry lands are deserts, rather than cold lands, which snow.
const HOT: (f32, f32) = (0.45, 0.6);
// How far below the snow line snow starts falling instead of rain, in blocks.
const SLEET_BAND: f32 = 30.0;

/// What the sky's doing.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Sky {
    #[default]
    Clear,
    Cloudy,
    Rain,
    /// Rain, with thunder and lightning and gusts.
    Storm,
}

impl Sky {
    pub const ALL: [Sky; 4] = [Sky::Clear, Sky::Cloudy, Sky::Rain, Sky::Storm];

    pub fn name(self) -> &'static str {
        match self {
            Sky::Clear => "clear",
            Sky::Cloudy => "cloudy",
            Sky::Rain => "rain",
            Sky::Storm => "storm",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|sky| sky.name() == name)
    }

    /// How the weather is once this sky has set in.
    pub fn levels(self) -> Levels {
        let (cover, rain, storm) = match self {
            Sky::Clear => (0.0, 0.0, 0.0),
            Sky::Cloudy => (0.55, 0.0, 0.0),
            Sky::Rain => (0.85, 0.7, 0.0),
            Sky::Storm => (1.0, 1.0, 1.0),
        };
        Levels { cover, rain, storm }
    }

    /// How long a spell of it lasts, in days, for `roll` from 0 to 1: clear skies longest and
    /// storms shortest.
    pub fn length(self, roll: f64) -> f64 {
        let (shortest, longest) = match self {
            Sky::Clear => (0.4, 1.6),
            Sky::Cloudy => (0.2, 0.7),
            Sky::Rain => (0.15, 0.45),
            Sky::Storm => (0.08, 0.25),
        };
        shortest + (longest - shortest) * roll
    }
}

/// How the weather is, each from 0 to 1: how much of the sky is under cloud, how hard it
/// rains (or snows, where it's cold, see `precipitation`), and how stormy it is: thunder,
/// lightning and gusts.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Default)]
pub struct Levels {
    pub cover: f32,
    pub rain: f32,
    pub storm: f32,
}

impl Levels {
    /// How grey and closed in the sky is, from 0 under scattered cloud to 1 under a storm's.
    pub fn overcast(self) -> f32 {
        smoothstep(0.3, 1.0, self.cover)
    }

    /// How much of the sun's light gets through the clouds, 0 to 1: little, under a storm,
    /// so shadows all but go.
    pub fn sunlight(self) -> f32 {
        1.0 - 0.85 * self.overcast()
    }

    /// How much of the sky's light gets through, 0 to 1, which lights the land in shade and
    /// sets how dark it is under cover: a storm by day is about as dark as dusk. What decides
    /// how dark it is for monsters goes by the sky's light times this (M3.3).
    pub fn daylight(self) -> f32 {
        1.0 - 0.5 * self.overcast() - 0.2 * self.storm
    }
}

/// The weather over the whole world: the spell it's in, which began at `since` and lasts until
/// `until`, in days as `TimeOfDay` counts them, turning from how it was as it began. The seed
/// decides each spell after it (`spell`), unless an operator sets one with `/weather`. Saved in
/// `world.toml`, and sent to clients as it changes.
#[derive(Resource, Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Weather {
    pub sky: Sky,
    pub since: f64,
    pub until: f64,
    /// How it was as this spell began.
    pub from: Levels,
    /// Which of the world's spells this is, from 0, its first.
    pub spell: u64,
}

impl Default for Weather {
    /// A clear sky that never changes, for where there's no world, such as the title screen.
    fn default() -> Self {
        Self {
            sky: Sky::Clear,
            since: 0.0,
            until: f64::INFINITY,
            from: Levels::default(),
            spell: 0,
        }
    }
}

impl Weather {
    /// A world's first spell, from `time`: always clear, for as long as its seed says.
    pub fn new(seed: u32, time: f64) -> Self {
        let (sky, days) = spell(seed, 0);
        Self {
            sky,
            since: time,
            until: time + days,
            from: sky.levels(),
            spell: 0,
        }
    }

    /// How the weather is at `time`, partway through turning to this spell's sky if it's not
    /// long begun.
    pub fn levels(&self, time: f64) -> Levels {
        let through = ((time - self.since) / CHANGE_DAYS).clamp(0.0, 1.0) as f32;
        let to = self.sky.levels();
        let ease = |from: f32, to: f32, t: f32| from + (to - from) * smoothstep(0.0, 1.0, t);
        // rain comes once the clouds have gathered, and stops before they clear
        let late = |from: f32, to: f32| {
            let t = if to > from {
                2.0 * through - 1.0
            } else {
                2.0 * through
            };
            ease(from, to, t)
        };
        Levels {
            cover: ease(self.from.cover, to.cover, through),
            rain: late(self.from.rain, to.rain),
            storm: late(self.from.storm, to.storm),
        }
    }

    /// Moves on to the seed's next spells, as many as have passed by `time`, which after a
    /// night slept through may be several. Whether it changed.
    pub fn run(&mut self, seed: u32, time: f64) -> bool {
        let mut changed = false;
        while time >= self.until {
            let next = self.spell + 1;
            let (sky, days) = spell(seed, next);
            *self = Self {
                sky,
                since: self.until,
                until: self.until + days,
                from: self.levels(self.until),
                spell: next,
            };
            changed = true;
        }
        changed
    }

    /// Turns the weather to `sky` from `time`, for `days`, after which the seed's spells go on.
    pub fn set(&mut self, sky: Sky, time: f64, days: f64) {
        *self = Self {
            sky,
            since: time,
            until: time + days,
            from: self.levels(time),
            spell: self.spell,
        };
    }

    /// Whether it's storming at `time`, which beds can be slept through as nights can.
    pub fn is_stormy(&self, time: f64) -> bool {
        self.levels(time).storm > 0.5
    }

    /// Wakes sleepers at `time` to the rain clearing, if it was raining: sleeping skips storms
    /// and rain as it does the night.
    pub fn wake(&mut self, time: f64) {
        if self.levels(time).rain > 0.0 || matches!(self.sky, Sky::Rain | Sky::Storm) {
            self.set(Sky::Clear, time, Sky::Clear.length(0.5));
        }
    }
}

/// The seed's spell `n`, its sky and how long it lasts, in days. The first is clear.
fn spell(seed: u32, n: u64) -> (Sky, f64) {
    let roll = |salt: i64| {
        let bits = hash(u64::from(seed) ^ SALT, n as i64, salt) >> 11;
        bits as f64 / (1u64 << 53) as f64
    };
    let sky = if n == 0 {
        Sky::Clear
    } else {
        let mut pick = roll(0);
        let mut chosen = Sky::Clear;
        for (sky, chance) in Sky::ALL.into_iter().zip(CHANCES) {
            chosen = sky;
            if pick < chance {
                break;
            }
            pick -= chance;
        }
        chosen
    };
    (sky, sky.length(roll(1)))
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

/// What falls out of weather like `levels` where the climate is `climate`, `above_sea` blocks
/// up: snow where it's cold enough for it to lie, as generation has it, rain elsewhere, and in
/// hot, dry lands little but a storm's.
pub fn precipitation(levels: Levels, climate: Climate, above_sea: f32) -> Precipitation {
    let wet = smoothstep(DRY.0, DRY.1, climate.humidity);
    let hot = smoothstep(HOT.0, HOT.1, climate.temperature);
    let wet = 1.0 - hot * (1.0 - wet);
    let falling = levels.rain * (wet + (1.0 - wet) * levels.storm * DESERT_STORM_RAIN);
    let snow = snowing(climate.temperature, above_sea);
    Precipitation {
        rain: falling * (1.0 - snow),
        snow: falling * snow,
    }
}

// How much of what falls is snow, 0 to 1, at `temperature` `above_sea` blocks up: all of it
// where even the grass is under snow, and on the heights above the snow line, which it turns
// to over the band below.
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

#[cfg(test)]
mod tests {
    use super::*;

    const SEED: u32 = 7;

    fn climate(temperature: f32, humidity: f32) -> Climate {
        Climate {
            temperature,
            humidity,
        }
    }

    // The skies of the first `count` spells for `seed`, as a server running from day 0 has them.
    fn spells(seed: u32, count: usize) -> Vec<(Sky, f64)> {
        let mut weather = Weather::new(seed, 0.0);
        let mut seen = vec![(weather.sky, weather.since)];
        while seen.len() < count {
            weather.run(seed, weather.until);
            seen.push((weather.sky, weather.since));
        }
        seen
    }

    #[test]
    fn test_a_seed_has_the_same_spells_every_time_starting_clear() {
        let sequence = spells(SEED, 200);
        assert_eq!(sequence, spells(SEED, 200));
        assert_ne!(sequence, spells(SEED + 1, 200));
        assert_eq!(Sky::Clear, sequence[0].0);
        // every sky comes up, about as often as it's meant to
        for (sky, chance) in Sky::ALL.into_iter().zip(CHANCES) {
            let share = sequence.iter().filter(|(s, _)| *s == sky).count() as f64 / 200.0;
            assert!((share - chance).abs() < 0.1, "{sky:?} {share}");
        }
        // and spells follow on, each starting where the last ended
        let mut weather = Weather::new(SEED, 3.0);
        let first = weather.until;
        assert!(weather.run(SEED, first));
        assert_eq!(first, weather.since);
        assert!(weather.until > weather.since);
        assert!(!weather.run(SEED, first));
    }

    #[test]
    fn test_a_long_sleep_runs_through_every_spell_it_missed() {
        let mut slept = Weather::new(SEED, 0.0);
        slept.run(SEED, 20.0);
        let mut awake = Weather::new(SEED, 0.0);
        let mut time = 0.0;
        while time <= 20.0 {
            awake.run(SEED, time);
            time += 0.001;
        }
        assert_eq!(slept, awake);
        assert!(slept.spell > 10, "{}", slept.spell);
    }

    #[test]
    fn test_weather_turns_smoothly_with_rain_after_the_clouds_and_before_they_clear() {
        let mut weather = Weather::new(SEED, 0.0);
        weather.set(Sky::Storm, 1.0, 0.2);
        assert_eq!(Levels::default(), weather.levels(1.0));
        let quarter = weather.levels(1.0 + CHANGE_DAYS * 0.25);
        assert!(quarter.cover > 0.1 && quarter.rain == 0.0, "{quarter:?}");
        assert_eq!(Sky::Storm.levels(), weather.levels(1.0 + CHANGE_DAYS));
        // never a jump along the way
        let mut last = weather.levels(1.0);
        for step in 1..=100 {
            let now = weather.levels(1.0 + CHANGE_DAYS * step as f64 / 100.0);
            for (a, b) in [
                (last.cover, now.cover),
                (last.rain, now.rain),
                (last.storm, now.storm),
            ] {
                assert!((b - a).abs() < 0.05, "{last:?} to {now:?}");
            }
            last = now;
        }

        // clearing partway through, from where it had got to
        let partway = weather.levels(1.0 + CHANGE_DAYS * 0.75);
        weather.set(Sky::Clear, 1.0 + CHANGE_DAYS * 0.75, 1.0);
        assert_eq!(partway, weather.levels(weather.since));
        let rain_stopped = weather.levels(weather.since + CHANGE_DAYS * 0.5);
        assert!(rain_stopped.rain == 0.0 && rain_stopped.cover > 0.3);
        assert_eq!(Levels::default(), weather.levels(weather.since + CHANGE_DAYS));
    }

    #[test]
    fn test_a_spell_runs_into_the_next_from_how_it_ended() {
        let mut weather = Weather::new(SEED, 0.0);
        weather.set(Sky::Rain, 0.0, 0.3);
        let ended = weather.levels(0.3);
        weather.run(SEED, 0.3);
        assert_eq!(ended, weather.levels(0.3));
        assert_eq!(1, weather.spell);
    }

    #[test]
    fn test_waking_clears_rain_and_storms_but_leaves_a_clear_sky_alone() {
        let mut weather = Weather::new(SEED, 0.0);
        weather.set(Sky::Storm, 1.0, 0.2);
        assert!(weather.is_stormy(1.1) && !weather.is_stormy(1.0));
        weather.wake(1.1);
        assert_eq!(Sky::Clear, weather.sky);
        assert_eq!(Levels::default(), weather.levels(1.1 + CHANGE_DAYS));
        let clear = Weather::new(SEED, 0.0);
        let mut woken = clear;
        woken.wake(0.1);
        assert_eq!(clear, woken);
    }

    #[test]
    fn test_snow_falls_in_the_cold_and_up_high_and_deserts_get_little_rain() {
        let storm = Sky::Storm.levels();
        let rain = Sky::Rain.levels();
        let meadow = precipitation(rain, climate(0.5, 0.5), 10.0);
        assert_eq!((rain.rain, 0.0), (meadow.rain, meadow.snow));
        // taiga's snowy half and glaciers
        for temperature in [0.05, 0.2] {
            let cold = precipitation(rain, climate(temperature, 0.5), 10.0);
            assert_eq!((0.0, rain.rain), (cold.rain, cold.snow), "{temperature}");
        }
        // on a temperate mountain above the snow line, and rain lower down it
        let line = snow_line(0.5) as f32;
        assert_eq!(0.0, precipitation(rain, climate(0.5, 0.5), line + 5.0).rain);
        assert_eq!(0.0, precipitation(rain, climate(0.5, 0.5), line - 40.0).snow);
        // too hot for snow however high
        assert_eq!(0.0, precipitation(rain, climate(0.7, 0.5), 600.0).snow);
        let desert = climate(0.9, 0.1);
        assert_eq!(0.0, precipitation(rain, desert, 10.0).total());
        let desert_storm = precipitation(storm, desert, 10.0).rain;
        assert!(desert_storm > 0.0 && desert_storm < 0.5, "{desert_storm}");
        // a cold, dry land isn't a desert: it snows
        assert_eq!(rain.rain, precipitation(rain, climate(0.1, 0.1), 10.0).snow);
        assert_eq!(Precipitation::default(), precipitation(Levels::default(), climate(0.1, 0.5), 0.0));
    }

    #[test]
    fn test_storms_dim_the_sun_most_and_the_sky_less() {
        let clear = Sky::Clear.levels();
        assert_eq!((1.0, 1.0), (clear.sunlight(), clear.daylight()));
        let storm = Sky::Storm.levels();
        assert!(storm.sunlight() < 0.2);
        assert!(storm.daylight() > storm.sunlight() && storm.daylight() < 0.4);
        let cloudy = Sky::Cloudy.levels();
        assert!(cloudy.sunlight() > 0.7, "{}", cloudy.sunlight());
    }

    #[test]
    fn test_weather_round_trips_by_name() {
        for sky in Sky::ALL {
            assert_eq!(Some(sky), Sky::from_name(sky.name()));
        }
        assert_eq!(None, Sky::from_name("hail"));
        let mut weather = Weather::new(SEED, 2.5);
        weather.set(Sky::Storm, 2.6, 0.1);
        let bytes = postcard::to_stdvec(&weather).unwrap();
        assert_eq!(weather, postcard::from_bytes(&bytes).unwrap());
    }
}
