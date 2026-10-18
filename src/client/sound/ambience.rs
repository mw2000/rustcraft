//! Ambience: loops of wind, the sea, birds, crickets, a cave's drips and being under water, each
//! as loud as where the player is calls for, crossfading as they move. And now and then, after a
//! long quiet, a piece of music.

use std::time::Duration;

use bevy::audio::Volume;
use bevy::prelude::*;
use rand::seq::IndexedRandom;
use rand::RngExt;

use super::volume::Volumes;
use crate::block::BlockType;
use crate::chunks::generate::WATER_LEVEL;
use crate::client::player::LocalPlayer;
use crate::clock::{day_fraction, TimeOfDay};
use crate::world::World;

// How often where the player is is looked at again, which costs some noise samples, and how
// long the loops take to fade to what it calls for.
const LOOK_INTERVAL: Duration = Duration::from_millis(500);
const FADE_SECONDS: f32 = 2.5;
// How far round the player, in blocks, the sea's listened for.
const SEA_REACH: f64 = 28.0;
// How far under the ground's surface, in blocks, the eyes must be for a cave's sound.
const CAVE_DEPTH: f32 = 3.0;
// Music: the first piece a few minutes in, then long quiets between, in seconds.
const FIRST_MUSIC: (f32, f32) = (90.0, 240.0);
const MUSIC_QUIET: (f32, f32) = (480.0, 1080.0);
const MUSIC_PIECES: [&str; 3] = ["morning", "drift", "embers"];
// Each loop's and the music's volume at its loudest, against the one-off sounds.
const LOOP_VOLUME: f32 = 0.6;
const MUSIC_VOLUME: f32 = 0.5;

pub(super) struct AmbiencePlugin;

impl Plugin for AmbiencePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Mix>()
            .insert_resource(LookAgain(Timer::new(LOOK_INTERVAL, TimerMode::Repeating)))
            .insert_resource(Music::new())
            .add_systems(Startup, start_loops)
            .add_systems(Update, (look_around, fade_loops).chain())
            .add_systems(Update, play_music);
    }
}

/// The ambience loops, each a file in `assets/sounds/ambience/`.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum Loop {
    Wind,
    HighWind,
    Sea,
    Birds,
    Crickets,
    Cave,
    Underwater,
}

impl Loop {
    const ALL: [Self; 7] = [
        Self::Wind,
        Self::HighWind,
        Self::Sea,
        Self::Birds,
        Self::Crickets,
        Self::Cave,
        Self::Underwater,
    ];

    fn file(self) -> &'static str {
        match self {
            Self::Wind => "wind",
            Self::HighWind => "wind_high",
            Self::Sea => "sea",
            Self::Birds => "birds",
            Self::Crickets => "crickets",
            Self::Cave => "cave",
            Self::Underwater => "underwater",
        }
    }
}

/// What the ambience goes by: where the player's eyes are and what's round them.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Surroundings {
    /// Blocks above the sea.
    height: f32,
    /// Deep enough under the ground to be in a cave.
    in_cave: bool,
    underwater: bool,
    /// How much of the land round about is sea, 0 to 1.
    sea: f32,
    /// How thick the trees grow here, as generation has it, 0 to 1.
    trees: f32,
    temperature: f32,
    humidity: f32,
    /// The time of day, in days.
    time: f32,
}

/// How loud each loop should be, 0 to 1, in the order of `Loop::ALL`.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
struct Mix([f32; 7]);

impl Mix {
    fn of(self, which: Loop) -> f32 {
        self.0[which as usize]
    }
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// What's heard where `s` says: under water only the water; in a cave its drips and hum; and
/// in the open wind rising with height, the sea near it, birds by day where it's mild, more
/// among trees, and crickets by night. Biomes come in by their climate and trees, so a new one
/// sounds right without being named here: a desert's too hot and dry for birds, a glacier too
/// cold, and its wind is the high, whistling one.
fn mix(s: &Surroundings) -> Mix {
    let mut mix = Mix::default();
    let mut set = |which: Loop, volume: f32| mix.0[which as usize] = volume.clamp(0.0, 1.0);
    if s.underwater {
        set(Loop::Underwater, 1.0);
        return mix;
    }
    if s.in_cave {
        set(Loop::Cave, 1.0);
        return mix;
    }
    let day = smoothstep(0.21, 0.29, s.time.rem_euclid(1.0))
        * (1.0 - smoothstep(0.71, 0.79, s.time.rem_euclid(1.0)));
    let cold = 1.0 - smoothstep(0.1, 0.25, s.temperature);
    let mild = smoothstep(0.15, 0.3, s.temperature)
        * (1.0 - smoothstep(0.75, 0.9, s.temperature))
        * smoothstep(0.15, 0.3, s.humidity);
    // life thins out up the mountains
    let lowland = 1.0 - smoothstep(60.0, 160.0, s.height);
    let wooded = smoothstep(0.02, 0.2, s.trees);
    set(Loop::Wind, 0.3 + 0.5 * smoothstep(0.0, 120.0, s.height));
    set(
        Loop::HighWind,
        smoothstep(90.0, 250.0, s.height).max(cold * 0.6),
    );
    set(Loop::Sea, s.sea * (1.0 - smoothstep(15.0, 60.0, s.height)));
    set(Loop::Birds, day * mild * lowland * (0.35 + 0.65 * wooded));
    set(Loop::Crickets, (1.0 - day) * mild * lowland * 0.8);
    mix
}

impl Surroundings {
    /// What's round eyes at `eyes` in `world` at `time`.
    fn at(world: &World, eyes: Vec3, time: f64) -> Self {
        let noise = world.region_store.noise();
        let (x, z) = (eyes.x as f64, eyes.z as f64);
        let here = noise.sample(x, z);
        let climate = noise.climate(x, z);
        let block = eyes.floor().as_i64vec3();
        let sea = (0..8)
            .filter(|i| {
                let angle = *i as f64 * std::f64::consts::TAU / 8.0;
                let sample = noise.sample(x + SEA_REACH * angle.cos(), z + SEA_REACH * angle.sin());
                sample.height < WATER_LEVEL as f64
            })
            .count() as f32
            / 8.0;
        Self {
            height: eyes.y - WATER_LEVEL as f32,
            // the ground as generated, which edits don't change: under a roof, or an overhang,
            // isn't a cave
            in_cave: world.is_under_ground(block) && eyes.y < here.height as f32 - CAVE_DEPTH,
            underwater: world.get_block_at(block) == BlockType::Water,
            sea,
            trees: here.trees as f32,
            temperature: climate.temperature,
            humidity: climate.humidity,
            time: day_fraction(time),
        }
    }
}

#[derive(Resource)]
struct LookAgain(Timer);

fn start_loops(mut commands: Commands, assets: Res<AssetServer>) {
    for which in Loop::ALL {
        commands.spawn((
            which,
            AudioPlayer::new(assets.load(format!("sounds/ambience/{}.ogg", which.file()))),
            PlaybackSettings::LOOP.with_volume(Volume::SILENT).paused(),
        ));
    }
}

fn look_around(
    time: Res<Time<Real>>,
    mut again: ResMut<LookAgain>,
    world: Option<Res<World>>,
    clock: Res<TimeOfDay>,
    eyes: Query<&GlobalTransform, (With<Camera3d>, With<ChildOf>)>,
    player: Query<(), With<LocalPlayer>>,
    mut wanted: ResMut<Mix>,
) {
    if !again.0.tick(time.delta()).just_finished() {
        return;
    }
    // nothing's heard until there's a world to hear and a player to hear it
    let (Some(world), Ok(eyes), Ok(())) = (world, eyes.single(), player.single()) else {
        *wanted = Mix::default();
        return;
    };
    *wanted = mix(&Surroundings::at(&world, eyes.translation(), clock.0));
}

// Eases each loop towards what's wanted, pausing those faded out, so they cost nothing.
fn fade_loops(
    time: Res<Time<Real>>,
    wanted: Res<Mix>,
    volumes: Res<Volumes>,
    mut loops: Query<(&Loop, &mut AudioSink)>,
    mut playing: Local<Mix>,
) {
    let step = (time.delta_secs() / FADE_SECONDS).min(1.0);
    for (&which, mut sink) in &mut loops {
        let now = &mut playing.0[which as usize];
        let target = wanted.of(which);
        *now += (target - *now).clamp(-step, step);
        if *now <= 0.0 && target <= 0.0 {
            if !sink.is_paused() {
                sink.pause();
            }
            continue;
        }
        sink.set_volume(Volume::Linear(*now * LOOP_VOLUME * volumes.ambience()));
        if sink.is_paused() {
            sink.play();
        }
    }
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
    use super::*;

    const NOON: f32 = 0.5;
    const MIDNIGHT: f32 = 0.0;

    // a mild meadow on low ground at noon, with no sea near
    fn meadow() -> Surroundings {
        Surroundings {
            height: 10.0,
            in_cave: false,
            underwater: false,
            sea: 0.0,
            trees: 0.02,
            temperature: 0.5,
            humidity: 0.5,
            time: NOON,
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
    fn test_under_water_or_in_a_cave_hears_nothing_else() {
        let underwater = Surroundings {
            underwater: true,
            ..meadow()
        };
        assert_eq!(vec![Loop::Underwater], heard(mix(&underwater)));
        let cave = Surroundings {
            in_cave: true,
            ..meadow()
        };
        assert_eq!(vec![Loop::Cave], heard(mix(&cave)));
    }

    #[test]
    fn test_birds_sing_by_day_more_in_forests_and_crickets_by_night() {
        let day = mix(&meadow());
        let forest = mix(&Surroundings {
            trees: 0.4,
            ..meadow()
        });
        let night = mix(&Surroundings {
            time: MIDNIGHT + 3.0,
            ..meadow()
        });
        assert!(day.of(Loop::Birds) > 0.2 && day.of(Loop::Crickets) == 0.0);
        assert!(forest.of(Loop::Birds) > day.of(Loop::Birds) + 0.3);
        assert!(night.of(Loop::Crickets) > 0.5 && night.of(Loop::Birds) == 0.0);
        assert_eq!(vec![Loop::Wind, Loop::Birds], heard(day));
    }

    #[test]
    fn test_deserts_and_glaciers_have_no_birds_and_glaciers_a_cold_wind() {
        let desert = mix(&Surroundings {
            temperature: 0.95,
            humidity: 0.05,
            ..meadow()
        });
        let glacier = mix(&Surroundings {
            temperature: 0.05,
            ..meadow()
        });
        assert_eq!(vec![Loop::Wind], heard(desert));
        assert_eq!(vec![Loop::Wind, Loop::HighWind], heard(glacier));
    }

    #[test]
    fn test_the_wind_rises_with_height_and_the_sea_is_heard_near_it() {
        let peak = mix(&Surroundings {
            height: 300.0,
            trees: 0.4,
            ..meadow()
        });
        assert_eq!(vec![Loop::Wind, Loop::HighWind], heard(peak));
        assert!(peak.of(Loop::Wind) > mix(&meadow()).of(Loop::Wind));
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
