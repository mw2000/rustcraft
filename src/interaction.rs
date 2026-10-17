use bevy::math::I64Vec3;
use bevy::prelude::*;

use crate::block::BlockType;
use crate::chunks::chunk::ChunkCoordinate;
use crate::chunks::chunk_loader::apply_edits;
use crate::collision::Aabb;
use crate::commands::command_bar_closed;
use crate::edits::EditBlock;
use crate::game_mode::GameMode;
use crate::hotbar::Hotbar;
use crate::menu::GameState;
use crate::player::{player_look, player_move, Player, BODY};
use crate::world::World;

/// How far from the eye a block can be reached, in blocks.
pub const REACH: f32 = 5.0;

// Just over a block wide, so the outline isn't hidden in the block's own faces.
const OUTLINE_SIZE: f32 = 1.004;
const OUTLINE_COLOUR: Color = Color::srgba(0.0, 0.0, 0.0, 0.7);

/// Looking at blocks, outlining the one looked at, and breaking, placing and picking blocks
/// with the mouse (§5.1). None of it happens while the game's paused, the block picker's up or
/// the command bar's open.
pub struct InteractionPlugin;

impl Plugin for InteractionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Target>().add_systems(
            Update,
            (target_block, (outline_target, use_target))
                .chain()
                // from where the player is this frame, and edits are made the same frame
                .after(player_move)
                .after(player_look)
                .before(apply_edits)
                .run_if(in_state(GameState::Playing))
                .run_if(command_bar_closed),
        );
    }
}

/// The block looked at, if there's one in reach.
#[derive(Resource, Default, Debug)]
pub struct Target(pub Option<Hit>);

/// A block a ray stopped at, and the face it came in through, as the direction out of that
/// face. The face is zero if the ray started inside the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub block: I64Vec3,
    pub face: I64Vec3,
}

/// The first block that `stops` the ray from `origin` along `direction` within `reach` blocks,
/// stepping through the blocks it crosses in order (a DDA).
pub fn raycast(
    origin: Vec3,
    direction: Vec3,
    reach: f32,
    stops: impl Fn(I64Vec3) -> bool,
) -> Option<Hit> {
    let direction = direction.normalize_or_zero();
    let mut block = origin.floor().as_i64vec3();
    let mut face = I64Vec3::ZERO;
    let step = I64Vec3::from_array(direction.to_array().map(|d| {
        if d > 0.0 {
            1
        } else if d < 0.0 {
            -1
        } else {
            0
        }
    }));
    // how far along the ray it crosses one block, and the next block boundary, on each axis
    let across = direction.abs().recip();
    let mut next = Vec3::from_array(std::array::from_fn(|axis| {
        let to_boundary = match step[axis] {
            1 => block[axis] as f32 + 1.0 - origin[axis],
            -1 => origin[axis] - block[axis] as f32,
            _ => return f32::INFINITY,
        };
        to_boundary * across[axis]
    }));
    loop {
        if stops(block) {
            return Some(Hit { block, face });
        }
        let axis = next.min_position();
        if next[axis] > reach {
            return None;
        }
        block[axis] += step[axis];
        face = I64Vec3::ZERO;
        face[axis] = -step[axis];
        next[axis] += across[axis];
    }
}

/// Whether a block at `block` would overlap a body of `body` at `position`. Touching doesn't
/// count, so a block can go just under the player's feet.
pub fn overlaps(body: Aabb, position: Vec3, block: I64Vec3) -> bool {
    // a little slack, as collision has, for a body resting on a face give or take rounding
    const SLACK: f32 = 1e-3;
    let (min, max) = (position + body.min, position + body.max);
    let block = block.as_vec3();
    (min + SLACK).cmplt(block + 1.0).all() && (max - SLACK).cmpgt(block).all()
}

#[allow(clippy::type_complexity)]
fn target_block(
    world: Res<World>,
    mut target: ResMut<Target>,
    camera: Query<(&ChildOf, &Transform), (With<Camera>, Without<Player>)>,
    player: Query<&Transform, With<Player>>,
) {
    let (parent, camera) = camera.single().expect("camera does not exist");
    let player = player.get(parent.parent()).expect("player does not exist");
    let eye = player.mul_transform(*camera);
    // blocks not generated yet can't be reached, nor can air and water, which are looked through
    let stops = |block: I64Vec3| {
        world.is_chunk_generated(ChunkCoordinate::containing(block))
            && world.get_block_at(block).is_solid()
    };
    target.0 = raycast(eye.translation, *eye.forward(), REACH, stops);
}

fn outline_target(target: Res<Target>, mut gizmos: Gizmos) {
    if let Some(hit) = target.0 {
        let centre = hit.block.as_vec3() + 0.5;
        gizmos.cube(
            Transform::from_translation(centre).with_scale(Vec3::splat(OUTLINE_SIZE)),
            OUTLINE_COLOUR,
        );
    }
}

// Left click breaks the block looked at, right click places the selected block in the hotbar
// against the face looked at, and middle click picks the block looked at into the hotbar.
fn use_target(
    world: Res<World>,
    target: Res<Target>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut player: Query<(&Transform, &GameMode, &mut Hotbar), With<Player>>,
    mut edits: MessageWriter<EditBlock>,
) {
    let Some(hit) = target.0 else {
        return;
    };
    let (feet, mode, mut hotbar) = player.single_mut().expect("player does not exist");
    // at once in either mode, until M1.2 gives survival breaking times and drops
    if mouse.just_pressed(MouseButton::Left) {
        edits.write(EditBlock {
            block: hit.block,
            to: BlockType::Air,
        });
    }
    if mouse.just_pressed(MouseButton::Right) && hit.face != I64Vec3::ZERO {
        let block = hit.block + hit.face;
        // into air or water, which it replaces, and never into the player
        if !world.get_block_at(block).is_solid() && !overlaps(BODY, feet.translation, block) {
            // never used up, until M1.1 has survival take it from the slot
            if let Some(held) = hotbar.held() {
                edits.write(EditBlock { block, to: held });
            }
        }
    }
    if mouse.just_pressed(MouseButton::Middle) {
        hotbar.pick(world.get_block_at(hit.block), mode.has_every_block());
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::{I64Vec3, Vec3};

    use super::{overlaps, raycast, Hit, REACH};
    use crate::player::BODY;

    // a floor from y 0 down, and a pillar at x 3, z 0 standing on it
    fn stops(block: I64Vec3) -> bool {
        block.y <= 0 || (block.x == 3 && block.z == 0)
    }

    #[test]
    fn test_hits_the_block_looked_at_through_the_face_facing_the_eye() {
        let eye = Vec3::new(0.5, 1.5, 0.5);
        assert_eq!(
            Some(Hit {
                block: I64Vec3::new(3, 1, 0),
                face: I64Vec3::NEG_X,
            }),
            raycast(eye, Vec3::X, REACH, stops)
        );
        assert_eq!(
            Some(Hit {
                block: I64Vec3::new(0, 0, 0),
                face: I64Vec3::Y,
            }),
            raycast(eye, Vec3::NEG_Y, REACH, stops)
        );
        // at a slant, onto the floor two blocks off
        let hit = raycast(eye, Vec3::new(-4.0, -1.0, 0.0), REACH, stops).unwrap();
        assert_eq!((I64Vec3::new(-2, 0, 0), I64Vec3::Y), (hit.block, hit.face));
    }

    #[test]
    fn test_nothing_past_reach_is_hit() {
        let eye = Vec3::new(-2.5, 1.5, 0.5);
        assert!(raycast(eye, Vec3::X, REACH, stops).is_none());
        assert!(raycast(eye + Vec3::X, Vec3::X, REACH, stops).is_some());
        assert!(raycast(eye, Vec3::Y, REACH, stops).is_none());
    }

    #[test]
    fn test_air_and_water_are_looked_through() {
        use crate::block::BlockType;
        // a pool two deep over the floor at y 0, with a stone in it at x 2
        let block = |b: I64Vec3| match b {
            b if b.y <= 0 => BlockType::Sand,
            b if b == I64Vec3::new(2, 1, 0) => BlockType::Stone,
            b if b.y <= 2 => BlockType::Water,
            _ => BlockType::Air,
        };
        let stops = |b| block(b).is_solid();
        let hit = raycast(Vec3::new(-1.5, 1.5, 0.5), Vec3::X, REACH, stops).unwrap();
        assert_eq!(I64Vec3::new(2, 1, 0), hit.block);
        // and from above the water, down through it to the bottom
        let hit = raycast(Vec3::new(0.5, 3.5, 0.5), Vec3::NEG_Y, REACH, stops).unwrap();
        assert_eq!((I64Vec3::ZERO, I64Vec3::Y), (hit.block, hit.face));
    }

    #[test]
    fn test_a_ray_starting_in_a_block_hits_it_with_no_face() {
        let hit = raycast(Vec3::new(0.5, -0.5, 0.5), Vec3::X, REACH, stops).unwrap();
        assert_eq!(
            (I64Vec3::new(0, -1, 0), I64Vec3::ZERO),
            (hit.block, hit.face)
        );
    }

    #[test]
    fn test_blocks_can_go_beside_and_under_the_player_but_not_in_them() {
        let feet = Vec3::new(0.5, 1.0, 0.5);
        assert!(overlaps(BODY, feet, I64Vec3::new(0, 1, 0)));
        assert!(overlaps(BODY, feet, I64Vec3::new(0, 2, 0)));
        // under the feet, over the head and beside the body only touch it
        assert!(!overlaps(BODY, feet, I64Vec3::new(0, 0, 0)));
        assert!(!overlaps(BODY, feet, I64Vec3::new(0, 3, 0)));
        assert!(!overlaps(BODY, feet, I64Vec3::new(1, 1, 0)));
        // straddling two columns, the body is in both
        let between = Vec3::new(1.0, 1.0, 0.5);
        assert!(overlaps(BODY, between, I64Vec3::new(0, 1, 0)));
        assert!(overlaps(BODY, between, I64Vec3::new(1, 1, 0)));
    }

    mod clicks {
        use bevy::ecs::message::Messages;
        use bevy::math::I64Vec3;
        use bevy::prelude::*;

        use super::super::{use_target, Hit, Target};
        use crate::block::BlockType;
        use crate::chunks::chunk::{ChunkCoordinate, ChunkData};
        use crate::edits::EditBlock;
        use crate::game_mode::GameMode;
        use crate::hotbar::Hotbar;
        use crate::player::Player;

        // Looking down at the top of a block of sand at the origin, from `feet`.
        fn app(feet: Vec3, mode: GameMode) -> App {
            let mut world = crate::world::World::new(16);
            world.insert_chunk(ChunkCoordinate(I64Vec3::ZERO), ChunkData::default());
            world.edit(I64Vec3::ZERO, BlockType::Sand);
            let mut app = App::new();
            app.add_plugins(MinimalPlugins)
                .add_message::<EditBlock>()
                .init_resource::<ButtonInput<MouseButton>>()
                .insert_resource(world)
                .insert_resource(Target(Some(Hit {
                    block: I64Vec3::ZERO,
                    face: I64Vec3::Y,
                })))
                .add_systems(Update, use_target);
            app.world_mut()
                .spawn((Player::default(), mode, Transform::from_translation(feet)));
            app
        }

        const AWAY: Vec3 = Vec3::new(3.5, 1.0, 0.5);

        fn click(app: &mut App, button: MouseButton) -> Vec<EditBlock> {
            let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
            mouse.release_all();
            mouse.clear();
            mouse.press(button);
            app.update();
            let mut messages = app.world_mut().resource_mut::<Messages<EditBlock>>();
            messages.drain().collect()
        }

        fn hotbar(app: &mut App) -> Mut<'_, Hotbar> {
            app.world_mut()
                .query::<&mut Hotbar>()
                .single_mut(app.world_mut())
                .unwrap()
        }

        #[test]
        fn test_left_click_breaks_and_right_click_places_against_the_face() {
            let mut app = app(AWAY, GameMode::Creative);
            let at = |block, to| vec![EditBlock { block, to }];
            assert_eq!(
                at(I64Vec3::ZERO, BlockType::Air),
                click(&mut app, MouseButton::Left)
            );
            assert_eq!(
                at(I64Vec3::Y, BlockType::Stone),
                click(&mut app, MouseButton::Right)
            );
        }

        #[test]
        fn test_placing_takes_the_selected_slots_block() {
            let mut app = app(AWAY, GameMode::Creative);
            hotbar(&mut app).selected = 3;
            let to = |edits: Vec<EditBlock>| edits.iter().map(|edit| edit.to).collect::<Vec<_>>();
            assert_eq!(
                vec![BlockType::Bricks],
                to(click(&mut app, MouseButton::Right))
            );
            // and an empty slot places nothing
            hotbar(&mut app).slots[3] = None;
            assert!(click(&mut app, MouseButton::Right).is_empty());
        }

        #[test]
        fn test_nothing_is_placed_where_the_player_stands() {
            let mut app = app(Vec3::new(0.5, 1.0, 0.5), GameMode::Creative);
            assert!(click(&mut app, MouseButton::Right).is_empty());
        }

        #[test]
        fn test_middle_click_picks_the_block_into_the_selected_slot_or_selects_it() {
            let mut app = app(AWAY, GameMode::Creative);
            hotbar(&mut app).selected = 2;
            assert!(click(&mut app, MouseButton::Middle).is_empty());
            assert_eq!(Some(BlockType::Sand), hotbar(&mut app).slots[2]);
            // already in the hotbar, it's selected rather than put in a second slot
            hotbar(&mut app).selected = 6;
            click(&mut app, MouseButton::Middle);
            assert_eq!(2, hotbar(&mut app).selected);
            assert_eq!(Some(BlockType::Dirt), hotbar(&mut app).slots[6]);
        }

        #[test]
        fn test_middle_click_in_survival_only_selects() {
            let mut app = app(AWAY, GameMode::Survival);
            click(&mut app, MouseButton::Middle);
            assert_eq!(Hotbar::default(), *hotbar(&mut app));
        }
    }
}
