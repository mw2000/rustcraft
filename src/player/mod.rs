pub mod input;
pub mod physics;
pub mod underwater;

use std::f32::consts::TAU;

use bevy::{
    ecs::component::Component,
    math::{Quat, Vec3},
};
use bevy_replicon::prelude::ClientId;

use crate::{collision::Aabb, game_mode::GameMode, hotbar::Hotbar};

/// The player's body, at its feet, and how it's moving.
#[derive(Component, Default)]
#[require(GameMode, Hotbar, PlayerLook)]
pub struct Player {
    pub movement: Movement,
    velocity: Vec3,
    grounded: bool,
    // whether it walked into a wall last step, so swimming against a bank climbs out
    against_wall: bool,
}

/// The client a player belongs to: `ClientId::Server` for the host's own, the one player in
/// singleplayer.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerOwner(pub ClientId);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Movement {
    /// Flying anywhere, through anything, the way the player is looking.
    #[default]
    Fly,
    /// Walking under gravity, kept out of solid blocks.
    Walk,
}

/// Which way a player looks, in radians: its turn about y, 0 facing -z, and up or down. It's
/// what movement, reaching for blocks and saving go by; the camera only follows it.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq)]
pub struct PlayerLook {
    pub yaw: f32,
    pub pitch: f32,
}

// Just short of straight up or down, where the view would flip over.
const MAX_PITCH: f32 = 89.0_f32.to_radians();

impl PlayerLook {
    /// Turns it by `yaw` and `pitch`, never past straight up or down.
    pub fn turn(&mut self, yaw: f32, pitch: f32) {
        // kept within a turn, so it doesn't lose precision as the player spins
        self.yaw = (self.yaw + yaw).rem_euclid(TAU);
        self.pitch = (self.pitch + pitch).clamp(-MAX_PITCH, MAX_PITCH);
    }

    /// The eyes' rotation: turned about y, then tilted up or down.
    pub fn rotation(self) -> Quat {
        Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(self.pitch)
    }

    /// The way the eyes look.
    pub fn forward(self) -> Vec3 {
        self.rotation() * Vec3::NEG_Z
    }
}

/// How high the eyes are above the player's feet, far enough under the top of its head that
/// the camera's near plane stays out of the ceiling.
pub const EYE_HEIGHT: f32 = 1.85;
/// The player's size: two blocks tall, so it just fits through a two-block gap.
pub const BODY: Aabb = Aabb {
    min: Vec3::new(-0.3, 0.0, -0.3),
    max: Vec3::new(0.3, 2.0, 0.3),
};

#[cfg(test)]
mod tests {
    use bevy::math::Vec3;

    use super::{PlayerLook, MAX_PITCH};

    #[test]
    fn test_looking_turns_about_y_and_stops_short_of_straight_up() {
        let mut look = PlayerLook::default();
        assert!(look.forward().dot(Vec3::NEG_Z) > 0.9999);
        look.turn(std::f32::consts::FRAC_PI_2, 10.0);
        assert_eq!(MAX_PITCH, look.pitch);
        let forward = look.forward();
        assert!(forward.y > 0.999 && forward.x < 0.0, "{forward}");
        look.turn(0.0, -20.0);
        assert_eq!(-MAX_PITCH, look.pitch);
    }
}
