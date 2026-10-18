use bevy::math::{BVec3, I64Vec3, Quat, Vec2, Vec3, Vec3Swizzles};

use super::{Movement, Player, PlayerLook, BODY};
use crate::block::shape::{Boxes, Use};
use crate::block::{Block, BlockType};
use crate::collision::{boxes_at, move_and_collide, Aabb};

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
// How high a body walks up onto without jumping: a slab or a stair, not a block.
const STEP_HEIGHT: f32 = 0.6;
// Blocks a second up and down a ladder or vines.
const CLIMB_SPEED: f32 = 3.0;

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
    block: impl Fn(I64Vec3) -> Option<Block>,
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
        Movement::Walk => walk_player(position, player, look, input, dt, block),
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

fn walk_player(
    position: Vec3,
    player: &mut Player,
    look: PlayerLook,
    input: MoveInput,
    dt: f32,
    block: impl Fn(I64Vec3) -> Option<Block>,
) -> Vec3 {
    let mut speed = WALK_SPEED;
    if input.sprint {
        speed *= WALK_SPRINT_MULTIPLIER;
    }
    // where the body faces, not the eyes, so looking down doesn't slow it
    let wanted = Quat::from_rotation_y(look.yaw) * input.across.normalize_or_zero() * speed;
    walk(
        position,
        &mut player.momentum,
        BODY,
        wanted,
        input.jump,
        dt,
        block,
    )
}

/// How a body walking under gravity is moving: how fast, and whether it stood on the ground
/// and walked into a wall at the last step.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Momentum {
    pub velocity: Vec3,
    pub grounded: bool,
    // whether it walked into a wall, so swimming against a bank climbs out
    pub against_wall: bool,
}

/// Moves a body of size `body` with its feet at `position` through `dt` seconds of walking
/// under gravity at `wanted` across, slower in water, and jumping, or swimming up, if `jump`,
/// and returns where it gets to. It's a player's walking, and a creature's (§5.8). As in `step`,
/// `block` is `None` where the world isn't generated, which is taken to be solid.
pub fn walk(
    position: Vec3,
    momentum: &mut Momentum,
    body: Aabb,
    wanted: Vec3,
    jump: bool,
    dt: f32,
    block: impl Fn(I64Vec3) -> Option<Block>,
) -> Vec3 {
    let dt = dt.min(MAX_STEP);
    let kind = |at: Vec3| block(at.floor().as_i64vec3()).map(|block| block.kind);
    let middle = position + Vec3::Y * body.max.y / 2.0;
    let swimming = kind(middle) == Some(BlockType::Water);
    let climbing = [position, middle]
        .into_iter()
        .any(|at| kind(at).is_some_and(|kind| kind.is_climbable()));
    let wanted = if swimming {
        wanted * SWIM_MULTIPLIER
    } else {
        wanted
    };
    momentum.velocity.x = wanted.x;
    momentum.velocity.z = wanted.z;

    if swimming {
        momentum.velocity.y = if jump && momentum.against_wall {
            JUMP_SPEED
        } else if jump {
            SWIM_UP_SPEED
        } else {
            (momentum.velocity.y - WATER_GRAVITY * dt).max(-SINK_SPEED)
        };
    } else if climbing {
        // up while jumping or walking into it, and otherwise slowly down
        momentum.velocity.y = if jump || momentum.against_wall {
            CLIMB_SPEED
        } else {
            (momentum.velocity.y - GRAVITY * dt).max(-CLIMB_SPEED)
        };
    } else {
        if jump && momentum.grounded {
            momentum.velocity.y = JUMP_SPEED;
        }
        momentum.velocity.y = (momentum.velocity.y - GRAVITY * dt).max(-TERMINAL_SPEED);
    }

    // the world not generated yet is solid
    let boxes = |at: I64Vec3| match block(at) {
        None => Boxes::cube(),
        Some(_) => boxes_at(at, Use::Collision, |at| block(at).unwrap_or_default()),
    };
    let delta = momentum.velocity * dt;
    let (mut end, mut hit) = move_and_collide(position, delta, body, boxes);
    let mut stepped = false;
    // on the ground and stopped across, it steps up onto what's low enough, as onto a slab, if
    // that gets it further
    if momentum.grounded && (hit.x || hit.z) && !swimming {
        let up = move_and_collide(position, Vec3::Y * STEP_HEIGHT, body, boxes).0;
        let (across, across_hit) = move_and_collide(up, delta.with_y(0.0), body, boxes);
        let (down, down_hit) =
            move_and_collide(across, Vec3::NEG_Y * (up.y - position.y), body, boxes);
        let further = |at: Vec3| (at - position).xz().length_squared();
        if down_hit.y && further(down) > further(end) + 1e-6 {
            (end, hit) = (down, BVec3::new(across_hit.x, true, across_hit.z));
            stepped = true;
        }
    }
    momentum.grounded = stepped || (hit.y && momentum.velocity.y < 0.0);
    if hit.y {
        momentum.velocity.y = 0.0;
    }
    momentum.against_wall = (hit.x || hit.z) && wanted.xz() != Vec2::ZERO;
    end
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
    fn block(at: I64Vec3) -> Option<Block> {
        if at.z > 100 {
            None
        } else if at.y < 0 || at.x == 3 {
            Some(BlockType::Grass.into())
        } else {
            Some(Block::AIR)
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
        assert!(player.momentum.grounded);
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
        assert!(!player.momentum.grounded);
        let landed = run(
            later,
            &mut player,
            PlayerLook::default(),
            MoveInput::default(),
            1.0,
        );
        assert_eq!(0.0, landed.y);
        assert!(player.momentum.grounded);
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
        assert!(player.momentum.against_wall);
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

    // Walks a body east from x 0.5 over a floor at y 0 for `seconds`, jumping if `jump`, with
    // `put` the blocks on it, and returns where it ends up.
    fn walk_east(put: &[(I64Vec3, &str)], jump: bool, seconds: f32) -> Vec3 {
        let blocks: Vec<(I64Vec3, Block)> = put
            .iter()
            .map(|&(at, text)| (at, text.parse().unwrap()))
            .collect();
        let block = |at: I64Vec3| {
            Some(match blocks.iter().find(|(b, _)| *b == at) {
                Some(&(_, block)) => block,
                None if at.y < 0 => BlockType::Stone.into(),
                None => Block::AIR,
            })
        };
        let mut momentum = Momentum::default();
        let mut at = Vec3::new(0.5, 0.0, 0.5);
        for _ in 0..(seconds / DT).round() as usize {
            at = walk(
                at,
                &mut momentum,
                BODY,
                Vec3::X * WALK_SPEED,
                jump,
                DT,
                block,
            );
        }
        at
    }

    #[test]
    fn test_walks_up_slabs_and_stairs_without_jumping_but_not_onto_a_block() {
        let slabs: Vec<_> = (2..8)
            .map(|x| (I64Vec3::new(x, 0, 0), "planks_slab"))
            .collect();
        let slab = walk_east(&slabs, false, 1.0);
        assert!(slab.x > 3.0 && (slab.y - 0.5).abs() < 1e-4, "{slab}");
        let mut stairs = vec![(I64Vec3::new(2, 0, 0), "cobblestone_stairs[facing=east]")];
        stairs.extend((3..10).map(|x| (I64Vec3::new(x, 0, 0), "cobblestone")));
        let up = walk_east(&stairs, false, 1.5);
        assert!(up.x > 4.0 && (up.y - 1.0).abs() < 1e-4, "{up}");
        // a top slab is a block to step onto
        let block = walk_east(
            &[(I64Vec3::new(2, 0, 0), "planks_slab[half=top]")],
            false,
            1.0,
        );
        assert!((block.x - 1.7).abs() < 1e-4 && block.y == 0.0, "{block}");
        // and stairs turned the wrong way are a block from that side
        let back = walk_east(
            &[(I64Vec3::new(2, 0, 0), "planks_stairs[facing=west]")],
            false,
            1.0,
        );
        assert!((back.x - 1.7).abs() < 1e-4, "{back}");
    }

    #[test]
    fn test_fences_and_walls_cant_be_jumped_and_only_open_doors_walked_through() {
        for blocker in ["planks_fence", "dark_planks_fence", "cobblestone_wall"] {
            let at = walk_east(&[(I64Vec3::new(2, 0, 0), blocker)], true, 2.0);
            assert!(at.x < 2.5, "over the {blocker} to {at}");
        }
        let door = |open: &'static str, upper: &'static str| {
            [
                (I64Vec3::new(2, 0, 0), open),
                (I64Vec3::new(2, 1, 0), upper),
            ]
        };
        let shut = walk_east(
            &door("door[facing=west]", "door[facing=west,half=top]"),
            true,
            2.0,
        );
        // shut on the far side of its cell
        assert!(
            (shut.x - (3.0 - 3.0 / 16.0 - 0.3)).abs() < 1e-4,
            "through the door to {shut}"
        );
        let open = door(
            "door[facing=west,open=true]",
            "door[facing=west,half=top,open=true]",
        );
        let open = walk_east(&open, false, 1.0);
        assert!(open.x > 3.0, "{open}");
        let hatches: Vec<_> = (2..8)
            .map(|x| (I64Vec3::new(x, 0, 0), "trapdoor"))
            .collect();
        let hatch = walk_east(&hatches, false, 1.0);
        assert!(
            hatch.x > 3.0 && (hatch.y - 3.0 / 16.0).abs() < 1e-4,
            "{hatch}"
        );
        // vines and torches are walked through
        let vines = walk_east(&[(I64Vec3::new(2, 0, 0), "vines[facing=east]")], false, 1.0);
        assert!(vines.x > 3.0, "{vines}");
    }

    #[test]
    fn test_ladders_are_climbed_walking_into_them_and_slid_down_slowly() {
        // a ladder up a wall at x 2, three blocks tall, onto a floor above
        let mut put = vec![];
        for y in 0..3 {
            put.push((I64Vec3::new(2, y, 0), "stone"));
            put.push((I64Vec3::new(1, y, 0), "ladder[facing=west]"));
        }
        put.extend((2..12).map(|x| (I64Vec3::new(x, 2, 0), "stone")));
        let top = walk_east(&put, false, 3.0);
        assert!(top.x > 2.5 && top.y == 3.0, "climbed to {top}");
        // let go, it comes down no faster than it climbs
        let blocks: Vec<(I64Vec3, Block)> = put
            .iter()
            .map(|&(at, text)| (at, text.parse().unwrap()))
            .collect();
        let block = |at: I64Vec3| {
            Some(match blocks.iter().find(|(b, _)| *b == at) {
                Some(&(_, block)) => block,
                None if at.y < 0 => BlockType::Stone.into(),
                None => Block::AIR,
            })
        };
        let mut momentum = Momentum::default();
        let mut at = Vec3::new(1.5, 2.0, 0.5);
        for _ in 0..30 {
            at = walk(at, &mut momentum, BODY, Vec3::ZERO, false, DT, block);
            assert!(momentum.velocity.y >= -CLIMB_SPEED);
        }
        assert!(at.y < 2.0 && at.y > 0.5, "{at}");
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
