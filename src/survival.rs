//! Survival (§5.5): a player's health, food and breath, which only the server changes
//! (`server::survival`, INV-4) and tells each client of its own player's, and the bed they
//! respawn at.

use bevy::math::I64Vec3;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Full health, in half hearts.
pub const MAX_HEALTH: u8 = 20;
/// A full food bar, in half drumsticks.
pub const MAX_FOOD: u8 = 20;
/// Full breath, in bubbles.
pub const MAX_AIR: u8 = 10;

/// How a player's body is: their health, how full they are, and how much breath they have left
/// under water. At no health they're dead, until they respawn. Saved with them.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Vitals {
    pub health: u8,
    pub food: u8,
    pub air: u8,
}

impl Default for Vitals {
    fn default() -> Self {
        Self {
            health: MAX_HEALTH,
            food: MAX_FOOD,
            air: MAX_AIR,
        }
    }
}

impl Vitals {
    pub fn is_dead(self) -> bool {
        self.health == 0
    }
}

/// On a player asleep in a bed, until they get up, by moving from where they lay down, or the
/// night's skipped.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Asleep {
    pub at: Vec3,
}

/// The last blow a player took (M3.3), for their game to feel: where it came from, how it
/// knocks them, in blocks a second, and how many they've taken, which goes up with each.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bruise {
    pub from: Vec3,
    pub push: Vec3,
    pub count: u32,
}

/// The bed a player last used, which they respawn on, or none for the world's spawn.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SpawnBed(pub Option<I64Vec3>);

/// Where a player respawning on the bed at `bed` stands: on top of it, in the middle.
pub fn on_bed(bed: I64Vec3) -> Vec3 {
    bed.as_vec3() + Vec3::new(0.5, 1.0, 0.5)
}

// How far a player falls without hurting, in blocks.
const SAFE_FALL: f32 = 3.0;

/// The damage a fall of `fallen` blocks does: a point for each whole block beyond the first
/// three, as in Minecraft.
pub fn fall_damage(fallen: f32) -> u8 {
    (fallen - SAFE_FALL).floor().clamp(0.0, f32::from(u8::MAX)) as u8
}

#[cfg(test)]
mod tests {
    use super::{fall_damage, Vitals, MAX_HEALTH};

    #[test]
    fn test_falls_hurt_a_point_a_block_beyond_three() {
        assert_eq!(0, fall_damage(0.0));
        assert_eq!(0, fall_damage(1.25));
        assert_eq!(0, fall_damage(3.0));
        assert_eq!(0, fall_damage(3.9));
        assert_eq!(1, fall_damage(4.0));
        assert_eq!(MAX_HEALTH, fall_damage(23.0));
        assert_eq!(u8::MAX, fall_damage(1e9));
    }

    #[test]
    fn test_vitals_missing_from_a_save_are_full() {
        let vitals: Vitals = toml::from_str("health = 7").unwrap();
        assert_eq!(
            Vitals {
                health: 7,
                ..Vitals::default()
            },
            vitals
        );
        assert!(!vitals.is_dead());
    }
}
