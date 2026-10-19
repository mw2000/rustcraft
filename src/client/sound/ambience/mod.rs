//! Ambience: the wind, and loops of the sea, birds, crickets, a desert's air, leaves rustling, a
//! cave's drips and being under water, each as loud as where the player is calls for, crossfading
//! as they move; owls hooting now and then in the woods at night; rain, and thunder after
//! lightning; and, after a long quiet, a piece of music.
//!
//! The wind, the recorded loops and the owl, and how the birds, crickets and the desert come and
//! go and the leaves stir with the wind, are ported from upstream rustcraft's ambience (#77), by
//! Aymen Qader, into this mixer, which picks what's heard from where the player is; the rain and
//! thunder, and birds and crickets falling quiet in the rain, from its weather (#80), by him too.

mod rain;
mod wind;

use crate::client::player::WorldCamera;
use std::f32::consts::TAU;
use std::time::Duration;

use bevy::audio::{AddAudioSource, SpatialScale, Volume};
use bevy::prelude::*;
use rand::seq::IndexedRandom;
use rand::RngExt;

use super::volume::Volumes;
use super::OneShot;
use crate::block::BlockType;
use crate::chunks::generate::noise::TerrainSample;
use crate::chunks::generate::WATER_LEVEL;
use crate::client::player::LocalPlayer;
use crate::clock::TimeOfDay;
use crate::weather::look::{Thunderclap, THUNDER_SPEED};
use crate::weather::Weather;
use crate::world::World;
use rain::{RainSound, Thunder, RAIN_PLAYBACK};
use wind::{wind_strength, Dial, Gust, Wind, WindDials, WIND_PLAYBACK};

// How often where the player is is looked at again, which costs some noise samples, and how
// long the loops take to fade to what it calls for.
const LOOK_INTERVAL: Duration = Duration::from_millis(500);
const FADE_SECONDS: f32 = 2.5;
// How far round the player, in blocks, the sea's listened for.
const SEA_REACH: f64 = 28.0;
// How far round the player, in blocks, the trees count as overhead, for the leaves.
const CANOPY_REACH: f64 = 16.0;
// The tree density, as generation has it, that sounds like the depths of a forest overhead.
const DENSE_FOREST: f64 = 0.4;
// How far under the ground's surface, in blocks, the eyes must be for a cave's sound.
const CAVE_DEPTH: f32 = 3.0;
// Faster than this, in blocks a second, the eyes didn't move but were teleported.
const TELEPORT_SPEED: f32 = 200.0;
// Music: the first piece a few minutes in, then long quiets between, in seconds.
const FIRST_MUSIC: (f32, f32) = (90.0, 240.0);
const MUSIC_QUIET: (f32, f32) = (480.0, 1080.0);
const MUSIC_PIECES: [&str; 3] = ["morning", "drift", "embers"];
// Each loop's and the music's volume at its loudest, against the one-off sounds, before each
// loop's own, see `Loop::volume`.
const LOOP_VOLUME: f32 = 0.6;
const MUSIC_VOLUME: f32 = 0.5;
// The wind's volume: a gale a little over the birds and the sea at their loudest, as upstream
// had it and as loud as the high wind loop it replaced (about -29.5 dB by `analyse.py`'s
// measure), and a breeze on the ground under the quietest loops.
const WIND_VOLUME: f32 = 0.85;
// Under water the wind is only a murmur through the surface.
const UNDERWATER_WIND: f32 = 0.02;
// How long the wind takes to follow a change in where the player is, in seconds.
const WIND_FOLLOW_SECONDS: f32 = 0.6;
// How weak the wind is before it's paused.
const STILL: f32 = 1e-3;
// How strong the wind blows on the ground in the heaviest rain: strong enough to keep blowing
// between spells, see `wind::STEADY`. From upstream's weather (#80).
const STORM_WIND: f32 = 0.6;
// Rain's volume at its heaviest, and thunder's, as upstream played them.
const RAIN_VOLUME: f32 = 1.0;
const THUNDER_VOLUME: f32 = 1.0;
// How loud thunder is heard in a cave, through the rock, and under water.
const MUFFLED_THUNDER: f32 = 0.35;
// How long between owls' hoots in the woods at night, in seconds, at the least and most, and
// how far off one hoots, in blocks.
const HOOT_SECONDS: (f32, f32) = (12.0, 40.0);
const OWL_DISTANCE: (f32, f32) = (20.0, 45.0);
// Shrinks blocks for an owl's falloff with distance, so one a few dozen blocks off is faint but
// clear rather than out of earshot as one-off sounds there are.
const OWL_SCALE: f32 = 0.06;
// Puts a hoot a few dozen blocks off about as loud as the crickets.
const OWL_VOLUME: f32 = 0.8;

pub(super) struct AmbiencePlugin;

impl Plugin for AmbiencePlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<Wind>()
            .add_audio_source::<RainSound>()
            .add_audio_source::<Thunder>()
            .add_message::<Thunderclap>()
            .init_resource::<Mix>()
            .init_resource::<WindDials>()
            .init_resource::<RainDial>()
            .insert_resource(LookAgain(Timer::new(LOOK_INTERVAL, TimerMode::Repeating)))
            .insert_resource(Music::new())
            .add_systems(Startup, start_loops)
            .add_systems(
                Update,
                (look_around, (fade_loops, blow_wind, hoot, rain, thunder)).chain(),
            )
            .add_systems(Update, play_music);
    }
}

/// The ambience loops, each a file in `assets/sounds/ambience/`: the cave synthesised by the
/// sounds skill, the rest recordings (`assets/CREDITS.md`).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum Loop {
    Sea,
    Birds,
    Crickets,
    Desert,
    Leaves,
    Cave,
    Underwater,
}

impl Loop {
    const ALL: [Self; 7] = [
        Self::Sea,
        Self::Birds,
        Self::Crickets,
        Self::Desert,
        Self::Leaves,
        Self::Cave,
        Self::Underwater,
    ];

    fn file(self) -> &'static str {
        match self {
            Self::Sea => "sea",
            Self::Birds => "birds",
            Self::Crickets => "crickets",
            Self::Desert => "desert",
            Self::Leaves => "leaves",
            Self::Cave => "cave",
            Self::Underwater => "underwater",
        }
    }

    /// How loud this loop plays at its loudest, as the files are all levelled alike: the sea
    /// and birds up front, crickets, the desert's air and the leaves behind, as upstream had
    /// them.
    fn volume(self) -> f32 {
        match self {
            Self::Sea | Self::Birds | Self::Cave => 1.0,
            Self::Underwater => 1.25,
            // -5 dB
            Self::Crickets => 0.56,
            // -7 dB
            Self::Desert | Self::Leaves => 0.45,
        }
    }

    /// Whether this loop comes and goes on its own, as birds and crickets do, rather than
    /// playing on as long as where the player is calls for it.
    fn bursts(self) -> Option<Bursts> {
        match self {
            // birds strike up now and then, and fall quiet more than they sing
            Self::Birds => Some(Bursts {
                seconds: (3.0, 12.0),
                quiet: (0.5, 0.85),
            }),
            // a cricket or two starts up, chirps a while and stops
            Self::Crickets => Some(Bursts {
                seconds: (2.0, 8.0),
                quiet: (0.6, 0.9),
            }),
            Self::Desert => Some(Bursts {
                seconds: (12.0, 40.0),
                quiet: (0.35, 0.75),
            }),
            // the leaves come and go with the wind's breezes instead, see `fade_loops`
            _ => None,
        }
    }
}

/// How a loop comes and goes: it follows a value wandering 0 to 1, see `Gust`.
struct Bursts {
    // how long the value takes to wander to each new target, in seconds, at the least and most
    seconds: (f32, f32),
    // where the value goes from silence (under the first) to full (over the second): the
    // higher they are, the more of the time it's silent
    quiet: (f32, f32),
}

/// Where a loop that comes and goes is in its comings and goings.
#[derive(Component, Default)]
struct Activity(Gust);

/// What the ambience goes by: where the player's eyes are and what's round them.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Surroundings {
    /// Blocks above the sea.
    height: f32,
    /// Blocks above the ground, or the sea where it's over the ground, as generated.
    altitude: f32,
    /// How fast the eyes are moving, in blocks a second.
    speed: f32,
    /// Deep enough under the ground to be in a cave.
    in_cave: bool,
    underwater: bool,
    /// How much of the land round about is sea, 0 to 1.
    sea: f32,
    /// How thick the trees grow here, as generation has it, 0 to 1.
    trees: f32,
    /// How much of the sky right overhead the trees' crowns cover, 0 to 1.
    canopy: f32,
    temperature: f32,
    humidity: f32,
    /// The time of day, in days.
    time: f32,
    /// How hard it's raining, 0 to 1.
    rain: f32,
}

/// What should be heard: how loud each loop should be, 0 to 1, in the order of `Loop::ALL`,
/// the wind's strength, how likely owls are to hoot, how hard the rain's heard, and how loud
/// thunder is.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
struct Mix {
    loops: [f32; 7],
    wind: f32,
    owls: f32,
    rain: f32,
    thunder: f32,
}

impl Mix {
    fn of(self, which: Loop) -> f32 {
        self.loops[which as usize]
    }
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// What's heard where `s` says: under water only the water; in a cave its drips and hum; and in
/// the open the wind, rising with height above the ground, on peaks and in the cold, and with
/// speed; the sea near it, unless it's frozen; birds by day where it's mild, more among trees,
/// crickets by night, both thinning up the mountains and off the ground, and owls at night in
/// the woods; a desert's air where it's hot and dry; and leaves under the trees' crowns, for the
/// wind to stir; and rain, which blows the wind up and quiets the birds and crickets.
/// Biomes come in by their climate and trees, so a new one sounds right without being named
/// here. Thunder's heard everywhere, muffled in caves and under water.
fn mix(s: &Surroundings) -> Mix {
    let mut mix = Mix {
        thunder: 1.0,
        ..Mix::default()
    };
    let mut set = |which: Loop, volume: f32| mix.loops[which as usize] = volume.clamp(0.0, 1.0);
    if s.underwater {
        set(Loop::Underwater, 1.0);
        mix.wind = UNDERWATER_WIND;
        mix.thunder = MUFFLED_THUNDER;
        return mix;
    }
    if s.in_cave {
        set(Loop::Cave, 1.0);
        mix.thunder = MUFFLED_THUNDER;
        return mix;
    }
    let day = smoothstep(0.21, 0.29, s.time.rem_euclid(1.0))
        * (1.0 - smoothstep(0.71, 0.79, s.time.rem_euclid(1.0)));
    let cold = 1.0 - smoothstep(0.1, 0.25, s.temperature);
    let mild = smoothstep(0.15, 0.3, s.temperature)
        * (1.0 - smoothstep(0.75, 0.9, s.temperature))
        * smoothstep(0.15, 0.3, s.humidity);
    // the desert biome's own climate
    let arid = smoothstep(0.55, 0.65, s.temperature) * (1.0 - smoothstep(0.35, 0.45, s.humidity));
    // life thins out up the mountains, and what's on the ground fades as the eyes climb off it
    let lowland = 1.0 - smoothstep(60.0, 160.0, s.height);
    let near = |from, to| 1.0 - smoothstep(from, to, s.altitude);
    let wooded = smoothstep(0.02, 0.2, s.trees);
    // birds and crickets fall silent in the rain
    let wildlife = mild * lowland * (1.0 - arid) * near(15.0, 90.0) * (1.0 - s.rain);
    mix.wind = wind_strength(s.altitude, s.speed)
        .max(0.75 * smoothstep(90.0, 250.0, s.height))
        .max(0.6 * cold)
        .max(STORM_WIND * s.rain);
    mix.rain = s.rain;
    set(
        Loop::Sea,
        s.sea * (1.0 - smoothstep(15.0, 60.0, s.height)) * (1.0 - cold),
    );
    set(Loop::Birds, day * wildlife * (0.35 + 0.65 * wooded));
    set(Loop::Crickets, (1.0 - day) * wildlife * (1.0 - s.sea));
    set(Loop::Desert, arid * near(30.0, 150.0));
    set(Loop::Leaves, s.canopy * near(8.0, 40.0));
    mix.owls = (1.0 - day) * wildlife * wooded;
    mix
}

impl Surroundings {
    /// What's round eyes at `eyes` in `world` at `time`, moving at `speed`, in `weather`.
    fn at(world: &World, eyes: Vec3, time: &TimeOfDay, speed: f32, weather: &Weather) -> Self {
        let noise = world.region_store.noise();
        let (x, z) = (eyes.x as f64, eyes.z as f64);
        let here = noise.sample(x, z);
        let climate = noise.climate(x, z);
        let block = eyes.floor().as_i64vec3();
        let ring = |reach: f64| {
            (0..8).map(move |i| {
                let angle = i as f64 * std::f64::consts::TAU / 8.0;
                noise.sample(x + reach * angle.cos(), z + reach * angle.sin())
            })
        };
        let sea = ring(SEA_REACH)
            .filter(|sample| sample.height < WATER_LEVEL as f64)
            .count() as f32
            / 8.0;
        let cover = |sample: &TerrainSample| {
            if sample.height < WATER_LEVEL as f64 {
                0.0
            } else {
                (sample.trees / DENSE_FOREST).min(1.0) as f32
            }
        };
        // the eyes' own column counts as much as the ring round it
        let around: f32 = ring(CANOPY_REACH).map(|s| cover(&s)).sum();
        let canopy = (8.0 * cover(&here) + around) / 16.0;
        Self {
            height: eyes.y - WATER_LEVEL as f32,
            altitude: eyes.y - here.height.max(WATER_LEVEL as f64) as f32,
            speed,
            // the ground as generated, which edits don't change: under a roof, or an overhang,
            // isn't a cave
            in_cave: world.is_under_ground(block) && eyes.y < here.height as f32 - CAVE_DEPTH,
            underwater: world.get_block_at(block) == BlockType::Water,
            sea,
            trees: here.trees as f32,
            canopy,
            temperature: climate.temperature,
            humidity: climate.humidity,
            time: time.days(),
            rain: weather.rain(*time),
        }
    }
}

#[derive(Resource)]
struct LookAgain(Timer);

/// The wind's player, see `blow_wind`.
#[derive(Component)]
struct WindPlayer;

/// How hard the rain's heard, which its sound follows on the audio thread, see `rain::RainSound`.
#[derive(Resource, Default)]
struct RainDial(Dial);

/// The rain's player, see `rain`.
#[derive(Component)]
struct RainPlayer;

fn start_loops(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut winds: ResMut<Assets<Wind>>,
    dials: Res<WindDials>,
    mut rains: ResMut<Assets<RainSound>>,
    rain_dial: Res<RainDial>,
) {
    for which in Loop::ALL {
        commands.spawn((
            which,
            Activity::default(),
            AudioPlayer::new(assets.load(format!("sounds/ambience/{}.ogg", which.file()))),
            PlaybackSettings::LOOP.with_volume(Volume::SILENT).paused(),
        ));
    }
    let wind = winds.add(Wind {
        dials: dials.clone(),
    });
    commands.spawn((WindPlayer, AudioPlayer(wind), WIND_PLAYBACK));
    let rain = rains.add(RainSound {
        level: rain_dial.0.clone(),
    });
    commands.spawn((RainPlayer, AudioPlayer(rain), RAIN_PLAYBACK));
    commands.insert_resource(Owl(assets.load("sounds/owl.ogg")));
}

#[allow(clippy::too_many_arguments)]
fn look_around(
    time: Res<Time<Real>>,
    mut again: ResMut<LookAgain>,
    world: Option<Res<World>>,
    clock: Res<TimeOfDay>,
    weather: Res<Weather>,
    eyes: Query<&GlobalTransform, (WorldCamera, With<ChildOf>)>,
    player: Query<(), With<LocalPlayer>>,
    mut wanted: ResMut<Mix>,
    mut last: Local<Option<Vec3>>,
) {
    if !again.0.tick(time.delta()).just_finished() {
        return;
    }
    // nothing's heard until there's a world to hear and a player to hear it
    let (Some(world), Ok(eyes), Ok(())) = (world, eyes.single(), player.single()) else {
        *wanted = Mix::default();
        *last = None;
        return;
    };
    let eyes = eyes.translation();
    let speed = last.replace(eyes).map_or(0.0, |last| {
        let speed = last.distance(eyes) / LOOK_INTERVAL.as_secs_f32();
        if speed > TELEPORT_SPEED {
            0.0
        } else {
            speed
        }
    });
    *wanted = mix(&Surroundings::at(&world, eyes, &clock, speed, &weather));
}

// Eases each loop towards what's wanted, as it comes and goes, pausing those faded out, so they
// cost nothing.
fn fade_loops(
    time: Res<Time<Real>>,
    wanted: Res<Mix>,
    volumes: Res<Volumes>,
    dials: Res<WindDials>,
    mut loops: Query<(&Loop, &mut Activity, &mut AudioSink)>,
    mut playing: Local<[f32; 7]>,
) {
    let step = (time.delta_secs() / FADE_SECONDS).min(1.0);
    let mut rng = rand::rng();
    for (&which, mut activity, mut sink) in &mut loops {
        let now = &mut playing[which as usize];
        let mut target = wanted.of(which);
        if let Some(bursts) = which.bursts() {
            let (a, b) = (rng.random(), rng.random());
            let wander = activity.0.advance(time.delta_secs(), bursts.seconds, a, b);
            target *= smoothstep(bursts.quiet.0, bursts.quiet.1, wander);
        }
        if which == Loop::Leaves {
            target *= dials.breeze.get();
        }
        *now += (target - *now).clamp(-step, step);
        if *now <= 0.0 && target <= 0.0 {
            if !sink.is_paused() {
                sink.pause();
            }
            continue;
        }
        sink.set_volume(Volume::Linear(
            *now * LOOP_VOLUME * which.volume() * volumes.ambience(),
        ));
        if sink.is_paused() {
            sink.play();
        }
    }
}

// Glides the wind's strength after what's wanted, for its sound to follow on the audio thread,
// pausing it in still air.
fn blow_wind(
    time: Res<Time<Real>>,
    wanted: Res<Mix>,
    volumes: Res<Volumes>,
    dials: Res<WindDials>,
    mut wind: Query<&mut AudioSink, With<WindPlayer>>,
) {
    let follow = 1.0 - (-time.delta_secs() / WIND_FOLLOW_SECONDS).exp();
    let current = dials.strength.get();
    let strength = current + (wanted.wind - current) * follow;
    dials.strength.set(strength);
    let Ok(mut sink) = wind.single_mut() else {
        return;
    };
    let volume = WIND_VOLUME * volumes.ambience();
    if (strength < STILL && wanted.wind < STILL) || volume == 0.0 {
        if !sink.is_paused() {
            sink.pause();
        }
        return;
    }
    sink.set_volume(Volume::Linear(volume));
    if sink.is_paused() {
        sink.play();
    }
}

// Sets the rain's sound to how hard it's raining, which it glides after, and pauses it once
// it's dry.
fn rain(
    wanted: Res<Mix>,
    volumes: Res<Volumes>,
    dial: Res<RainDial>,
    mut player: Query<&mut AudioSink, With<RainPlayer>>,
) {
    dial.0.set(wanted.rain);
    let Ok(mut sink) = player.single_mut() else {
        return;
    };
    let volume = RAIN_VOLUME * volumes.ambience();
    if wanted.rain < STILL || volume == 0.0 {
        if !sink.is_paused() {
            sink.pause();
        }
        return;
    }
    sink.set_volume(Volume::Linear(volume));
    if sink.is_paused() {
        sink.play();
    }
}

// Plays each lightning strike's thunder once the sound's had time to come that far, at the
// speed of sound, keeping real time as the strikes do.
fn thunder(
    mut commands: Commands,
    time: Res<Time<Real>>,
    wanted: Res<Mix>,
    volumes: Res<Volumes>,
    mut claps: MessageReader<Thunderclap>,
    mut thunders: ResMut<Assets<Thunder>>,
    mut coming: Local<Vec<(f32, f32)>>,
) {
    for clap in claps.read() {
        coming.push((clap.distance / THUNDER_SPEED, clap.distance));
    }
    let dt = time.delta_secs();
    let volume = THUNDER_VOLUME * wanted.thunder * volumes.ambience();
    coming.retain_mut(|(left, distance)| {
        *left -= dt;
        if *left > 0.0 {
            return true;
        }
        if volume > 0.0 {
            commands.spawn((
                AudioPlayer(thunders.add(Thunder {
                    distance: *distance,
                    seed: rand::random(),
                })),
                PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume)),
            ));
        }
        false
    });
}

#[derive(Resource)]
struct Owl(Handle<AudioSource>);

// Now and then, where owls are likely, one hoots from somewhere off in the trees.
fn hoot(
    mut commands: Commands,
    time: Res<Time<Real>>,
    wanted: Res<Mix>,
    volumes: Res<Volumes>,
    owl: Option<Res<Owl>>,
    ears: Query<&GlobalTransform, (WorldCamera, With<ChildOf>)>,
    mut wait: Local<Option<f32>>,
) {
    let mut rng = rand::rng();
    let left = wait.get_or_insert_with(|| rng.random_range(HOOT_SECONDS.0..HOOT_SECONDS.1));
    // owls only call where the woods are thick enough at night, and wait longer in thin ones
    *left -= time.delta_secs() * wanted.owls;
    if *left > 0.0 || wanted.owls < 0.25 {
        return;
    }
    *wait = None;
    let (Some(owl), Ok(ears)) = (owl, ears.single()) else {
        return;
    };
    if volumes.ambience() == 0.0 {
        return;
    }
    let angle = rng.random_range(0.0..TAU);
    let distance = rng.random_range(OWL_DISTANCE.0..OWL_DISTANCE.1);
    let offset =
        Vec3::new(angle.cos(), 0.0, angle.sin()) * distance + Vec3::Y * rng.random_range(4.0..12.0);
    commands.spawn((
        OneShot(0.0),
        AudioPlayer(owl.0.clone()),
        PlaybackSettings {
            spatial: true,
            spatial_scale: Some(SpatialScale::new(OWL_SCALE)),
            volume: Volume::Linear(OWL_VOLUME * volumes.ambience()),
            speed: rng.random_range(0.95..1.05),
            ..PlaybackSettings::DESPAWN
        },
        Transform::from_translation(ears.translation() + offset),
    ));
}

/// When the next piece of music plays, in seconds of the game running, and the one playing.
#[derive(Resource)]
struct Music {
    next: f32,
    playing: Option<Entity>,
}

impl Music {
    fn new() -> Self {
        Self {
            next: rand::rng().random_range(FIRST_MUSIC.0..FIRST_MUSIC.1),
            playing: None,
        }
    }
}

#[derive(Component)]
struct Piece;

fn play_music(
    mut commands: Commands,
    time: Res<Time<Real>>,
    assets: Res<AssetServer>,
    volumes: Res<Volumes>,
    mut music: ResMut<Music>,
    mut pieces: Query<&mut AudioSink, With<Piece>>,
) {
    if let Some(piece) = music.playing {
        match pieces.get_mut(piece) {
            // the volume can change while it plays
            Ok(mut sink) if volumes.is_changed() => {
                sink.set_volume(Volume::Linear(MUSIC_VOLUME * volumes.music()));
            }
            Ok(_) => {}
            // it's finished, and despawned, or it never started: with no audio device nothing
            // does, and it waits as long again
            Err(_) if time.elapsed_secs() < music.next => {}
            Err(_) => {
                commands.entity(piece).try_despawn();
                music.playing = None;
                music.next =
                    time.elapsed_secs() + rand::rng().random_range(MUSIC_QUIET.0..MUSIC_QUIET.1);
            }
        }
        return;
    }
    if time.elapsed_secs() < music.next || volumes.music() == 0.0 {
        return;
    }
    let name = MUSIC_PIECES.choose(&mut rand::rng()).unwrap();
    let piece = commands
        .spawn((
            Piece,
            AudioPlayer::new(assets.load(format!("sounds/music/{name}.ogg"))),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(MUSIC_VOLUME * volumes.music())),
        ))
        .id();
    music.playing = Some(piece);
    // how long to wait for it before giving up on it, past its length
    music.next = time.elapsed_secs() + 120.0;
}

#[cfg(test)]
mod tests {
    use super::wind::GROUND_BREEZE;
    use super::*;

    const NOON: f32 = 0.5;
    const MIDNIGHT: f32 = 0.0;

    // a mild meadow on low ground at noon, standing still, with no sea or trees near
    fn meadow() -> Surroundings {
        Surroundings {
            height: 10.0,
            altitude: 1.6,
            speed: 0.0,
            in_cave: false,
            underwater: false,
            sea: 0.0,
            trees: 0.02,
            canopy: 0.0,
            temperature: 0.5,
            humidity: 0.5,
            time: NOON,
            rain: 0.0,
        }
    }

    // a mild wood
    fn wood() -> Surroundings {
        Surroundings {
            trees: 0.4,
            canopy: 0.9,
            ..meadow()
        }
    }

    // only the loops at all loud
    fn heard(mix: Mix) -> Vec<Loop> {
        Loop::ALL
            .into_iter()
            .filter(|&l| mix.of(l) > 0.05)
            .collect()
    }

    #[test]
    fn test_every_loop_and_the_owl_have_their_files() {
        let files = Loop::ALL
            .iter()
            .map(|l| format!("ambience/{}", l.file()))
            .chain(["owl".to_string()]);
        for file in files {
            let path = crate::settings::asset_path(&format!("assets/sounds/{file}.ogg"));
            assert!(path.exists(), "needs {}", path.display());
        }
    }

    #[test]
    fn test_under_water_or_in_a_cave_hears_nothing_else() {
        let underwater = mix(&Surroundings {
            underwater: true,
            ..wood()
        });
        assert_eq!(vec![Loop::Underwater], heard(underwater));
        assert_eq!(UNDERWATER_WIND, underwater.wind);
        let cave = mix(&Surroundings {
            in_cave: true,
            ..wood()
        });
        assert_eq!(vec![Loop::Cave], heard(cave));
        assert_eq!((0.0, 0.0), (cave.wind, cave.owls));
    }

    #[test]
    fn test_birds_sing_by_day_more_in_forests_and_crickets_and_owls_by_night() {
        let day = mix(&meadow());
        let forest = mix(&wood());
        let night = mix(&Surroundings {
            time: MIDNIGHT + 3.0,
            ..meadow()
        });
        let forest_night = mix(&Surroundings {
            time: MIDNIGHT,
            ..wood()
        });
        assert!(day.of(Loop::Birds) > 0.2 && day.of(Loop::Crickets) == 0.0);
        assert!(forest.of(Loop::Birds) > day.of(Loop::Birds) + 0.3);
        assert!(night.of(Loop::Crickets) > 0.5 && night.of(Loop::Birds) == 0.0);
        assert_eq!(vec![Loop::Birds], heard(day));
        assert_eq!(0.0, day.owls.max(forest.owls).max(night.owls));
        assert!(forest_night.owls > 0.5);
    }

    #[test]
    fn test_leaves_rustle_only_under_the_trees_and_near_the_ground() {
        assert!(mix(&wood()).of(Loop::Leaves) > 0.8);
        assert_eq!(0.0, mix(&meadow()).of(Loop::Leaves));
        let above = mix(&Surroundings {
            altitude: 60.0,
            ..wood()
        });
        assert_eq!(0.0, above.of(Loop::Leaves));
    }

    #[test]
    fn test_deserts_have_their_air_and_no_birds_and_glaciers_a_cold_wind() {
        let desert = mix(&Surroundings {
            temperature: 0.95,
            humidity: 0.05,
            ..meadow()
        });
        let glacier = mix(&Surroundings {
            temperature: 0.05,
            sea: 0.5,
            ..meadow()
        });
        assert_eq!(vec![Loop::Desert], heard(desert));
        // only the breeze on the ground, and a little for the eyes being off it
        assert!(
            (desert.wind - GROUND_BREEZE).abs() < 0.05,
            "{}",
            desert.wind
        );
        // a frozen sea doesn't break on the shore
        assert_eq!(Vec::<Loop>::new(), heard(glacier));
        assert!(glacier.wind > 0.5);
    }

    #[test]
    fn test_rain_is_heard_blows_the_wind_up_and_quiets_birds_and_crickets() {
        let raining = |surroundings: Surroundings| {
            mix(&Surroundings {
                rain: 0.9,
                ..surroundings
            })
        };
        let wet = raining(wood());
        assert_eq!(0.9, wet.rain);
        assert!(wet.wind >= STORM_WIND * 0.9 && wet.wind > mix(&wood()).wind);
        assert!(wet.of(Loop::Birds) < mix(&wood()).of(Loop::Birds) * 0.2);
        let night = Surroundings {
            time: MIDNIGHT,
            ..meadow()
        };
        assert!(raining(night).of(Loop::Crickets) < mix(&night).of(Loop::Crickets) * 0.2);
        // in a cave it's not heard, but thunder is, muffled
        let cave = raining(Surroundings {
            in_cave: true,
            ..wood()
        });
        assert_eq!((0.0, MUFFLED_THUNDER), (cave.rain, cave.thunder));
        assert_eq!(1.0, wet.thunder);
    }

    #[test]
    fn test_the_wind_rises_with_height_and_speed_and_the_sea_is_heard_near_it() {
        // standing on a peak: little of the ground's life, but a strong wind
        let peak = mix(&Surroundings {
            height: 300.0,
            ..meadow()
        });
        assert!(heard(peak).is_empty() && peak.wind > 0.7);
        let flying = mix(&Surroundings {
            altitude: 200.0,
            height: 200.0,
            ..wood()
        });
        assert!(heard(flying).is_empty() && flying.wind > 0.7);
        let running = mix(&Surroundings {
            speed: 40.0,
            ..meadow()
        });
        assert!(running.wind > mix(&meadow()).wind);
        let shore = mix(&Surroundings {
            sea: 0.5,
            ..meadow()
        });
        assert!(shore.of(Loop::Sea) > 0.3);
        let cliff_top = mix(&Surroundings {
            sea: 0.5,
            height: 80.0,
            ..meadow()
        });
        assert_eq!(0.0, cliff_top.of(Loop::Sea));
    }
}
