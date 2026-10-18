use bevy::math::{I64Vec3, Quat, Vec3};

use super::{Movement, Player, PlayerLook, BODY};
use crate::{block::BlockType, collision::move_and_collide};

// Flying, blocks a second, and how much faster sprinting.
const FLY_SPEED: f32 = 20.0;
const FLY_SPRINT_MULTIPLIER: f32 = 4.0;
// Walking, blocks a second, and how much faster sprinting.
const WALK_SPEED: f32 = 4.3;
const WALK_SPRINT_MULTIPLIER: f32 = 1.3;
// Blocks a second squared, and the speed falling tops out at.
const GRAVITY: f32 = 32.0;
const TERMINAL_SPEED: f32 = 60.0;
// Enough to clear one block, not two.
const JUMP_SPEED: f32 = 9.0;
// In water: slower across, sinking slowly, and swimming up while jumping.
const SWIM_MULTIPLIER: f32 = 0.6;
const WATER_GRAVITY: f32 = 8.0;
const SINK_SPEED: f32 = 2.0;
const SWIM_UP_SPEED: f32 = 4.0;
// The longest frame moved in one step, so a hitch doesn't fling the player.
const MAX_STEP: f32 = 0.1;

/// What a player asks of a step of movement, from their keys.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct MoveInput {
    /// Which way to go as the player faces, x to the right and -z ahead, each -1, 0 or 1.
    pub across: Vec3,
    /// Up (1) or down (-1), while flying.
    pub rise: f32,
    /// Jumping, or swimming up, while walking.
    pub jump: bool,
    pub sprint: bool,
}

/// Moves a player with its feet at `position` through `dt` seconds of `input`, facing `look`,
/// and returns where it gets to. `block` is the world's block at a position, or `None` where
/// the world isn't generated yet, which is taken to be solid: the player waits where it is for
/// the ground to load rather than falling through it.
pub fn step(
    position: Vec3,
    player: &mut Player,
    look: PlayerLook,
    input: MoveInput,
    dt: f32,
    block: impl Fn(I64Vec3) -> Option<BlockType>,
) -> Vec3 {
    let dt = dt.min(MAX_STEP);
    match player.movement {
        Movement::Fly => {
            let sprint = if input.sprint {
                FLY_SPRINT_MULTIPLIER
            } else {
                1.0
            };
            let speed = FLY_SPEED * dt * sprint;
            position + (look.rotation() * input.across + Vec3::Y * input.rise) * speed
        }
        Movement::Walk => walk(position, player, look, input, dt, block),
    }
}

/// The fastest `step` moves a player with `movement`, in blocks a second: across, up and down.
/// Flying goes fastest across diagonally, and can rise or sink as fast at the same time.
pub fn top_speed(movement: Movement) -> (f32, f32, f32) {
    match movement {
        Movement::Fly => {
            let straight = FLY_SPEED * FLY_SPRINT_MULTIPLIER;
            (straight * std::f32::consts::SQRT_2, straight, straight)
        }
        Movement::Walk => (
            WALK_SPEED * WALK_SPRINT_MULTIPLIER,
            JUMP_SPEED,
            TERMINAL_SPEED,
        ),
    }
}

fn walk(
    position: Vec3,
    player: &mut Player,
    look: PlayerLook,
    input: MoveInput,
    dt: f32,
    block: impl Fn(I64Vec3) -> Option<BlockType>,
) -> Vec3 {
    let middle = position + Vec3::Y * BODY.max.y / 2.0;
    let swimming = block(middle.floor().as_i64vec3()) == Some(BlockType::Water);

    let mut speed = WALK_SPEED;
    if input.sprint {
        speed *= WALK_SPRINT_MULTIPLIER;
    }
    if swimming {
        speed *= SWIM_MULTIPLIER;
    }
    // where the body faces, not the eyes, so looking down doesn't slow it
    let wanted = Quat::from_rotation_y(look.yaw) * input.across.normalize_or_zero() * speed;
    player.velocity.x = wanted.x;
    player.velocity.z = wanted.z;

    if swimming {
        player.velocity.y = if input.jump && player.against_wall {
            JUMP_SPEED
        } else if input.jump {
            SWIM_UP_SPEED
        } else {
            (player.velocity.y - WATER_GRAVITY * dt).max(-SINK_SPEED)
        };
    } else {
        if input.jump && player.grounded {
            player.velocity.y = JUMP_SPEED;
        }
        player.velocity.y = (player.velocity.y - GRAVITY * dt).max(-TERMINAL_SPEED);
    }

    let solid = |at: I64Vec3| block(at).is_none_or(|block| block.is_solid());
    let (position, hit) = move_and_collide(position, player.velocity * dt, BODY, solid);
    player.grounded = hit.y && player.velocity.y < 0.0;
    if hit.y {
        player.velocity.y = 0.0;
    }
    player.against_wall = (hit.x || hit.z) && input.across != Vec3::ZERO;
    position
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use bevy::math::{I64Vec3, Vec3};

    use super::*;

    const DT: f32 = 1.0 / 60.0;
    const AHEAD: MoveInput = MoveInput {
        across: Vec3::NEG_Z,
        rise: 0.0,
        jump: false,
        sprint: false,
    };

    // grass from y 0 down, a wall at x 3, and nothing generated past z 100
    fn block(at: I64Vec3) -> Option<BlockType> {
        if at.z > 100 {
            None
        } else if at.y < 0 || at.x == 3 {
            Some(BlockType::Grass)
        } else {
            Some(BlockType::Air)
        }
    }

    fn walker() -> Player {
        Player {
            movement: Movement::Walk,
            ..Player::default()
        }
    }

    // Steps `player` for `seconds`, returning where it ends up.
    fn run(
        position: Vec3,
        player: &mut Player,
        look: PlayerLook,
        input: MoveInput,
        seconds: f32,
    ) -> Vec3 {
        (0..(seconds / DT).round() as usize)
            .fold(position, |at, _| step(at, player, look, input, DT, block))
    }

    #[test]
    fn test_walks_the_way_the_body_faces_at_walking_speed() {
        let mut player = walker();
        // turned to face -x and looking down, which doesn't slow it
        let look = PlayerLook {
            yaw: FRAC_PI_2,
            pitch: -1.0,
        };
        let start = Vec3::new(0.5, 0.0, 0.5);
        let end = run(start, &mut player, look, AHEAD, 1.0);
        assert!(
            (end - start - Vec3::NEG_X * WALK_SPEED).length() < 1e-3,
            "{end}"
        );
        assert!(player.grounded);
    }

    #[test]
    fn test_falls_faster_and_faster_and_lands() {
        let mut player = walker();
        let start = Vec3::new(0.5, 10.0, 0.5);
        let fallen = |at: Vec3| start.y - at.y;
        let at = run(
            start,
            &mut player,
            PlayerLook::default(),
            MoveInput::default(),
            0.25,
        );
        let later = run(
            at,
            &mut player,
            PlayerLook::default(),
            MoveInput::default(),
            0.25,
        );
        assert!(fallen(later) - fallen(at) > fallen(at) * 2.0);
        assert!(!player.grounded);
        let landed = run(
            later,
            &mut player,
            PlayerLook::default(),
            MoveInput::default(),
            1.0,
        );
        assert_eq!(0.0, landed.y);
        assert!(player.grounded);
    }

    #[test]
    fn test_jumps_from_the_ground_over_one_block() {
        let mut player = walker();
        let jump = MoveInput {
            jump: true,
            ..MoveInput::default()
        };
        let start = Vec3::new(0.5, 0.0, 0.5);
        let top = (0..30)
            .scan(start, |at, _| {
                *at = step(*at, &mut player, PlayerLook::default(), jump, DT, block);
                Some(at.y)
            })
            .fold(0.0, f32::max);
        assert!((1.0..2.0).contains(&top), "{top}");
    }

    #[test]
    fn test_flies_the_way_the_eyes_look() {
        let mut player = Player::default();
        let look = PlayerLook {
            yaw: FRAC_PI_2,
            pitch: 0.5,
        };
        let start = Vec3::new(0.5, 10.0, 0.5);
        // through the wall at x 3, as flying goes through anything
        let end = step(start, &mut player, look, AHEAD, 0.1, block);
        let moved = end - start;
        assert!((moved.length() - FLY_SPEED * 0.1).abs() < 1e-4);
        assert!(
            (moved.normalize() - look.forward()).length() < 1e-5,
            "{moved}"
        );

        let up = MoveInput {
            rise: 1.0,
            sprint: true,
            ..MoveInput::default()
        };
        let end = step(start, &mut player, look, up, 0.1, block);
        let wanted = start + Vec3::Y * FLY_SPEED * FLY_SPRINT_MULTIPLIER * 0.1;
        assert!((end - wanted).length() < 1e-4, "{end}");
    }

    #[test]
    fn test_walking_into_a_wall_stops_against_it() {
        let mut player = walker();
        let right = MoveInput {
            across: Vec3::X,
            ..MoveInput::default()
        };
        let end = run(
            Vec3::new(0.5, 0.0, 0.5),
            &mut player,
            PlayerLook::default(),
            right,
            2.0,
        );
        assert!((end.x - (3.0 - BODY.max.x)).abs() < 1e-5, "{end}");
        assert!(player.against_wall);
    }

    #[test]
    fn test_waits_at_the_edge_of_what_is_generated() {
        let mut player = walker();
        let back = MoveInput {
            across: Vec3::Z,
            ..MoveInput::default()
        };
        let end = run(
            Vec3::new(0.5, 0.0, 99.5),
            &mut player,
            PlayerLook::default(),
            back,
            2.0,
        );
        assert!((end.z - (101.0 - BODY.max.z)).abs() < 1e-5, "{end}");
    }

    #[test]
    fn test_a_long_frame_moves_no_further_than_a_short_step() {
        let mut player = Player::default();
        let end = step(
            Vec3::ZERO,
            &mut player,
            PlayerLook::default(),
            AHEAD,
            5.0,
            block,
        );
        assert!(
            (end - Vec3::NEG_Z * FLY_SPEED * MAX_STEP).length() < 1e-5,
            "{end}"
        );
    }
}
