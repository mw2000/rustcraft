//! How loud the game is: a master volume and one each for effects, ambience and music, set in
//! the Settings screen's Audio tab and kept with the player's other settings (`Preferences`).

use std::path::Path;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Where M-A3 kept the volumes, in the data directory, before they joined the player's other
/// settings, which take them from it until they've been saved there.
pub const OLD_VOLUMES_FILE: &str = "volume.toml";

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

    /// The volumes kept in `file`, or the defaults if there are none or they can't be read.
    pub fn read(file: &Path) -> Self {
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

    /// Each volume between 0 and 1, and a full one in place of one that isn't a number.
    pub fn clamped(self) -> Self {
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

#[cfg(test)]
mod tests {
    use super::Volumes;

    #[test]
    fn test_volumes_are_read_and_mixed_with_the_master_volume() {
        let dir = std::env::temp_dir().join(format!("rustcraft-volumes-{}", std::process::id()));
        let file = dir.join("volume.toml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&file, "master = 0.8\neffects = 0.5").unwrap();
        let volumes = Volumes::read(&file);
        assert_eq!((0.8, 0.5), (volumes.master, volumes.effects));
        assert!((volumes.effects() - 0.4).abs() < 1e-6);
        std::fs::remove_dir_all(dir).unwrap();
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
