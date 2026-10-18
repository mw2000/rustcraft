//! The player's own settings (M-U1): how the game looks, sounds, plays and reads, saved in a file of
//! theirs under the data directory, over the defaults in `assets/settings.toml`, and applied as
//! they're changed wherever that's possible.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::text::{RemSize, DEFAULT_REM_SIZE_PX};
use bevy::window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode};
use serde::{Deserialize, Serialize};

use crate::client::sound::volume::{Volumes, OLD_VOLUMES_FILE};
use crate::clouds::CloudTile;
use crate::controls::Bindings;
use crate::menu::GameState;
use crate::player::name::PlayerName;
use crate::save::write_atomically;
use crate::settings::{RendererSettings, Settings};
use crate::sky::Sun;

/// The file under the data directory the player's settings are kept in.
pub const PREFERENCES_FILE: &str = "settings.toml";
// How often changes are written, at most, so dragging a slider doesn't write every frame.
const SAVE_INTERVAL: Duration = Duration::from_secs(1);
// How much bigger the larger text is.
const LARGER_TEXT: f32 = 1.25;

/// The player's settings, as the Settings screen changes them.
#[derive(Resource, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Preferences {
    /// The name to play under, or empty for the computer's user's (`PlayerName::own`).
    pub name: String,
    pub video: Video,
    pub controls: ControlSettings,
    pub audio: Volumes,
    pub accessibility: Accessibility,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Video {
    /// How far terrain is drawn, and at full detail, in chunks, as `RendererSettings` has them.
    pub render_distance: u32,
    pub full_detail_distance: u32,
    /// The camera's vertical field of view, in degrees.
    pub field_of_view: f32,
    pub fullscreen: bool,
    pub vsync: bool,
    /// The most frames drawn a second, or 0 for as many as can be.
    pub frame_cap: u32,
    /// How big the menus and the HUD are drawn.
    pub ui_scale: f32,
    pub shadows: bool,
    pub clouds: bool,
    /// Butterflies, fireflies and birds about the world, see `client::critters`.
    pub ambient_life: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ControlSettings {
    /// How far the mouse turns the view, 1 as the game first had it.
    pub sensitivity: f32,
    /// Moving the mouse up looks down.
    pub invert_y: bool,
    pub bindings: Bindings,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct Accessibility {
    /// The view tilts for a moment as the player's hurt.
    pub hurt_tilt: bool,
    pub larger_text: bool,
    pub crosshair: Crosshair,
}

/// What marks the middle of the screen, where blocks are reached for.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Crosshair {
    #[default]
    Cross,
    Dot,
    None,
}

impl Crosshair {
    pub const ALL: [Crosshair; 3] = [Crosshair::Cross, Crosshair::Dot, Crosshair::None];

    pub fn label(self) -> &'static str {
        match self {
            Crosshair::Cross => "Cross",
            Crosshair::Dot => "Dot",
            Crosshair::None => "None",
        }
    }
}

/// A set of video settings that trade how far and how much is drawn against the frame rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Low,
    Medium,
    High,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::Low, Preset::Medium, Preset::High];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Low => "Low",
            Preset::Medium => "Medium",
            Preset::High => "High",
        }
    }

    /// `video` with this preset's distances, shadows and clouds.
    pub fn apply(self, video: Video) -> Video {
        let (render_distance, full_detail_distance, shadows, clouds) = match self {
            Preset::Low => (256, 8, false, false),
            Preset::Medium => (512, 12, true, true),
            Preset::High => (1024, 16, true, true),
        };
        Video {
            render_distance,
            full_detail_distance,
            shadows,
            clouds,
            ..video
        }
    }

    /// The preset `video` is set as, if it's one of them.
    pub fn of(video: Video) -> Option<Preset> {
        Self::ALL
            .into_iter()
            .find(|preset| preset.apply(video) == video)
    }
}

impl Preferences {
    /// What a player has before they change anything, drawing as far as `renderer` does.
    pub fn defaults(renderer: &RendererSettings) -> Self {
        Self {
            name: String::new(),
            video: Video {
                render_distance: renderer.render_distance,
                full_detail_distance: renderer.full_detail_distance,
                field_of_view: 45.0,
                fullscreen: false,
                vsync: false,
                frame_cap: 0,
                ui_scale: 1.0,
                shadows: true,
                clouds: true,
                ambient_life: true,
            },
            controls: ControlSettings {
                sensitivity: 1.0,
                invert_y: false,
                bindings: Bindings::default(),
            },
            audio: Volumes::default(),
            accessibility: Accessibility {
                hurt_tilt: true,
                larger_text: false,
                crosshair: Crosshair::default(),
            },
        }
    }

    /// The settings saved in `path` over `defaults`: what the file leaves out, as it will once
    /// there are settings it didn't know of, and what it has that can't be read, keep their
    /// defaults, so an old or damaged file loses nothing else.
    pub fn read(path: &Path, defaults: Self) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return defaults;
        };
        let saved: toml::Table = match toml::from_str(&text) {
            Ok(saved) => saved,
            Err(error) => {
                warn!("couldn't read the settings in {}: {error}", path.display());
                return defaults;
            }
        };
        let mut table = toml::Table::try_from(&defaults).expect("settings always encode");
        merge(&mut table, saved);
        table.try_into().unwrap_or_else(|error| {
            warn!("couldn't read the settings in {}: {error}", path.display());
            defaults
        })
    }

    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string(self).expect("settings always encode");
        write_atomically(path, text.as_bytes())
    }

    /// The name to play under.
    pub fn player_name(&self) -> PlayerName {
        if self.name.trim().is_empty() {
            PlayerName::own()
        } else {
            PlayerName::new(&self.name)
        }
    }

    /// `renderer` as these settings have it drawn.
    pub fn renderer(&self, renderer: RendererSettings) -> RendererSettings {
        RendererSettings {
            render_distance: self.video.render_distance,
            full_detail_distance: self.video.full_detail_distance,
            ..renderer
        }
    }
}

impl Default for Preferences {
    fn default() -> Self {
        Self::defaults(&RendererSettings::default())
    }
}

// Puts what `over` has into `table`, a table's keys one by one, and only where both are the same
// type, so a value that can't be read keeps the default.
fn merge(table: &mut toml::Table, over: toml::Table) {
    for (key, value) in over {
        match (table.get_mut(&key), value) {
            (Some(toml::Value::Table(inner)), toml::Value::Table(over)) => merge(inner, over),
            (Some(old), value) if old.same_type(&value) => *old = value,
            // what this game doesn't know of is kept, for a newer one to read again
            (None, value) => {
                table.insert(key, value);
            }
            _ => {}
        }
    }
}

/// Where the player's settings are saved.
#[derive(Resource, Clone)]
pub struct PreferencesFile(pub PathBuf);

/// The player's settings: read as the game starts, saved as they change, and applied to the
/// window, the camera, the light, the clouds, the UI and the key bindings as they do. The
/// renderer's distances are applied once the Settings screen is closed, as changing them draws
/// the terrain again. Needs the `Settings` read from `assets/settings.toml` and a
/// `PreferencesFile`.
pub struct PreferencesPlugin;

impl Plugin for PreferencesPlugin {
    fn build(&self, app: &mut App) {
        let settings = *app.world().resource::<Settings>();
        let file = app.world().resource::<PreferencesFile>().0.clone();
        let mut defaults = Preferences::defaults(&settings.renderer);
        // the volumes as they were before they were kept here, if they've not been since
        defaults.audio = Volumes::read(&file.with_file_name(OLD_VOLUMES_FILE));
        let mut preferences = Preferences::read(&file, defaults);
        preferences.audio = preferences.audio.clamped();
        app.insert_resource(Settings {
            renderer: preferences.renderer(settings.renderer),
            ..settings
        })
        .insert_resource(preferences.controls.bindings.clone())
        .insert_resource(preferences.audio)
        .insert_resource(preferences)
        .add_systems(
            Update,
            (
                (
                    follow_bindings,
                    follow_volumes,
                    apply_video,
                    save_preferences,
                )
                    .run_if(resource_changed::<Preferences>),
                cap_frames,
            ),
        );
    }
}

fn follow_bindings(preferences: Res<Preferences>, mut bindings: ResMut<Bindings>) {
    bindings.set_if_neq(preferences.controls.bindings.clone());
}

fn follow_volumes(preferences: Res<Preferences>, mut volumes: ResMut<Volumes>) {
    volumes.set_if_neq(preferences.audio);
}

/// Applies the video settings and the larger text, all but the renderer's distances, which
/// `redraw_for_settings` does.
#[allow(clippy::too_many_arguments)]
fn apply_video(
    preferences: Res<Preferences>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut cameras: Query<&mut Projection, With<Camera3d>>,
    mut suns: Query<&mut DirectionalLight, With<Sun>>,
    mut clouds: Query<&mut Visibility, With<CloudTile>>,
    mut ui_scale: ResMut<UiScale>,
    mut rem: ResMut<RemSize>,
) {
    let video = preferences.video;
    for mut window in &mut windows {
        let mode = if video.fullscreen {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        } else {
            WindowMode::Windowed
        };
        let present_mode = if video.vsync {
            PresentMode::AutoVsync
        } else {
            PresentMode::AutoNoVsync
        };
        if window.mode != mode {
            window.mode = mode;
        }
        if window.present_mode != present_mode {
            window.present_mode = present_mode;
        }
    }
    for mut projection in &mut cameras {
        if let Projection::Perspective(perspective) = &mut *projection {
            perspective.fov = video.field_of_view.to_radians();
        }
    }
    for mut sun in &mut suns {
        sun.shadow_maps_enabled = video.shadows;
    }
    for mut visibility in &mut clouds {
        *visibility = if video.clouds {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if ui_scale.0 != video.ui_scale {
        ui_scale.0 = video.ui_scale;
    }
    let text = if preferences.accessibility.larger_text {
        LARGER_TEXT
    } else {
        1.0
    };
    rem.set_if_neq(RemSize(DEFAULT_REM_SIZE_PX * text));
}

/// Draws the terrain again at the distances the settings have now, if they've changed, as the
/// Settings screen closes. On the title screen, whose scenery is drawn nearer, they're kept for
/// the next world.
pub fn redraw_for_settings(
    preferences: Res<Preferences>,
    mut settings: ResMut<Settings>,
    state: Res<State<GameState>>,
    mut commands: Commands,
) {
    let renderer = preferences.renderer(settings.renderer);
    let distances = |r: RendererSettings| (r.render_distance, r.full_detail_distance);
    if distances(renderer) == distances(settings.renderer) {
        return;
    }
    settings.renderer = renderer;
    if !matches!(state.get(), GameState::Title | GameState::Loading) {
        commands.run_system_cached_with(crate::chunks::redraw_terrain, renderer);
    }
}

// Writes the settings once they've stopped changing for a moment, so a slider dragged is
// written once.
fn save_preferences(
    preferences: Res<Preferences>,
    file: Res<PreferencesFile>,
    mut last: Local<Option<Instant>>,
) {
    if last.is_some_and(|last| last.elapsed() < SAVE_INTERVAL) {
        return;
    }
    *last = Some(Instant::now());
    if let Err(error) = preferences.write(&file.0) {
        error!(
            "couldn't save the settings in {}: {error}",
            file.0.display()
        );
    }
}

// Holds each frame back until the frame cap's time is up, if there is one.
fn cap_frames(preferences: Res<Preferences>, mut last: Local<Option<Instant>>) {
    let cap = preferences.video.frame_cap;
    if let Some(last) = *last {
        if cap > 0 {
            let frame = Duration::from_secs_f64(1.0 / f64::from(cap));
            if let Some(left) = frame.checked_sub(last.elapsed()) {
                std::thread::sleep(left);
            }
        }
    }
    *last = Some(Instant::now());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::{Action, Binding};

    fn temp_file(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rustcraft-prefs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join(PREFERENCES_FILE)
    }

    #[test]
    fn test_settings_round_trip_and_start_from_the_games_defaults() {
        let file = temp_file("round-trip");
        let renderer = RendererSettings {
            render_distance: 1024,
            full_detail_distance: 16,
            ..default()
        };
        let defaults = Preferences::defaults(&renderer);
        assert_eq!(defaults, Preferences::read(&file, defaults.clone()));

        let mut changed = defaults.clone();
        changed.name = "Ada".into();
        changed.video.field_of_view = 70.0;
        changed.video.render_distance = 256;
        changed.controls.invert_y = true;
        changed.controls.sensitivity = 1.5;
        changed
            .controls
            .bindings
            .set(Action::Jump, Binding::Key(KeyCode::KeyJ));
        changed.accessibility.crosshair = Crosshair::Dot;
        changed.write(&file).unwrap();
        assert_eq!(changed, Preferences::read(&file, defaults.clone()));
        assert_eq!(256, changed.renderer(renderer).render_distance);
        std::fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn test_a_file_missing_settings_or_with_unreadable_ones_keeps_the_defaults_for_them() {
        let file = temp_file("partial");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let text = "name = \"Ada\"\nfuture = 3\n[video]\nfield_of_view = \"wide\"\nvsync = true";
        std::fs::write(&file, text).unwrap();
        let defaults = Preferences::default();
        let read = Preferences::read(&file, defaults.clone());
        assert_eq!("Ada", read.name);
        assert!(read.video.vsync);
        assert_eq!(defaults.video.field_of_view, read.video.field_of_view);
        assert_eq!(defaults.controls, read.controls);
        std::fs::remove_dir_all(file.parent().unwrap()).unwrap();
    }

    #[test]
    fn test_volumes_kept_before_settings_were_are_taken_until_saved_here() {
        let file = temp_file("old-volumes");
        let dir = file.parent().unwrap();
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(OLD_VOLUMES_FILE), "music = 0.25").unwrap();
        let read = |app: &App| *app.world().resource::<Volumes>();
        let app = |file: &Path| {
            let mut app = App::new();
            app.insert_resource(Settings::default())
                .insert_resource(PreferencesFile(file.to_owned()))
                .add_plugins(PreferencesPlugin);
            app
        };
        assert_eq!(0.25, read(&app(&file)).music);
        let mut saved = Preferences::default();
        saved.audio.music = 0.5;
        saved.write(&file).unwrap();
        assert_eq!(0.5, read(&app(&file)).music);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_presets_set_the_distances_shadows_and_clouds_and_are_recognised() {
        let video = Preferences::default().video;
        let low = Preset::Low.apply(video);
        assert_eq!(Some(Preset::Low), Preset::of(low));
        assert!(!low.shadows && !low.clouds);
        assert_eq!(
            None,
            Preset::of(Video {
                render_distance: 300,
                ..low
            })
        );
    }

    #[test]
    fn test_settings_are_applied_live() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<UiScale>()
            .init_resource::<RemSize>()
            .insert_resource(Settings::default())
            .insert_resource(PreferencesFile(temp_file("live")))
            .add_plugins(PreferencesPlugin);
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let camera = app
            .world_mut()
            .spawn((Camera3d::default(), Projection::default()))
            .id();
        let sun = app
            .world_mut()
            .spawn((DirectionalLight::default(), Sun))
            .id();
        app.update();

        let mut preferences = app.world_mut().resource_mut::<Preferences>();
        preferences.video.fullscreen = true;
        preferences.video.field_of_view = 90.0;
        preferences.video.ui_scale = 1.5;
        preferences.video.shadows = false;
        preferences.accessibility.larger_text = true;
        preferences
            .controls
            .bindings
            .set(Action::Map, Binding::Key(KeyCode::KeyN));
        app.update();

        let world = app.world();
        let mode = world.get::<Window>(window).unwrap().mode;
        assert!(matches!(mode, WindowMode::BorderlessFullscreen(_)));
        let Projection::Perspective(perspective) = world.get::<Projection>(camera).unwrap() else {
            panic!("the camera's perspective");
        };
        assert!((perspective.fov - 90f32.to_radians()).abs() < 1e-6);
        assert_eq!(1.5, world.resource::<UiScale>().0);
        assert!(world.resource::<RemSize>().0 > DEFAULT_REM_SIZE_PX);
        assert!(
            !world
                .get::<DirectionalLight>(sun)
                .unwrap()
                .shadow_maps_enabled
        );
        let bindings = world.resource::<Bindings>();
        assert_eq!(Binding::Key(KeyCode::KeyN), bindings.get(Action::Map));
        std::fs::remove_dir_all(temp_file("live").parent().unwrap()).ok();
    }
}
