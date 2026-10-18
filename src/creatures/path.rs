//! Finding a way across the blocks for a creature (§5.8): A* over the cells its feet can be in,
//! standing on something solid with room for its body over it, stepping up a block at a time
//! and down as far as it'll drop, kept within a reach and a number of cells looked at so a
//! search that can't succeed gives up soon. A search reads its blocks from a `Blocks`, a copy
//! of the chunks round it, so it runs on the compute pool rather than the main thread (INV-7).

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::Arc;

use bevy::math::{I64Vec3, Vec3Swizzles};
use bevy::platform::collections::HashMap;

use crate::block::BlockType;
use crate::chunks::chunk::{linear_index, ChunkCoordinate, ChunkData, CELLS};
use crate::world::World;

// What a step costs, in tenths of a block walked: across, diagonally, up a block, down a block,
// and into water, which a swimmer goes round if it can.
const STRAIGHT: u32 = 10;
const DIAGONAL: u32 = 14;
const UP: u32 = 10;
const DOWN: u32 = 2;
const WATER: u32 = 20;

/// What a path must fit: a body so many cells across (an odd number, round its middle) and so
/// many tall, how far it'll drop down in one go, and whether it'll swim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Walker {
    pub across: i64,
    pub tall: i64,
    pub drop: i64,
    pub swims: bool,
}

impl Walker {
    /// A walker for a body `width` wide and `height` tall, which drops `drop` blocks at most.
    pub fn sized(width: f32, height: f32, drop: i64, swims: bool) -> Self {
        Self {
            across: (width.ceil() as i64 / 2) * 2 + 1,
            tall: height.ceil() as i64,
            drop,
            swims,
        }
    }
}

/// How far a search goes: the cells it looks at, at most, and how far from where it starts,
/// across and up or down, it looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub cells: usize,
    pub reach: i64,
}

/// A way found: the cells to walk through in turn, the start left out, and whether it gets
/// where it was asked to, or only as near as the search could get.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path {
    pub cells: Vec<I64Vec3>,
    pub arrives: bool,
}

/// The way for `walker` from the cell its feet are in, `from`, to within `near` blocks across
/// (and one up or down) of `to`, the cheapest within `limits`, or else the way to wherever it
/// got nearest, if that's anywhere but where it started. `block` is the block at a position, or
/// none where it isn't known, which is taken as solid.
pub fn find_path(
    from: I64Vec3,
    to: I64Vec3,
    near: i64,
    walker: Walker,
    limits: Limits,
    mut block: impl FnMut(I64Vec3) -> Option<BlockType>,
) -> Option<Path> {
    let mut cells = Cells {
        walker,
        block: &mut block,
    };
    let arrived =
        |cell: I64Vec3| (cell - to).xz().abs().max_element() <= near && (cell.y - to.y).abs() <= 1;
    let estimate = |cell: I64Vec3| {
        let d = (cell - to).abs();
        let (long, short) = (d.x.max(d.z), d.x.min(d.z));
        (short * DIAGONAL as i64 + (long - short) * STRAIGHT as i64 + d.y * STRAIGHT as i64) as u32
    };
    // each cell reached, by what it cost to get there and where from
    let mut reached: HashMap<I64Vec3, (u32, I64Vec3)> = HashMap::default();
    reached.insert(from, (0, from));
    let mut open = BinaryHeap::new();
    open.push(Reverse((estimate(from), 0, from.to_array())));
    let (mut best, mut best_estimate) = (from, estimate(from));
    let mut looked = 0;
    let mut end = None;
    while let Some(Reverse((_, cost, cell))) = open.pop() {
        let cell = I64Vec3::from_array(cell);
        if reached[&cell].0 < cost {
            continue;
        }
        if arrived(cell) {
            end = Some(cell);
            break;
        }
        looked += 1;
        if looked > limits.cells {
            break;
        }
        for (next, step) in cells.steps(cell) {
            let off = next - from;
            if off.xz().abs().max_element() > limits.reach || off.y.abs() > limits.reach {
                continue;
            }
            let cost = cost + step;
            if reached.get(&next).is_some_and(|&(was, _)| was <= cost) {
                continue;
            }
            reached.insert(next, (cost, cell));
            let left = estimate(next);
            if left < best_estimate {
                (best, best_estimate) = (next, left);
            }
            open.push(Reverse((cost + left, cost, next.to_array())));
        }
    }
    let arrives = end.is_some();
    let mut cell = end.unwrap_or(best);
    if cell == from {
        return None;
    }
    let mut path = vec![cell];
    while let Some(&(_, back)) = reached.get(&cell).filter(|&&(_, back)| back != from) {
        path.push(back);
        cell = back;
    }
    path.reverse();
    Some(Path {
        cells: path,
        arrives,
    })
}

// The cells a walker can be in, by the blocks round them.
struct Cells<'a, F> {
    walker: Walker,
    block: &'a mut F,
}

impl<F: FnMut(I64Vec3) -> Option<BlockType>> Cells<'_, F> {
    // Whether its body fits with its feet in `cell`: nothing solid, or unknown, in the way.
    fn fits(&mut self, cell: I64Vec3) -> bool {
        let half = self.walker.across / 2;
        (0..self.walker.tall).all(|y| {
            (-half..=half).all(|x| {
                (-half..=half).all(|z| {
                    (self.block)(cell + I64Vec3::new(x, y, z)).is_some_and(|b| !b.is_solid())
                })
            })
        })
    }

    // What holds it up with its feet in `cell`, which it fits in: solid ground under some of
    // it, or water it floats in, if it swims, or nothing.
    fn footing(&mut self, cell: I64Vec3) -> Option<Footing> {
        let half = self.walker.across / 2;
        let water = (self.block)(cell) == Some(BlockType::Water);
        if water {
            return self.walker.swims.then_some(Footing::Water);
        }
        let ground = (-half..=half).any(|x| {
            (-half..=half)
                .any(|z| (self.block)(cell + I64Vec3::new(x, -1, z)).is_some_and(|b| b.is_solid()))
        });
        ground.then_some(Footing::Ground)
    }

    // Where it can go from `cell` in one step, and what each costs: a block to each side, up
    // one, or down as far as it drops, and diagonally on the level where nothing's in the way.
    fn steps(&mut self, cell: I64Vec3) -> Vec<(I64Vec3, u32)> {
        let mut steps = Vec::with_capacity(8);
        let room_to_jump = self.fits(cell + I64Vec3::Y);
        let sides = [I64Vec3::X, I64Vec3::NEG_X, I64Vec3::Z, I64Vec3::NEG_Z];
        let mut open = [false; 4];
        for (i, side) in sides.into_iter().enumerate() {
            let next = cell + side;
            if self.fits(next) {
                open[i] = true;
                for down in 0..=self.walker.drop {
                    let below = next - I64Vec3::Y * down;
                    if down > 0 && !self.fits(below) {
                        break;
                    }
                    if let Some(footing) = self.footing(below) {
                        steps.push((below, STRAIGHT + DOWN * down as u32 + footing.cost()));
                        break;
                    }
                }
            } else if room_to_jump && self.fits(next + I64Vec3::Y) {
                if let Some(footing) = self.footing(next + I64Vec3::Y) {
                    steps.push((next + I64Vec3::Y, STRAIGHT + UP + footing.cost()));
                }
            }
        }
        for (a, b) in [(0, 2), (0, 3), (1, 2), (1, 3)] {
            let next = cell + sides[a] + sides[b];
            if open[a] && open[b] && self.fits(next) {
                if let Some(footing) = self.footing(next) {
                    steps.push((next, DIAGONAL + footing.cost()));
                }
            }
        }
        steps
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Footing {
    Ground,
    Water,
}

impl Footing {
    fn cost(self) -> u32 {
        match self {
            Footing::Ground => 0,
            Footing::Water => WATER,
        }
    }
}

/// A copy of the chunks round a place, to find a way through apart from the world: each chunk
/// shared with the world as it was, and unpacked into a grid the first time a search reads it,
/// as reading a chunk's runs block by block is slow.
pub struct Blocks {
    chunks: HashMap<ChunkCoordinate, Arc<ChunkData>>,
    unpacked: HashMap<ChunkCoordinate, Option<Box<[BlockType; CELLS]>>>,
}

impl Blocks {
    /// The chunks of `world` within `reach` blocks of `at` across and up or down, those that
    /// are generated.
    pub fn around(world: &World, at: I64Vec3, reach: i64) -> Self {
        let (min, max) = (
            ChunkCoordinate::containing(at - reach).0,
            ChunkCoordinate::containing(at + reach).0,
        );
        let mut chunks = HashMap::default();
        for x in min.x..=max.x {
            for y in min.y..=max.y {
                for z in min.z..=max.z {
                    let coord = ChunkCoordinate(I64Vec3::new(x, y, z));
                    if let Some(data) = world.get_chunk_data(coord) {
                        chunks.insert(coord, data);
                    }
                }
            }
        }
        Self {
            chunks,
            unpacked: HashMap::default(),
        }
    }

    /// The block at `at`, if its chunk was copied.
    pub fn get(&mut self, at: I64Vec3) -> Option<BlockType> {
        let coord = ChunkCoordinate::containing(at);
        let chunks = &self.chunks;
        let grid = self.unpacked.entry(coord).or_insert_with(|| {
            chunks.get(&coord).map(|data| {
                let mut grid = Box::new([BlockType::Air; CELLS]);
                data.decode_into(&mut grid);
                grid
            })
        });
        let index = linear_index(ChunkCoordinate::local_block(at));
        grid.as_ref().map(|grid| grid[index])
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use bevy::math::{I64Vec3, Vec3Swizzles};

    use super::{find_path, Limits, Walker};
    use crate::block::BlockType;

    const SHEEP: Walker = Walker {
        across: 1,
        tall: 2,
        drop: 3,
        swims: false,
    };
    const LIMITS: Limits = Limits {
        cells: 2000,
        reach: 32,
    };

    // A field of grass at y 0, nothing known past 40 blocks out, with whatever `extra` puts on
    // it.
    fn field(
        extra: impl Fn(I64Vec3) -> Option<BlockType>,
    ) -> impl FnMut(I64Vec3) -> Option<BlockType> {
        move |at: I64Vec3| {
            if at.x.abs() > 40 || at.z.abs() > 40 {
                return None;
            }
            extra(at).or(Some(if at.y <= 0 {
                BlockType::Grass
            } else {
                BlockType::Air
            }))
        }
    }

    fn walk(path: &[I64Vec3], from: I64Vec3) {
        let mut at = from;
        for &next in path {
            let step = next - at;
            assert!(
                step.x.abs() <= 1 && step.z.abs() <= 1 && step.y <= 1,
                "{at} to {next}"
            );
            at = next;
        }
    }

    #[test]
    fn test_walks_straight_across_open_ground() {
        let from = I64Vec3::new(0, 1, 0);
        let path = find_path(
            from,
            I64Vec3::new(10, 1, 0),
            0,
            SHEEP,
            LIMITS,
            field(|_| None),
        );
        let path = path.unwrap();
        assert!(path.arrives);
        assert_eq!(10, path.cells.len());
        assert_eq!(I64Vec3::new(10, 1, 0), *path.cells.last().unwrap());
        walk(&path.cells, from);
    }

    #[test]
    fn test_goes_round_a_wall_by_its_gap() {
        // a wall three high along x 5, open only at z 6
        let wall = |at: I64Vec3| {
            (at.x == 5 && (1..=3).contains(&at.y) && at.z != 6).then_some(BlockType::Stone)
        };
        let from = I64Vec3::new(0, 1, 0);
        let path = find_path(from, I64Vec3::new(10, 1, 0), 0, SHEEP, LIMITS, field(wall)).unwrap();
        assert!(path.arrives);
        assert!(
            path.cells.contains(&I64Vec3::new(5, 1, 6)),
            "{:?}",
            path.cells
        );
        walk(&path.cells, from);
    }

    #[test]
    fn test_steps_up_a_block_at_a_time_but_not_two() {
        // steps up at x 3 and x 6, and a cliff two high at z 10 everywhere but the steps
        let steps = |at: I64Vec3| {
            let ground = if at.x >= 6 {
                2
            } else if at.x >= 3 {
                1
            } else {
                0
            };
            (at.y <= ground).then_some(BlockType::Dirt)
        };
        let from = I64Vec3::new(0, 1, 0);
        let path = find_path(from, I64Vec3::new(8, 3, 0), 0, SHEEP, LIMITS, field(steps)).unwrap();
        assert!(path.arrives);
        walk(&path.cells, from);
        let cliff = |at: I64Vec3| (at.x >= 3 && at.y <= 2).then_some(BlockType::Dirt);
        let path = find_path(from, I64Vec3::new(8, 3, 0), 0, SHEEP, LIMITS, field(cliff)).unwrap();
        assert!(!path.arrives, "{:?}", path.cells);
    }

    #[test]
    fn test_drops_down_only_as_far_as_it_will() {
        // a pit 3 deep, and one 5 deep beyond it
        let pit = |at: I64Vec3| match at.x {
            3..=5 if at.y > -3 => Some(BlockType::Air),
            8..=10 if at.y > -5 => Some(BlockType::Air),
            _ => None,
        };
        let from = I64Vec3::new(0, 1, 0);
        let path = find_path(from, I64Vec3::new(4, -2, 0), 0, SHEEP, LIMITS, field(pit)).unwrap();
        assert!(path.arrives, "{:?}", path.cells);
        let deep = find_path(from, I64Vec3::new(9, -4, 0), 0, SHEEP, LIMITS, field(pit)).unwrap();
        assert!(!deep.arrives);
    }

    #[test]
    fn test_needs_headroom_for_its_body() {
        // a roof at y 2 over x 3..=6 everywhere, so a two-tall body can't get under it, and a
        // body one tall can
        let roof =
            |at: I64Vec3| ((3..=6).contains(&at.x) && at.y == 2).then_some(BlockType::Planks);
        let from = I64Vec3::new(0, 1, 0);
        let to = I64Vec3::new(5, 1, 0);
        let path = find_path(from, to, 0, SHEEP, LIMITS, field(roof)).unwrap();
        assert!(!path.arrives);
        let short = Walker { tall: 1, ..SHEEP };
        assert!(
            find_path(from, to, 0, short, LIMITS, field(roof))
                .unwrap()
                .arrives
        );
        // and three across doesn't fit through a gap one wide
        let gap = |at: I64Vec3| (at.x == 3 && at.y >= 1 && at.z != 0).then_some(BlockType::Stone);
        let wide = Walker { across: 3, ..SHEEP };
        let to = I64Vec3::new(6, 1, 0);
        assert!(
            find_path(from, to, 0, SHEEP, LIMITS, field(gap))
                .unwrap()
                .arrives
        );
        assert!(
            !find_path(from, to, 0, wide, LIMITS, field(gap))
                .unwrap()
                .arrives
        );
        assert_eq!(3, Walker::sized(1.4, 0.9, 3, false).across);
        assert_eq!((1, 2), {
            let sheep = Walker::sized(0.9, 1.3, 3, false);
            (sheep.across, sheep.tall)
        });
    }

    #[test]
    fn test_keeps_out_of_water_unless_it_swims() {
        // a channel of water across x 4..=6, bridged by nothing
        let channel = |at: I64Vec3| {
            ((4..=6).contains(&at.x) && (-2..=0).contains(&at.y)).then_some(BlockType::Water)
        };
        let from = I64Vec3::new(0, 1, 0);
        let to = I64Vec3::new(10, 1, 0);
        assert!(
            !find_path(from, to, 0, SHEEP, LIMITS, field(channel))
                .unwrap()
                .arrives
        );
        let swimmer = Walker {
            swims: true,
            ..SHEEP
        };
        let path = find_path(from, to, 0, swimmer, LIMITS, field(channel)).unwrap();
        assert!(path.arrives);
    }

    #[test]
    fn test_gives_up_within_its_limits_and_goes_as_near_as_it_can() {
        // a goal walled in all round
        let walled = |at: I64Vec3| {
            let ring = (at - I64Vec3::new(20, 0, 0)).xz().abs().max_element() == 2;
            (ring && at.y >= 1).then_some(BlockType::Stone)
        };
        let from = I64Vec3::new(0, 1, 0);
        let to = I64Vec3::new(20, 1, 0);
        let started = Instant::now();
        let path = find_path(from, to, 0, SHEEP, LIMITS, field(walled)).unwrap();
        assert!(started.elapsed().as_millis() < 50);
        assert!(!path.arrives);
        let end = *path.cells.last().unwrap();
        assert!((end - to).xz().abs().max_element() <= 3, "{end}");
        // nowhere past its reach
        let short = Limits {
            cells: 2000,
            reach: 5,
        };
        let path = find_path(from, to, 0, SHEEP, short, field(walled)).unwrap();
        assert!(path.cells.iter().all(|cell| cell.x <= 5));
        // and nowhere at all from somewhere shut in
        let shut = |at: I64Vec3| {
            (at != I64Vec3::new(0, 1, 0) && at != I64Vec3::new(0, 2, 0) && at.y >= 1)
                .then_some(BlockType::Stone)
        };
        assert_eq!(None, find_path(from, to, 0, SHEEP, LIMITS, field(shut)));
    }

    #[test]
    fn test_stops_near_enough() {
        let from = I64Vec3::new(0, 1, 0);
        let path = find_path(
            from,
            I64Vec3::new(10, 1, 0),
            2,
            SHEEP,
            LIMITS,
            field(|_| None),
        );
        assert_eq!(I64Vec3::new(8, 1, 0), *path.unwrap().cells.last().unwrap());
    }
}
