use bevy::math::{BVec3, I64Vec3, Vec3};

use crate::block::shape::{self, Boxes, Use};
use crate::block::Block;
use crate::chunks::chunk::ChunkCoordinate;
use crate::player::{PlayerLook, EYE_HEIGHT, REACH};
use crate::world::World;

// Overlaps thinner than this don't count, so a box resting on a face, give or take rounding,
// isn't taken to be inside the block under it and fall through.
const EPSILON: f32 = 1e-3;

/// A block a ray stopped at, the face it came in through, as the direction out of that face,
/// and where on it the ray met it. The face is zero if the ray started inside the block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub block: I64Vec3,
    pub face: I64Vec3,
    pub point: Vec3,
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
    let mut entered = 0.0;
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
            let point = origin + direction * entered;
            return Some(Hit { block, face, point });
        }
        let axis = next.min_position();
        if next[axis] > reach {
            return None;
        }
        entered = next[axis];
        block[axis] += step[axis];
        face = I64Vec3::ZERO;
        face[axis] = -step[axis];
        next[axis] += across[axis];
    }
}

/// The block in reach that a player with their feet at `feet` looks at, if there's one, and
/// the face and point of its shape the look meets, so a slab's top is looked at on its top.
pub fn looked_at(world: &World, feet: Vec3, look: PlayerLook) -> Option<Hit> {
    let eye = feet + Vec3::Y * EYE_HEIGHT;
    let forward = look.forward();
    let get = |at| world.get(at);
    // where the look first meets one of the block's boxes, and through which face
    let meets = |block: I64Vec3| {
        let corner = block.as_vec3();
        boxes_at(block, Use::Outline, get)
            .iter()
            .filter_map(|b| ray_box(eye, forward, corner + b.min, corner + b.max))
            .min_by(|a, b| a.0.total_cmp(&b.0))
    };
    // blocks not generated yet can't be reached, nor can air and water, which are looked through
    let stops = |block: I64Vec3| {
        world.is_chunk_generated(ChunkCoordinate::containing(block))
            && world.get_block_at(block).is_tangible()
            && meets(block).is_some()
    };
    let hit = raycast(eye, forward, REACH, stops)?;
    let (distance, face) = meets(hit.block)?;
    (distance <= REACH).then(|| Hit {
        face,
        point: eye + forward.normalize_or_zero() * distance,
        ..hit
    })
}

// How far along the ray from `origin` along `direction` it first meets the box from `min` to
// `max`, and the face it meets, as the direction out of it, zero if it starts inside.
fn ray_box(origin: Vec3, direction: Vec3, min: Vec3, max: Vec3) -> Option<(f32, I64Vec3)> {
    let enter = ray_distance(
        origin,
        direction,
        min,
        Aabb {
            min: Vec3::ZERO,
            max: max - min,
        },
    )?;
    if enter == 0.0 {
        return Some((0.0, I64Vec3::ZERO));
    }
    // the face is the one whose plane the ray crosses last on its way in
    let direction = direction.normalize_or_zero();
    let at = origin + direction * enter;
    let axis = (0..3)
        .min_by(|&a, &b| {
            let off = |axis: usize| {
                (at[axis] - min[axis])
                    .abs()
                    .min((at[axis] - max[axis]).abs())
            };
            off(a).total_cmp(&off(b))
        })
        .unwrap();
    let mut face = I64Vec3::ZERO;
    face[axis] = if direction[axis] > 0.0 { -1 } else { 1 };
    Some((enter, face))
}

/// The boxes of the block at `at` for `purpose`, with `get` the block at a position, which
/// fences, walls and panes join their neighbours by.
pub fn boxes_at(at: I64Vec3, purpose: Use, get: impl Fn(I64Vec3) -> Block) -> Boxes {
    let block = get(at);
    shape::boxes(block, purpose, |side| {
        shape::joins(block.kind, get(at + side.offset().as_i64vec3()).kind)
    })
}

/// An axis-aligned box, as offsets from the position it's moved by.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

/// Moves `aabb` at `position` by `delta` an axis at a time, up first, stopping each axis
/// against the first of the boxes that `boxes` says each block is made of (`block::shape`)
/// it would move into. Returns where it ends up, and which axes were stopped. Boxes it already
/// overlaps are ignored, so it can always get out of them.
pub fn move_and_collide(
    position: Vec3,
    delta: Vec3,
    aabb: Aabb,
    boxes: impl Fn(I64Vec3) -> Boxes,
) -> (Vec3, BVec3) {
    let mut position = position;
    let mut hit = BVec3::FALSE;
    for axis in [1, 0, 2] {
        let (moved, stopped) = sweep_axis(position, aabb, axis, delta[axis], &boxes);
        position[axis] += moved;
        hit.set(axis, stopped);
    }
    (position, hit)
}

// How far the box gets along `axis` towards `delta`, and whether a block stopped it.
fn sweep_axis(
    position: Vec3,
    aabb: Aabb,
    axis: usize,
    delta: f32,
    boxes: &impl Fn(I64Vec3) -> Boxes,
) -> (f32, bool) {
    if delta == 0.0 {
        return (0.0, false);
    }
    let min = position + aabb.min;
    let max = position + aabb.max;
    // the cells the box passes through, and those under them, whose fences and walls stand up
    // into them
    let mut from = (min + EPSILON).floor().as_i64vec3();
    let mut to = (max - EPSILON).floor().as_i64vec3();
    if delta > 0.0 {
        to[axis] = (max[axis] + delta - EPSILON).floor() as i64;
    } else {
        from[axis] = (min[axis] + delta + EPSILON).floor() as i64;
    }
    from.y -= 1;
    let [a, b] = [(axis + 1) % 3, (axis + 2) % 3];
    let mut allowed = delta.abs();
    for x in from.x..=to.x {
        for y in from.y..=to.y {
            for z in from.z..=to.z {
                let cell = I64Vec3::new(x, y, z);
                for part in boxes(cell).iter() {
                    let (low, high) = (cell.as_vec3() + part.min, cell.as_vec3() + part.max);
                    let across = |i: usize| low[i] < max[i] - EPSILON && high[i] > min[i] + EPSILON;
                    if !(across(a) && across(b)) {
                        continue;
                    }
                    // only boxes ahead of the moving face, not those it's in already
                    let gap = if delta > 0.0 {
                        low[axis] - max[axis]
                    } else {
                        min[axis] - high[axis]
                    };
                    if gap > -EPSILON && gap < allowed {
                        allowed = gap.max(0.0);
                    }
                }
            }
        }
    }
    if allowed < delta.abs() {
        (allowed * delta.signum(), true)
    } else {
        (delta, false)
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

/// How far along the ray from `origin` along `direction` it first meets `aabb` at `position`,
/// if it does ahead of `origin`, or 0 if `origin`'s inside it.
pub fn ray_distance(origin: Vec3, direction: Vec3, position: Vec3, aabb: Aabb) -> Option<f32> {
    let direction = direction.normalize_or_zero();
    let inverse = direction.recip();
    let to_min = (position + aabb.min - origin) * inverse;
    let to_max = (position + aabb.max - origin) * inverse;
    // a ray along a face's plane makes 0 * inf, which is no limit on that axis
    let near = to_min
        .min(to_max)
        .to_array()
        .map(|t| if t.is_nan() { f32::NEG_INFINITY } else { t });
    let far = to_min
        .max(to_max)
        .to_array()
        .map(|t| if t.is_nan() { f32::INFINITY } else { t });
    let (enter, leave) = (
        near.into_iter().fold(0.0, f32::max),
        far.into_iter().fold(f32::INFINITY, f32::min),
    );
    (enter <= leave).then_some(enter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a_ray_meets_a_box_at_its_near_face() {
        let at = Vec3::new(5.0, 0.0, 0.0);
        assert_eq!(
            Some(4.7),
            ray_distance(Vec3::new(0.0, 1.0, 0.0), Vec3::X, at, BOX)
        );
        // passing over it, and pointing away
        assert_eq!(
            None,
            ray_distance(Vec3::new(0.0, 2.5, 0.0), Vec3::X, at, BOX)
        );
        assert_eq!(
            None,
            ray_distance(Vec3::new(0.0, 1.0, 0.0), Vec3::NEG_X, at, BOX)
        );
        // from above at a slant onto its top
        let distance = ray_distance(
            Vec3::new(5.0, 3.8, -2.0),
            Vec3::new(0.0, -1.0, 1.0),
            at,
            BOX,
        );
        assert!(
            (distance.unwrap() - 2.0 * 2f32.sqrt()).abs() < 1e-4,
            "{distance:?}"
        );
    }

    const BOX: Aabb = Aabb {
        min: Vec3::new(-0.3, 0.0, -0.3),
        max: Vec3::new(0.3, 1.8, 0.3),
    };

    // solid at and below y = 0
    fn floor(cell: I64Vec3) -> Boxes {
        solid(cell.y < 0)
    }

    fn solid(solid: bool) -> Boxes {
        if solid {
            Boxes::cube()
        } else {
            Boxes::default()
        }
    }

    #[test]
    fn test_falls_onto_the_floor_and_stops_on_it() {
        let (position, hit) =
            move_and_collide(Vec3::new(0.5, 3.0, 0.5), -Vec3::Y * 10.0, BOX, floor);
        assert_eq!(position, Vec3::new(0.5, 0.0, 0.5));
        assert!(hit.y && !hit.x && !hit.z);
    }

    #[test]
    fn test_falls_freely_where_nothing_is_solid() {
        let (position, hit) = move_and_collide(Vec3::new(0.5, 3.0, 0.5), -Vec3::Y, BOX, floor);
        assert_eq!(position, Vec3::new(0.5, 2.0, 0.5));
        assert_eq!(hit, BVec3::FALSE);
    }

    #[test]
    fn test_rests_on_the_floor_over_many_small_steps() {
        let mut position = Vec3::new(0.5, 0.0, 0.5);
        for _ in 0..1000 {
            position = move_and_collide(position, Vec3::new(0.001, -0.016, 0.0), BOX, floor).0;
        }
        assert_eq!(position.y, 0.0);
        assert!((position.x - 1.5).abs() < 1e-3);
    }

    #[test]
    fn test_slides_along_a_wall() {
        let wall = |cell: I64Vec3| solid(cell.y < 0 || cell.x >= 2);
        let (position, hit) = move_and_collide(
            Vec3::new(0.5, 0.0, 0.5),
            Vec3::new(5.0, 0.0, 3.0),
            BOX,
            wall,
        );
        assert!((position.x - 1.7).abs() < 1e-5);
        assert_eq!(position.z, 3.5);
        assert!(hit.x && !hit.z);
    }

    #[test]
    fn test_stops_under_a_ceiling() {
        let ceiling = |cell: I64Vec3| solid(cell.y >= 3);
        let (position, hit) =
            move_and_collide(Vec3::new(0.5, 0.0, 0.5), Vec3::Y * 5.0, BOX, ceiling);
        assert!((position.y - 1.2).abs() < 1e-5);
        assert!(hit.y);
    }

    #[test]
    fn test_moves_out_of_a_block_it_starts_inside() {
        let one_block = |cell: I64Vec3| solid(cell == I64Vec3::new(0, 1, 0));
        let (position, hit) =
            move_and_collide(Vec3::new(0.5, 0.5, 0.5), Vec3::X * 2.0, BOX, one_block);
        assert_eq!(position, Vec3::new(2.5, 0.5, 0.5));
        assert_eq!(hit, BVec3::FALSE);
    }

    #[test]
    fn test_doesnt_tunnel_through_a_thin_wall_in_one_big_step() {
        let wall = |cell: I64Vec3| solid(cell.x == 10);
        let (position, hit) =
            move_and_collide(Vec3::new(0.5, 0.0, 0.5), Vec3::X * 100.0, BOX, wall);
        assert!((position.x - 9.7).abs() < 1e-5);
        assert!(hit.x);
    }

    #[test]
    fn test_fits_through_a_gap_its_own_width() {
        // walls at x = -1 and x = 1, so the one-block gap at x = 0 is open
        let walls = |cell: I64Vec3| solid(cell.x != 0);
        let (position, hit) = move_and_collide(Vec3::new(0.5, 0.0, 0.5), Vec3::Z * 4.0, BOX, walls);
        assert_eq!(position.z, 4.5);
        assert!(!hit.z);
    }

    #[test]
    fn test_box_as_tall_as_a_gap_walks_through_it() {
        let tall = Aabb {
            min: BOX.min,
            max: Vec3::new(0.3, 2.0, 0.3),
        };
        let tunnel = |cell: I64Vec3| solid(cell.y < 0 || cell.y >= 2);
        let (position, hit) =
            move_and_collide(Vec3::new(0.5, 0.0, 0.5), Vec3::Z * 4.0, tall, tunnel);
        assert_eq!(position, Vec3::new(0.5, 0.0, 4.5));
        assert_eq!(hit, BVec3::FALSE);
    }
}
