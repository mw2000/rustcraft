use std::f32::consts::PI;
use std::io;

use bevy::math::{I64Vec3, Vec3};
use bevy::transform::components::Transform;
use serde::{Deserialize, Serialize};

use crate::game_mode::GameMode;
use crate::inventory::Hotbar;
use crate::inventory::Inventory;
use crate::player::{Movement, PlayerLook};
use crate::skin::{ArmStyle, Skin};
use crate::survival::{SpawnBed, Vitals};

/// The version of the save format, raised whenever a change to it would mislead an older game.
/// Format 2 moved the hotbar's blocks into the player's inventory (M1.1), format 3 added
/// containers to the region files (M1.6), format 4 crops (M1.9b) and format 5 blocks' states
/// (M2.4a) and format 6 crops' planting times as `f64`s (M-R1), each reading the region files before it as they were.
pub const FORMAT_VERSION: u32 = 6;

/// A world's `world.toml`: everything about it but its edits.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct WorldFile {
    /// The save format version it's written in.
    pub format: u32,
    /// The version of rustcraft that wrote it.
    pub game_version: String,
    pub seed: u32,
    /// In days, as `TimeOfDay` has it.
    pub time_of_day: f64,
    /// The names of the players besides the host who may run every command (`Operators`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ops: Vec<String>,
    /// The game mode players new to it start in, chosen as it's made, survival in worlds made
    /// before there was a choice.
    #[serde(default, skip_serializing_if = "GameMode::is_survival")]
    pub new_player_mode: GameMode,
    /// The villages settled, by their cells on the villages' grid, whose villagers live on or
    /// are gone (M3.4); older games ignore it, finding no villagers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settled: Vec<[i64; 2]>,
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
    /// The bed they respawn on, if they've used one.
    #[serde(default)]
    pub bed: Option<[i64; 3]>,
    #[serde(default)]
    pub hotbar: Hotbar,
    #[serde(default)]
    inventory: Option<Inventory>,
    // and before health, whole
    #[serde(default)]
    pub vitals: Vitals,
    /// The skin they wear, if they've one of their own, kept in the world's `skins/` (M-A4).
    #[serde(default)]
    pub skin: Option<SavedSkin>,
}

/// A player's own skin as their save names it: its file in the world's `skins/`, named by its
/// hash so each is kept once however many wear it, and the arms it's drawn with.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SavedSkin {
    pub file: String,
    pub arms: ArmStyle,
}

impl SavedSkin {
    pub fn of(skin: &Skin) -> Self {
        Self {
            file: format!("{:016x}.png", skin.hash()),
            arms: skin.arms(),
        }
    }

    /// Whether its file's name is one `of` would give, so it's never read from anywhere else.
    pub fn is_named_by_hash(&self) -> bool {
        self.file
            .strip_suffix(".png")
            .is_some_and(|hash| hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
    }
}

impl PlayerState {
    /// A player arriving in a new world at `position` in `game_mode`, facing +z, walking unless
    /// their game mode flies.
    pub fn arriving_at(position: Vec3, game_mode: GameMode) -> Self {
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
            bed: None,
            hotbar: Hotbar::default(),
            inventory: None,
            vitals: Vitals::default(),
            skin: None,
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
            bed: None,
            hotbar,
            inventory: Some(inventory.clone()),
            vitals: Vitals::default(),
            skin: None,
        }
    }

    /// The same, with how they are and the bed they respawn on.
    pub fn surviving(self, vitals: Vitals, bed: SpawnBed) -> Self {
        Self {
            vitals,
            bed: bed.0.map(|bed| bed.to_array()),
            ..self
        }
    }

    /// The same, wearing `skin`, or the default skin with none.
    pub fn wearing(self, skin: Option<&Skin>) -> Self {
        Self {
            skin: skin.map(SavedSkin::of),
            ..self
        }
    }

    pub fn spawn_bed(&self) -> SpawnBed {
        SpawnBed(self.bed.map(I64Vec3::from_array))
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
    pub fn new(seed: u32, time_of_day: f64) -> Self {
        Self {
            format: FORMAT_VERSION,
            game_version: env!("CARGO_PKG_VERSION").into(),
            seed,
            time_of_day,
            ops: Vec::new(),
            new_player_mode: GameMode::default(),
            settled: Vec::new(),
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
    use bevy::math::{I64Vec3, Vec3};
    use bevy::transform::components::Transform;

    use super::{PlayerState, WorldFile, FORMAT_VERSION};
    use crate::block::BlockType;
    use crate::game_mode::GameMode;
    use crate::inventory::Hotbar;
    use crate::inventory::Inventory;
    use crate::items::{Item, ItemStack};
    use crate::player::{Movement, PlayerLook};
    use crate::survival::{SpawnBed, Vitals};

    #[test]
    fn test_world_files_round_trip() {
        let mut file = WorldFile::new(1234, 2.75);
        assert_eq!(file, WorldFile::from_toml(&file.to_toml()).unwrap());
        let mut player =
            PlayerState::arriving_at(Vec3::new(10.5, 470.0, -3000.25), GameMode::default());
        player.movement = Movement::Walk;
        player.game_mode = GameMode::Survival;
        file.player = Some(player);
        let text = file.to_toml();
        assert!(text.contains("movement = \"walk\""), "{text}");
        assert!(text.contains("game_mode = \"survival\""), "{text}");
        assert_eq!(file, WorldFile::from_toml(&text).unwrap());
    }

    #[test]
    fn test_a_players_skin_is_saved_by_its_files_hash_and_arms() {
        use super::SavedSkin;
        use crate::skin::{ArmStyle, Skin};
        let png = std::fs::read("assets/textures/skins/ranger.png").unwrap();
        let skin = Skin::new(png, ArmStyle::Slim).unwrap();
        let mut file = WorldFile::new(1, 0.5);
        let player = PlayerState::arriving_at(Vec3::ONE, GameMode::default());
        file.player = Some(player.clone().wearing(Some(&skin)));
        let text = file.to_toml();
        assert!(text.contains("arms = \"slim\""), "{text}");
        let read = WorldFile::from_toml(&text).unwrap();
        let saved = read.player.unwrap().skin.unwrap();
        assert_eq!(SavedSkin::of(&skin), saved);
        assert!(saved.is_named_by_hash());
        // and only a name it gives is ever read
        for name in [
            "../world.toml",
            "0123456789abcdef.toml",
            "xyz.png",
            "0123.png",
        ] {
            let named = SavedSkin {
                file: name.into(),
                ..saved.clone()
            };
            assert!(!named.is_named_by_hash(), "{name}");
        }
        // with none, nothing's written, and an old save reads as wearing none
        assert!(!WorldFile::new(1, 0.5).to_toml().contains("skin"));
        assert_eq!(None, player.wearing(None).skin);
    }

    #[test]
    fn test_the_time_keeps_every_day_of_a_long_lived_world() {
        let file = WorldFile::new(1, 1_000_000.123_456);
        let back = WorldFile::from_toml(&file.to_toml()).unwrap();
        assert_eq!(1_000_000.123_456, back.time_of_day);
        // and a time written when it was an `f32` reads as the same number of days
        let text = file.to_toml().replace("1000000.123456", "12.75");
        assert_eq!(12.75, WorldFile::from_toml(&text).unwrap().time_of_day);
    }

    #[test]
    fn test_operators_round_trip_and_none_write_nothing() {
        let mut file = WorldFile::new(1, 0.5);
        assert!(!file.to_toml().contains("ops"));
        file.ops = vec!["alex".into(), "Sam".into()];
        file.player = Some(PlayerState::arriving_at(Vec3::ONE, GameMode::default()));
        let text = file.to_toml();
        assert!(text.contains(r#"ops = ["alex", "Sam"]"#), "{text}");
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
        file.player = Some(PlayerState::arriving_at(Vec3::ONE, GameMode::default()));
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
        let arriving = PlayerState::arriving_at(Vec3::ONE, GameMode::default()).look();
        assert!(arriving.forward().dot(Vec3::Z) > 0.9999);
    }

    #[test]
    fn test_health_hunger_breath_and_the_bed_round_trip_and_older_saves_are_whole() {
        let mut file = WorldFile::new(1, 0.5);
        let vitals = Vitals {
            health: 7,
            food: 3,
            air: 4,
        };
        let bed = SpawnBed(Some(I64Vec3::new(-12, 470, 1 << 40)));
        file.player =
            Some(PlayerState::arriving_at(Vec3::ONE, GameMode::default()).surviving(vitals, bed));
        let text = file.to_toml();
        assert!(text.contains("health = 7"), "{text}");
        let player = WorldFile::from_toml(&text).unwrap().player.unwrap();
        assert_eq!((vitals, bed), (player.vitals, player.spawn_bed()));
        // and sent, as in `Welcome`
        let bytes = postcard::to_stdvec(&player).unwrap();
        assert_eq!(player, postcard::from_bytes(&bytes).unwrap());

        let older = &text[..text.find("bed").unwrap()];
        let player = WorldFile::from_toml(older).unwrap().player.unwrap();
        assert_eq!(
            (Vitals::default(), SpawnBed(None)),
            (player.vitals, player.spawn_bed())
        );
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
