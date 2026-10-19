use bevy::math::{DVec3, I64Vec2, I64Vec3, IVec3, Vec2, Vec3, Vec3Swizzles};

use super::biome::BiomeId;
use super::generator::column_block;
use super::noise::NoiseGenerator;
use super::region_store::{HeightSampler, SLOPE_SPAN};
use super::WATER_LEVEL;
use crate::block::BlockType;

/// What trees need to know about the ground to take root, from the full-detail heightmap or
/// from the coarser grid a distant LOD node samples.
pub trait TreeGround {
    fn trees(&mut self, x: i64, z: i64) -> f64;
    fn height(&mut self, x: i64, z: i64) -> f64;
    fn biome(&mut self, x: i64, z: i64) -> BiomeId;

    /// The slope there, as `HeightSampler::gradient` measures it.
    fn gradient(&mut self, x: i64, z: i64) -> (f64, f64) {
        let span = SLOPE_SPAN;
        let across = |a: f64, b: f64| (a - b) * (1.0 / span as f64);
        (
            across(self.height(x + span, z), self.height(x - span, z)),
            across(self.height(x, z + span), self.height(x, z - span)),
        )
    }

    /// Whether no tree with this `chance` can grow at `x`, `z`, known without `trees` there.
    fn rules_out(&mut self, _x: i64, _z: i64, _chance: f64) -> bool {
        false
    }

    /// The height a tree's base stands at, which a coarse node sets to the ground it draws, so
    /// trees sit on it, while the rest above stay exact, so the same trees grow at every level.
    fn base_height(&mut self, x: i64, z: i64) -> i64 {
        self.height(x, z).round() as i64
    }
}

impl TreeGround for HeightSampler<'_> {
    fn trees(&mut self, x: i64, z: i64) -> f64 {
        HeightSampler::trees(self, x, z)
    }

    fn height(&mut self, x: i64, z: i64) -> f64 {
        HeightSampler::height(self, x, z)
    }

    fn biome(&mut self, x: i64, z: i64) -> BiomeId {
        HeightSampler::biome(self, x, z)
    }
}

// Trees stand on a grid of cells this many blocks across, at most one somewhere in each cell.
const CELL: i64 = 10;
/// How far out from its trunk any tree reaches.
pub const TREE_REACH: i64 = 10;
/// How far above its ground any tree reaches.
pub const TREE_HEIGHT: i64 = 27;
// Colder than this, the base terrain's trees are pines.
const PINE_TEMPERATURE: f32 = 0.38;
// Ground steeper than this (see `terrain_block`) is too steep to take root on.
const MAX_GRADIENT: f64 = 1.5;
// Crowns other than pines' are a big crown with smaller lobes off it on branches.
const LOBES: usize = 3;
// Trunks this tall are two blocks thick.
const THICK_TRUNK: i64 = 12;
// Cells this wide or wider judge leaves by coverage: a quarter of a 3 x 3 x 3 spread of points
// inside, or small crowns would fall between cell centres.
const COVERAGE_CELL: i64 = 4;
const COVERAGE_POINTS: usize = 7;
/// Cells this wide or wider leave out the trunk, which would stand a pillar far thicker than it
/// is. Meshing draws it as a thin box instead, see `generator::LodBox`.
pub const THIN_TRUNK_CELL: i64 = 4;
// How far in leaves are nibbled from the edge of a crown.
const NIBBLE: f64 = 0.9;
// How far each tree's leaves stray from the grass's tint, so a forest isn't one flat green:
// brighter or darker by up to this much, and warmer or cooler by up to this much.
const TINT_BRIGHTNESS: f32 = 0.22;
const TINT_WARMTH: f32 = 0.18;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TreeKind {
    // a broad, lumpy crown on a stout trunk
    Broadleaf,
    // tiers of leaves narrowing up a tall trunk
    Pine,
    // a wide, flattened crown high on a tall, thick trunk, branching only near the top
    Jungle,
    // flat pads of leaves on limbs spreading from a short trunk
    Acacia,
}

impl TreeKind {
    // Its trunk's height in logs and its crown's spread, `growth` 0..1 of the way from the
    // smallest it grows to the biggest. Jungle trees are all thick enough for 2 x 2 trunks.
    fn size(self, growth: f64) -> (i64, f64) {
        match self {
            TreeKind::Broadleaf => (10 + (growth * 6.0) as i64, 5.0 + growth * 2.0),
            TreeKind::Pine => (16 + (growth * 9.0) as i64, 4.5 + growth * 1.5),
            TreeKind::Jungle => (15 + (growth * 6.0) as i64, 5.5 + growth * 1.5),
            TreeKind::Acacia => (5 + (growth * 4.0) as i64, 4.5 + growth * 1.5),
        }
    }

    // How its crown and lobes are shaped. Pines' tiers are drawn without them.
    fn crown(self) -> Crown {
        match self {
            TreeKind::Broadleaf | TreeKind::Pine => Crown {
                rise: -1.0,
                squash: 0.7,
                out: 0.75,
                drop: (0.25, 0.5),
                lobe: 0.6,
                lobe_squash: 0.75,
                fork: 0.6,
            },
            TreeKind::Jungle => Crown {
                rise: -1.0,
                squash: 0.5,
                out: 0.75,
                drop: (0.1, 0.35),
                lobe: 0.6,
                lobe_squash: 0.55,
                fork: 0.75,
            },
            // pads level with the crown or a little over it, on limbs from low down
            TreeKind::Acacia => Crown {
                rise: 0.0,
                squash: 0.3,
                out: 0.8,
                drop: (-0.25, 0.05),
                lobe: 0.55,
                lobe_squash: 0.35,
                fork: 0.45,
            },
        }
    }
}

// The shape of a crown of blobs, in proportion to its spread.
struct Crown {
    // the crown's centre, in blocks over the top of the trunk
    rise: f64,
    squash: f64,
    // how far out the lobes' centres are, and how far below the crown's, from the first to the
    // second, the last two in spreads
    out: f64,
    drop: (f64, f64),
    // each lobe's radius, in spreads, and its squash
    lobe: f64,
    lobe_squash: f64,
    // how far up the trunk the branches out to the lobes leave it, in trunks
    fork: f64,
}

#[derive(Debug, Copy, Clone)]
pub struct Tree {
    pub kind: TreeKind,
    // the trunk's lowest log, at its -x, -z corner when it's thick
    pub base: I64Vec3,
    // logs up the trunk
    pub trunk: i64,
    // how far the leaves spread from the trunk
    spread: f64,
    // roughens the edge of the leaves
    seed: u64,
    // the crown, and smaller lobes around it each held up by a branch, but for pines
    crown: Blob,
    lobes: [Blob; LOBES],
}

// A ball of leaves, squashed by `squash` vertically.
#[derive(Debug, Copy, Clone)]
struct Blob {
    centre: DVec3,
    radius: f64,
    squash: f64,
}

impl Blob {
    // Whether `point` is inside the blob with its edge drawn in by `nibble` there.
    fn contains(&self, point: DVec3, nibble: impl Fn() -> f64) -> bool {
        let d = (point - self.centre) / DVec3::new(1.0, self.squash, 1.0);
        let distance = d.length_squared();
        if distance > self.radius * self.radius {
            return false;
        }
        let inner = (self.radius - NIBBLE).max(0.0);
        // only the rim gets nibbled, so most points skip working it out
        distance <= inner * inner || distance <= (self.radius - nibble()).powi(2)
    }
}

impl Tree {
    fn new(kind: TreeKind, base: I64Vec3, trunk: i64, spread: f64, seed: u64) -> Self {
        let width = if trunk >= THICK_TRUNK { 2 } else { 1 };
        let middle = (width - 1) as f64 / 2.0;
        let shape = kind.crown();
        let crown = Blob {
            centre: DVec3::new(middle, trunk as f64 + shape.rise, middle),
            radius: spread,
            squash: shape.squash,
        };
        let lobes = std::array::from_fn(|i| {
            let h = hash(seed, i as i64, 17);
            let angle = (i as f64 + (h % 1000) as f64 / 1000.0 * 0.6) * std::f64::consts::TAU
                / LOBES as f64;
            let out = spread * shape.out;
            let (least, most) = shape.drop;
            let drop = spread * (least + (most - least) * ((h >> 10) % 100) as f64 / 100.0);
            Blob {
                centre: crown.centre + DVec3::new(angle.cos() * out, -drop, angle.sin() * out),
                radius: spread * shape.lobe,
                squash: shape.lobe_squash,
            }
        });
        Self {
            kind,
            base,
            trunk,
            spread,
            seed,
            crown,
            lobes,
        }
    }

    /// The tree's own tint over the grass's, the same at every level of detail.
    pub fn tint(&self) -> Vec3 {
        let h = hash(self.seed, 31, 59);
        let unit = |shift: u32| ((h >> shift) & 0xffff) as f32 / 65535.0 * 2.0 - 1.0;
        let warm = Vec3::new(1.0, 0.35, -1.0) * TINT_WARMTH * unit(16);
        (1.0 + TINT_BRIGHTNESS * unit(0)) * (Vec3::ONE + warm)
    }

    /// Whether the crown is over the point `x`, `z` across the world, in blocks, seen from above.
    pub fn shades(&self, x: f64, z: f64) -> bool {
        // from the trunk's base, as `in_leaves` takes it, where a block's index is its middle
        let offset = DVec3::new(
            x - 0.5 - self.base.x as f64,
            0.0,
            z - 0.5 - self.base.z as f64,
        );
        let under = |blob: &Blob| {
            (offset - blob.centre.with_y(0.0)).length_squared() <= blob.radius * blob.radius
        };
        match self.kind {
            // the lowest tier is the widest
            TreeKind::Pine => {
                (offset - self.axis()).length_squared() <= (0.6 + self.spread).powi(2)
            }
            _ => under(&self.crown) || self.lobes.iter().any(under),
        }
    }

    /// The top of the crown, in blocks.
    pub fn crown_top(&self) -> f64 {
        let top = match self.kind {
            TreeKind::Pine => (self.trunk + 1) as f64,
            _ => self.crown.centre.y + self.crown.radius * self.crown.squash,
        };
        self.base.y as f64 + top + 1.0
    }

    pub fn width(&self) -> i64 {
        if self.trunk >= THICK_TRUNK {
            2
        } else {
            1
        }
    }

    // the middle of the trunk, as an offset from `base`
    fn axis(&self) -> DVec3 {
        let middle = (self.width() - 1) as f64 / 2.0;
        DVec3::new(middle, 0.0, middle)
    }

    // The lowest and highest block the tree can fill.
    pub fn bounds(&self) -> (I64Vec3, I64Vec3) {
        let reach = I64Vec3::new(TREE_REACH, 0, TREE_REACH);
        let top = I64Vec3::Y * TREE_HEIGHT;
        (
            self.base - reach,
            self.base + reach + top + I64Vec3::new(1, 0, 1),
        )
    }

    // The tree's block in the cell `size` blocks across with `min` its lowest block, if any:
    // leaves if its centre is among them, a log if the trunk or a branch runs through it. Cells
    // one block across are just blocks, where wood runs up through the leaves; in coarser cells
    // the leaves win, or a whole cell's width of trunk would stand in for the crown. Cells this
    // wide or wider hold leaves if a quarter of them would be, or small crowns would fall between
    // cell centres, and from `THIN_TRUNK_CELL` they leave out the trunk.
    pub fn block_in_cell(&self, min: I64Vec3, size: i64) -> Option<BlockType> {
        let overlaps = |lo: i64, from: i64, to: i64| lo < to && lo + size > from;
        let width = self.width();
        let trunk = overlaps(min.x, self.base.x, self.base.x + width)
            && overlaps(min.z, self.base.z, self.base.z + width)
            && overlaps(min.y, self.base.y, self.base.y + self.trunk);
        let offset = min.as_dvec3() + (size - 1) as f64 / 2.0 - self.base.as_dvec3();
        let leaves = if size >= COVERAGE_CELL {
            self.leaf_coverage(offset, size)
        } else {
            self.in_leaves(offset)
        };
        match (trunk, leaves) {
            (true, true) if size > 1 => Some(BlockType::Leaves),
            (true, _) if size >= THIN_TRUNK_CELL => None,
            (true, _) => Some(BlockType::Log),
            (false, _) if size == 1 && self.in_branch(offset) => Some(BlockType::Log),
            (false, true) => Some(BlockType::Leaves),
            (false, false) => None,
        }
    }

    // Whether enough of the cell centred `offset` from the trunk's base is among the leaves, judged
    // from a 3 x 3 x 3 spread of points through it.
    fn leaf_coverage(&self, offset: DVec3, size: i64) -> bool {
        let step = size as f64 / 3.0;
        let mut inside = 0;
        for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    let point = DVec3::new(x as f64, y as f64, z as f64) * step;
                    if self.in_leaves(offset + point) {
                        inside += 1;
                    }
                }
            }
        }
        inside >= COVERAGE_POINTS
    }

    /// The crown as `layers` stacked boxes, in blocks from `base`'s lower corner, for cells too
    /// wide to hold it: each layer is as wide as the leaves reach there, so a crown keeps its dome
    /// or tiers instead of being one cuboid.
    pub fn crown_boxes(&self, layers: i64) -> Vec<(DVec3, DVec3)> {
        let (low, high) = self.leaf_bounds();
        // only each row's first and last leaf, found from its ends, bear on the layers' bounds
        let mut leaves = Vec::new();
        let xs = low.x.max(-TREE_REACH)..=high.x.min(TREE_REACH);
        for y in low.y.max(0)..=high.y.min(TREE_HEIGHT) {
            for z in low.z.max(-TREE_REACH)..=high.z.min(TREE_REACH) {
                let leaf = |x: i64| self.in_leaves(DVec3::new(x as f64, y as f64, z as f64));
                if let Some(first) = xs.clone().find(|&x| leaf(x)) {
                    let last = xs.clone().rev().find(|&x| leaf(x)).unwrap_or(first);
                    leaves.extend([I64Vec3::new(first, y, z), I64Vec3::new(last, y, z)]);
                }
            }
        }
        let (Some(low), Some(high)) = (
            leaves.iter().map(|l| l.y).min(),
            leaves.iter().map(|l| l.y).max(),
        ) else {
            return Vec::new();
        };
        let height = high - low + 1;
        let layer = |y: i64| ((y - low) * layers / height) as usize;
        let mut bounds: Vec<Option<(I64Vec3, I64Vec3)>> = vec![None; layers as usize];
        for leaf in &leaves {
            let (min, max) = bounds[layer(leaf.y)].get_or_insert((*leaf, *leaf));
            *min = min.min(*leaf);
            *max = max.max(*leaf);
        }
        bounds
            .into_iter()
            .flatten()
            .map(|(min, max)| {
                (
                    (self.base + min).as_dvec3(),
                    (self.base + max + I64Vec3::ONE).as_dvec3(),
                )
            })
            .collect()
    }

    /// Whether any of the tree is over the columns `min..max` (x, z), seen from above: the same
    /// at every level of detail, as only the height of a tree's base differs between them.
    pub fn overhangs(&self, min: I64Vec2, max: I64Vec2) -> bool {
        let (low, high) = self.leaf_bounds();
        let base = self.base.xz();
        let low = base + low.xz().min(I64Vec2::ZERO);
        let high = base + high.xz().max(I64Vec2::splat(self.width() - 1));
        low.cmplt(max).all() && high.cmpge(min).all()
    }

    // Whole-block offsets from the trunk's base that every point `in_leaves` lies within.
    fn leaf_bounds(&self) -> (I64Vec3, I64Vec3) {
        let (low, high) = match self.kind {
            TreeKind::Pine => {
                let wrap = self.width() as f64 / 2.0 + 1.0;
                let reach = (0.6 + self.spread).max(wrap);
                let axis = self.axis();
                (
                    DVec3::new(axis.x - reach, (self.trunk / 4) as f64, axis.z - reach),
                    DVec3::new(axis.x + reach, (self.trunk + 1) as f64, axis.z + reach),
                )
            }
            _ => std::iter::once(&self.crown)
                .chain(&self.lobes)
                .map(|blob| {
                    let reach = DVec3::new(blob.radius, blob.radius * blob.squash, blob.radius);
                    (blob.centre - reach, blob.centre + reach)
                })
                .fold((DVec3::MAX, DVec3::MIN), |(low, high), (min, max)| {
                    (low.min(min), high.max(max))
                }),
        };
        (low.floor().as_i64vec3(), high.ceil().as_i64vec3())
    }

    // Whether the point `offset` from the trunk's base is among the leaves.
    fn in_leaves(&self, offset: DVec3) -> bool {
        // nibbled at the edges, so crowns aren't perfect balls
        let nibble = || {
            let rounded = offset.round().as_i64vec3();
            if hash(self.seed, rounded.x * 7919 + rounded.y, rounded.z).is_multiple_of(3) {
                NIBBLE
            } else {
                0.0
            }
        };
        match self.kind {
            TreeKind::Pine => {
                let (bottom, top) = ((self.trunk / 4) as f64, (self.trunk + 1) as f64);
                if !(bottom..=top).contains(&offset.y) {
                    return false;
                }
                let t = (offset.y - bottom) / (top - bottom);
                // each tier is widest at its foot and drawn in towards the next
                let tier = 1.0 - 0.3 * ((offset.y.round() as i64).rem_euclid(3) as f64 / 2.0);
                // never too narrow to wrap the trunk, which runs up to the tip
                let wrap = self.width() as f64 / 2.0 + 1.0;
                let radius = ((0.6 + self.spread * (1.0 - t)) * tier - nibble() * 0.5).max(wrap);
                let across = offset - self.axis();
                across.x * across.x + across.z * across.z <= radius * radius
            }
            _ => {
                self.crown.contains(offset, nibble)
                    || self.lobes.iter().any(|lobe| lobe.contains(offset, nibble))
            }
        }
    }

    // Whether the point `offset` from the trunk's base is on a branch out to a lobe.
    fn in_branch(&self, offset: DVec3) -> bool {
        if self.kind == TreeKind::Pine {
            return false;
        }
        let fork = self.axis() + DVec3::Y * (self.trunk as f64 * self.kind.crown().fork);
        self.lobes.iter().any(|lobe| {
            let along = lobe.centre - fork;
            let t = ((offset - fork).dot(along) / along.length_squared()).clamp(0.0, 1.0);
            (offset - (fork + along * t)).length_squared() <= 0.36
        })
    }

    // Puts the tree into a grid of `size`-block cells whose cell 0 has `origin` as its lowest
    // block, calling `place` with each cell index in `first..=last` that the tree fills.
    pub fn plant(
        &self,
        origin: I64Vec3,
        size: i64,
        (first, last): (IVec3, IVec3),
        mut place: impl FnMut(IVec3, BlockType),
    ) {
        // the leaves, the trunk, and the branches running between them, within a block of each
        let (leaves_low, leaves_high) = self.leaf_bounds();
        let width = self.width();
        let low = self.base + leaves_low.min(I64Vec3::ZERO) - 1;
        let high = self.base + leaves_high.max(I64Vec3::new(width, self.trunk, width) - 1) + 1;
        let cell_of = |block: I64Vec3| (block - origin).div_euclid(I64Vec3::splat(size)).as_ivec3();
        let (low, high) = (cell_of(low).max(first), cell_of(high).min(last));
        for x in low.x..=high.x {
            for z in low.z..=high.z {
                for y in low.y..=high.y {
                    let cell = IVec3::new(x, y, z);
                    if let Some(block) = self.block_in_cell(origin + cell.as_i64vec3() * size, size)
                    {
                        place(cell, block);
                    }
                }
            }
        }
    }
}

/// Which tree's tint each leaf takes, for leaves placed block by block, where which tree put
/// them is lost: the one whose trunk is nearest across, which is the one they belong to
/// but where crowns meet.
#[derive(Default, Debug, Clone)]
pub struct TreeTints(Vec<(Vec2, Vec3)>);

impl TreeTints {
    /// Every tree in `trees`, placed by its trunk's axis less `origin`, in cells `cell` blocks
    /// wide whose centres are whole numbers.
    pub fn new<'a>(trees: impl IntoIterator<Item = &'a Tree>, origin: I64Vec3, cell: i64) -> Self {
        Self(
            trees
                .into_iter()
                .map(|t| {
                    let axis = (t.base - origin).as_dvec3() + t.axis();
                    (
                        Vec2::new(axis.x as f32, axis.z as f32) / cell as f32
                            - (cell - 1) as f32 / (2.0 * cell as f32),
                        t.tint(),
                    )
                })
                .collect(),
        )
    }

    /// The tint of the leaf at `coord`, in cells.
    pub fn at(&self, coord: IVec3) -> Vec3 {
        let p = Vec2::new(coord.x as f32, coord.z as f32);
        self.0
            .iter()
            .min_by(|a, b| a.0.distance_squared(p).total_cmp(&b.0.distance_squared(p)))
            .map_or(Vec3::ONE, |&(_, tint)| tint)
    }

    pub fn heap_size(&self) -> usize {
        self.0.capacity() * size_of::<(Vec2, Vec3)>()
    }
}

// Whether a tree's `block` may go where `existing` is: into air and over plants, and logs
// through leaves.
pub fn grows_into(existing: BlockType, block: BlockType) -> bool {
    existing == BlockType::Air
        || existing.is_plant()
        || (existing == BlockType::Leaves && block == BlockType::Log)
}

// Every tree that could reach into the columns `min..=max` (x, z).
pub fn trees_near(
    noise: &NoiseGenerator,
    heights: &mut impl TreeGround,
    min: I64Vec2,
    max: I64Vec2,
) -> Vec<Tree> {
    let first = (min - TREE_REACH).div_euclid(I64Vec2::splat(CELL));
    let last = (max + TREE_REACH).div_euclid(I64Vec2::splat(CELL));
    let mut trees = Vec::new();
    for cz in first.y..=last.y {
        for cx in first.x..=last.x {
            if let Some(tree) = tree_in_cell(noise, heights, cx, cz) {
                trees.push(tree);
            }
        }
    }
    trees
}

fn tree_in_cell(
    noise: &NoiseGenerator,
    heights: &mut impl TreeGround,
    cx: i64,
    cz: i64,
) -> Option<Tree> {
    let seed = hash(noise.seed() as u64, cx, cz);
    let x = cx * CELL + ((seed >> 16) % CELL as u64) as i64;
    let z = cz * CELL + ((seed >> 24) % CELL as u64) as i64;
    let chance = (seed & 0xffff) as f64 / 65536.0;
    if heights.rules_out(x, z, chance) || chance >= heights.trees(x, z) {
        return None;
    }

    // the biome before the slope, which samples elsewhere, while the ground here is to hand
    let height = heights.height(x, z).round() as i64;
    let biome = heights.biome(x, z);
    let (gradient_x, gradient_z) = heights.gradient(x, z);
    let gradient = gradient_x.abs() + gradient_z.abs();
    if gradient > MAX_GRADIENT {
        return None;
    }
    let temperature = noise.temperature(x as f64, z as f64);
    let biomes = noise.biomes();
    let ground = column_block(biomes, biome, height - 1, height, gradient, temperature);
    if !biomes.roots(biome, ground, height - 1 - WATER_LEVEL) {
        return None;
    }

    // 0..1, which kind its biome grows, and how big this one grew
    let pick = (hash(seed, 3, 5) % 1000) as f64 / 1000.0;
    let growth = ((seed >> 32) % 1000) as f64 / 1000.0;
    let kind = biomes
        .tree_kind(biome, pick)
        .unwrap_or(if temperature < PINE_TEMPERATURE {
            TreeKind::Pine
        } else {
            TreeKind::Broadleaf
        });
    let (trunk, spread) = kind.size(growth);
    Some(Tree::new(
        kind,
        I64Vec3::new(x, heights.base_height(x, z), z),
        trunk,
        spread,
        seed,
    ))
}

// splitmix64 over a seed and two coordinates
pub(crate) fn hash(seed: u64, a: i64, b: i64) -> u64 {
    let mut h = seed
        ^ (a as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (b as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^ (h >> 31)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::chunks::generate::region_store::RegionStore;

    fn tree(kind: TreeKind, trunk: i64, spread: f64) -> Tree {
        Tree::new(kind, I64Vec3::new(100, 500, -40), trunk, spread, 7)
    }

    const KINDS: [TreeKind; 4] = [
        TreeKind::Broadleaf,
        TreeKind::Pine,
        TreeKind::Jungle,
        TreeKind::Acacia,
    ];

    // the smallest and largest each kind grows
    fn extremes() -> Vec<Tree> {
        KINDS
            .into_iter()
            .flat_map(|kind| {
                [0.0, 0.999].map(|growth| {
                    let (trunk, spread) = kind.size(growth);
                    tree(kind, trunk, spread)
                })
            })
            .collect()
    }

    fn blocks(tree: &Tree) -> Vec<(I64Vec3, BlockType)> {
        let (low, high) = tree.bounds();
        let mut blocks = Vec::new();
        tree.plant(
            low,
            1,
            (IVec3::ZERO, (high - low).as_ivec3()),
            |cell, block| {
                blocks.push((low + cell.as_i64vec3(), block));
            },
        );
        blocks
    }

    #[test]
    fn test_leaves_lie_within_their_bounds() {
        for tree in extremes() {
            let (low, high) = tree.leaf_bounds();
            for y in -2..=TREE_HEIGHT + 2 {
                for x in -TREE_REACH - 2..=TREE_REACH + 2 {
                    for z in -TREE_REACH - 2..=TREE_REACH + 2 {
                        let at = I64Vec3::new(x, y, z);
                        if tree.in_leaves(at.as_dvec3()) {
                            assert!(at.cmpge(low).all() && at.cmple(high).all(), "{at}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_trees_are_a_trunk_under_big_crowns() {
        for t in extremes() {
            let blocks = blocks(&t);
            let in_trunk = |p: I64Vec3| {
                (0..t.width()).contains(&(p.x - t.base.x))
                    && (0..t.width()).contains(&(p.z - t.base.z))
            };
            let trunk_logs = blocks
                .iter()
                .filter(|(p, b)| *b == BlockType::Log && in_trunk(*p))
                .count();
            assert_eq!((t.trunk * t.width() * t.width()) as usize, trunk_logs);
            let branches = blocks
                .iter()
                .filter(|(p, b)| *b == BlockType::Log && !in_trunk(*p))
                .count();
            assert_eq!(
                t.kind != TreeKind::Pine,
                branches > 0,
                "{:?} has {branches} branch logs",
                t.kind
            );
            let leaves = blocks
                .iter()
                .filter(|(_, b)| *b == BlockType::Leaves)
                .count();
            // an acacia's pads are thin
            let least = if t.kind == TreeKind::Acacia { 100 } else { 200 };
            assert!(leaves > least, "{:?} has only {leaves} leaves", t.kind);
            // the trunk's top is hidden in the leaves, or the limbs leaving it, all the way round
            let top = t.base + I64Vec3::Y * (t.trunk - 1);
            assert_eq!(
                Some(BlockType::Leaves),
                t.block_in_cell(top + I64Vec3::Y, 1)
            );
            for side in [
                I64Vec3::NEG_X,
                I64Vec3::NEG_Z,
                I64Vec3::X * t.width(),
                I64Vec3::Z * t.width(),
            ] {
                assert!(
                    t.block_in_cell(top + side, 1).is_some(),
                    "{:?} trunk bare at {side}",
                    t.kind
                );
            }
        }
    }

    #[test]
    fn test_trees_stay_within_their_reach() {
        for t in extremes() {
            let (low, high) = (
                t.base - I64Vec3::new(TREE_REACH, 0, TREE_REACH),
                t.base + I64Vec3::new(TREE_REACH, TREE_HEIGHT, TREE_REACH),
            );
            // look well past the bounds, so anything poking out is caught
            for x in low.x - 4..=high.x + 4 {
                for z in low.z - 4..=high.z + 4 {
                    for y in low.y - 4..=high.y + 4 {
                        let p = I64Vec3::new(x, y, z);
                        if t.block_in_cell(p, 1).is_some() {
                            assert!(
                                p.cmpge(low).all() && p.cmple(high).all(),
                                "{:?} reaches {p}",
                                t.kind
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn test_coarse_cells_keep_the_trunk_and_crown() {
        let t = tree(TreeKind::Broadleaf, 12, 6.0);
        // 2-block cells aligned so the trunk's column isn't at a cell's centre
        let origin = I64Vec3::new(96, 496, -44);
        let mut seen = Vec::new();
        t.plant(
            origin,
            2,
            (IVec3::splat(-8), IVec3::splat(20)),
            |_, block| seen.push(block),
        );
        assert!(seen.contains(&BlockType::Log));
        assert!(seen.contains(&BlockType::Leaves));
        // from `THIN_TRUNK_CELL` the trunk is drawn as a box of its own, so the cells are crown alone
        let mut wide = Vec::new();
        t.plant(
            origin,
            THIN_TRUNK_CELL,
            (IVec3::splat(-4), IVec3::splat(10)),
            |_, block| wide.push(block),
        );
        assert!(wide.contains(&BlockType::Leaves) && !wide.contains(&BlockType::Log));
    }

    #[test]
    fn test_logs_grow_through_leaves_but_nothing_into_ground() {
        assert!(grows_into(BlockType::Air, BlockType::Leaves));
        assert!(grows_into(BlockType::Leaves, BlockType::Log));
        assert!(!grows_into(BlockType::Log, BlockType::Leaves));
        assert!(grows_into(BlockType::TallGrass, BlockType::Log));
        assert!(grows_into(BlockType::Poppy, BlockType::Leaves));
        assert!(!grows_into(BlockType::Grass, BlockType::Log));
        assert!(!grows_into(BlockType::Water, BlockType::Leaves));
    }

    #[test]
    fn test_trees_stand_on_grass_and_agree_between_lookups() {
        let noise = Arc::new(NoiseGenerator::new(5));
        let regions = RegionStore::new(noise.clone());
        let mut heights = regions.sampler();
        let (min, max) = (I64Vec2::new(-600, -600), I64Vec2::new(600, 600));
        let trees = trees_near(&noise, &mut heights, min, max);
        assert!(!trees.is_empty(), "no trees in {min}..{max}");
        for t in &trees {
            let ground = heights.height(t.base.x, t.base.z).round() as i64;
            assert_eq!(ground, t.base.y);
        }
        // a smaller window finds the same trees where they overlap
        let some = trees_near(
            &noise,
            &mut heights,
            I64Vec2::new(0, 0),
            I64Vec2::new(40, 40),
        );
        for t in &some {
            assert!(trees
                .iter()
                .any(|o| o.base == t.base && o.trunk == t.trunk && o.kind == t.kind));
        }
    }
    // Every cell `size` blocks across the tree fills, from its bounds.
    fn cells(tree: &Tree, size: i64) -> Vec<(I64Vec3, BlockType)> {
        let (low, high) = tree.bounds();
        let origin = I64Vec3::new(
            low.x.div_euclid(size),
            low.y.div_euclid(size),
            low.z.div_euclid(size),
        ) * size;
        let mut cells = Vec::new();
        tree.plant(
            origin,
            size,
            (IVec3::ZERO, ((high - origin) / size).as_ivec3()),
            |cell, block| {
                cells.push((origin + cell.as_i64vec3() * size, block));
            },
        );
        cells
    }

    #[test]
    fn test_every_tree_leaves_a_crown_in_cells_four_and_eight_wide() {
        for t in extremes() {
            // the thinnest pines are a spire a cell or two across at eight, and an acacia's pads
            // can fall between eight-wide cells' points, though LODs draw crowns as boxes from
            // there, `BOX_CROWN_CELL` (an acacia's from four), anyway
            let at_eight = (t.kind != TreeKind::Acacia) as usize;
            for (size, least) in [(4, 2), (8, at_eight)] {
                let leaves = cells(&t, size)
                    .into_iter()
                    .filter(|(_, b)| *b == BlockType::Leaves)
                    .count();
                assert!(
                    leaves >= least,
                    "{:?} {} trunk at {size}: {leaves} leaf cells",
                    t.kind,
                    t.trunk
                );
            }
        }
    }

    #[test]
    fn test_trunks_stand_in_cells_up_to_two_wide_and_not_beyond() {
        let t = tree(TreeKind::Pine, 24, 6.0);
        let logs = |size| {
            cells(&t, size)
                .into_iter()
                .filter(|(_, b)| *b == BlockType::Log)
                .count()
        };
        assert!(logs(1) > 0 && logs(2) > 0);
        assert_eq!(0, logs(THIN_TRUNK_CELL));
        assert_eq!(0, logs(8));
    }

    #[test]
    fn test_crowns_in_cells_four_wide_keep_to_full_details_within_a_cell() {
        // level 2 plants its trees as leaf cells 4 wide, so they should stand where full
        // detail's leaves are, and be about as big, to within its cells. Not acacias, whose
        // thin pads would fill cells this wide over twice over, so are boxes there
        const SIZE: i64 = 4;
        for t in extremes()
            .into_iter()
            .filter(|t| t.kind != TreeKind::Acacia)
        {
            let full: Vec<I64Vec3> = blocks(&t)
                .into_iter()
                .filter(|(_, b)| *b == BlockType::Leaves)
                .map(|(p, _)| p)
                .collect();
            let coarse: Vec<I64Vec3> = cells(&t, SIZE)
                .into_iter()
                .filter(|(_, b)| *b == BlockType::Leaves)
                .map(|(p, _)| p)
                .collect();
            let what = format!("{:?} {} trunk", t.kind, t.trunk);
            assert!(!coarse.is_empty(), "{what}: no leaf cells");
            // how far a block is outside the cell at `p`, in blocks, each way
            let outside = |block: I64Vec3, p: I64Vec3| {
                (p - block).max(block - (p + SIZE - 1)).max(I64Vec3::ZERO)
            };
            // every leaf cell holds full detail's leaves, or touches them
            for &p in &coarse {
                assert!(
                    full.iter().any(|&b| outside(b, p).max_element() <= 1),
                    "{what}: the leaf cell at {p} holds no leaves"
                );
            }
            // and every leaf is in a leaf cell, or a cell from one, as a crown's thin edges
            // don't fill a cell
            for &b in &full {
                assert!(
                    coarse.iter().any(|&p| outside(b, p).max_element() <= SIZE),
                    "{what}: the leaf at {b} is more than a cell from any leaf cell"
                );
            }
            // the cells hold about as much crown as there is, a little more, as a cell is leaves
            // where most of it is
            let volume = (coarse.len() as i64 * SIZE.pow(3)) as f64 / full.len() as f64;
            assert!(
                (1.0..1.8).contains(&volume),
                "{what}: leaf cells hold {volume:.2} times the leaves"
            );
        }
    }

    #[test]
    fn test_coverage_widens_a_crown_by_less_than_a_cell() {
        let t = tree(TreeKind::Broadleaf, 15, 7.0);
        for size in [4, 8] {
            let reach = cells(&t, size)
                .into_iter()
                .filter(|(_, b)| *b == BlockType::Leaves)
                .map(|(p, _)| {
                    (p.x + size / 2 - t.base.x)
                        .abs()
                        .max((p.z + size / 2 - t.base.z).abs())
                })
                .max()
                .unwrap();
            // a crown's lobes hang out to about 1.35 times its spread, and a cell edge can add up to a cell
            assert!(
                reach as f64 <= t.spread * 1.35 + size as f64,
                "reach {reach} at {size}"
            );
        }
    }
}
