//! The rules of placing and using blocks with states (§5.1): which state a block's placed in,
//! from the way the player looks and where they click, what holds it up, and doors and
//! trapdoors opening. The client places by them and the server checks by them.

use bevy::math::{I64Vec3, Vec3};

use crate::block::{Block, BlockState, BlockType, Direction, Property, RenderStyle};

/// The block of `kind` a player looking along `forward` places by clicking the face out along
/// `face`, at `point` within the block clicked (0 to 1 along each axis): `face` is zero if
/// it's placed in place of a replaceable block. Stairs face the way the player looks and slabs
/// and trapdoors take the half clicked, both turned over against a ceiling; logs lie along the
/// face's axis and barrels face out of it; furnaces face the player; ladders and vines go
/// against the face, and doors turn on the side of their middle clicked.
pub fn placed(kind: BlockType, face: I64Vec3, point: Vec3, forward: Vec3) -> Block {
    let out = Direction::of_offset(face);
    let looking = Direction::horizontal(forward);
    // the top half against a ceiling, or the top of a side
    let top = match out {
        Some(Direction::Up) => false,
        Some(Direction::Down) => true,
        _ => point.y > 0.5,
    };
    let state = BlockState::default();
    let properties = kind.state_properties();
    let state = match kind.style() {
        RenderStyle::Pillar => state.with_axis(out.map_or(1, Direction::axis)),
        RenderStyle::Lidded => state.with_facing(out.unwrap_or(Direction::Up)),
        RenderStyle::Fronted(_) => state.with_facing(looking.opposite()),
        RenderStyle::Stairs | RenderStyle::Slab => state.with_facing(looking).with_top(top),
        RenderStyle::Ladder | RenderStyle::Vine | RenderStyle::Trapdoor => {
            // against the wall clicked, or from the side the player's looking at
            let facing = out.filter(|d| d.axis() != 1).unwrap_or(looking.opposite());
            state.with_facing(facing).with_top(top)
        }
        RenderStyle::Door => {
            let right = looking.turned(1);
            let across = point[right.axis()];
            let clicked_right = if right.offset()[right.axis()] > 0 {
                across > 0.5
            } else {
                across < 0.5
            };
            state.with_facing(looking).with_hinge_right(clicked_right)
        }
        RenderStyle::Lantern => state.with_top(out == Some(Direction::Down)),
        _ => state,
    };
    // each style takes only its own properties from that, as slabs face no way
    let mut kept = BlockState::default();
    for &property in properties {
        kept = kept.with(property, state.get(property));
    }
    Block::new(kind, kept).expect("a placed block's state is its own")
}

/// The other block of a door, two blocks tall, and where it is, if `block` at `at` is one.
pub fn other_half(at: I64Vec3, block: Block) -> Option<(I64Vec3, Block)> {
    if block.kind.style() != RenderStyle::Door {
        return None;
    }
    let top = block.state.top();
    let other = if top {
        at - I64Vec3::Y
    } else {
        at + I64Vec3::Y
    };
    let state = block.state.with_top(!top);
    Some((other, Block { state, ..block }))
}

/// `block` opened if it's shut, or shut if it's open, if it's a door or a trapdoor, which a
/// right click does.
pub fn toggled(block: Block) -> Option<Block> {
    block
        .kind
        .state_properties()
        .contains(&Property::Open)
        .then(|| Block {
            state: block.state.with_open(!block.state.open()),
            ..block
        })
}

/// Whether `block` needs something to hold it up: plants and torches the ground, ladders and
/// vines a wall, doors the ground, and lanterns the floor or ceiling.
pub fn needs_support(kind: BlockType) -> bool {
    kind.needs_ground()
        || matches!(
            kind.style(),
            RenderStyle::Ladder | RenderStyle::Vine | RenderStyle::Door | RenderStyle::Lantern
        )
}

/// Whether `block` at `at` is held up, with `get` the block at a position: what stands on the
/// ground has ground it stands on, what hangs on a wall a whole solid block behind it (or for
/// vines, vines over it), a door's upper block its lower one, and a lantern the block it
/// stands on or hangs from.
pub fn supported(at: I64Vec3, block: Block, get: impl Fn(I64Vec3) -> Block) -> bool {
    let solid = |at: I64Vec3| {
        let block = get(at).kind;
        block.style().is_cube() && block.is_solid()
    };
    let (above, below) = (at + I64Vec3::Y, at - I64Vec3::Y);
    let state = block.state;
    match block.kind.style() {
        RenderStyle::Ladder | RenderStyle::Vine => {
            let behind = at + state.facing().opposite().offset().as_i64vec3();
            solid(behind)
                || (block.kind.style() == RenderStyle::Vine && get(above).kind == block.kind)
        }
        RenderStyle::Door if state.top() => {
            let lower = get(below);
            lower.kind == block.kind && !lower.state.top()
        }
        RenderStyle::Door => get(below).kind.is_solid(),
        RenderStyle::Lantern if state.top() => get(above).kind.is_solid(),
        RenderStyle::Lantern => get(below).kind.is_solid(),
        _ if block.kind.needs_ground() => block.kind.stands_on(get(below).kind),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NORTH: Vec3 = Vec3::NEG_Z;
    const MIDDLE: Vec3 = Vec3::splat(0.5);

    fn text(block: Block) -> String {
        block.to_string()
    }

    #[test]
    fn test_stairs_face_the_way_the_player_looks_and_turn_over_against_a_ceiling() {
        let stairs = BlockType::StoneBrickStairs;
        let on_floor = placed(stairs, I64Vec3::Y, MIDDLE, Vec3::X);
        assert_eq!("stone_brick_stairs[facing=east]", text(on_floor));
        let on_ceiling = placed(stairs, I64Vec3::NEG_Y, MIDDLE, NORTH);
        assert_eq!("stone_brick_stairs[half=top]", text(on_ceiling));
        // against a wall, by the half of it clicked
        let high = placed(stairs, I64Vec3::Z, Vec3::new(0.5, 0.8, 1.0), NORTH);
        let low = placed(stairs, I64Vec3::Z, Vec3::new(0.5, 0.2, 1.0), NORTH);
        assert!(high.state.top() && !low.state.top());
    }

    #[test]
    fn test_slabs_take_the_half_clicked_and_face_no_way() {
        let slab = BlockType::PlanksSlab;
        assert_eq!(
            "planks_slab",
            text(placed(slab, I64Vec3::Y, MIDDLE, Vec3::X))
        );
        assert_eq!(
            "planks_slab[half=top]",
            text(placed(slab, I64Vec3::NEG_Y, MIDDLE, Vec3::X))
        );
        let side = placed(slab, I64Vec3::X, Vec3::new(1.0, 0.7, 0.5), NORTH);
        assert_eq!("planks_slab[half=top]", text(side));
    }

    #[test]
    fn test_logs_lie_along_the_face_clicked_and_furnaces_face_the_player() {
        assert_eq!(
            "log",
            text(placed(BlockType::Log, I64Vec3::Y, MIDDLE, NORTH))
        );
        assert_eq!(
            "log[axis=x]",
            text(placed(BlockType::Log, I64Vec3::NEG_X, MIDDLE, NORTH))
        );
        assert_eq!(
            "log[axis=z]",
            text(placed(BlockType::Log, I64Vec3::Z, MIDDLE, NORTH))
        );
        let furnace = placed(BlockType::Furnace, I64Vec3::Y, MIDDLE, NORTH);
        assert_eq!("furnace[facing=south]", text(furnace));
        let barrel = placed(BlockType::Barrel, I64Vec3::X, MIDDLE, NORTH);
        assert_eq!("barrel[facing=east]", text(barrel));
        assert_eq!(
            "barrel[facing=up]",
            text(placed(BlockType::Barrel, I64Vec3::Y, MIDDLE, NORTH))
        );
    }

    #[test]
    fn test_ladders_go_against_the_wall_clicked_and_lanterns_hang_from_ceilings() {
        let ladder = placed(BlockType::Ladder, I64Vec3::Z, MIDDLE, NORTH);
        assert_eq!("ladder[facing=south]", text(ladder));
        let wall = I64Vec3::new(0, 5, 0);
        let at = wall + I64Vec3::Z;
        let stone = |p: I64Vec3| {
            Block::from(if p == wall {
                BlockType::Stone
            } else {
                BlockType::Air
            })
        };
        assert!(supported(at, ladder, stone));
        assert!(!supported(
            at,
            placed(BlockType::Ladder, I64Vec3::X, MIDDLE, NORTH),
            stone
        ));
        let hanging = placed(BlockType::Lantern, I64Vec3::NEG_Y, MIDDLE, NORTH);
        assert_eq!("lantern[hanging=true]", text(hanging));
        assert!(supported(wall - I64Vec3::Y, hanging, stone));
        assert!(!supported(wall + I64Vec3::Y, hanging, stone));
    }

    #[test]
    fn test_doors_have_two_halves_that_open_and_shut_together() {
        let door = placed(BlockType::Door, I64Vec3::Y, Vec3::new(0.8, 1.0, 0.5), NORTH);
        // looking north, east is to the right
        assert_eq!("door[hinge=right]", text(door));
        let left = placed(BlockType::Door, I64Vec3::Y, Vec3::new(0.2, 1.0, 0.5), NORTH);
        assert!(!left.state.hinge_right());
        let at = I64Vec3::new(3, 10, 3);
        let (above, upper) = other_half(at, door).unwrap();
        assert_eq!(at + I64Vec3::Y, above);
        assert!(upper.state.top());
        assert_eq!(Some((at, door)), other_half(above, upper));
        let open = toggled(door).unwrap();
        assert!(open.state.open());
        assert_eq!(Some(door), toggled(open));
        assert_eq!(None, toggled(Block::from(BlockType::Stone)));
        assert!(toggled(Block::from(BlockType::Trapdoor)).is_some());
    }
}
