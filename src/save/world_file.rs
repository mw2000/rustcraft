use std::f32::consts::PI;
use std::io;

use bevy::math::{EulerRot, Quat, Vec3};
use bevy::transform::components::Transform;
use serde::{Deserialize, Serialize};

use crate::game_mode::GameMode;
use crate::hotbar::Hotbar;
use crate::player::Movement;

/// The version of the save format, raised whenever a change to it would mislead an older game.
pub const FORMAT_VERSION: u32 = 1;

/// A world's `world.toml`: everything about it but its edits.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WorldFile {
    /// The save format version it's written in.
    pub format: u32,
    /// The version of rustcraft that wrote it.
    pub game_version: String,
    pub seed: u32,
    /// In days, as `TimeOfDay` has it.
    pub time_of_day: f32,
    /// The local player, once they've played in it. There's one until there's networking.
    pub player: Option<PlayerState>,
}

/// Where a player is, how they're moving and playing, and what they hold.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
pub struct PlayerState {
    /// Their feet, in blocks.
    pub position: [f32; 3],
    /// Which way they face, in radians: the body's turn about y, and the head's up or down.
    pub yaw: f32,
    pub pitch: f32,
    pub movement: Movement,
    // saves from before game modes and the hotbar get the ones a new player has
    #[serde(default)]
    pub game_mode: GameMode,
    #[serde(default)]
    pub hotbar: Hotbar,
}

impl PlayerState {
    /// A player arriving in a new world at `position`, facing +z.
    pub fn arriving_at(position: Vec3) -> Self {
        Self {
            position: position.into(),
            yaw: PI,
            pitch: 0.0,
            movement: Movement::default(),
            game_mode: GameMode::default(),
            hotbar: Hotbar::default(),
        }
    }

    /// The state of a player with the body and camera transforms given.
    pub fn of(
        body: &Transform,
        camera: &Transform,
        movement: Movement,
        game_mode: GameMode,
        hotbar: Hotbar,
    ) -> Self {
        Self {
            position: body.translation.into(),
            yaw: body.rotation.to_euler(EulerRot::YXZ).0,
            pitch: camera.rotation.to_euler(EulerRot::XYZ).0,
            movement,
            game_mode,
            hotbar,
        }
    }

    pub fn body(&self) -> Transform {
        Transform::from_translation(self.position.into())
            .with_rotation(Quat::from_rotation_y(self.yaw))
    }

    /// How the camera turns up or down, under the body.
    pub fn head(&self) -> Quat {
        Quat::from_rotation_x(self.pitch)
    }
}

impl WorldFile {
    pub fn new(seed: u32, time_of_day: f32) -> Self {
        Self {
            format: FORMAT_VERSION,
            game_version: env!("CARGO_PKG_VERSION").into(),
            seed,
            time_of_day,
            player: None,
        }
    }

    pub fn to_toml(&self) -> String {
        toml::to_string(self).expect("world files always encode")
    }

    /// Reads a `world.toml`, refusing one in a format newer than this game knows.
    pub fn from_toml(text: &str) -> io::Result<Self> {
        let invalid = |message: String| io::Error::new(io::ErrorKind::InvalidData, message);
        let table: toml::Table = toml::from_str(text).map_err(|e| invalid(e.to_string()))?;
        // checked before anything else, which a newer format might have changed
        let format = table.get("format").and_then(toml::Value::as_integer);
        match format {
            Some(format) if format > FORMAT_VERSION as i64 => {
                return Err(invalid(format!(
                    "it's in save format {format}, from a newer version of rustcraft, which \
                     only reads format {FORMAT_VERSION}; update rustcraft to play it"
                )))
            }
            Some(_) => {}
            None => return Err(invalid("it has no save format version".into())),
        }
        table.try_into().map_err(|e| invalid(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::{Quat, Vec3};
    use bevy::transform::components::Transform;

    use super::{PlayerState, WorldFile, FORMAT_VERSION};
    use crate::block::BlockType;
    use crate::game_mode::GameMode;
    use crate::hotbar::Hotbar;
    use crate::player::Movement;

    #[test]
    fn test_world_files_round_trip() {
        let mut file = WorldFile::new(1234, 2.75);
        assert_eq!(file, WorldFile::from_toml(&file.to_toml()).unwrap());
        file.player = Some(PlayerState {
            position: [10.5, 470.0, -3000.25],
            yaw: 1.5,
            pitch: -0.25,
            movement: Movement::Walk,
            game_mode: GameMode::Survival,
            hotbar: Hotbar::default(),
        });
        let text = file.to_toml();
        assert!(text.contains("movement = \"walk\""), "{text}");
        assert!(text.contains("game_mode = \"survival\""), "{text}");
        assert_eq!(file, WorldFile::from_toml(&text).unwrap());
    }

    #[test]
    fn test_hotbars_round_trip_by_block_name() {
        let mut file = WorldFile::new(1, 0.5);
        let mut player = PlayerState::arriving_at(Vec3::ZERO);
        player.hotbar.slots[1] = None;
        player.hotbar.slots[4] = Some(BlockType::CraftingTable);
        player.hotbar.selected = 4;
        file.player = Some(player);
        let text = file.to_toml();
        assert!(text.contains(r#""stone", "", "stone_bricks""#), "{text}");
        assert!(text.contains(r#""crafting_table""#), "{text}");
        assert_eq!(file, WorldFile::from_toml(&text).unwrap());

        // a block or slot this game doesn't have is refused, as an edit's is
        let unknown = text.replace("crafting_table", "loom");
        assert!(WorldFile::from_toml(&unknown).is_err());
        let off_the_end = text.replace("selected = 4", "selected = 9");
        assert!(WorldFile::from_toml(&off_the_end).is_err());
    }

    #[test]
    fn test_older_saves_get_a_new_players_mode_and_hotbar() {
        let mut file = WorldFile::new(1, 0.5);
        file.player = Some(PlayerState::arriving_at(Vec3::ONE));
        let text = file.to_toml();
        let older = &text[..text.find("game_mode").unwrap()];
        assert!(!older.contains("hotbar"), "{older}");
        let player = WorldFile::from_toml(older).unwrap().player.unwrap();
        assert_eq!(GameMode::Creative, player.game_mode);
        assert_eq!(Hotbar::default(), player.hotbar);
    }

    #[test]
    fn test_a_player_faces_the_way_they_did() {
        let body = Transform::from_xyz(1.0, 2.0, 3.0).looking_to(Vec3::new(1.0, 0.0, 2.0), Vec3::Y);
        let camera = Transform::from_rotation(Quat::from_rotation_x(-0.5));
        let state = PlayerState::of(
            &body,
            &camera,
            Movement::Fly,
            GameMode::Creative,
            Hotbar::default(),
        );
        assert_eq!(body.translation, state.body().translation);
        assert!(body.rotation.angle_between(state.body().rotation) < 1e-5);
        assert!(camera.rotation.angle_between(state.head()) < 1e-5);
        let arriving = PlayerState::arriving_at(Vec3::ONE).body();
        assert!(arriving.forward().dot(Vec3::Z) > 0.9999);
    }

    #[test]
    fn test_a_newer_format_is_refused() {
        let mut file = WorldFile::new(1234, 0.4);
        file.format = FORMAT_VERSION + 1;
        // as a newer format might, with fields this version doesn't know or has lost
        let text = file.to_toml().replace("seed", "seeds");
        let error = WorldFile::from_toml(&text).unwrap_err();
        assert!(error.to_string().contains("newer version"), "{error}");
        assert!(WorldFile::from_toml("seed = 1").is_err());
    }
}
