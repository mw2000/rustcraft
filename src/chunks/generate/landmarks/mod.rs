//! Landmarks (§5.7): rare, enormous features seen from tens of kilometres away, to make a
//! player want to go there: great peaks, volcanoes and giant trees. The world is split into a
//! grid of cells `LANDMARK_CELL` blocks across, each holding at most one, which stays inside
//! it, so a column only ever asks its own cell (INV-3). They're placed once, as the world's
//! noise is made, from the seed and the terrain as it is without them (INV-2), so every level
//! of detail and every machine finds the same ones.

mod giant_tree;
mod peak;
mod volcano;

use bevy::math::{DVec2, I64Vec2, I64Vec3, Vec3Swizzles};

use super::biome::landmark::{CRATER, SUMMIT, VOLCANO};
use super::biome::BiomeId;
use super::fractal::Fractal;
use super::noise::{NoiseGenerator, WORLD_HALF_SIZE};
use super::trees::hash;
use super::WATER_LEVEL;
pub use giant_tree::GiantTree;
use peak::Peak;
use volcano::Volcano;

/// Landmarks stand one at most in each cell of a grid this many blocks across, about 67 km²,
/// so a world holds a dozen or so, and the spawn, near where four cells meet, has a few within
/// sight.
pub const LANDMARK_CELL: i64 = 8192;
const SEED_OFFSET: u64 = 5000;

// Seeds after the biomes' and the caves', see `NoiseGenerator::new`.
const NOISE_SEED_OFFSET: u32 = 5000;
// Crags and gullies a few dozen blocks across, and the slow warp that keeps cones from being
// perfectly round.
const CRAG_WAVELENGTH: f64 = 70.0;
const WARP_WAVELENGTH: f64 = 900.0;

#[derive(Debug, Clone)]
pub enum Landmark {
    Peak(Peak),
    Volcano(Volcano),
    GiantTree(GiantTree),
}

impl Landmark {
    pub fn name(&self) -> &'static str {
        match self {
            Landmark::Peak(_) => "great peak",
            Landmark::Volcano(_) => "volcano",
            Landmark::GiantTree(_) => "giant tree",
        }
    }

    /// Where it stands, across the world.
    pub fn centre(&self) -> DVec2 {
        match self {
            Landmark::Peak(peak) => peak.centre,
            Landmark::Volcano(volcano) => volcano.centre,
            Landmark::GiantTree(tree) => tree.base.xz().as_dvec2(),
        }
    }

    /// How far from its centre it changes anything.
    pub fn reach(&self) -> f64 {
        match self {
            Landmark::Peak(peak) => peak.radius * peak::REACH,
            Landmark::Volcano(volcano) => volcano.radius * volcano::REACH,
            Landmark::GiantTree(tree) => tree.reach() as f64,
        }
    }

    // How far from its centre structures keep away from it: out of the steep heart of a
    // mountain, off a volcano's ash, and from under a giant tree's crown.
    fn clearing(&self) -> f64 {
        match self {
            Landmark::Peak(peak) => peak.radius * peak::CLEARING,
            Landmark::Volcano(volcano) => volcano.radius * volcano::ASH_LINE,
            Landmark::GiantTree(tree) => tree.reach() as f64,
        }
    }
}

/// The noise landmarks are shaped with, shared by every kind.
pub struct Shaping {
    crags: Fractal,
    warp_x: Fractal,
    warp_z: Fractal,
}

impl Shaping {
    fn new(seed: u32) -> Self {
        let seed = seed.wrapping_add(NOISE_SEED_OFFSET);
        Self {
            crags: Fractal::new(seed, CRAG_WAVELENGTH, 3, 2.0, 0.5),
            warp_x: Fractal::new(seed.wrapping_add(3), WARP_WAVELENGTH, 2, 2.0, 0.5),
            warp_z: Fractal::new(seed.wrapping_add(5), WARP_WAVELENGTH, 2, 2.0, 0.5),
        }
    }

    // A slow wander in -1..1 each way, the same for every cell size, as it's far wider than
    // any cell.
    fn warp(&self, x: f64, z: f64) -> DVec2 {
        DVec2::new(self.warp_x.get(x, z, 1.0), self.warp_z.get(x, z, 1.0))
    }
}

/// Every landmark in the world, by cell.
pub struct Landmarks {
    // the cells of the world and the ocean round its edge, row-major from `first`
    cells: Vec<Option<Landmark>>,
    first: I64Vec2,
    across: i64,
    shaping: Shaping,
    summit: BiomeId,
    volcano: BiomeId,
    crater: BiomeId,
}

impl Landmarks {
    /// A world without landmarks, as they're placed on the terrain without them.
    pub fn none(seed: u32) -> Self {
        let first = I64Vec2::splat((-WORLD_HALF_SIZE).div_euclid(LANDMARK_CELL));
        let last = I64Vec2::splat((WORLD_HALF_SIZE - 1).div_euclid(LANDMARK_CELL));
        let across = last.x - first.x + 1;
        Self {
            cells: vec![None; (across * across) as usize],
            first,
            across,
            shaping: Shaping::new(seed),
            summit: BiomeId::BASE,
            volcano: BiomeId::BASE,
            crater: BiomeId::BASE,
        }
    }

    /// The landmarks of `noise`'s world, placed on its terrain, which mustn't have any yet.
    pub fn place(noise: &NoiseGenerator) -> Self {
        let biomes = noise.biomes();
        let mut landmarks = Self {
            summit: biomes.id(SUMMIT).unwrap(),
            volcano: biomes.id(VOLCANO).unwrap(),
            crater: biomes.id(CRATER).unwrap(),
            ..Self::none(noise.seed())
        };
        for i in 0..landmarks.cells.len() as i64 {
            let cell = landmarks.first + I64Vec2::new(i % landmarks.across, i / landmarks.across);
            landmarks.cells[i as usize] = place_in(noise, cell);
        }
        landmarks
    }

    /// Every landmark in the world, from north west to south east.
    pub fn all(&self) -> impl Iterator<Item = &Landmark> {
        self.cells.iter().flatten()
    }

    // The landmark in the cell holding (`x`, `z`), if there is one.
    #[inline]
    fn at(&self, x: f64, z: f64) -> Option<&Landmark> {
        let cell = DVec2::new(x, z)
            .div_euclid(DVec2::splat(LANDMARK_CELL as f64))
            .as_i64vec2()
            - self.first;
        if cell.min_element() < 0 || cell.max_element() >= self.across {
            return None;
        }
        self.cells[(cell.y * self.across + cell.x) as usize].as_ref()
    }

    /// The column's height, biome and tree density, given them without landmarks, at (`x`,
    /// `z`) as a cell `cell` blocks wide sees it. Most of the world is far from any, and pays
    /// a lookup and a distance.
    #[inline]
    pub fn shape(
        &self,
        x: f64,
        z: f64,
        cell: f64,
        column: (f64, BiomeId, f64),
    ) -> (f64, BiomeId, f64) {
        let Some(landmark) = self.at(x, z) else {
            return column;
        };
        if landmark.centre().distance_squared(DVec2::new(x, z)) >= landmark.reach().powi(2) {
            return column;
        }
        match landmark {
            Landmark::Peak(peak) => peak.shape(&self.shaping, x, z, cell, column, self.summit),
            Landmark::Volcano(volcano) => volcano.shape(
                &self.shaping,
                x,
                z,
                cell,
                column,
                (self.volcano, self.crater),
            ),
            Landmark::GiantTree(_) => column,
        }
    }

    /// The giant trees reaching over any of the columns `min..=max` (x, z).
    pub fn giant_trees_near(&self, min: I64Vec2, max: I64Vec2) -> Vec<&GiantTree> {
        let cell = I64Vec2::splat(LANDMARK_CELL);
        let (first, last) = (
            (min.div_euclid(cell) - self.first).max(I64Vec2::ZERO),
            (max.div_euclid(cell) - self.first).min(I64Vec2::splat(self.across - 1)),
        );
        let mut found = Vec::new();
        for cz in first.y..=last.y {
            for cx in first.x..=last.x {
                if let Some(Landmark::GiantTree(tree)) =
                    &self.cells[(cz * self.across + cx) as usize]
                {
                    let (low, high) = tree.bounds();
                    if low.x <= max.x && low.z <= max.y && high.x > min.x && high.z > min.y {
                        found.push(tree);
                    }
                }
            }
        }
        found
    }

    /// Whether a giant tree is drawn in the blocks `min..max` of cells `cell` wide, which the
    /// ground alone would leave empty: where it reaches, or where its trunk's foot is for
    /// cells that draw it as boxes.
    pub fn stands_in(&self, min: I64Vec3, max: I64Vec3, cell: i64) -> bool {
        self.giant_trees_near(min.xz(), max.xz() - 1)
            .iter()
            .any(|tree| {
                let (low, high) = if GiantTree::drawn_as_boxes(cell) {
                    (tree.base, tree.base + 1)
                } else {
                    tree.bounds()
                };
                low.cmplt(max).all() && high.cmpgt(min).all()
            })
    }

    /// The height caves in column `x`, `z`, whose ground is `ground()` high, must stay under, as
    /// under ground with no mouth (see `Caves::top_under`), if a landmark keeps them shut
    /// there: under a giant tree's trunk, which would otherwise be left standing on nothing,
    /// and under a crater's magma.
    pub fn cave_roof(&self, x: i64, z: i64, ground: impl FnOnce() -> i64) -> Option<i64> {
        match self.at(x as f64, z as f64)? {
            Landmark::GiantTree(tree) => tree.cave_roof(x, z, ground),
            Landmark::Volcano(volcano) => volcano.cave_roof(x, z, ground),
            Landmark::Peak(_) => None,
        }
    }

    /// Whether a structure at `spot` would stand where a landmark keeps clear.
    pub fn keeps_clear(&self, spot: I64Vec2) -> bool {
        let at = spot.as_dvec2();
        self.at(at.x, at.y)
            .is_some_and(|l| l.centre().distance_squared(at) < l.clearing().powi(2))
    }
}

// The landmark in `cell`, if one suits a spot in it: its kinds in an order hashed from the
// seed and the cell, each trying a few spots, far enough in from the cell's edge that all it
// changes stays inside the cell.
fn place_in(noise: &NoiseGenerator, cell: I64Vec2) -> Option<Landmark> {
    let seed = hash(noise.seed() as u64 ^ SEED_OFFSET, cell.x, cell.y);
    let mut kinds = [Kind::Peak, Kind::Volcano, Kind::GiantTree];
    // a shuffle by the hash's bits, the same everywhere
    for i in (1..kinds.len()).rev() {
        kinds.swap(i, (hash(seed, i as i64, 0) % (i as u64 + 1)) as usize);
    }
    let corner = cell * LANDMARK_CELL;
    for kind in kinds {
        let margin = kind.reach().ceil() as i64;
        let span = (LANDMARK_CELL - 2 * margin) as u64;
        for attempt in 0..kind.tries() {
            let h = hash(seed, kind as i64 + 7, attempt);
            let spot = corner + margin + I64Vec2::new((h % span) as i64, ((h >> 32) % span) as i64);
            let site = Site::new(noise, spot);
            let placed = match kind {
                Kind::Peak => Peak::place(noise, &site, h).map(Landmark::Peak),
                Kind::Volcano => Volcano::place(noise, &site, h).map(Landmark::Volcano),
                Kind::GiantTree => GiantTree::place(noise, &site, h).map(Landmark::GiantTree),
            };
            if placed.is_some() {
                return placed;
            }
        }
    }
    None
}

#[derive(Debug, Copy, Clone)]
enum Kind {
    Peak,
    Volcano,
    GiantTree,
}

impl Kind {
    // Each kind tries so many spots in a cell before the next kind tries: giant trees more,
    // as only forests and jungles grow them.
    fn tries(self) -> i64 {
        match self {
            Kind::GiantTree => 24,
            _ => 6,
        }
    }

    // The most any of the kind can change round its centre, which keeps it inside its cell.
    fn reach(self) -> f64 {
        match self {
            Kind::Peak => peak::RADIUS.1 * peak::REACH,
            Kind::Volcano => volcano::RADIUS.1 * volcano::REACH,
            Kind::GiantTree => giant_tree::MAX_REACH as f64,
        }
    }
}

// A spot a landmark might stand at, with the ground there.
struct Site {
    at: I64Vec2,
    height: f64,
    above_sea: f64,
    temperature: f32,
    biome: Option<&'static str>,
}

impl Site {
    fn new(noise: &NoiseGenerator, at: I64Vec2) -> Self {
        let sample = noise.sample(at.x as f64, at.y as f64);
        Self {
            at,
            height: sample.height,
            above_sea: sample.height - WATER_LEVEL as f64,
            temperature: noise.temperature(at.x as f64, at.y as f64),
            biome: noise.biomes().name(sample.biome),
        }
    }

    // The heights of the ground at `points` evenly round a ring `radius` blocks out.
    fn ring(&self, noise: &NoiseGenerator, radius: f64, points: usize) -> Vec<f64> {
        (0..points)
            .map(|i| {
                let angle = i as f64 / points as f64 * std::f64::consts::TAU;
                let at = self.at.as_dvec2() + DVec2::from_angle(angle) * radius;
                noise.sample(at.x, at.y).height
            })
            .collect()
    }
}

// A number 0..1 from bits `shift..shift + 16` of `h`.
fn unit(h: u64, shift: u32) -> f64 {
    ((h >> shift) & 0xffff) as f64 / 65535.0
}

// `range.0` to `range.1` by bits `shift..shift + 16` of `h`.
fn pick(h: u64, shift: u32, range: (f64, f64)) -> f64 {
    range.0 + (range.1 - range.0) * unit(h, shift)
}

#[cfg(test)]
mod tests;
