//! The boxes shapes are made of within their cells (`RenderStyle::is_shape`), which meshing
//! draws, collision stops bodies against and the outline and raycasts fit.

use bevy::math::Vec3;

use super::{Block, BlockType, Direction, RenderStyle};
use crate::collision::Aabb;

/// The most boxes a shape is made of: a fence's post and two rails each way.
const MOST: usize = 9;

/// A few boxes within a cell, each from its lowest corner in blocks.
#[derive(Debug, Clone, Copy, Default)]
pub struct Boxes {
    len: usize,
    boxes: [Aabb; MOST],
}

impl Boxes {
    fn push(&mut self, min: [f32; 3], max: [f32; 3]) {
        // written in sixteenths, as tiles are drawn
        self.boxes[self.len] = Aabb {
            min: Vec3::from(min) / 16.0,
            max: Vec3::from(max) / 16.0,
        };
        self.len += 1;
    }

    fn one(min: [f32; 3], max: [f32; 3]) -> Self {
        let mut boxes = Self::default();
        boxes.push(min, max);
        boxes
    }

    /// The whole cell.
    pub fn cube() -> Self {
        Self::one([0.0; 3], [16.0; 3])
    }
}

impl std::ops::Deref for Boxes {
    type Target = [Aabb];

    fn deref(&self) -> &[Aabb] {
        &self.boxes[..self.len]
    }
}

/// What a shape's boxes are for, which differ for some: fences and walls stand taller than
/// they're drawn, so they can't be jumped, and vines are walked through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    Drawing,
    Collision,
    Outline,
}

/// Whether a fence, wall or pane of `kind` joins `neighbour` beside it: one of its own style,
/// walls and panes each other, and whole solid blocks.
pub fn joins(kind: BlockType, neighbour: BlockType) -> bool {
    use RenderStyle::*;
    match (kind.style(), neighbour.style()) {
        (a, b) if a == b => true,
        (Wall, Pane) | (Pane, Wall) => true,
        (_, b) => b.is_cube() && neighbour.is_solid(),
    }
}

// A box against the side of the cell toward `side`, `thickness` sixteenths thick, filling the
// rest of the cell from `low` to `high` up it.
fn against(boxes: &mut Boxes, side: Direction, thickness: f32, low: f32, high: f32) {
    let (mut min, mut max) = ([0.0, low, 0.0], [16.0, high, 16.0]);
    let axis = side.axis();
    if side.offset()[axis] > 0 {
        min[axis] = 16.0 - thickness;
    } else {
        max[axis] = thickness;
    }
    boxes.push(min, max);
}

// A box `width` sixteenths wide from the middle of the cell out to its side toward `side`,
// from `low` to `high` up it.
fn arm(boxes: &mut Boxes, side: Direction, width: f32, low: f32, high: f32) {
    let (near, far) = (8.0 - width / 2.0, 8.0 + width / 2.0);
    let (mut min, mut max) = ([near, low, near], [far, high, far]);
    let axis = side.axis();
    if side.offset()[axis] > 0 {
        max[axis] = 16.0;
    } else {
        min[axis] = 0.0;
    }
    boxes.push(min, max);
}

const HORIZONTAL: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

/// The boxes `block` is made of for `purpose`, where `joined` says whether a fence, wall or
/// pane joins its neighbour each way round (see `joins`). Cubes are the whole cell, and what
/// isn't solid has none to collide with.
pub fn boxes(block: Block, purpose: Use, joined: impl Fn(Direction) -> bool) -> Boxes {
    let state = block.state;
    let style = block.kind.style();
    if purpose == Use::Collision && !block.kind.is_solid() {
        return Boxes::default();
    }
    // fences and walls are a jump and a half tall to bodies
    let tall = if purpose == Use::Collision {
        24.0
    } else {
        16.0
    };
    let mut boxes = Boxes::default();
    match style {
        RenderStyle::Stairs => {
            let (slab, step) = if state.top() {
                ([8.0, 16.0], [0.0, 8.0])
            } else {
                ([0.0, 8.0], [8.0, 16.0])
            };
            boxes.push([0.0, slab[0], 0.0], [16.0, slab[1], 16.0]);
            // the step's half of the cell, toward the way the stairs face
            let mut min = [0.0, step[0], 0.0];
            let mut max = [16.0, step[1], 16.0];
            let facing = state.facing();
            let axis = facing.axis();
            if facing.offset()[axis] > 0 {
                min[axis] = 8.0;
            } else {
                max[axis] = 8.0;
            }
            boxes.push(min, max);
        }
        RenderStyle::Slab if state.top() => boxes.push([0.0, 8.0, 0.0], [16.0; 3]),
        RenderStyle::Slab => boxes.push([0.0; 3], [16.0, 8.0, 16.0]),
        RenderStyle::Fence => {
            boxes.push([6.0, 0.0, 6.0], [10.0, tall, 10.0]);
            for side in HORIZONTAL.into_iter().filter(|&side| joined(side)) {
                if purpose == Use::Drawing {
                    arm(&mut boxes, side, 2.0, 6.0, 9.0);
                    arm(&mut boxes, side, 2.0, 12.0, 15.0);
                } else {
                    arm(&mut boxes, side, 4.0, 0.0, tall);
                }
            }
        }
        RenderStyle::Wall => {
            let sides = HORIZONTAL.map(&joined);
            // a wall running straight through has no post
            let straight =
                sides == [true, false, true, false] || sides == [false, true, false, true];
            let height = if purpose == Use::Collision {
                tall
            } else {
                14.0
            };
            if !straight {
                boxes.push([4.0, 0.0, 4.0], [12.0, tall, 12.0]);
            }
            for side in HORIZONTAL.into_iter().filter(|&side| joined(side)) {
                arm(&mut boxes, side, 6.0, 0.0, height);
            }
        }
        RenderStyle::Pane => {
            boxes.push([7.0, 0.0, 7.0], [9.0, 16.0, 9.0]);
            for side in HORIZONTAL.into_iter().filter(|&side| joined(side)) {
                arm(&mut boxes, side, 2.0, 0.0, 16.0);
            }
        }
        RenderStyle::Door => {
            let facing = state.facing();
            let side = match (state.open(), state.hinge_right()) {
                (false, _) => facing.opposite(),
                (true, true) => facing.turned(1),
                (true, false) => facing.turned(3),
            };
            against(&mut boxes, side, 3.0, 0.0, 16.0);
        }
        RenderStyle::Trapdoor if state.open() => {
            against(&mut boxes, state.facing().opposite(), 3.0, 0.0, 16.0);
        }
        RenderStyle::Trapdoor if state.top() => boxes.push([0.0, 13.0, 0.0], [16.0; 3]),
        RenderStyle::Trapdoor => boxes.push([0.0; 3], [16.0, 3.0, 16.0]),
        RenderStyle::Ladder | RenderStyle::Vine => {
            let thickness = if style == RenderStyle::Ladder {
                3.0
            } else {
                1.0
            };
            against(&mut boxes, state.facing().opposite(), thickness, 0.0, 16.0);
        }
        RenderStyle::Lantern => {
            let lift = if state.top() { 1.0 } else { 0.0 };
            boxes.push([5.0, lift, 5.0], [11.0, lift + 7.0, 11.0]);
            boxes.push([6.0, lift + 7.0, 6.0], [10.0, lift + 9.0, 10.0]);
        }
        _ => return Boxes::cube(),
    }
    boxes
}

/// Whether `block`'s shape fills the whole of its cell's face toward `side`, hiding the face
/// of the block against it there.
pub fn fills_face(block: Block, side: Direction) -> bool {
    let state = block.state;
    match block.kind.style() {
        RenderStyle::Slab => {
            side == if state.top() {
                Direction::Up
            } else {
                Direction::Down
            }
        }
        RenderStyle::Stairs => {
            side == state.facing()
                || side
                    == if state.top() {
                        Direction::Up
                    } else {
                        Direction::Down
                    }
        }
        _ => block.kind.is_opaque(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::BlockState;

    fn block(kind: BlockType, state: BlockState) -> Block {
        Block::new(kind, state).unwrap()
    }

    fn volume(boxes: &Boxes) -> f32 {
        boxes
            .iter()
            .map(|b| (b.max - b.min).element_product())
            .sum()
    }

    #[test]
    fn test_stairs_are_a_slab_and_a_step_toward_the_way_they_face() {
        let east = BlockState::default().with_facing(Direction::East);
        let stairs = boxes(block(BlockType::PlanksStairs, east), Use::Collision, |_| {
            false
        });
        assert_eq!(0.75, volume(&stairs));
        let step = stairs[1];
        assert_eq!(Vec3::new(0.5, 0.5, 0.0), step.min);
        assert_eq!(Vec3::ONE, step.max);
        // turned over, the step hangs under the slab
        let over = boxes(
            block(BlockType::PlanksStairs, east.with_top(true)),
            Use::Drawing,
            |_| false,
        );
        assert_eq!(0.5, over[0].min.y);
        assert_eq!(0.0, over[1].min.y);
        assert!(fills_face(
            block(BlockType::PlanksStairs, east),
            Direction::East
        ));
        assert!(fills_face(
            block(BlockType::PlanksStairs, east),
            Direction::Down
        ));
        assert!(!fills_face(
            block(BlockType::PlanksStairs, east),
            Direction::Up
        ));
        assert!(!fills_face(
            block(BlockType::PlanksStairs, east),
            Direction::West
        ));
    }

    #[test]
    fn test_a_slab_fills_only_its_own_half_and_hides_only_that_face() {
        let bottom = block(BlockType::StoneBrickSlab, BlockState::default());
        let top = block(
            BlockType::StoneBrickSlab,
            BlockState::default().with_top(true),
        );
        assert_eq!(0.5, boxes(bottom, Use::Drawing, |_| false)[0].max.y);
        assert_eq!(0.5, boxes(top, Use::Drawing, |_| false)[0].min.y);
        assert!(fills_face(bottom, Direction::Down) && !fills_face(bottom, Direction::Up));
        assert!(fills_face(top, Direction::Up) && !fills_face(top, Direction::Down));
        assert!(fills_face(Block::from(BlockType::Stone), Direction::Up));
        assert!(!fills_face(Block::from(BlockType::Glass), Direction::Up));
    }

    #[test]
    fn test_fences_join_fences_and_solid_blocks_and_stand_too_tall_to_jump() {
        let fence = BlockType::PlanksFence;
        assert!(joins(fence, BlockType::DarkPlanksFence) && joins(fence, BlockType::PlanksFence));
        assert!(joins(fence, BlockType::Stone) && joins(fence, BlockType::Log));
        assert!(!joins(fence, BlockType::Air) && !joins(fence, BlockType::TallGrass));
        assert!(!joins(fence, BlockType::CobblestoneWall));
        assert!(joins(BlockType::GlassPane, BlockType::StoneBrickWall));
        assert!(joins(BlockType::StoneBrickWall, BlockType::GlassPane));
        let alone = boxes(Block::from(fence), Use::Collision, |_| false);
        assert_eq!(1, alone.len());
        assert_eq!(1.5, alone[0].max.y);
        let east = boxes(Block::from(fence), Use::Drawing, |d| d == Direction::East);
        // a post and two rails, drawn a block tall
        assert_eq!(3, east.len());
        assert!(east.iter().all(|b| b.max.y <= 1.0));
        assert_eq!(1.0, east[1].max.x);
    }

    #[test]
    fn test_a_straight_wall_has_no_post() {
        let wall = Block::from(BlockType::CobblestoneWall);
        let through = |d| matches!(d, Direction::North | Direction::South);
        assert_eq!(2, boxes(wall, Use::Drawing, through).len());
        let corner = |d| matches!(d, Direction::North | Direction::East);
        assert_eq!(3, boxes(wall, Use::Drawing, corner).len());
    }

    #[test]
    fn test_doors_shut_across_their_cell_and_open_against_their_hinge() {
        let north = BlockState::default().with_facing(Direction::North);
        let shut = boxes(block(BlockType::Door, north), Use::Collision, |_| false)[0];
        // against the south side, three sixteenths thick
        assert_eq!((13.0 / 16.0, 1.0), (shut.min.z, shut.max.z));
        let open = boxes(
            block(BlockType::Door, north.with_open(true)),
            Use::Collision,
            |_| false,
        )[0];
        assert_eq!((0.0, 3.0 / 16.0), (open.min.x, open.max.x));
        let right = north.with_open(true).with_hinge_right(true);
        let open = boxes(block(BlockType::Door, right), Use::Collision, |_| false)[0];
        assert_eq!((13.0 / 16.0, 1.0), (open.min.x, open.max.x));
    }

    #[test]
    fn test_vines_are_walked_through_and_cubes_fill_their_cell() {
        let vines = block(BlockType::Vines, BlockState::default());
        assert!(boxes(vines, Use::Collision, |_| false).is_empty());
        assert_eq!(1, boxes(vines, Use::Outline, |_| false).len());
        assert_eq!(
            1.0,
            volume(&boxes(
                Block::from(BlockType::Stone),
                Use::Collision,
                |_| false
            ))
        );
        assert_eq!(
            1.0,
            volume(&boxes(
                Block::from(BlockType::TallGrass),
                Use::Outline,
                |_| false
            ))
        );
        assert!(boxes(Block::from(BlockType::TallGrass), Use::Collision, |_| false).is_empty());
    }
}
