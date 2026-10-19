//! Structures (§5.6): templates read from `assets/structures/`, placed on a grid of cells as
//! trees are (INV-3), and built into full-detail chunks and LOD nodes alike (INV-5).

mod template;
pub mod village;

use std::sync::{Arc, LazyLock};

use bevy::math::{I64Vec2, I64Vec3, IVec3, Vec3Swizzles};

use super::columns::Recent;
use super::generator::ground_top;
use super::landmarks::GiantTree;
use super::noise::NoiseGenerator;
use super::trees::{hash, Tree, TREE_REACH};
use super::WATER_LEVEL;
use crate::block::{Block, BlockType};
pub use template::{
    Choice, Placement, Structure, Template, LOOT, SPAWNER, STRUCTURE_DEPTH, STRUCTURE_HEIGHT,
};
use village::{lay_out, Path, Village, PIECES, VILLAGES, VILLAGE_REACH};

// Each structure type's file, built into the game. Where two would stand together the one listed
// later gives way (`Placements::yields`), so new types go at the end.
const FILES: [&str; 7] = [
    include_str!("../../../../assets/structures/watchtower.toml"),
    include_str!("../../../../assets/structures/desert_temple.toml"),
    include_str!("../../../../assets/structures/jungle_temple.toml"),
    include_str!("../../../../assets/structures/forest_ruin.toml"),
    include_str!("../../../../assets/structures/glacier_outpost.toml"),
    include_str!("../../../../assets/structures/mineshaft.toml"),
    include_str!("../../../../assets/structures/dungeon.toml"),
];

/// Every structure type, read once from the files built into the game, so generation reads no
/// files, on a server as on a client, and stays a function of the seed (INV-1).
pub static STRUCTURES: LazyLock<Vec<Structure>> = LazyLock::new(|| {
    FILES
        .iter()
        .map(|text| toml::from_str(text).unwrap_or_else(|e| panic!("a structure won't load: {e}")))
        .collect()
});

/// What placements call the base terrain, the land no biome owns.
pub const BASE_TERRAIN: &str = "base";
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
// The ground over a structure underground is sampled every this many blocks across its
// footprint, and its top kept at least `COVER` blocks under the lowest found, which leaves room
// for the ground between the samples dipping, as caves keep under the ground (§5.4).
const UNDER_STRIDE: i64 = 4;
const COVER: i64 = 8;
// Cells remembered, a few dozen bytes each, and villages, a few kilobytes each.
const PLACEMENTS_KEPT: usize = 65_536;
const VILLAGES_KEPT: usize = 4096;

/// A structure standing in the world: one of a type, or a piece or a stretch of path of a
/// village.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Placed {
    kind: Kind,
    /// Quarter turns from its template as written, 0..4.
    pub rotation: u8,
    /// Its footprint's lowest corner, at the height of its floor.
    pub min: I64Vec3,
    /// The lowest and highest ground under its footprint: its foundation fills up from the
    /// one, and the ground is cleared down to its floor from the other. Over one underground.
    pub lowest: i64,
    pub highest: i64,
    /// Its own seed, from which it makes its template's choices (`Choice`), so no two are alike.
    pub variant: u64,
}

// What a structure is: of a type, in `STRUCTURES`, a village's piece, in `PIECES`, or a stretch
// of a village's path.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Kind {
    Type(usize),
    Piece(usize),
    Path(Path),
}

impl Placed {
    /// Its type's name, or its piece's, or `path`.
    pub fn name(&self) -> &'static str {
        match self.kind {
            Kind::Type(kind) => &STRUCTURES[kind].name,
            Kind::Piece(piece) => &PIECES[piece].name,
            Kind::Path(_) => "path",
        }
    }

    /// The loot table its chests roll from: its type's, or a village's.
    pub fn loot_table(&self) -> &'static str {
        match self.kind {
            Kind::Type(kind) => &STRUCTURES[kind].name,
            _ => &VILLAGES.name,
        }
    }

    /// The blocks it's built of, unless it's a path.
    pub fn template(&self) -> Option<&'static Template> {
        match self.kind {
            Kind::Type(kind) => Some(&STRUCTURES[kind].template),
            Kind::Piece(piece) => Some(&PIECES[piece].template),
            Kind::Path(_) => None,
        }
    }

    /// The stretch of path it is, if it's one of a village's.
    pub fn path(&self) -> Option<Path> {
        match self.kind {
            Kind::Path(path) => Some(path),
            _ => None,
        }
    }

    /// Whether it's under the ground, out of sight from far away: LODs leave it out, as they do
    /// caves, and the ground over it is left alone.
    pub fn is_underground(&self) -> bool {
        matches!(self.kind, Kind::Type(kind) if STRUCTURES[kind].placement.depth.is_some())
    }

    /// Its footprint across x and z.
    pub fn footprint(&self) -> I64Vec2 {
        match self.kind {
            Kind::Path(path) => path.footprint(),
            _ => self.template().unwrap().footprint(self.rotation),
        }
    }

    // Whether its footprint overlaps the columns `min..=max`.
    fn overlaps(&self, min: I64Vec2, max: I64Vec2) -> bool {
        self.min.xz().cmple(max).all() && (self.min.xz() + self.footprint()).cmpgt(min).all()
    }

    /// Whether column `x`, `z` is in its footprint.
    pub fn covers(&self, x: i64, z: i64) -> bool {
        let at = I64Vec2::new(x, z) - self.min.xz();
        at.cmpge(I64Vec2::ZERO).all() && at.cmplt(self.footprint()).all()
    }

    // Its lowest layer's corner, where its template's cell 0 is.
    fn corner(&self) -> I64Vec3 {
        self.min - I64Vec3::Y * self.template().map_or(0, |t| t.floor)
    }

    /// The lowest block it can change, and the block past the highest.
    pub fn bounds(&self) -> (I64Vec3, I64Vec3) {
        let corner = self.corner();
        let (mut bottom, mut top) = match self.kind {
            Kind::Path(path) => path.bounds(self.min),
            _ => (corner.y, corner.y + self.template().unwrap().size.y),
        };
        if !self.is_underground() {
            bottom = bottom.min(ground_top(self.lowest));
            top = top.max(ground_top(self.highest) + 1);
        }
        let footprint = self.footprint();
        (
            corner.with_y(bottom),
            I64Vec3::new(corner.x + footprint.x, top, corner.z + footprint.y),
        )
    }

    // What the block `at` in its footprint becomes, over ground `height` high, if the structure
    // changes it: its own blocks and the air it clears; on the surface, under its floor, the
    // foundation up from the ground, and over it, air down to the floor, clearing the ground and
    // what grows on it. `height` is only asked for on the surface.
    fn stamp(&self, at: I64Vec3, height: impl FnOnce() -> i64) -> Option<Block> {
        let template = match self.kind {
            Kind::Path(path) => {
                return path.stamp(self.min, at, self.variant, ground_top(height()));
            }
            _ => self.template().unwrap(),
        };
        let cell = template.cell(self.rotation, at - self.corner(), self.variant);
        if cell.is_some() || self.is_underground() {
            return cell;
        }
        let ground = ground_top(height());
        if at.y < self.min.y {
            (at.y >= ground).then_some(template.foundation.into())
        } else {
            (at.y <= ground).then_some(Block::AIR)
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
        mut place: impl FnMut(IVec3, Block),
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
                            let height = &mut heights[column as usize];
                            for by in min.y..min.y + size {
                                let ground = || *height.get_or_insert_with(|| height_at(bx, bz));
                                match self.stamp(I64Vec3::new(bx, by, bz), ground) {
                                    Some(Block::AIR) => cleared += 1,
                                    // cells wider than a block draw only its type
                                    Some(block) if size > 1 => blocks.add(block.kind.into()),
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
                        place(cell, Block::AIR);
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
        // a path is too flat to see from so far
        let Some(template) = self.template() else {
            return Vec::new();
        };
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
                        let block = template.cell(self.rotation, at, self.variant);
                        if let Some(block) = block.filter(|&b| b != Block::AIR) {
                            (low, high) = (low.min(at), high.max(at));
                            blocks.add(block.kind.into());
                        }
                    }
                }
            }
            if let Some(block) = blocks.commonest() {
                boxes.push((corner + low, corner + high + 1, block.kind));
            }
        }
        boxes
    }

    /// Whether `tree` would grow into it, or into the clearing kept round it, which those
    /// underground never do.
    pub fn crowds(&self, tree: &Tree) -> bool {
        let min = self.min.xz();
        !self.is_underground() && tree.overhangs(min - CLEARING, min + self.footprint() + CLEARING)
    }

    /// Its markers, by kind, where they are in the world.
    pub fn markers(&self) -> impl Iterator<Item = (&'static str, I64Vec3)> + '_ {
        self.template().into_iter().flat_map(move |template| {
            template.markers.iter().map(move |(kind, at)| {
                (
                    kind.as_str(),
                    self.corner() + template.turn(self.rotation, *at),
                )
            })
        })
    }
}

/// Takes out the trees that would grow into any of `structures`, or the clearings round them,
/// or into the roots of any of `giants`.
pub fn clear_trees(trees: &mut Vec<Tree>, structures: &[Placed], giants: &[&GiantTree]) {
    if !structures.is_empty() || !giants.is_empty() {
        trees.retain(|tree| {
            !structures.iter().any(|s| s.crowds(tree)) && !giants.iter().any(|g| g.crowds(tree))
        });
    }
}

// How many of each block, in the order they're first seen.
#[derive(Default)]
struct Tally(Vec<(Block, i64)>);

impl Tally {
    fn add(&mut self, block: Block) {
        match self.0.iter_mut().find(|(b, _)| *b == block) {
            Some((_, count)) => *count += 1,
            None => self.0.push((block, 1)),
        }
    }

    fn total(&self) -> i64 {
        self.0.iter().map(|(_, count)| count).sum()
    }

    // the first seen of those seen most
    fn commonest(&self) -> Option<Block> {
        let most = self.0.iter().map(|(_, count)| *count).max()?;
        self.0.iter().find(|(_, c)| *c == most).map(|(b, _)| *b)
    }
}

/// The structures placed so far, remembered by cell, and villages laid out: deciding one takes
/// dozens of noise samples, a village a few thousand, and every chunk and LOD node near it asks.
pub struct Placements {
    types: Recent<(usize, i64, i64), Option<Placed>>,
    villages: Recent<(i64, i64), Option<Arc<Village>>>,
}

impl Default for Placements {
    fn default() -> Self {
        Self {
            types: Recent::new(PLACEMENTS_KEPT),
            villages: Recent::new(VILLAGES_KEPT),
        }
    }
}

impl Placements {
    /// Every structure whose footprint overlaps the columns `min..=max` (x, z), in a fixed
    /// order: those of each type, then the paths and pieces of villages. Blocks while another
    /// thread places one, so only call from a task.
    pub fn near(&self, noise: &NoiseGenerator, min: I64Vec2, max: I64Vec2) -> Vec<Placed> {
        let mut found: Vec<Placed> = (0..STRUCTURES.len())
            .flat_map(|kind| self.near_of(noise, kind, min, max))
            .collect();
        for village in self.villages_near(noise, min, max) {
            let all = village.paths.iter().chain(&village.pieces);
            found.extend(all.filter(|p| p.overlaps(min, max)));
        }
        found
    }

    /// Every village reaching into the columns `min..=max`, in a fixed order. Blocks while
    /// another thread lays one out, so only call from a task.
    pub fn villages_near(
        &self,
        noise: &NoiseGenerator,
        min: I64Vec2,
        max: I64Vec2,
    ) -> Vec<Arc<Village>> {
        let placement = &VILLAGES.placement;
        let cell = I64Vec2::splat(placement.cell);
        let (first, last) = (
            (min - VILLAGE_REACH).div_euclid(cell),
            (max + VILLAGE_REACH).div_euclid(cell),
        );
        let mut found = Vec::new();
        for cz in first.y..=last.y {
            for cx in first.x..=last.x {
                let village = self.village(noise, I64Vec2::new(cx, cz));
                found.extend(village.filter(|v| v.overlaps(min, max)));
            }
        }
        found
    }

    /// The village in `cell` of the villages' grid, if one stands there. Blocks while another
    /// thread lays it out, so only call from a task, or once it's been laid out for the chunks
    /// round it.
    pub fn village(&self, noise: &NoiseGenerator, cell: I64Vec2) -> Option<Arc<Village>> {
        let placement = &VILLAGES.placement;
        let seed = chosen(noise, &VILLAGES.name, placement.chance, cell.x, cell.y)?;
        let village = self.villages.get((cell.x, cell.y), || {
            // villages give way to every surface structure, as they're listed last
            let earlier = |min, max| {
                (0..STRUCTURES.len())
                    .filter(|&kind| STRUCTURES[kind].placement.depth.is_none())
                    .any(|kind| !self.near_of(noise, kind, min, max).is_empty())
            };
            lay_out(noise, cell, seed, earlier).map(Arc::new)
        });
        village.as_ref().clone()
    }

    // Those of `kind` alone.
    fn near_of(
        &self,
        noise: &NoiseGenerator,
        kind: usize,
        min: I64Vec2,
        max: I64Vec2,
    ) -> Vec<Placed> {
        let structure = &STRUCTURES[kind];
        let cell = I64Vec2::splat(structure.placement.cell);
        // its footprint is round the spot it's placed at, which is in its cell
        let reach = structure.template.size.xz().max_element() / 2 + 1;
        let (first, last) = (
            (min - reach).div_euclid(cell),
            (max + reach).div_euclid(cell),
        );
        let mut found = Vec::new();
        for cz in first.y..=last.y {
            for cx in first.x..=last.x {
                // most cells are ruled out by their hash alone, without waiting on others
                let Some(seed) = chosen(noise, &structure.name, structure.placement.chance, cx, cz)
                else {
                    continue;
                };
                let cell = I64Vec2::new(cx, cz);
                let placed = *self.types.get((kind, cx, cz), || {
                    place(noise, kind, cell, seed).filter(|p| !self.yields(noise, p))
                });
                found.extend(placed.filter(|p| p.overlaps(min, max)));
            }
        }
        found
    }

    // Whether `placed` gives way to a structure of a type listed before it, on the surface or
    // underground as it is, whose footprint comes within the clearing round its own. Types
    // listed earlier never give way to later ones, so this is decided in a bounded
    // neighbourhood (INV-3), and a type added at the end of the list moves none before it.
    fn yields(&self, noise: &NoiseGenerator, placed: &Placed) -> bool {
        let min = placed.min.xz() - CLEARING;
        let max = placed.min.xz() + placed.footprint() + CLEARING - 1;
        let underground = placed.is_underground();
        let Kind::Type(before) = placed.kind else {
            unreachable!("only types are placed by cell")
        };
        (0..before)
            .filter(|&kind| STRUCTURES[kind].placement.depth.is_some() == underground)
            .any(|kind| !self.near_of(noise, kind, min, max).is_empty())
    }
}

// The hash deciding the structure of the type `name` in cell `cx`, `cz`, if the cell is among
// the `chance` of cells that may have one: only those go on to sample the ground.
fn chosen(noise: &NoiseGenerator, name: &str, chance: f64, cx: i64, cz: i64) -> Option<u64> {
    // each type's grid is salted by its name, so types don't line up, nor shift as others come
    let salt = name.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3)
    });
    let seed = hash(noise.seed() as u64 ^ salt, cx, cz);
    let draw = (seed >> 11) as f64 / (1u64 << 53) as f64;
    (draw < chance).then(|| hash(seed, 1, 0))
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
        let name = noise.biomes().name(biome).unwrap_or(BASE_TERRAIN);
        placement.biomes.iter().any(|b| b == name)
    };
    if noise.landmarks().keeps_clear(spot) || !suits(spot) {
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
        if !summit || noise.landmarks().keeps_clear(spot) || !suits(spot) {
            return None;
        }
    }

    let rotation = (seed >> 62) as u8;
    let variant = hash(seed, 2, 0);
    let footprint = template.footprint(rotation);
    let min = spot - footprint / 2;
    if let Some([shallowest, deepest]) = placement.depth {
        let across = |n: i64| (0..n).step_by(UNDER_STRIDE as usize).chain([n - 1]);
        let heights: Vec<i64> = across(footprint.x)
            .flat_map(|x| across(footprint.y).map(move |z| min + I64Vec2::new(x, z)))
            .map(|at| height(noise, at))
            .collect();
        let (lowest, highest) = (*heights.iter().min()?, *heights.iter().max()?);
        let depth = shallowest + (hash(seed, 3, 0) % (deepest - shallowest + 1) as u64) as i64;
        let floor = ground_top(lowest) - depth;
        let top = floor - template.floor + template.size.y;
        let suits = highest - lowest <= placement.max_rise
            && lowest - WATER_LEVEL >= placement.min_above_sea
            && top <= ground_top(lowest) - COVER;
        return suits.then_some(Placed {
            kind: Kind::Type(kind),
            rotation,
            min: I64Vec3::new(min.x, floor, min.y),
            lowest,
            highest,
            variant,
        });
    }
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
        kind: Kind::Type(kind),
        rotation,
        min: I64Vec3::new(min.x, floor, min.y),
        lowest,
        highest,
        variant,
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
    // Glaciers, twelve kilometres north of it.
    const NORTH: (I64Vec2, I64Vec2) = (I64Vec2::new(-6000, 11000), I64Vec2::new(5999, 14999));

    fn named(name: &str) -> &'static Structure {
        STRUCTURES.iter().find(|s| s.name == name).unwrap()
    }

    #[test]
    fn test_every_structure_loads_with_its_chests_on_floors_inside_it() {
        let names: Vec<_> = STRUCTURES.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            vec![
                "watchtower",
                "desert_temple",
                "jungle_temple",
                "forest_ruin",
                "glacier_outpost",
                "mineshaft",
                "dungeon"
            ],
            names
        );
        for structure in STRUCTURES.iter() {
            let template = &structure.template;
            let size = template.size;
            assert!(
                template.floor <= STRUCTURE_DEPTH && size.y - template.floor <= STRUCTURE_HEIGHT
            );
            let chests: Vec<_> = template.markers.iter().filter(|(k, _)| k == LOOT).collect();
            assert!(!chests.is_empty(), "{} has no chest", structure.name);
            // whatever an instance chooses
            for seed in 0..16 {
                let solid = |at: I64Vec3| {
                    template
                        .cell(0, at, seed)
                        .is_some_and(|b| b.kind.is_solid())
                };
                for (_, at) in &chests {
                    let at = *at;
                    assert!(
                        at.xz().cmpgt(I64Vec2::ZERO).all() && at.xz().cmplt(size.xz() - 1).all()
                    );
                    assert_eq!(Some(BlockType::Chest.into()), template.cell(0, at, seed));
                    let room = template.cell(0, at + I64Vec3::Y, seed);
                    assert!(
                        (at.y == 0 || solid(at - I64Vec3::Y))
                            && !room.is_some_and(|b| b.kind.is_solid()),
                        "{}'s chest at {at} with seed {seed}",
                        structure.name
                    );
                }
            }
        }
        // the watchtower is still tall, and the dungeon keeps a place for its spawner (M3.3)
        let tower = &named("watchtower").template;
        assert!(tower.size.y - tower.floor >= 20);
        let dungeon = &named("dungeon").template;
        let (_, spawner) = dungeon
            .markers
            .iter()
            .find(|(k, _)| k == "spawner")
            .unwrap();
        assert_eq!(Some(Block::AIR), dungeon.cell(0, *spawner, 0));
        assert!(dungeon
            .cell(0, *spawner - I64Vec3::Y, 0)
            .is_some_and(|b| b.kind.is_solid()));
    }

    #[test]
    fn test_instances_choose_their_blocks_the_same_each_time_and_differently_from_each_other() {
        let noise = NoiseGenerator::new(3);
        let all = Placements::default().near(&noise, AREA.0, AREA.1);
        let towers: Vec<_> = all.iter().filter(|p| p.name() == "watchtower").collect();
        assert!(towers.len() >= 2);
        let blocks = |p: &Placed| -> Vec<Option<Block>> {
            let size = p.template().unwrap().size;
            (0..size.x * size.y * size.z)
                .map(|i| I64Vec3::new(i % size.x, i / size.x / size.z, i / size.x % size.z))
                .map(|at| p.template().unwrap().cell(0, at, p.variant))
                .collect()
        };
        assert_eq!(blocks(towers[0]), blocks(towers[0]));
        assert_ne!(blocks(towers[0]), blocks(towers[1]));
        // and differ only where their template gives a choice
        let changed = blocks(towers[0])
            .iter()
            .zip(blocks(towers[1]))
            .filter(|(a, b)| **a != *b)
            .count();
        assert!(changed > 50, "{changed}");
    }

    #[test]
    fn test_every_structure_stands_in_biomes_that_exist() {
        let biomes = super::super::biome::Biomes::new(3);
        for structure in STRUCTURES.iter() {
            for name in &structure.placement.biomes {
                assert!(
                    name == BASE_TERRAIN || biomes.names().any(|b| b == name),
                    "{} stands in {name}, which isn't a biome",
                    structure.name
                );
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
    fn test_structures_stand_where_their_placement_suits() {
        let noise = NoiseGenerator::new(3);
        // and the glaciers to the north, which the glacier outposts need
        let placements = Placements::default();
        let mut all = placements.near(&noise, AREA.0, AREA.1);
        all.extend(placements.near(&noise, NORTH.0, NORTH.1));
        // villages have tests of their own
        let types: Vec<_> = all.iter().filter(|p| type_of(p).is_some()).collect();
        for p in &types {
            let placement = &type_of(p).unwrap().placement;
            let spot = p.min.xz() + p.footprint() / 2;
            let biome = noise.sample(spot.x as f64, spot.y as f64).biome;
            let biome = noise.biomes().name(biome).unwrap_or(BASE_TERRAIN);
            assert!(
                placement.biomes.iter().any(|b| b == biome),
                "{p:?} in {biome}"
            );
            assert!(p.highest - p.lowest <= placement.max_rise);
            let heights: Vec<_> = (0..p.footprint().x)
                .flat_map(|x| (0..p.footprint().y).map(move |z| I64Vec2::new(x, z)))
                .map(|at| height(&noise, p.min.xz() + at))
                .collect();
            if let Some([shallowest, deepest]) = placement.depth {
                // well under every column of the ground over it, at the depth it's given
                let depth = ground_top(p.lowest) - p.min.y;
                assert!((shallowest..=deepest).contains(&depth), "{p:?}");
                let top = p.bounds().1.y;
                assert!(heights.iter().all(|&h| top <= h - 5), "{p:?}");
                assert!(p.lowest - WATER_LEVEL >= placement.min_above_sea);
                continue;
            }
            assert!((p.lowest..=p.highest).contains(&p.min.y));
            assert!(p.lowest > WATER_LEVEL && p.min.y - WATER_LEVEL >= placement.min_above_sea);
            if placement.hilltop {
                for step in [I64Vec2::X, I64Vec2::NEG_X, I64Vec2::Y, I64Vec2::NEG_Y] {
                    assert!(height(&noise, spot + step * CLIMB_STRIDE) <= height(&noise, spot));
                }
            }
            // the ground's extremes under it are as sampled
            assert_eq!(p.lowest, *heights.iter().min().unwrap());
            assert_eq!(p.highest, *heights.iter().max().unwrap());
        }
        for p in &types {
            for other in all.iter().filter(|o| *o != *p) {
                let apart = (other.min - p.min).xz().abs().max_element();
                if other.kind == p.kind {
                    // each in a cell of its own, kept well apart from those in the cells round it
                    let cell = type_of(p).unwrap().placement.cell;
                    let width = p.footprint().max_element();
                    assert!(
                        apart >= cell / EDGE_MARGIN * 2 - width,
                        "{p:?} and {other:?}"
                    );
                } else if other.is_underground() == p.is_underground() {
                    // and other types and villages keep out of its clearing
                    let (min, max) = (p.min.xz() - CLEARING, p.min.xz() + p.footprint() + CLEARING);
                    assert!(!other.overlaps(min, max - 1), "{p:?} and {other:?}");
                }
            }
        }
        // every type, turned every way
        for kind in 0..STRUCTURES.len() {
            assert!(
                types.iter().any(|p| p.kind == Kind::Type(kind)),
                "no {}",
                STRUCTURES[kind].name
            );
        }
        for rotation in 0..4 {
            assert!(types.iter().any(|p| p.rotation == rotation));
        }
    }

    fn type_of(placed: &Placed) -> Option<&'static Structure> {
        match placed.kind {
            Kind::Type(kind) => Some(&STRUCTURES[kind]),
            _ => None,
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
