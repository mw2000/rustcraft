//! A giant tree: a trunk a dozen blocks thick on buttress roots, rising a couple of hundred
//! blocks to a crown of branches each ending in a mass of leaves. Its trunk is hollow, with a
//! door at its foot and a stair winding up the inside to where the lowest branch leaves it, so
//! it can be climbed into the crown. It grows in forests and jungles, built block by block like
//! trees but standing on its own grid, and drawn as cells close up and as boxes far off.

use std::f64::consts::TAU;

use bevy::math::{DVec2, DVec3, I64Vec2, I64Vec3, IVec3, Vec3Swizzles};

use super::super::biome::smoothstep;
use super::super::noise::NoiseGenerator;
use super::super::trees::{hash, Tree};
use super::{pick, unit, Site};
use crate::block::BlockType;

// How tall it grows, ground to the top of its crown.
const HEIGHT: (f64, f64) = (150.0, 230.0);
// The trunk's radius over its roots, from the thinnest to the thickest, and how far it tapers
// by its top, from this share of the way up, so the hollow under the crown stays roomy. Its
// trunk rises this share of its height, into the crown.
const RADIUS: (f64, f64) = (6.0, 8.0);
const TAPER: f64 = 0.45;
const TAPER_FROM: f64 = 0.5;
const TRUNK: f64 = 0.7;
// Buttress roots: the trunk flares out by this many blocks at the ground, the flare fading over
// this many blocks up, in ridges round it, flaring least between them.
const FLARE: f64 = 6.0;
const FLARE_HEIGHT: f64 = 12.0;
const ROOTS: (f64, f64) = (5.0, 7.99);
const BETWEEN_ROOTS: f64 = 0.25;
// The trunk's wall, round the hollow.
const WALL: f64 = 2.5;
// The stair: a ramp a block thick, as wide as this, against the inside of the wall, climbing
// this many blocks each turn, gently enough to walk up.
const STAIR_WIDTH: f64 = 2.6;
const STAIR_PITCH: f64 = 12.0;
// The door at its foot, and windows up the trunk every so many blocks, turned this far round
// from the last.
const DOOR_HALF_WIDTH: f64 = 1.6;
const DOOR_HEIGHT: i64 = 5;
const WINDOW_SPACING: i64 = 23;
const WINDOW_TURN: f64 = 2.4;
const WINDOW_HALF_WIDTH: f64 = 1.1;
const WINDOW_HEIGHT: i64 = 3;
// The way out onto the lowest branch, which the stair is turned to reach just as it passes it,
// opens over a turn of the stair, with room to stand up through the wall.
const EXIT_HALF_WIDTH: f64 = 1.6;
const EXIT_HEIGHT: i64 = STAIR_PITCH as i64 + 2;
// Branches leave the trunk this far up it, reaching out this share of its height and rising
// this share of their reach, thick at the trunk and thinner at the leaves.
const BRANCHES: (f64, f64) = (6.0, 9.99);
const FORK: (f64, f64) = (0.55, 0.92);
const BRANCH_REACH: (f64, f64) = (0.26, 0.4);
const BRANCH_RISE: (f64, f64) = (0.2, 0.5);
const BRANCH_RADIUS: (f64, f64) = (3.2, 1.5);
// A mass of leaves at each branch's end, and a smaller one part way along, and the biggest
// over the top of the trunk, as shares of its height.
const LEAVES: (f64, f64) = (0.085, 0.115);
const INNER_LEAVES: f64 = 0.62;
const TOP_LEAVES: f64 = 0.13;
const SQUASH: f64 = 0.6;
// How far in leaves are nibbled from the edge of a mass.
const NIBBLE: f64 = 1.4;
/// The furthest any part of one reaches from its trunk, across.
pub(super) const MAX_REACH: i64 =
    ((BRANCH_REACH.1 + LEAVES.1) * HEIGHT.1) as i64 + NIBBLE as i64 + 2;
// Where one grows: forest or jungle, a little over the sea, on ground the trunk can stand on.
const BIOMES: [&str; 2] = ["forest", "jungle"];
const SITE_ABOVE_SEA: (f64, f64) = (6.0, 150.0);
const GROUND_POINTS: usize = 16;
const MAX_RISE: i64 = 10;
// Its trunk goes this far under the lowest ground round it.
const BURIED: i64 = 3;
// Cells under this wide hold it as blocks, wider ones as boxes. Its branches are drawn as boxes
// in cells up to `BRANCH_MAX_CELL` wide, past which they're under a pixel.
const BOX_CELL: i64 = 8;
const BRANCH_MAX_CELL: i64 = 16;
// Cells wider than a block hold its wood or leaves where at least this many of a 3 x 3 x 3
// spread of points through them are, as trees' crowns do.
const COVERAGE_POINTS: usize = 7;
// Each mass of leaves as this many boxes stacked up it, and branches as boxes this many of
// their radii apart.
const LEAF_LAYERS: usize = 3;
const BRANCH_STEP: f64 = 1.5;

#[derive(Debug, Clone)]
pub struct GiantTree {
    /// The middle of its trunk at the floor of its hollow, which is the ground's middle height
    /// round it.
    pub base: I64Vec3,
    // the trunk's foot, under the lowest ground round it
    bottom: i64,
    radius: f64,
    // heights over the floor: the top of the trunk and of the hollow in it
    trunk: f64,
    hollow: i64,
    roots: f64,
    root_turn: f64,
    door: f64,
    // the stair's turn, so it reaches the way out just as it passes it
    stair_turn: f64,
    // which way the exit faces, and the height over the floor it opens from
    exit: (DVec2, i64),
    seed: u64,
    branches: Vec<Branch>,
    leaves: Vec<Blob>,
    reach: i64,
    top: i64,
}

// A branch from the trunk out to its leaves, as offsets from the base.
#[derive(Debug, Clone)]
struct Branch {
    from: DVec3,
    to: DVec3,
}

// A mass of leaves, an ellipsoid squashed by `SQUASH`, as an offset from the base.
#[derive(Debug, Clone)]
struct Blob {
    centre: DVec3,
    radius: f64,
}

impl Branch {
    // Its radius `t` of the way out, 0..1.
    fn radius(t: f64) -> f64 {
        BRANCH_RADIUS.0 + (BRANCH_RADIUS.1 - BRANCH_RADIUS.0) * t
    }

    fn contains(&self, point: DVec3) -> bool {
        let along = self.to - self.from;
        let t = ((point - self.from).dot(along) / along.length_squared()).clamp(0.0, 1.0);
        (point - (self.from + along * t)).length_squared() <= Self::radius(t).powi(2)
    }

    fn bounds(&self) -> (DVec3, DVec3) {
        let r = DVec3::splat(BRANCH_RADIUS.0);
        (self.from.min(self.to) - r, self.from.max(self.to) + r)
    }
}

impl Blob {
    fn reach(&self) -> DVec3 {
        DVec3::new(self.radius, self.radius * SQUASH, self.radius)
    }

    fn bounds(&self) -> (DVec3, DVec3) {
        (self.centre - self.reach(), self.centre + self.reach())
    }
}

// Whether the box `min..max` and `low..high` overlap.
fn overlaps((min, max): (DVec3, DVec3), (low, high): (DVec3, DVec3)) -> bool {
    min.cmple(high).all() && max.cmpge(low).all()
}

// The branches and leaves near a box, by index, so each block there tests only those.
struct Near {
    branches: Vec<usize>,
    leaves: Vec<usize>,
}

impl GiantTree {
    // A giant tree at `site`, with `h` deciding its shape, if the ground there suits one.
    pub(super) fn place(noise: &NoiseGenerator, site: &Site, h: u64) -> Option<Self> {
        if !site.biome.is_some_and(|b| BIOMES.contains(&b))
            || !(SITE_ABOVE_SEA.0..SITE_ABOVE_SEA.1).contains(&site.above_sea)
        {
            return None;
        }
        let height = pick(h, 0, HEIGHT);
        let radius = pick(h, 16, RADIUS);
        // the ground round the trunk's roots, rounded as chunks round it
        let round = |height: f64| (height as f32 as f64).round() as i64;
        let mut ground: Vec<i64> = site
            .ring(noise, radius + FLARE, GROUND_POINTS)
            .into_iter()
            .chain([site.height])
            .map(round)
            .collect();
        ground.sort_unstable();
        let (lowest, highest) = (ground[0], ground[ground.len() - 1]);
        if highest - lowest > MAX_RISE {
            return None;
        }
        let base = I64Vec3::new(site.at.x, ground[ground.len() / 2], site.at.y);
        Some(Self::grow(base, lowest - BURIED, height, radius, h))
    }

    fn grow(base: I64Vec3, bottom: i64, height: f64, radius: f64, seed: u64) -> Self {
        let trunk = height * TRUNK;
        let count = pick(seed, 32, BRANCHES).floor() as usize;
        let mut branches = Vec::with_capacity(count);
        let mut leaves = Vec::with_capacity(2 * count + 1);
        for i in 0..count {
            let h = hash(seed, i as i64, 23);
            let angle = (i as f64 + unit(h, 0) * 0.6) * TAU / count as f64;
            let out = DVec2::from_angle(angle);
            let fork = trunk * pick(h, 16, FORK);
            let reach = height * pick(h, 32, BRANCH_REACH);
            let rise = reach * pick(h, 48, BRANCH_RISE);
            // from inside the wall, so it's rooted in the trunk without crossing its hollow
            let wall =
                radius * (1.0 - TAPER * smoothstep(TAPER_FROM, 1.0, fork / trunk)) - WALL / 2.0;
            let from = (out * wall).extend(fork).xzy();
            let to = (out * reach).extend(fork + rise).xzy();
            let size = height * pick(h, 8, LEAVES);
            leaves.push(Blob {
                centre: to + DVec3::Y * size * 0.3,
                radius: size,
            });
            leaves.push(Blob {
                centre: from.lerp(to, INNER_LEAVES) + DVec3::Y * size * 0.25,
                radius: size * 0.7,
            });
            branches.push(Branch { from, to });
        }
        leaves.push(Blob {
            centre: DVec3::Y * (trunk + height * TOP_LEAVES * 0.3),
            radius: height * TOP_LEAVES,
        });
        // the hollow climbs to the way out onto the lowest branch
        let lowest = branches
            .iter()
            .min_by(|a, b| a.from.y.total_cmp(&b.from.y))
            .unwrap();
        let exit_floor = (lowest.from.y + BRANCH_RADIUS.0).ceil() as i64;
        let exit = lowest.to.xz().normalize();
        let reach = leaves
            .iter()
            .map(|blob| blob.centre.xz().length() + blob.radius + NIBBLE)
            .fold(radius + FLARE, f64::max)
            .ceil() as i64;
        let top = leaves
            .iter()
            .map(|blob| blob.centre.y + blob.radius * SQUASH)
            .fold(trunk, f64::max)
            .ceil() as i64;
        Self {
            base,
            bottom,
            radius,
            trunk,
            hollow: exit_floor + EXIT_HEIGHT + 1,
            roots: pick(seed, 40, ROOTS).floor(),
            root_turn: unit(seed, 24) * TAU,
            door: unit(seed, 56) * TAU,
            stair_turn: exit.to_angle()
                - ((exit_floor - 1) as f64).rem_euclid(STAIR_PITCH) / STAIR_PITCH * TAU,
            exit: (exit, exit_floor),
            seed,
            branches,
            leaves,
            reach,
            top: top + 1,
        }
    }

    /// The furthest any of it reaches from its trunk, across.
    pub fn reach(&self) -> i64 {
        self.reach
    }

    /// The lowest block it fills, and the block past the highest.
    pub fn bounds(&self) -> (I64Vec3, I64Vec3) {
        let reach = I64Vec3::new(self.reach, 0, self.reach);
        (
            (self.base - reach).with_y(self.bottom),
            self.base + reach + I64Vec3::new(1, self.top, 1),
        )
    }

    // The trunk's radius `y` blocks over the floor, before its roots flare.
    fn trunk_radius(&self, y: f64) -> f64 {
        self.radius * (1.0 - TAPER * smoothstep(TAPER_FROM, 1.0, y / self.trunk))
    }

    // Its radius there, `angle` round, with the roots flaring out at its foot.
    fn flared_radius(&self, y: f64, angle: f64) -> f64 {
        let ridge = ((angle - self.root_turn) * self.roots / 2.0)
            .cos()
            .abs()
            .powi(3);
        let flare = FLARE * (-y.max(0.0) / FLARE_HEIGHT).exp();
        self.trunk_radius(y) + flare * (BETWEEN_ROOTS + (1.0 - BETWEEN_ROOTS) * ridge)
    }

    // Whether the point `across` from the trunk's middle, `y` over the floor, is in an opening
    // through its wall: the door, a window or the way out.
    fn opening(&self, across: DVec2, y: i64) -> bool {
        let through = |direction: DVec2, half_width: f64| {
            direction.dot(across) > 0.0 && direction.perp_dot(across).abs() < half_width
        };
        if (0..DOOR_HEIGHT).contains(&y) && through(DVec2::from_angle(self.door), DOOR_HALF_WIDTH) {
            return true;
        }
        let (exit, exit_floor) = self.exit;
        if (exit_floor..exit_floor + EXIT_HEIGHT).contains(&y) && through(exit, EXIT_HALF_WIDTH) {
            return true;
        }
        let window = y / WINDOW_SPACING;
        window > 0
            && y % WINDOW_SPACING < WINDOW_HEIGHT
            && through(
                DVec2::from_angle(self.door + window as f64 * WINDOW_TURN),
                WINDOW_HALF_WIDTH,
            )
    }

    // What's at the point `offset` from the base, if any of the tree is, testing only the
    // branches and leaves `near` it. Air in its hollow and openings, so they carve out the
    // ground.
    fn at(&self, offset: DVec3, near: &Near) -> Option<BlockType> {
        let y = offset.y.floor() as i64;
        let across = offset.xz();
        if (self.bottom - self.base.y..self.trunk as i64).contains(&y) {
            let distance = across.length();
            if distance <= self.flared_radius(offset.y, across.to_angle()) {
                let inner = self.trunk_radius(offset.y) - WALL;
                // through the wall, leaving the stair inside it whole
                if y >= 0 && distance >= inner && self.opening(across, y) {
                    return Some(BlockType::Air);
                }
                if !(0..self.hollow).contains(&y) || distance >= inner {
                    return Some(BlockType::Log);
                }
                // the hollow, through the roots of the branches too, with the stair up it
                let turn = (across.to_angle() - self.stair_turn).rem_euclid(TAU) / TAU;
                let step = (offset.y - turn * STAIR_PITCH).rem_euclid(STAIR_PITCH) < 1.0;
                let stair = step && distance >= inner - STAIR_WIDTH;
                return Some(if stair {
                    BlockType::Log
                } else {
                    BlockType::Air
                });
            }
        }
        if near
            .branches
            .iter()
            .any(|&i| self.branches[i].contains(offset))
        {
            return Some(BlockType::Log);
        }
        near.leaves
            .iter()
            .any(|&i| self.in_leaves(&self.leaves[i], offset))
            .then_some(BlockType::Leaves)
    }

    // Whether `offset` from the base is among the leaves of `blob`, nibbled at its edge.
    fn in_leaves(&self, blob: &Blob, offset: DVec3) -> bool {
        let d = (offset - blob.centre) / DVec3::new(1.0, SQUASH, 1.0);
        let distance = d.length_squared();
        if distance > blob.radius * blob.radius {
            return false;
        }
        let inner = blob.radius - NIBBLE;
        if distance <= inner * inner {
            return true;
        }
        let rounded = offset.round().as_i64vec3();
        let nibble = hash(self.seed, rounded.x * 7919 + rounded.y, rounded.z) % 3;
        distance <= (blob.radius - nibble as f64 * NIBBLE / 2.0).powi(2)
    }

    // The branches and leaves reaching into the blocks `min..max`, offsets from the base.
    fn near(&self, min: DVec3, max: DVec3) -> Near {
        let area = (min, max);
        Near {
            branches: (0..self.branches.len())
                .filter(|&i| overlaps(area, self.branches[i].bounds()))
                .collect(),
            leaves: (0..self.leaves.len())
                .filter(|&i| overlaps(area, self.leaves[i].bounds()))
                .collect(),
        }
    }

    /// Puts the tree into a grid of `size`-block cells whose cell 0 has `origin` as its lowest
    /// block, calling `place` with each cell index in `first..=last` it fills and what goes
    /// there: blocks, with air in its hollow, in cells a block wide, and in wider ones its
    /// wood or leaves where enough of the cell is them. Cells `BOX_CELL` wide or wider leave it
    /// to `boxes`.
    pub fn plant(
        &self,
        origin: I64Vec3,
        size: i64,
        (first, last): (IVec3, IVec3),
        mut place: impl FnMut(IVec3, BlockType),
    ) {
        if size >= BOX_CELL {
            return;
        }
        let (low, high) = self.bounds();
        let cell_of = |block: I64Vec3| (block - origin).div_euclid(I64Vec3::splat(size)).as_ivec3();
        let (from, to) = (cell_of(low).max(first), cell_of(high - 1).min(last));
        if from.cmpgt(to).any() {
            return;
        }
        let offset_of = |cell: IVec3| (origin + cell.as_i64vec3() * size - self.base).as_dvec3();
        let near = self.near(offset_of(from), offset_of(to + 1));
        if near.branches.is_empty()
            && near.leaves.is_empty()
            && !self.trunk_overlaps(offset_of(from), offset_of(to + 1))
        {
            return;
        }
        // the middle of a block, or a spread of points through a wider cell
        let middle = (size - 1) as f64 / 2.0;
        let step = size as f64 / 3.0;
        for x in from.x..=to.x {
            for z in from.z..=to.z {
                for y in from.y..=to.y {
                    let cell = IVec3::new(x, y, z);
                    let corner = offset_of(cell);
                    let block = if size == 1 {
                        self.at(corner, &near)
                    } else {
                        let (mut wood, mut leaves) = (0, 0);
                        for i in 0..27 {
                            let spread = DVec3::new(
                                (i % 3) as f64 - 1.0,
                                (i / 3 % 3) as f64 - 1.0,
                                (i / 9) as f64 - 1.0,
                            );
                            match self.at(corner + middle + spread * step, &near) {
                                Some(BlockType::Log) => wood += 1,
                                Some(BlockType::Leaves) => leaves += 1,
                                _ => {}
                            }
                        }
                        if leaves >= COVERAGE_POINTS {
                            Some(BlockType::Leaves)
                        } else if wood >= COVERAGE_POINTS {
                            Some(BlockType::Log)
                        } else {
                            None
                        }
                    };
                    if let Some(block) = block {
                        place(cell, block);
                    }
                }
            }
        }
    }

    // Whether the trunk could reach into the blocks `min..max`, offsets from the base.
    fn trunk_overlaps(&self, min: DVec3, max: DVec3) -> bool {
        let r = self.radius + FLARE;
        let trunk = (
            DVec3::new(-r, (self.bottom - self.base.y) as f64, -r),
            DVec3::new(r, self.trunk, r),
        );
        overlaps((min, max), trunk)
    }

    /// The tree for cells `cell` wide, `BOX_CELL` or wider, as boxes of one block each, their
    /// lowest corners and the corners past their highest, in blocks: its trunk, its branches
    /// while they're wide enough to see, and its leaves, each mass as boxes stacked up it.
    pub fn boxes(&self, cell: i64) -> Vec<(DVec3, DVec3, BlockType)> {
        let base = self.base.as_dvec3() + DVec3::new(0.5, 0.0, 0.5);
        let mut boxes = Vec::new();
        // as wide as the round trunk's area, a third of the way up
        let half = self.trunk_radius(self.trunk / 3.0) * std::f64::consts::PI.sqrt() / 2.0;
        boxes.push((
            base + DVec3::new(-half, (self.bottom - self.base.y) as f64, -half),
            base + DVec3::new(half, self.trunk, half),
            BlockType::Log,
        ));
        if cell <= BRANCH_MAX_CELL {
            for branch in &self.branches {
                let length = branch.from.distance(branch.to);
                let steps = (length / (BRANCH_RADIUS.1 * BRANCH_STEP)).ceil() as usize;
                for i in 0..=steps {
                    let t = i as f64 / steps as f64;
                    let r = DVec3::splat(Branch::radius(t) * 0.8);
                    let at = base + branch.from.lerp(branch.to, t);
                    boxes.push((at - r, at + r, BlockType::Log));
                }
            }
        }
        for blob in &self.leaves {
            let reach = blob.reach();
            for layer in 0..LEAF_LAYERS {
                // each layer as wide as the ellipsoid at its middle height
                let (bottom, top) = (
                    -1.0 + 2.0 * layer as f64 / LEAF_LAYERS as f64,
                    -1.0 + 2.0 * (layer + 1) as f64 / LEAF_LAYERS as f64,
                );
                let middle = (bottom + top) / 2.0;
                let width = (1.0 - middle * middle).sqrt() * reach.x * 0.9;
                let centre = base + blob.centre;
                boxes.push((
                    centre + DVec3::new(-width, bottom * reach.y, -width),
                    centre + DVec3::new(width, top * reach.y, width),
                    BlockType::Leaves,
                ));
            }
        }
        boxes
    }

    /// Whether cells `cell` wide take the tree as boxes rather than cells.
    pub fn drawn_as_boxes(cell: i64) -> bool {
        cell >= BOX_CELL
    }

    /// Whether `tree` would grow into its trunk or roots.
    pub fn crowds(&self, tree: &Tree) -> bool {
        let reach = (self.radius + FLARE).ceil() as i64 + 2;
        let middle = self.base.xz();
        tree.overhangs(middle - reach, middle + reach + 1)
    }

    /// Caves stay shut under its trunk, so it never stands on nothing, see
    /// `Landmarks::cave_roof`.
    pub(super) fn cave_roof(&self, x: i64, z: i64, ground: impl FnOnce() -> i64) -> Option<i64> {
        let across = (I64Vec2::new(x, z) - self.base.xz()).as_dvec2();
        (across.length() <= self.radius + FLARE + 1.0).then(|| ground().min(self.bottom))
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub(in super::super) fn tree() -> GiantTree {
        GiantTree::grow(I64Vec3::new(1000, 500, -300), 496, 200.0, 6.5, 12345)
    }

    // Every block of it near `min..max`, offsets from its base, as cells a block wide have it.
    fn blocks(
        tree: &GiantTree,
        min: I64Vec3,
        max: I64Vec3,
    ) -> std::collections::HashMap<I64Vec3, BlockType> {
        let mut blocks = std::collections::HashMap::new();
        let origin = tree.base + min;
        tree.plant(
            origin,
            1,
            (IVec3::ZERO, (max - min).as_ivec3()),
            |cell, block| {
                blocks.insert(min + cell.as_i64vec3(), block);
            },
        );
        blocks
    }

    #[test]
    fn test_it_can_be_climbed_from_its_door_up_the_stair_and_out_onto_a_branch() {
        // the smallest and biggest they grow, thinnest and thickest
        for (seed, height, radius) in [
            (1, HEIGHT.0, RADIUS.0),
            (2, HEIGHT.1, RADIUS.0),
            (3, HEIGHT.0, RADIUS.1),
            (12345, HEIGHT.1, RADIUS.1),
        ] {
            let tree = GiantTree::grow(I64Vec3::new(0, 500, 0), 496, height, radius, seed);
            let reach = (tree.radius + FLARE) as i64 + 4;
            let (min, max) = (
                I64Vec3::new(-reach, -1, -reach),
                I64Vec3::new(reach, tree.hollow + 4, reach),
            );
            let blocks = blocks(&tree, min, max);
            // outside it, the ground is the floor's height, and air over it
            let solid = |at: I64Vec3| match blocks.get(&at) {
                Some(block) => *block != BlockType::Air,
                None => at.y < 0,
            };
            let stands =
                |at: I64Vec3| solid(at - I64Vec3::Y) && !solid(at) && !solid(at + I64Vec3::Y);
            // walking, stepping a block up or dropping down, from outside the door
            let door = DVec2::from_angle(tree.door) * (reach - 1) as f64;
            let start = I64Vec3::new(door.x.round() as i64, 0, door.y.round() as i64);
            assert!(stands(start));
            let mut seen = std::collections::HashSet::from([start]);
            let mut next = vec![start];
            while let Some(at) = next.pop() {
                for side in [I64Vec3::X, I64Vec3::NEG_X, I64Vec3::Z, I64Vec3::NEG_Z] {
                    for up in -(STAIR_PITCH as i64)..=1 {
                        let to = at + side + I64Vec3::Y * up;
                        let inside = to.cmpge(min).all() && to.cmplt(max).all();
                        // room to step up, over its head too, or to step off before dropping
                        let clear = (to.y..=at.y.max(to.y) + 1)
                            .all(|y| !solid((at + side).with_y(y)))
                            && (up < 1 || !solid(at + I64Vec3::Y * 2));
                        if inside && clear && stands(to) && seen.insert(to) {
                            next.push(to);
                        }
                    }
                }
            }
            // and out of the trunk through the way out, onto the lowest branch
            let (_, exit_floor) = tree.exit;
            let outside = tree.trunk_radius(exit_floor as f64);
            let out = seen
                .iter()
                .any(|p| p.y >= exit_floor && p.xz().as_dvec2().length() > outside);
            assert!(
                out,
                "seed {seed}: no way out onto the branch at {exit_floor}"
            );
        }
    }

    #[test]
    fn test_it_lies_within_its_bounds_and_coarse_cells_hold_its_wood_and_leaves() {
        let tree = tree();
        let (low, high) = tree.bounds();
        let all = blocks(&tree, low - tree.base - 4, high - tree.base + 4);
        assert!(all.keys().all(|&at| {
            let world = tree.base + at;
            world.cmpge(low).all() && world.cmplt(high).all()
        }));
        let count = |block| all.values().filter(|&&b| b == block).count();
        assert!(count(BlockType::Log) > 10_000 && count(BlockType::Leaves) > 50_000);
        for size in [2, 4] {
            let (mut wood, mut leaves) = (0, 0);
            let cells = ((high - low) / size + 1).as_ivec3();
            tree.plant(low, size, (IVec3::ZERO, cells), |_, block| match block {
                BlockType::Log => wood += 1,
                BlockType::Leaves => leaves += 1,
                _ => panic!("{block:?} in a coarse cell"),
            });
            let volume = (size * size * size) as usize;
            // much as full detail has, cell by cell
            let ratio = |cells: usize, blocks: usize| (cells * volume) as f64 / blocks as f64;
            assert!(
                (0.6..1.5).contains(&ratio(leaves, count(BlockType::Leaves))),
                "{size}"
            );
            assert!(
                (0.4..1.5).contains(&ratio(wood, count(BlockType::Log))),
                "{size}"
            );
        }
        // and cells wider draw it as boxes instead
        tree.plant(
            low,
            BOX_CELL,
            (IVec3::ZERO, IVec3::splat(100)),
            |_, _| panic!(),
        );
        let boxes = tree.boxes(BOX_CELL);
        assert!(boxes.iter().any(|b| b.2 == BlockType::Log));
        assert!(boxes.len() > tree.boxes(BRANCH_MAX_CELL * 2).len());
        let top = boxes.iter().map(|b| b.1.y).fold(f64::MIN, f64::max);
        assert!((top - high.y as f64).abs() < 8.0, "{top} {}", high.y);
    }

    #[test]
    fn test_its_reach_is_within_the_most_any_reaches() {
        for seed in 0..50 {
            let tree = GiantTree::grow(I64Vec3::ZERO, -4, HEIGHT.1, RADIUS.1, seed);
            assert!(tree.reach() <= MAX_REACH, "{} > {MAX_REACH}", tree.reach());
        }
    }
}
