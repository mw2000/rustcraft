//! Structures (§5.6): templates read from `assets/structures/`, placed on a grid of cells as
//! trees are (INV-3), and built into full-detail chunks and LOD nodes alike (INV-5).

mod template;

use std::sync::LazyLock;

use bevy::math::{I64Vec2, I64Vec3, IVec3, Vec3Swizzles};

use super::columns::Recent;
use super::generator::ground_top;
use super::noise::NoiseGenerator;
use super::trees::{hash, Tree, TREE_REACH};
use super::WATER_LEVEL;
use crate::block::BlockType;
pub use template::{Placement, Structure, Template, STRUCTURE_DEPTH, STRUCTURE_HEIGHT};

// Each structure type's file, built into the game.
const FILES: [&str; 1] = [include_str!(
    "../../../../assets/structures/watchtower.toml"
)];

/// Every structure type, read once from the files built into the game, so generation reads no
/// files, on a server as on a client, and stays a function of the seed (INV-1).
pub static STRUCTURES: LazyLock<Vec<Structure>> = LazyLock::new(|| {
    FILES
        .iter()
        .map(|text| toml::from_str(text).unwrap_or_else(|e| panic!("a structure won't load: {e}")))
        .collect()
});

// Trees keep this many blocks clear of a structure's footprint, so none grows into it.
const CLEARING: i64 = 2;
/// How far round an area to look for the structures that could change it: as far as trees
/// reach, and as far again for the clearings round structures that keep those trees out.
pub const STRUCTURE_REACH: i64 = 2 * TREE_REACH + CLEARING;
// A structure on a hilltop climbs from its spot to the top of its hill, in steps this many
// blocks long, at most this many of them. It keeps an eighth of its cell (1 / `EDGE_MARGIN`)
// from the cell's edge, so those in cells side by side never meet on one hilltop.
const CLIMB_STRIDE: i64 = 12;
const CLIMB_STEPS: usize = 16;
const EDGE_MARGIN: i64 = 8;
// Cells remembered, a few dozen bytes each.
const PLACEMENTS_KEPT: usize = 65_536;

/// A structure standing in the world.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Placed {
    // in `STRUCTURES`
    kind: usize,
    /// Quarter turns from its template as written, 0..4.
    pub rotation: u8,
    /// Its footprint's lowest corner, at the height of its floor.
    pub min: I64Vec3,
    /// The lowest and highest ground under its footprint: its foundation fills up from the
    /// one, and the ground is cleared down to its floor from the other.
    pub lowest: i64,
    pub highest: i64,
}

impl Placed {
    pub fn structure(&self) -> &'static Structure {
        &STRUCTURES[self.kind]
    }

    fn template(&self) -> &'static Template {
        &self.structure().template
    }

    /// Its footprint across x and z.
    pub fn footprint(&self) -> I64Vec2 {
        self.template().footprint(self.rotation)
    }

    /// Whether column `x`, `z` is in its footprint.
    pub fn covers(&self, x: i64, z: i64) -> bool {
        let at = I64Vec2::new(x, z) - self.min.xz();
        at.cmpge(I64Vec2::ZERO).all() && at.cmplt(self.footprint()).all()
    }

    // Its lowest layer's corner, where its template's cell 0 is.
    fn corner(&self) -> I64Vec3 {
        self.min - I64Vec3::Y * self.template().floor
    }

    /// The lowest block it can change, and the block past the highest.
    pub fn bounds(&self) -> (I64Vec3, I64Vec3) {
        let corner = self.corner();
        let bottom = corner.y.min(ground_top(self.lowest));
        let top = (corner.y + self.template().size.y).max(ground_top(self.highest) + 1);
        let footprint = self.footprint();
        (
            corner.with_y(bottom),
            I64Vec3::new(corner.x + footprint.x, top, corner.z + footprint.y),
        )
    }

    // What the block `at` in its footprint becomes, over ground `height` high, if the structure
    // changes it: its own blocks and the air it clears; under its floor, the foundation up from
    // the ground; and over it, air down to the floor, clearing the ground and what grows on it.
    fn stamp(&self, at: I64Vec3, height: i64) -> Option<BlockType> {
        let template = self.template();
        if let Some(block) = template.cell(self.rotation, at - self.corner()) {
            return Some(block);
        }
        let ground = ground_top(height);
        if at.y < self.min.y {
            (at.y >= ground).then_some(template.foundation)
        } else {
            (at.y <= ground).then_some(BlockType::Air)
        }
    }

    /// Builds the structure into a grid of `size`-block cells whose cell 0 has `origin` as its
    /// lowest block, calling `place` with each cell index in `first..=last` it changes and what
    /// that cell becomes, with `height_at(x, z)` the ground's height in each column it stands
    /// over. A cell takes the commonest of the structure's blocks in it where at least half of
    /// it is those, or air where at least half is those and the air it clears, so cells a block
    /// wide are exactly its blocks, and coarser ones what most of their blocks are.
    pub fn build(
        &self,
        origin: I64Vec3,
        size: i64,
        (first, last): (IVec3, IVec3),
        mut height_at: impl FnMut(i64, i64) -> i64,
        mut place: impl FnMut(IVec3, BlockType),
    ) {
        let (low, high) = self.bounds();
        let cell_of = |block: I64Vec3| (block - origin).div_euclid(I64Vec3::splat(size)).as_ivec3();
        let (from, to) = (cell_of(low).max(first), cell_of(high - 1).min(last));
        if from.cmpgt(to).any() {
            return;
        }
        let footprint = self.footprint();
        // asked once a column, and only for the columns the cells cover
        let mut heights = vec![None; (footprint.x * footprint.y) as usize];
        let volume = size * size * size;
        let mut blocks = Tally::default();
        for x in from.x..=to.x {
            for z in from.z..=to.z {
                for y in from.y..=to.y {
                    let cell = IVec3::new(x, y, z);
                    let min = origin + cell.as_i64vec3() * size;
                    blocks.0.clear();
                    let mut cleared = 0;
                    for bx in min.x..min.x + size {
                        for bz in (min.z..min.z + size).filter(|&bz| self.covers(bx, bz)) {
                            let column = (bx - self.min.x) * footprint.y + bz - self.min.z;
                            let height =
                                *heights[column as usize].get_or_insert_with(|| height_at(bx, bz));
                            for by in min.y..min.y + size {
                                match self.stamp(I64Vec3::new(bx, by, bz), height) {
                                    Some(BlockType::Air) => cleared += 1,
                                    Some(block) => blocks.add(block),
                                    None => {}
                                }
                            }
                        }
                    }
                    let built = blocks.total();
                    if let Some(block) = blocks.commonest().filter(|_| 2 * built >= volume) {
                        place(cell, block);
                    } else if 2 * (built + cleared) >= volume {
                        place(cell, BlockType::Air);
                    }
                }
            }
        }
    }

    /// The structure for cells too wide to hold it, as boxes of its commonest block, one for
    /// each `slab` blocks up from its lowest layer, each as wide as its blocks there, over a box
    /// of its foundation down from its lowest layer to the ground under it or to `ground`,
    /// whichever is lower. Each is its lowest block and the block past its highest.
    pub fn boxes(&self, slab: i64, ground: i64) -> Vec<(I64Vec3, I64Vec3, BlockType)> {
        let template = self.template();
        let corner = self.corner();
        let footprint = self.footprint();
        let mut boxes = Vec::new();
        let under = ground.min(ground_top(self.lowest));
        if under < corner.y {
            let top = I64Vec3::new(footprint.x, 0, footprint.y);
            boxes.push((corner.with_y(under), corner + top, template.foundation));
        }
        for start in (0..template.size.y).step_by(slab as usize) {
            let (mut low, mut high) = (I64Vec3::MAX, I64Vec3::MIN);
            let mut blocks = Tally::default();
            for y in start..(start + slab).min(template.size.y) {
                for z in 0..footprint.y {
                    for x in 0..footprint.x {
                        let at = I64Vec3::new(x, y, z);
                        let block = template.cell(self.rotation, at);
                        if let Some(block) = block.filter(|&b| b != BlockType::Air) {
                            (low, high) = (low.min(at), high.max(at));
                            blocks.add(block);
                        }
                    }
                }
            }
            if let Some(block) = blocks.commonest() {
                boxes.push((corner + low, corner + high + 1, block));
            }
        }
        boxes
    }

    /// Whether `tree` would grow into it, or into the clearing kept round it.
    pub fn crowds(&self, tree: &Tree) -> bool {
        let min = self.min.xz();
        tree.overhangs(min - CLEARING, min + self.footprint() + CLEARING)
    }

    /// Its markers, by kind, where they are in the world.
    pub fn markers(&self) -> impl Iterator<Item = (&'static str, I64Vec3)> + '_ {
        let template = self.template();
        template.markers.iter().map(move |(kind, at)| {
            (
                kind.as_str(),
                self.corner() + template.turn(self.rotation, *at),
            )
        })
    }
}

/// Takes out the trees that would grow into any of `structures`, or the clearings round them.
pub fn clear_trees(trees: &mut Vec<Tree>, structures: &[Placed]) {
    if !structures.is_empty() {
        trees.retain(|tree| !structures.iter().any(|s| s.crowds(tree)));
    }
}

// How many of each block, in the order they're first seen.
#[derive(Default)]
struct Tally(Vec<(BlockType, i64)>);

impl Tally {
    fn add(&mut self, block: BlockType) {
        match self.0.iter_mut().find(|(b, _)| *b == block) {
            Some((_, count)) => *count += 1,
            None => self.0.push((block, 1)),
        }
    }

    fn total(&self) -> i64 {
        self.0.iter().map(|(_, count)| count).sum()
    }

    // the first seen of those seen most
    fn commonest(&self) -> Option<BlockType> {
        let most = self.0.iter().map(|(_, count)| *count).max()?;
        self.0.iter().find(|(_, c)| *c == most).map(|(b, _)| *b)
    }
}

/// The structures placed so far, remembered by cell: deciding one takes dozens of noise samples,
/// and every chunk and LOD node near it asks.
pub struct Placements(Recent<(usize, i64, i64), Option<Placed>>);

impl Default for Placements {
    fn default() -> Self {
        Self(Recent::new(PLACEMENTS_KEPT))
    }
}

impl Placements {
    /// Every structure whose footprint overlaps the columns `min..=max` (x, z), in a fixed
    /// order. Blocks while another thread places one, so only call from a task.
    pub fn near(&self, noise: &NoiseGenerator, min: I64Vec2, max: I64Vec2) -> Vec<Placed> {
        let mut found = Vec::new();
        for (kind, structure) in STRUCTURES.iter().enumerate() {
            let cell = I64Vec2::splat(structure.placement.cell);
            // its footprint is round the spot it's placed at, which is in its cell
            let reach = structure.template.size.xz().max_element() / 2 + 1;
            let (first, last) = (
                (min - reach).div_euclid(cell),
                (max + reach).div_euclid(cell),
            );
            for cz in first.y..=last.y {
                for cx in first.x..=last.x {
                    // most cells are ruled out by their hash alone, without waiting on others
                    let Some(seed) = chosen(noise, kind, cx, cz) else {
                        continue;
                    };
                    let cell = I64Vec2::new(cx, cz);
                    let placed = *self
                        .0
                        .get((kind, cx, cz), || place(noise, kind, cell, seed));
                    let overlaps = |p: &Placed| {
                        p.min.xz().cmple(max).all() && (p.min.xz() + p.footprint()).cmpgt(min).all()
                    };
                    found.extend(placed.filter(overlaps));
                }
            }
        }
        found
    }
}

// The hash deciding the structure of `kind` in cell `cx`, `cz`, if the cell is among the
// `chance` of cells that may have one: only those go on to sample the ground.
fn chosen(noise: &NoiseGenerator, kind: usize, cx: i64, cz: i64) -> Option<u64> {
    let structure = &STRUCTURES[kind];
    // each type's grid is salted by its name, so types don't line up, nor shift as others come
    let salt = structure.name.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)
    });
    let seed = hash(noise.seed() as u64 ^ salt, cx, cz);
    let chance = (seed >> 11) as f64 / (1u64 << 53) as f64;
    (chance < structure.placement.chance).then(|| hash(seed, 1, 0))
}

// The structure of `kind` in `cell`, from its hash `seed`, if the ground there suits it. Decided
// from the noise alone, so full detail and every LOD level place the same ones (INV-5).
fn place(noise: &NoiseGenerator, kind: usize, cell: I64Vec2, seed: u64) -> Option<Placed> {
    let Structure {
        placement,
        template,
        ..
    } = &STRUCTURES[kind];
    let corner = cell * placement.cell;
    let margin = placement.cell / EDGE_MARGIN;
    let within = |spot: I64Vec2| {
        let at = spot - corner;
        at.cmpge(I64Vec2::splat(margin)).all()
            && at.cmplt(I64Vec2::splat(placement.cell - margin)).all()
    };
    let span = (placement.cell - 2 * margin) as u64;
    let mut spot =
        corner + margin + I64Vec2::new((seed % span) as i64, (seed >> 32) as i64 % span as i64);
    let suits = |at: I64Vec2| {
        let biome = noise.sample(at.x as f64, at.y as f64).biome;
        let name = noise.biomes().name(biome);
        placement.biomes.iter().any(|b| name == Some(b.as_str()))
    };
    if !suits(spot) {
        return None;
    }
    if placement.hilltop {
        let mut here = height(noise, spot);
        let mut summit = false;
        for _ in 0..CLIMB_STEPS {
            let (next, up) = [I64Vec2::X, I64Vec2::NEG_X, I64Vec2::Y, I64Vec2::NEG_Y]
                .map(|step| spot + step * CLIMB_STRIDE)
                .map(|at| (at, height(noise, at)))
                .into_iter()
                .max_by_key(|&(_, h)| h)
                .unwrap();
            if up <= here {
                summit = true;
                break;
            }
            if !within(next) {
                return None;
            }
            (spot, here) = (next, up);
        }
        if !summit || !suits(spot) {
            return None;
        }
    }

    let rotation = (seed >> 62) as u8;
    let footprint = template.footprint(rotation);
    let min = spot - footprint / 2;
    let mut heights: Vec<i64> = (0..footprint.x)
        .flat_map(|x| (0..footprint.y).map(move |z| min + I64Vec2::new(x, z)))
        .map(|at| height(noise, at))
        .collect();
    heights.sort_unstable();
    let (lowest, highest) = (heights[0], heights[heights.len() - 1]);
    // its floor is at the middle height, so it cuts into the ground as much as it stands over it
    let floor = ground_top(heights[heights.len() / 2]);
    let suits = highest - lowest <= placement.max_rise
        && lowest > WATER_LEVEL
        && floor - WATER_LEVEL >= placement.min_above_sea;
    suits.then_some(Placed {
        kind,
        rotation,
        min: I64Vec3::new(min.x, floor, min.y),
        lowest,
        highest,
    })
}

// The ground's height at `at`, rounded as chunks round the regions' heights, which are stored
// as floats.
fn height(noise: &NoiseGenerator, at: I64Vec2) -> i64 {
    (noise.sample(at.x as f64, at.y as f64).height as f32 as f64).round() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    // An area of seed 3's world twelve kilometres across, with a few dozen structures in it.
    const AREA: (I64Vec2, I64Vec2) = (I64Vec2::splat(-6000), I64Vec2::splat(5999));

    fn watchtower() -> &'static Structure {
        STRUCTURES.iter().find(|s| s.name == "watchtower").unwrap()
    }

    #[test]
    fn test_the_watchtower_loads_tall_with_its_loot_on_a_floor_inside() {
        let template = &watchtower().template;
        let size = template.size;
        assert!(size.y - template.floor >= 16 && size.y - template.floor <= STRUCTURE_HEIGHT);
        assert!(template.floor <= STRUCTURE_DEPTH);
        let loot: Vec<_> = template
            .markers
            .iter()
            .filter(|(k, _)| k == "loot")
            .collect();
        assert_eq!(1, loot.len());
        let at = loot[0].1;
        assert!(at.xz().cmpgt(I64Vec2::ZERO).all() && at.xz().cmplt(size.xz() - 1).all());
        let solid = |at: I64Vec3| template.cell(0, at).is_some_and(|b| b != BlockType::Air);
        assert!(solid(at - I64Vec3::Y) && !solid(at) && !solid(at + I64Vec3::Y));
        // its walls stand on a footing all the way round
        for x in 0..size.x {
            for z in 0..size.z {
                assert!(solid(I64Vec3::new(x, 0, z)));
            }
        }
    }

    #[test]
    fn test_placement_is_the_same_whatever_asks_and_in_whatever_order() {
        let noise = NoiseGenerator::new(3);
        let all = Placements::default().near(&noise, AREA.0, AREA.1);
        assert!(all.len() >= 20, "only {} structures", all.len());
        // a fresh store asked for windows of it, last first, places the same ones in each
        let placements = Placements::default();
        for window in (0..9).rev() {
            let min = AREA.0 + I64Vec2::new(window % 3, window / 3) * 4000;
            let max = min + 3999;
            let mut found = placements.near(&noise, min, max);
            let mut wanted: Vec<_> = all
                .iter()
                .filter(|p| {
                    (p.min.xz() + p.footprint()).cmpgt(min).all() && p.min.xz().cmple(max).all()
                })
                .copied()
                .collect();
            let key = |p: &Placed| (p.min.x, p.min.z);
            found.sort_by_key(key);
            wanted.sort_by_key(key);
            assert_eq!(wanted, found, "window {min}..={max}");
        }
        // and another world places others
        let elsewhere = Placements::default().near(&NoiseGenerator::new(4), AREA.0, AREA.1);
        assert!(!elsewhere.iter().any(|p| all.contains(p)));
    }

    #[test]
    fn test_structures_stand_on_hilltops_their_placement_suits() {
        let noise = NoiseGenerator::new(3);
        let all = Placements::default().near(&noise, AREA.0, AREA.1);
        for p in &all {
            let placement = &p.structure().placement;
            let spot = p.min.xz() + p.footprint() / 2;
            let biome = noise.sample(spot.x as f64, spot.y as f64).biome;
            let biome = noise.biomes().name(biome).unwrap_or("none");
            assert!(
                placement.biomes.iter().any(|b| b == biome),
                "{p:?} in {biome}"
            );
            assert!(p.highest - p.lowest <= placement.max_rise);
            assert!((p.lowest..=p.highest).contains(&p.min.y));
            assert!(p.lowest > WATER_LEVEL && p.min.y - WATER_LEVEL >= placement.min_above_sea);
            for step in [I64Vec2::X, I64Vec2::NEG_X, I64Vec2::Y, I64Vec2::NEG_Y] {
                assert!(height(&noise, spot + step * CLIMB_STRIDE) <= height(&noise, spot));
            }
            // the ground's extremes under it are as sampled
            let heights: Vec<_> = (0..p.footprint().x)
                .flat_map(|x| (0..p.footprint().y).map(move |z| I64Vec2::new(x, z)))
                .map(|at| height(&noise, p.min.xz() + at))
                .collect();
            assert_eq!(p.lowest, *heights.iter().min().unwrap());
            assert_eq!(p.highest, *heights.iter().max().unwrap());
            // each in a cell of its own, kept well apart from those in the cells round it
            let cell = placement.cell;
            for other in all.iter().filter(|o| *o != p) {
                let apart = (other.min - p.min).xz().abs().max_element();
                assert!(apart >= cell / EDGE_MARGIN * 2 - 7, "{p:?} and {other:?}");
            }
        }
        // turned every way
        for rotation in 0..4 {
            assert!(all.iter().any(|p| p.rotation == rotation));
        }
    }

    #[test]
    fn test_building_well_away_from_a_structure_asks_nothing() {
        let noise = NoiseGenerator::new(3);
        let placed = Placements::default().near(&noise, AREA.0, AREA.1)[0];
        let (mut asked, mut placed_any) = (false, false);
        let chunk = (IVec3::ZERO, IVec3::splat(15));
        for origin in [
            placed.min + I64Vec3::Y * 64,
            placed.min - I64Vec3::new(40, 0, 40),
        ] {
            placed.build(
                origin,
                1,
                chunk,
                |_, _| {
                    asked = true;
                    0
                },
                |_, _| placed_any = true,
            );
        }
        assert!(!asked && !placed_any);
    }
}
