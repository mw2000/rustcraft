//! How loud the game is: a master volume and one each for effects, ambience and music, set in
//! the pause menu and kept for this player in `volume.toml` in the game's data directory.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::tasks::IoTaskPool;
use bevy::ui_widgets::{Activate, Button};
use serde::{Deserialize, Serialize};

use crate::menu::{font, label, spawn_menu, Menu, BUTTON_BORDER_COLOR, BUTTON_COLOR, TEXT_SHADOW};

// Each press of a volume's − or + moves it this far, of 1.
const STEP: f32 = 0.1;

/// The volumes, each from 0 (silent) to 1.
#[derive(Resource, Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(default)]
pub struct Volumes {
    pub master: f32,
    pub effects: f32,
    pub ambience: f32,
    pub music: f32,
}

impl Default for Volumes {
    fn default() -> Self {
        Self {
            master: 1.0,
            effects: 1.0,
            ambience: 1.0,
            music: 0.7,
        }
    }
}

impl Volumes {
    /// How loud effects play, with the master volume.
    pub fn effects(&self) -> f32 {
        self.master * self.effects
    }

    pub fn ambience(&self) -> f32 {
        self.master * self.ambience
    }

    pub fn music(&self) -> f32 {
        self.master * self.music
    }

    /// The volumes kept, or the defaults if there are none or they can't be read.
    pub fn load() -> Self {
        file().map_or_else(Self::default, |file| Self::read(&file))
    }

    fn read(file: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(file) else {
            return Self::default();
        };
        toml::from_str::<Self>(&text)
            .unwrap_or_else(|error| {
                warn!("couldn't read the volumes in {}: {error}", file.display());
                Self::default()
            })
            .clamped()
    }

    fn write(self, file: &Path) -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string(&self).expect("volumes always make TOML");
        std::fs::write(file, text)
    }

    fn clamped(self) -> Self {
        let keep = |volume: f32| {
            if volume.is_finite() {
                volume.clamp(0.0, 1.0)
            } else {
                1.0
            }
        };
        Self {
            master: keep(self.master),
            effects: keep(self.effects),
            ambience: keep(self.ambience),
            music: keep(self.music),
        }
    }
}

// Beside the saved worlds.
fn file() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("rustcraft").join("volume.toml"))
}

/// The four volumes' rows in the pause menu, over Quit Game.
pub(super) struct VolumeMenuPlugin;

impl Plugin for VolumeMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_rows.after(spawn_menu))
            .add_systems(
                Update,
                (show_volumes, save).run_if(resource_changed::<Volumes>),
            );
    }
}

/// Which of the volumes a row sets.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum Which {
    Master,
    Effects,
    Ambience,
    Music,
}

impl Which {
    fn name(self) -> &'static str {
        match self {
            Self::Master => "Volume",
            Self::Effects => "Effects",
            Self::Ambience => "Ambience",
            Self::Music => "Music",
        }
    }

    fn of(self, volumes: &Volumes) -> f32 {
        match self {
            Self::Master => volumes.master,
            Self::Effects => volumes.effects,
            Self::Ambience => volumes.ambience,
            Self::Music => volumes.music,
        }
    }

    // Moves the volume by `by`, staying within 0 to 1.
    fn nudge(self, volumes: &mut Volumes, by: f32) {
        let volume = match self {
            Self::Master => &mut volumes.master,
            Self::Effects => &mut volumes.effects,
            Self::Ambience => &mut volumes.ambience,
            Self::Music => &mut volumes.music,
        };
        // rounded to the step, so ten presses down is silent
        *volume = ((*volume + by) / STEP).round().clamp(0.0, 1.0 / STEP) * STEP;
    }

    // What its row's label says.
    fn caption(self, volumes: &Volumes) -> String {
        format!("{}: {:.0}%", self.name(), self.of(volumes) * 100.0)
    }
}

fn spawn_rows(
    mut commands: Commands,
    menu: Query<(Entity, &Children), With<Menu>>,
    volumes: Res<Volumes>,
) {
    let Ok((menu, children)) = menu.single() else {
        return;
    };
    let rows: Vec<_> = [Which::Master, Which::Effects, Which::Ambience, Which::Music]
        .into_iter()
        .map(|which| {
            let caption = which.caption(&volumes);
            commands
                .spawn(Node {
                    width: Val::Px(400.0),
                    column_gap: Val::Px(10.0),
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(small_button()).with_child(label("-")).observe(
                        move |_: On<Activate>, mut volumes: ResMut<Volumes>| {
                            which.nudge(&mut volumes, -STEP);
                        },
                    );
                    row.spawn((
                        which,
                        Text::new(caption),
                        font(18.0),
                        TEXT_SHADOW,
                        TextLayout::justify(Justify::Center),
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                    ));
                    row.spawn(small_button()).with_child(label("+")).observe(
                        move |_: On<Activate>, mut volumes: ResMut<Volumes>| {
                            which.nudge(&mut volumes, STEP);
                        },
                    );
                })
                .id()
        })
        .collect();
    // over the last button, Quit Game
    let quit = children.len().saturating_sub(1);
    commands.entity(menu).insert_children(quit, &rows);
}

// A small square button, for − and +.
fn small_button() -> impl Bundle {
    (
        Button,
        bevy::picking::hover::Hovered::default(),
        Node {
            width: Val::Px(40.0),
            height: Val::Px(40.0),
            border: UiRect::all(Val::Px(2.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(BUTTON_COLOR),
        BorderColor::all(BUTTON_BORDER_COLOR),
    )
}

fn show_volumes(volumes: Res<Volumes>, mut captions: Query<(&Which, &mut Text)>) {
    for (&which, mut text) in &mut captions {
        text.0 = which.caption(&volumes);
    }
}

// Off the main thread, as it's a file (INV-7), and not when they're first put in.
fn save(volumes: Res<Volumes>) {
    let (Some(file), false) = (file(), volumes.is_added()) else {
        return;
    };
    let volumes = *volumes;
    IoTaskPool::get()
        .spawn(async move {
            if let Err(error) = volumes.write(&file) {
                warn!("couldn't keep the volumes in {}: {error}", file.display());
            }
        })
        .detach();
}

#[cfg(test)]
mod tests {
    use super::{Volumes, Which, STEP};

    #[test]
    fn test_volumes_are_kept_and_read_back() {
        let dir = std::env::temp_dir().join(format!("rustcraft-volumes-{}", std::process::id()));
        let file = dir.join("volume.toml");
        let volumes = Volumes {
            master: 0.8,
            effects: 0.5,
            ambience: 0.3,
            music: 0.0,
        };
        volumes.write(&file).unwrap();
        assert_eq!(volumes, Volumes::read(&file));
        assert!((volumes.effects() - 0.4).abs() < 1e-6);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn test_pressing_down_steps_to_silence_and_no_further() {
        let mut volumes = Volumes::default();
        for _ in 0..12 {
            Which::Music.nudge(&mut volumes, -STEP);
        }
        assert_eq!(0.0, volumes.music);
        Which::Music.nudge(&mut volumes, STEP);
        assert!((volumes.music - 0.1).abs() < 1e-6);
        assert_eq!("Music: 10%", Which::Music.caption(&volumes));
    }

    #[test]
    fn test_missing_or_wild_volumes_are_made_sensible() {
        let read = |text: &str| toml::from_str::<Volumes>(text).unwrap().clamped();
        assert_eq!(Volumes::default(), read(""));
        let wild = read("master = 3.0\nmusic = -1.0");
        assert_eq!((1.0, 0.0, 1.0), (wild.master, wild.music, wild.effects));
        assert_eq!(
            Volumes::default(),
            Volumes::read(std::path::Path::new("/no/such/volume.toml"))
        );
    }
}
