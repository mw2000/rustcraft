use std::f32::consts::PI;
use std::io;

use bevy::math::Vec3;
use bevy::transform::components::Transform;
use serde::{Deserialize, Serialize};

use crate::game_mode::GameMode;
use crate::hotbar::Hotbar;
use crate::inventory::Inventory;
use crate::player::{Movement, PlayerLook};

/// The version of the save format, raised whenever a change to it would mislead an older game.
/// Format 2 moved the hotbar's blocks into the player's inventory (M1.1).
pub const FORMAT_VERSION: u32 = 2;

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
    /// The host's player, once they've played in it, the only one saved until M0.6.
    pub player: Option<PlayerState>,
}

/// Where a player is, how they're moving and playing, and what they carry.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PlayerState {
    /// Their feet, in blocks.
    pub position: [f32; 3],
    /// Which way they look, in radians, as `PlayerLook` has it.
    pub yaw: f32,
    pub pitch: f32,
    pub movement: Movement,
    // saves from before game modes, the hotbar and the inventory get the ones a new player has
    #[serde(default)]
    pub game_mode: GameMode,
    #[serde(default)]
    pub hotbar: Hotbar,
    #[serde(default)]
    inventory: Option<Inventory>,
}

impl PlayerState {
    /// A player arriving in a new world at `position`, facing +z, walking unless their game
    /// mode flies.
    pub fn arriving_at(position: Vec3) -> Self {
        let game_mode = GameMode::default();
        Self {
            position: position.into(),
            yaw: PI,
            pitch: 0.0,
            movement: if game_mode.can_fly() {
                Movement::Fly
            } else {
                Movement::Walk
            },
            game_mode,
            hotbar: Hotbar::default(),
            inventory: None,
        }
    }

    /// The state of a player with the body, look and inventory given.
    pub fn of(
        body: &Transform,
        look: PlayerLook,
        movement: Movement,
        game_mode: GameMode,
        hotbar: Hotbar,
        inventory: &Inventory,
    ) -> Self {
        Self {
            position: body.translation.into(),
            yaw: look.yaw,
            pitch: look.pitch,
            movement,
            game_mode,
            hotbar,
            inventory: Some(inventory.clone()),
        }
    }

    /// What they carry, or what a new player in their game mode starts with if they're new or
    /// saved before inventories.
    pub fn inventory(&self) -> Inventory {
        self.inventory
            .clone()
            .unwrap_or_else(|| Inventory::starting(self.game_mode))
    }

    pub fn body(&self) -> Transform {
        Transform::from_translation(self.position.into())
    }

    pub fn look(&self) -> PlayerLook {
        PlayerLook {
            yaw: self.yaw,
            pitch: self.pitch,
        }
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
    use bevy::math::Vec3;
    use bevy::transform::components::Transform;

    use super::{PlayerState, WorldFile, FORMAT_VERSION};
    use crate::block::BlockType;
    use crate::game_mode::GameMode;
    use crate::hotbar::Hotbar;
    use crate::inventory::Inventory;
    use crate::items::{Item, ItemStack};
    use crate::player::{Movement, PlayerLook};

    #[test]
    fn test_world_files_round_trip() {
        let mut file = WorldFile::new(1234, 2.75);
        assert_eq!(file, WorldFile::from_toml(&file.to_toml()).unwrap());
        let mut player = PlayerState::arriving_at(Vec3::new(10.5, 470.0, -3000.25));
        player.movement = Movement::Walk;
        player.game_mode = GameMode::Survival;
        file.player = Some(player);
        let text = file.to_toml();
        assert!(text.contains("movement = \"walk\""), "{text}");
        assert!(text.contains("game_mode = \"survival\""), "{text}");
        assert_eq!(file, WorldFile::from_toml(&text).unwrap());
    }

    #[test]
    fn test_inventories_and_the_selected_slot_round_trip_by_item_name() {
        let mut file = WorldFile::new(1, 0.5);
        let mut inventory = Inventory::default();
        let tables = ItemStack::new(Item::Block(BlockType::CraftingTable), 3);
        inventory.slots[4] = Some(tables);
        inventory.slots[30] = Some(ItemStack::new(Item::Block(BlockType::Dirt), 64));
        let state = PlayerState::of(
            &Transform::default(),
            PlayerLook::default(),
            Movement::Walk,
            GameMode::Survival,
            Hotbar { selected: 4 },
            &inventory,
        );
        file.player = Some(state);
        let text = file.to_toml();
        assert!(text.contains("item = \"crafting_table\""), "{text}");
        let read = WorldFile::from_toml(&text).unwrap();
        assert_eq!(file, read);
        let player = read.player.unwrap();
        assert_eq!(inventory, player.inventory());
        assert_eq!(Some(tables), player.hotbar.held(&inventory));

        // an item or slot this game doesn't have is refused, as an edit's block is
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
        assert_eq!(GameMode::Survival, player.game_mode);
        assert_eq!(Hotbar::default(), player.hotbar);
        assert_eq!(Inventory::default(), player.inventory());
    }

    #[test]
    fn test_saves_from_before_inventories_start_survival_with_nothing() {
        // as format 1 had it, with the hotbar's blocks in the hotbar
        let format_1 = r#"
            format = 1
            game_version = "0.1.0"
            seed = 1
            time_of_day = 0.5

            [player]
            position = [0.0, 470.0, 0.0]
            yaw = 0.0
            pitch = 0.0
            movement = "walk"
            game_mode = "survival"

            [player.hotbar]
            slots = ["stone", "", "", "", "", "", "", "", "furnace"]
            selected = 8
        "#;
        let player = WorldFile::from_toml(format_1).unwrap().player.unwrap();
        assert_eq!(Hotbar { selected: 8 }, player.hotbar);
        assert_eq!(Inventory::default(), player.inventory());
        let creative = format_1.replace("\"survival\"", "\"creative\"");
        let player = WorldFile::from_toml(&creative).unwrap().player.unwrap();
        assert_eq!(Inventory::starting(GameMode::Creative), player.inventory());
    }

    #[test]
    fn test_a_player_faces_the_way_they_did() {
        let body = Transform::from_xyz(1.0, 2.0, 3.0);
        let look = PlayerLook {
            yaw: 2.5,
            pitch: -0.5,
        };
        let state = PlayerState::of(
            &body,
            look,
            Movement::Fly,
            GameMode::Creative,
            Hotbar::default(),
            &Inventory::default(),
        );
        assert_eq!(body, state.body());
        assert_eq!(look, state.look());
        let arriving = PlayerState::arriving_at(Vec3::ONE).look();
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
