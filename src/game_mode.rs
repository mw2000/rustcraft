use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::block::BlockType;
use crate::player::{Movement, Player};

/// How a player plays (§4.5), one each so players in a world can differ.
///
/// What the modes change is asked of here and nowhere else. Survival can't fly, and places only
/// what it carries, which it gets from what it breaks; it has no breaking times or health yet
/// (M1.2, M1.8).
#[derive(Component, Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameMode {
    Survival,
    // until survival has the breaking times and crafting that make it a game
    #[default]
    Creative,
}

impl GameMode {
    pub fn name(self) -> &'static str {
        match self {
            GameMode::Survival => "survival",
            GameMode::Creative => "creative",
        }
    }

    /// Whether the player may fly.
    pub fn can_fly(self) -> bool {
        self == GameMode::Creative
    }

    /// Whether the player has every block to build with, chosen from the block picker or by
    /// middle-clicking one in the world, and never used up by placing it. Otherwise a block is
    /// placed from the hotbar, and used up.
    pub fn has_every_block(self) -> bool {
        self == GameMode::Creative
    }

    /// Whether the player may place `block` at all: any but air and water, in either mode.
    pub fn can_place(self, block: BlockType) -> bool {
        block.is_tangible()
    }

    /// Whether what the player breaks drops an item to pick up.
    pub fn drops_broken_blocks(self) -> bool {
        self == GameMode::Survival
    }
}

/// Keeps players flying only in modes that allow it.
pub struct GameModePlugin;

impl Plugin for GameModePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, land_when_flying_is_lost);
    }
}

// Switching to survival in the air drops the player, as walking falls.
fn land_when_flying_is_lost(mut players: Query<(&GameMode, &mut Player), Changed<GameMode>>) {
    for (mode, mut player) in &mut players {
        if !mode.can_fly() && player.movement == Movement::Fly {
            player.movement = Movement::Walk;
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use super::{GameMode, GameModePlugin};
    use crate::player::{Movement, Player};

    #[test]
    fn test_switching_to_survival_stops_flying() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, GameModePlugin));
        let player = app.world_mut().spawn(Player::default()).id();
        app.update();
        let movement = |app: &App| app.world().get::<Player>(player).unwrap().movement;
        assert_eq!(
            GameMode::Creative,
            *app.world().get::<GameMode>(player).unwrap()
        );
        assert_eq!(Movement::Fly, movement(&app));

        app.world_mut()
            .entity_mut(player)
            .insert(GameMode::Survival);
        app.update();
        assert_eq!(Movement::Walk, movement(&app));

        // and back in creative it stays walking until F is pressed
        app.world_mut()
            .entity_mut(player)
            .insert(GameMode::Creative);
        app.update();
        assert_eq!(Movement::Walk, movement(&app));
    }
}
