//! Sound (§5.10): footsteps by surface, digging and building, the player's own noises, other
//! players heard where they are, ambience by where the player is and the time of day, and now
//! and then a little music. Every sound is made by the sounds skill's `synth.py` (INV-8). Only
//! the client has any of it: the dedicated server builds without Bevy's audio.

mod ambience;
mod creatures;
mod events;
mod footsteps;
mod surface;
pub mod volume;

use bevy::audio::{PlaybackMode, SpatialListener, SpatialScale, Volume};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use rand::seq::IndexedRandom;
use rand::RngExt;

pub use surface::Surface;
use volume::Volumes;

use crate::creatures::CreatureKind;

/// How many takes there are of each sound, played at random so repeats don't grate. Keep in
/// step with `VARIATIONS` in `synth.py`.
const VARIATIONS: usize = 4;
// The most one-off sounds playing at once: more are dropped, which in a crowd of footsteps
// nobody hears go.
const MAX_SOUNDS: usize = 24;
// How far off, in blocks, a sound in the world can be heard: beyond it it isn't played at all.
const HEARING: f32 = 32.0;
// Bevy's spatial audio falls off as the square of the distance, from full volume within one
// unit. Scaled so that's within 6 blocks: a block broken at arm's length is heard whole, one
// 12 blocks off at a quarter and one at the edge of hearing barely.
const SPATIAL_SCALE: f32 = 1.0 / 6.0;
// The ears' distance apart, in blocks, which pans what's heard to the side it's on.
const EAR_GAP: f32 = 0.3;
// Long enough for any one-off sound to have finished, after which it's cleared away whether or
// not it played: with no audio device nothing ever does.
const ONE_SHOT_LIFETIME: f32 = 4.0;

/// Plays the game's sounds. Needs Bevy's `AudioPlugin`, which `DefaultPlugins` has with the
/// `client` feature.
pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PlaySound>()
            .init_resource::<Volumes>()
            .add_systems(Startup, load_sounds)
            .add_systems(
                Update,
                (
                    listen_from_the_camera,
                    (play_sounds, clear_finished).chain(),
                ),
            )
            .add_plugins((
                creatures::CreatureSoundsPlugin,
                footsteps::FootstepsPlugin,
                events::SoundEventsPlugin,
                ambience::AmbiencePlugin,
            ));
    }
}

/// A sound to play, `at` a place in the world to be heard from there, or with none as the
/// player's own, the same in both ears.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct PlaySound {
    pub kind: SoundKind,
    pub at: Option<Vec3>,
}

/// The sounds there are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoundKind {
    /// A footstep on the surface.
    Step(Surface),
    /// Landing on the surface from a jump or a fall.
    Land(Surface),
    /// A blow at a block being broken.
    Hit(Surface),
    Break(Surface),
    Place(Surface),
    /// A stroke swimming.
    Swim,
    /// Falling into water.
    Splash,
    Pickup,
    /// A bite of food.
    Eat,
    Hurt,
    Death,
    ChestOpen,
    ChestClose,
    /// A button pressed.
    Click,
    /// A creature calling, see `CreatureSound`.
    Call(CreatureKind),
    /// A creature hurt.
    Yelp(CreatureKind),
}

/// A creature of `kind` calling `at` where it is, or crying out as it's hurt: what the code
/// drawing creatures sends, and anything else that wants one heard (M3.2's new creatures).
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct CreatureSound {
    pub kind: CreatureKind,
    pub hurt: bool,
    pub at: Vec3,
}

/// What a kind of sound plays: the files' name under `assets/sounds/`, how loud it's played
/// (the files are levelled alike by `synth.py`, so this is how it sits among the others), and
/// how far its pitch wanders take to take.
struct Sound {
    file: String,
    takes: usize,
    volume: f32,
    pitch: (f32, f32),
}

impl SoundKind {
    fn sound(self) -> Sound {
        let one = |file: &str, volume, pitch| Sound {
            file: file.into(),
            takes: 1,
            volume,
            pitch,
        };
        let takes = |file: String, volume, pitch| Sound {
            file,
            takes: VARIATIONS,
            volume,
            pitch,
        };
        match self {
            Self::Step(s) => takes(format!("step/{}", s.name()), 0.45, (0.9, 1.1)),
            // a step put down harder, and lower
            Self::Land(s) => takes(format!("step/{}", s.name()), 0.8, (0.75, 0.85)),
            // as Minecraft does, a quiet, low step
            Self::Hit(s) => takes(format!("step/{}", s.name()), 0.3, (0.65, 0.75)),
            Self::Break(s) => takes(format!("break/{}", s.name()), 0.8, (0.85, 1.0)),
            // a block set down is a lighter, higher break
            Self::Place(s) => takes(format!("break/{}", s.name()), 0.6, (1.1, 1.25)),
            Self::Swim => takes("swim".into(), 0.5, (0.9, 1.1)),
            Self::Splash => takes("splash".into(), 0.7, (0.9, 1.1)),
            Self::Pickup => takes("pickup".into(), 0.5, (0.8, 1.3)),
            Self::Eat => takes("eat".into(), 0.6, (0.85, 1.15)),
            Self::Hurt => takes("hurt".into(), 0.8, (0.9, 1.1)),
            Self::Death => one("death", 0.9, (0.95, 1.0)),
            Self::ChestOpen => one("chest_open", 0.7, (0.9, 1.1)),
            Self::ChestClose => one("chest_close", 0.7, (0.9, 1.1)),
            Self::Click => one("click", 0.5, (0.95, 1.05)),
            Self::Call(CreatureKind::Sheep) => takes("bleat".into(), 0.6, (0.85, 1.15)),
            // higher and sharper
            Self::Yelp(CreatureKind::Sheep) => takes("bleat".into(), 0.8, (1.25, 1.45)),
        }
    }

    // Every kind there is, to load the files of.
    fn all() -> impl Iterator<Item = Self> {
        let surfaces = Surface::ALL
            .into_iter()
            .flat_map(|s| [Self::Step(s), Self::Break(s)]);
        let rest = [
            Self::Swim,
            Self::Splash,
            Self::Pickup,
            Self::Eat,
            Self::Hurt,
            Self::Death,
            Self::ChestOpen,
            Self::ChestClose,
            Self::Click,
        ];
        let creatures = CreatureKind::ALL.iter().map(|&kind| Self::Call(kind));
        surfaces.chain(rest).chain(creatures)
    }
}

/// The loaded sounds, by file name, each with its takes. Bevy loads and decodes them off the
/// main thread (INV-7).
#[derive(Resource, Default)]
struct Sounds(HashMap<String, Vec<Handle<AudioSource>>>);

fn load_sounds(mut commands: Commands, assets: Res<AssetServer>) {
    let mut sounds = Sounds::default();
    for kind in SoundKind::all() {
        let Sound { file, takes, .. } = kind.sound();
        let handles = if takes == 1 {
            vec![assets.load(format!("sounds/{file}.ogg"))]
        } else {
            (1..=takes)
                .map(|take| assets.load(format!("sounds/{file}_{take}.ogg")))
                .collect()
        };
        sounds.0.insert(file, handles);
    }
    commands.insert_resource(sounds);
}

/// A one-off sound playing, and how long ago it started, in seconds.
#[derive(Component)]
struct OneShot(f32);

// The camera's where the player hears from, once it's hung from them.
fn listen_from_the_camera(
    mut commands: Commands,
    cameras: Query<Entity, (With<Camera3d>, Added<ChildOf>)>,
) {
    for camera in &cameras {
        commands
            .entity(camera)
            .insert(SpatialListener::new(EAR_GAP));
    }
}

fn play_sounds(
    mut commands: Commands,
    mut requests: MessageReader<PlaySound>,
    sounds: Option<Res<Sounds>>,
    volumes: Res<Volumes>,
    playing: Query<(), With<OneShot>>,
    listener: Query<&GlobalTransform, With<SpatialListener>>,
) {
    let Some(sounds) = sounds else {
        requests.clear();
        return;
    };
    let ears = listener.single().ok().map(GlobalTransform::translation);
    let mut count = playing.iter().count();
    let mut rng = rand::rng();
    for request in requests.read() {
        let out_of_earshot = request
            .at
            .zip(ears)
            .is_some_and(|(at, ears)| at.distance(ears) > HEARING);
        if count >= MAX_SOUNDS || out_of_earshot {
            continue;
        }
        let sound = request.kind.sound();
        let Some(take) = sounds
            .0
            .get(&sound.file)
            .and_then(|takes| takes.choose(&mut rng))
        else {
            continue;
        };
        let settings = PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(sound.volume * volumes.effects()),
            speed: rng.random_range(sound.pitch.0..=sound.pitch.1),
            spatial: request.at.is_some(),
            spatial_scale: Some(SpatialScale::new(SPATIAL_SCALE)),
            ..default()
        };
        commands.spawn((
            OneShot(0.0),
            AudioPlayer(take.clone()),
            settings,
            Transform::from_translation(request.at.unwrap_or_default()),
        ));
        count += 1;
    }
}

fn clear_finished(
    mut commands: Commands,
    time: Res<Time<Real>>,
    mut playing: Query<(Entity, &mut OneShot)>,
) {
    for (entity, mut shot) in &mut playing {
        shot.0 += time.delta_secs();
        if shot.0 > ONE_SHOT_LIFETIME {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::audio::AudioPlugin;

    use super::*;

    #[test]
    fn test_every_sound_has_its_files() {
        for kind in SoundKind::all() {
            let Sound { file, takes, .. } = kind.sound();
            let names: Vec<_> = if takes == 1 {
                vec![format!("{file}.ogg")]
            } else {
                (1..=takes)
                    .map(|take| format!("{file}_{take}.ogg"))
                    .collect()
            };
            for name in names {
                let path = crate::settings::asset_path(&format!("assets/sounds/{name}"));
                assert!(path.exists(), "{kind:?} needs {}", path.display());
            }
        }
    }

    #[test]
    fn test_no_more_than_the_most_sounds_play_at_once_and_none_out_of_earshot() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            AudioPlugin::default(),
        ))
        .add_message::<PlaySound>()
        .insert_resource(Volumes::default())
        .add_systems(Startup, load_sounds)
        .add_systems(Update, play_sounds);
        app.world_mut()
            .spawn((SpatialListener::new(EAR_GAP), Transform::default()));
        app.update();
        let step = SoundKind::Step(Surface::Stone);
        let far = PlaySound {
            kind: step,
            at: Some(Vec3::X * (HEARING + 1.0)),
        };
        app.world_mut().write_message(far);
        for _ in 0..MAX_SOUNDS + 10 {
            app.world_mut().write_message(PlaySound {
                kind: step,
                at: Some(Vec3::X),
            });
        }
        app.update();
        let mut playing = app.world_mut().query::<(&OneShot, &Transform)>();
        let playing: Vec<_> = playing.iter(app.world()).collect();
        assert_eq!(MAX_SOUNDS, playing.len());
        assert!(playing.iter().all(|(_, at)| at.translation == Vec3::X));
    }
}
