//! Villages (§5.6, M3.4), assembled from pieces by rules. A village is placed on a grid of cells
//! as the other structures are, and where one stands its layout is worked out once from the seed
//! and its cell and remembered (`Placements`): a well in the middle, streets running out from it
//! and branching off, following the ground a block up or down at a time, and its pieces, the
//! templates in `assets/structures/village/`, set along them facing the street, each on ground
//! levelled for it, with a short path from the street to its door. Every piece and stretch of
//! path is a `Placed`, so chunks and LOD nodes build them as they build any other structure, and
//! the whole is decided from the noise round the village alone (INV-2, INV-3).

use std::ops::RangeInclusive;
use std::sync::LazyLock;

use bevy::math::{I64Vec2, I64Vec3, Vec3Swizzles};
use serde::Deserialize;

use super::template::{Choice, Placement, Template};
use super::{height, Kind, Placed, BASE_TERRAIN, CLEARING, EDGE_MARGIN};
use crate::block::{Block, BlockType};
use crate::chunks::generate::generator::ground_top;
use crate::chunks::generate::noise::NoiseGenerator;
use crate::chunks::generate::trees::hash;
use crate::chunks::generate::WATER_LEVEL;

const VILLAGE_FILE: &str = include_str!("../../../../assets/structures/village.toml");
// Every piece, built into the game as the structure types are.
const PIECE_FILES: &[&str] = &[
    include_str!("../../../../assets/structures/village/adobe_courtyard_house.toml"),
    include_str!("../../../../assets/structures/village/adobe_farm.toml"),
    include_str!("../../../../assets/structures/village/adobe_fountain.toml"),
    include_str!("../../../../assets/structures/village/adobe_hall.toml"),
    include_str!("../../../../assets/structures/village/adobe_house.toml"),
    include_str!("../../../../assets/structures/village/adobe_lamp.toml"),
    include_str!("../../../../assets/structures/village/adobe_pen.toml"),
    include_str!("../../../../assets/structures/village/adobe_small_house.toml"),
    include_str!("../../../../assets/structures/village/adobe_smithy.toml"),
    include_str!("../../../../assets/structures/village/adobe_tower.toml"),
    include_str!("../../../../assets/structures/village/farm.toml"),
    include_str!("../../../../assets/structures/village/lodge_cabin.toml"),
    include_str!("../../../../assets/structures/village/lodge_hall.toml"),
    include_str!("../../../../assets/structures/village/lodge_homestead.toml"),
    include_str!("../../../../assets/structures/village/lodge_house.toml"),
    include_str!("../../../../assets/structures/village/lodge_lamp.toml"),
    include_str!("../../../../assets/structures/village/lodge_smithy.toml"),
    include_str!("../../../../assets/structures/village/lodge_tower.toml"),
    include_str!("../../../../assets/structures/village/lodge_well.toml"),
    include_str!("../../../../assets/structures/village/pen.toml"),
    include_str!("../../../../assets/structures/village/timber_bell_tower.toml"),
    include_str!("../../../../assets/structures/village/timber_cottage.toml"),
    include_str!("../../../../assets/structures/village/timber_house.toml"),
    include_str!("../../../../assets/structures/village/timber_inn.toml"),
    include_str!("../../../../assets/structures/village/timber_lamp.toml"),
    include_str!("../../../../assets/structures/village/timber_longhouse.toml"),
    include_str!("../../../../assets/structures/village/timber_smithy.toml"),
    include_str!("../../../../assets/structures/village/timber_stone_cottage.toml"),
    include_str!("../../../../assets/structures/village/timber_well.toml"),
];

/// Where villages stand and the styles they're built in, read from `village.toml`.
pub static VILLAGES: LazyLock<Villages> = LazyLock::new(|| {
    toml::from_str(VILLAGE_FILE).unwrap_or_else(|e| panic!("villages won't load: {e}"))
});

/// Every piece villages are built of.
pub static PIECES: LazyLock<Vec<Piece>> = LazyLock::new(|| {
    PIECE_FILES
        .iter()
        .map(|text| {
            toml::from_str(text).unwrap_or_else(|e| panic!("a village piece won't load: {e}"))
        })
        .collect()
});

/// The kinds of marker pieces have beyond loot: where someone stands outside the way in, where a
/// villager sleeps and works, where villagers meet round the well, and where a pen's animals are.
pub const ENTRANCE: &str = "entrance";
pub const BED: &str = "bed";
pub const WORK: &str = "work";
pub const GATHER: &str = "gather";
pub const ANIMAL: &str = "animal";

/// Villages' placement and styles.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Villages {
    pub name: String,
    pub placement: Placement,
    pub styles: Vec<Style>,
}

/// A style of village, which its pieces name: the biomes it's built in, and what its streets
/// are trodden of.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Style {
    pub name: String,
    pub biomes: Vec<String>,
    pub path: Choice,
}

/// A piece of a village, as its file in `assets/structures/village/` declares it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Piece {
    pub name: String,
    pub piece: Fits,
    pub template: Template,
}

/// What a piece is for, and the styles of village it's built in.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fits {
    pub role: Role,
    pub styles: Vec<String>,
}

/// What a piece is for: its villagers' homes, their workplaces, the well in the middle and the
/// lamps along the streets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    House,
    Hall,
    Smithy,
    Farm,
    Pen,
    Tower,
    Well,
    Lamp,
}

/// A village as it's laid out.
#[derive(Debug, Clone, PartialEq)]
pub struct Village {
    /// Its cell on the villages' grid, which names it.
    pub cell: I64Vec2,
    /// In `VILLAGES.styles`.
    pub style: usize,
    /// Its pieces, the well in the middle first.
    pub pieces: Vec<Placed>,
    /// Its streets, and the ways from them to its pieces' doors.
    pub paths: Vec<Placed>,
    /// The columns it covers, from its lowest corner to its highest.
    pub min: I64Vec2,
    pub max: I64Vec2,
}

impl Village {
    /// The well in its middle.
    pub fn well(&self) -> &Placed {
        &self.pieces[0]
    }

    /// Whether any of it is in the columns `min..=max`.
    pub fn overlaps(&self, min: I64Vec2, max: I64Vec2) -> bool {
        self.min.cmple(max).all() && self.max.cmpge(min).all()
    }
}

/// A stretch of a village's street, or the way from it to a piece's door: `width` blocks across
/// and `length` along x or z from the corner it's placed at, its surface stepping a block up or
/// down where the ground does, at most once a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Path {
    length: u8,
    width: u8,
    along_x: bool,
    // each step along it from its first row, 2 bits each: 0 level, 1 up, 2 down
    steps: u32,
    style: u8,
}

// The most rows a stretch of path holds, as its steps fit a `u32`.
const PATH_ROWS: usize = 16;

impl Path {
    pub fn footprint(&self) -> I64Vec2 {
        let (length, width) = (i64::from(self.length), i64::from(self.width));
        if self.along_x {
            I64Vec2::new(length, width)
        } else {
            I64Vec2::new(width, length)
        }
    }

    /// How far its surface is above its first row's, `row` rows along it.
    fn rise(&self, row: i64) -> i64 {
        (0..row)
            .map(|step| match (self.steps >> (2 * step)) & 3 {
                1 => 1,
                2 => -1,
                _ => 0,
            })
            .sum()
    }

    /// The first air over its surface in `column`, with `min` its corner at its first row's
    /// surface.
    pub fn top(&self, min: I64Vec3, column: I64Vec2) -> i64 {
        let row = if self.along_x {
            column.x - min.x
        } else {
            column.y - min.z
        };
        min.y + self.rise(row)
    }

    // The least and most it rises over its first row.
    fn rise_range(&self) -> (i64, i64) {
        (0..i64::from(self.length))
            .map(|row| self.rise(row))
            .fold((0, 0), |(lo, hi), r| (lo.min(r), hi.max(r)))
    }

    /// What its block `at` becomes, with `min` its corner at its first row's surface and
    /// `ground` the first air over the ground there: the path's own block for the surface,
    /// earth under it down to the ground where it's raised, and air over it up to the ground
    /// where it's cut in, clearing what grew there.
    pub(super) fn stamp(&self, min: I64Vec3, at: I64Vec3, seed: u64, ground: i64) -> Option<Block> {
        let top = self.top(min, at.xz());
        if at.y == top - 1 {
            Some(VILLAGES.styles[self.style as usize].path.pick(seed, at))
        } else if at.y < top - 1 {
            (at.y >= ground).then_some(BlockType::Dirt.into())
        } else {
            (at.y <= ground).then_some(Block::AIR)
        }
    }

    /// The lowest and the past the highest of its surface's blocks, with `min` its corner at
    /// its first row's surface, which with the ground under it is what it can change.
    pub(super) fn bounds(&self, min: I64Vec3) -> (i64, i64) {
        let (down, up) = self.rise_range();
        (min.y + down - 1, min.y + up)
    }
}

// How far a village's pieces and streets reach from its middle across x or z at most, its
// streets' half-width either side of their middle, the fewest blocks a street runs, how long the
// streets on its main way and across it run, and branches off them, and how many houses it has.
pub(super) const VILLAGE_REACH: i64 = 56;
const STREET_HALF: i64 = 1;
const SHORTEST_STREET: usize = 6;
const MAIN_STREET: RangeInclusive<i64> = 24..=44;
const SIDE_STREET: RangeInclusive<i64> = 12..=30;
const BRANCH: RangeInclusive<i64> = 12..=24;
const HOUSES: RangeInclusive<i64> = 4..=8;
// The most a street may be cut into the ground or raised over it, how much the ground under a
// piece may rise and fall, and how much under the well.
const CUT: i64 = 2;
const PIECE_RISE: i64 = 5;
const WELL_RISE: i64 = 3;
// How far apart the ground's sampled across a village to see that it's gentle, and lamps along
// its streets stand.
const SURVEY_STRIDE: i64 = 12;
const LAMP_EVERY: i64 = 9;
// The fewest buildings a village has, besides its well and lamps, and the most of the ground
// across it that may be under water, a share of 1 / `WETTEST`.
const FEWEST_BUILDINGS: usize = 4;
// How many spots a piece is tried at before it's left out.
const PATIENCE: usize = 16;
const WETTEST: usize = 4;

// Draws from a village's seed, one after another.
struct Draws {
    seed: u64,
    n: i64,
}

impl Draws {
    fn next(&mut self) -> u64 {
        self.n += 1;
        hash(self.seed, self.n, 0x5eed)
    }

    fn range(&mut self, range: RangeInclusive<i64>) -> i64 {
        let span = (range.end() - range.start() + 1) as u64;
        range.start() + (self.next() % span) as i64
    }

    fn chance(&mut self, p: f64) -> bool {
        ((self.next() >> 11) as f64 / (1u64 << 53) as f64) < p
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> Option<T> {
        (!items.is_empty()).then(|| items[(self.next() % items.len() as u64) as usize])
    }
}

// The columns from one corner to the other, both in it.
type Rect = (I64Vec2, I64Vec2);

fn meets((a0, a1): Rect, (b0, b1): Rect) -> bool {
    a0.cmple(b1).all() && b0.cmple(a1).all()
}

fn grown((min, max): Rect, by: i64) -> Rect {
    (min - by, max + by)
}

// A street as it's laid: its middle's first block, the way it runs, and its surface's first air
// over each block along it.
struct Street {
    start: I64Vec2,
    dir: I64Vec2,
    tops: Vec<i64>,
}

impl Street {
    fn tile(&self, along: i64) -> I64Vec2 {
        self.start + self.dir * along
    }

    // Across it, to its left as it runs.
    fn across(&self) -> I64Vec2 {
        I64Vec2::new(-self.dir.y, self.dir.x)
    }

    fn rect(&self) -> Rect {
        let side = self.across() * STREET_HALF;
        let end = self.tile(self.tops.len() as i64 - 1);
        let (a, b) = (self.start - side, end + side);
        (a.min(b), a.max(b))
    }
}

// The quarter turns that face a piece's front, written facing +z, towards `front`.
fn facing(front: I64Vec2) -> u8 {
    match (front.x, front.y) {
        (0, 1) => 0,
        (-1, 0) => 1,
        (0, -1) => 2,
        _ => 3,
    }
}

// A village being laid out.
struct Layout<'a> {
    noise: &'a NoiseGenerator,
    style: usize,
    seed: u64,
    middle: I64Vec2,
    pieces: Vec<Placed>,
    paths: Vec<Placed>,
    streets: Vec<Street>,
    // what's taken: pieces, streets and the ways to doors
    taken: Vec<Rect>,
}

impl Layout<'_> {
    // The pieces of the village's style for `role`.
    fn of(&self, role: Role) -> Vec<usize> {
        let style = &VILLAGES.styles[self.style].name;
        (0..PIECES.len())
            .filter(|&i| PIECES[i].piece.role == role && PIECES[i].piece.styles.contains(style))
            .collect()
    }

    fn within(&self, (min, max): Rect) -> bool {
        (min - self.middle).min_element() >= -VILLAGE_REACH
            && (max - self.middle).max_element() <= VILLAGE_REACH
    }

    fn free(&self, rect: Rect) -> bool {
        self.within(rect) && !self.taken.iter().any(|taken| meets(rect, *taken))
    }

    // `piece` turned `rotation` with its footprint's corner at `min`, on its ground levelled at
    // the middle height under it, if that ground's dry and gentle enough.
    fn fit(&mut self, piece: usize, rotation: u8, min: I64Vec2, rise: i64) -> Option<Placed> {
        let footprint = PIECES[piece].template.footprint(rotation);
        let mut heights: Vec<i64> = (0..footprint.x)
            .flat_map(|x| (0..footprint.y).map(move |z| min + I64Vec2::new(x, z)))
            .map(|at| height(self.noise, at))
            .collect();
        heights.sort_unstable();
        let (lowest, highest) = (heights[0], heights[heights.len() - 1]);
        let suits = highest - lowest <= rise && lowest > WATER_LEVEL;
        suits.then(|| Placed {
            kind: Kind::Piece(piece),
            rotation,
            min: I64Vec3::new(min.x, ground_top(heights[heights.len() / 2]), min.y),
            lowest,
            highest,
            variant: hash(self.seed, self.pieces.len() as i64, 0x91ece),
        })
    }

    fn add(&mut self, placed: Placed) {
        let min = placed.min.xz();
        self.taken.push((min, min + placed.footprint() - 1));
        self.pieces.push(placed);
    }

    // Lays the rows of path, each `width` wide from its corner across the way `along_x` says,
    // and the first air over its surface, split into stretches short enough for a `Path`.
    fn pave(&mut self, mut rows: Vec<(I64Vec2, i64)>, width: i64, along_x: bool) {
        let along = |at: I64Vec2| if along_x { at.x } else { at.y };
        rows.sort_by_key(|&(corner, _)| along(corner));
        for stretch in rows.chunks(PATH_ROWS) {
            let mut steps = 0;
            for (i, pair) in stretch.windows(2).enumerate() {
                let step = match pair[1].1 - pair[0].1 {
                    1 => 1,
                    -1 => 2,
                    _ => 0,
                };
                steps |= step << (2 * i);
            }
            let path = Path {
                length: stretch.len() as u8,
                width: width as u8,
                along_x,
                steps,
                style: self.style as u8,
            };
            let (first, top) = stretch[0];
            let footprint = path.footprint();
            let heights: Vec<i64> = (0..footprint.x)
                .flat_map(|x| (0..footprint.y).map(move |z| first + I64Vec2::new(x, z)))
                .map(|at| height(self.noise, at))
                .collect();
            self.paths.push(Placed {
                kind: Kind::Path(path),
                rotation: 0,
                min: I64Vec3::new(first.x, top, first.y),
                lowest: *heights.iter().min().unwrap(),
                highest: *heights.iter().max().unwrap(),
                variant: self.seed,
            });
        }
    }

    // A street from `start` running `dir` for up to `length` blocks, its surface starting within
    // a block of `top`, as far as the ground lets it: over dry land, cut in or raised no more than
    // `CUT`, and clear of what's taken. Returns its index if it's long enough.
    fn street(&mut self, start: I64Vec2, dir: I64Vec2, length: i64, mut top: i64) -> Option<usize> {
        let mut street = Street {
            start,
            dir,
            tops: Vec::new(),
        };
        let across = street.across();
        for along in 0..length {
            let tile = street.tile(along);
            let row = (tile - across).min(tile + across);
            if !self.free((row, (tile - across).max(tile + across))) {
                break;
            }
            let ground = [tile - across, tile, tile + across].map(|at| height(self.noise, at));
            if ground.iter().any(|&g| g <= WATER_LEVEL) {
                break;
            }
            let here = ground_top(ground[1]).clamp(top - 1, top + 1);
            if ground.iter().any(|&g| (ground_top(g) - here).abs() > CUT) {
                break;
            }
            street.tops.push(here);
            top = here;
        }
        if street.tops.len() < SHORTEST_STREET {
            return None;
        }
        let rows = (0..street.tops.len())
            .map(|along| {
                let tile = street.tile(along as i64);
                ((tile - across).min(tile + across), street.tops[along])
            })
            .collect();
        self.pave(rows, 2 * STREET_HALF + 1, dir.x != 0);
        self.taken.push(street.rect());
        self.streets.push(street);
        Some(self.streets.len() - 1)
    }

    // `piece` beside street `index` on the side `side` (1 to its left, -1 to its right), its
    // frontage starting `along` it, set back `setback` from it with a path to its door. Returns
    // how far along the street it reaches, if it fits.
    fn beside(
        &mut self,
        piece: usize,
        index: usize,
        side: i64,
        along: i64,
        setback: i64,
    ) -> Option<i64> {
        let street = &self.streets[index];
        let out = street.across() * side;
        let rotation = facing(-out);
        let template = &PIECES[piece].template;
        let footprint = template.footprint(rotation);
        let (frontage, depth) = if street.dir.x != 0 {
            (footprint.x, footprint.y)
        } else {
            (footprint.y, footprint.x)
        };
        if along + frontage > street.tops.len() as i64 {
            return None;
        }
        let near = STREET_HALF + 1 + setback;
        let a = street.tile(along) + out * near;
        let b = street.tile(along + frontage - 1) + out * (near + depth - 1);
        let rect = (a.min(b), a.max(b));
        // a gap kept round buildings, but lamps stand right by the street
        if !self.free(grown(rect, setback.min(1))) {
            return None;
        }
        // its door faces the street, on its front row, with a way to it from the street
        let entrance = template
            .markers
            .iter()
            .find(|(kind, _)| kind == ENTRANCE)
            .map(|&(_, at)| rect.0 + template.turn(rotation, at).xz());
        let Some(entrance) = entrance else {
            // a lamp, standing right by the street
            let placed = self.fit(piece, rotation, rect.0, PIECE_RISE)?;
            self.add(placed);
            return Some(along + frontage);
        };
        let offset = entrance - street.start;
        let (at, from) = (offset.dot(street.dir), offset.dot(out));
        if from != near {
            // not on its front row, which `test_every_piece_loads_whole` rules out
            return None;
        }
        let street_top = street.tops[at as usize];
        let way: Vec<I64Vec2> = (1..=setback).map(|step| entrance - out * step).collect();
        let way_rect = (
            way.iter().copied().reduce(I64Vec2::min).unwrap(),
            way.iter().copied().reduce(I64Vec2::max).unwrap(),
        );
        if !self.free(way_rect) {
            return None;
        }
        let placed = self.fit(piece, rotation, rect.0, PIECE_RISE)?;
        let floor = placed.min.y;
        if (floor - street_top).abs() > setback + 1 {
            return None;
        }
        // the way steps evenly from its floor to the street's surface
        let rows = way
            .iter()
            .zip(1..)
            .map(|(&at, step)| {
                let rise = (street_top - floor) as f64 * step as f64 / (setback + 1) as f64;
                (at, floor + rise.round() as i64)
            })
            .collect();
        self.add(placed);
        self.taken.push(way_rect);
        self.pave(rows, 1, out.x != 0);
        Some(along + frontage)
    }
}

/// The village in `cell` of the villages' grid, from its hash `seed`, if one stands there: where
/// the ground's dry and gentle in a biome with a style, and no structure listed before it
/// (`earlier`, asked of the columns between two corners) stands within its clearing.
pub(super) fn lay_out(
    noise: &NoiseGenerator,
    cell: I64Vec2,
    seed: u64,
    earlier: impl Fn(I64Vec2, I64Vec2) -> bool,
) -> Option<Village> {
    let placement = &VILLAGES.placement;
    let corner = cell * placement.cell;
    let margin = placement.cell / EDGE_MARGIN;
    let span = (placement.cell - 2 * margin) as u64;
    let middle =
        corner + margin + I64Vec2::new((seed % span) as i64, (seed >> 32) as i64 % span as i64);
    let biome = noise.sample(middle.x as f64, middle.y as f64).biome;
    let name = noise.biomes().name(biome).unwrap_or(BASE_TERRAIN);
    let style = VILLAGES
        .styles
        .iter()
        .position(|style| style.biomes.iter().any(|b| b == name))?;
    // the ground across it gentle and mostly dry, though it may come down to the water's edge,
    // and its middle well over the sea, clear of landmarks and other structures
    let (mut lowest, mut highest, mut wet) = (i64::MAX, i64::MIN, 0);
    let survey = (-VILLAGE_REACH..=VILLAGE_REACH).step_by(SURVEY_STRIDE as usize);
    for x in survey.clone() {
        for z in survey.clone() {
            let at = middle + I64Vec2::new(x, z);
            if noise.landmarks().keeps_clear(at) {
                return None;
            }
            let ground = height(noise, at);
            if ground <= WATER_LEVEL {
                wet += 1;
                continue;
            }
            (lowest, highest) = (lowest.min(ground), highest.max(ground));
        }
    }
    let samples = survey.count().pow(2);
    let centre = height(noise, middle);
    if highest - lowest > placement.max_rise
        || wet * WETTEST > samples
        || centre - WATER_LEVEL < placement.min_above_sea
        || earlier(
            middle - VILLAGE_REACH - CLEARING,
            middle + VILLAGE_REACH + CLEARING,
        )
    {
        return None;
    }

    let mut draws = Draws { seed, n: 0 };
    let mut layout = Layout {
        noise,
        style,
        seed,
        middle,
        pieces: Vec::new(),
        paths: Vec::new(),
        streets: Vec::new(),
        taken: Vec::new(),
    };
    let well = draws.pick(&layout.of(Role::Well))?;
    let rotation = (draws.next() % 4) as u8;
    let footprint = PIECES[well].template.footprint(rotation);
    let well = layout.fit(well, rotation, middle - footprint / 2, WELL_RISE)?;
    layout.add(well);

    // streets out from the middle of each side of the well, longest along its main way, and
    // some branching off those
    let (min, max) = (well.min.xz(), well.min.xz() + footprint - 1);
    let mid = (min + max) / 2;
    let main_x = draws.chance(0.5);
    let mut mains = Vec::new();
    for dir in [I64Vec2::X, I64Vec2::NEG_X, I64Vec2::Y, I64Vec2::NEG_Y] {
        let main = (dir.x != 0) == main_x;
        let length = if main {
            draws.range(MAIN_STREET)
        } else if draws.chance(0.75) {
            draws.range(SIDE_STREET)
        } else {
            continue;
        };
        let start = match (dir.x, dir.y) {
            (1, _) => I64Vec2::new(max.x + 1, mid.y),
            (-1, _) => I64Vec2::new(min.x - 1, mid.y),
            (_, 1) => I64Vec2::new(mid.x, max.y + 1),
            _ => I64Vec2::new(mid.x, min.y - 1),
        };
        let street = layout.street(start, dir, length, well.min.y);
        if let Some(street) = street.filter(|_| main) {
            mains.push(street);
        }
    }
    for index in mains {
        let street = &layout.streets[index];
        let len = street.tops.len() as i64;
        if len < 20 || !draws.chance(0.6) {
            continue;
        }
        let at = draws.range(10..=len - 6);
        let side = if draws.chance(0.5) { 1 } else { -1 };
        let out = street.across() * side;
        let (start, top) = (
            street.tile(at) + out * (STREET_HALF + 1),
            street.tops[at as usize],
        );
        let length = draws.range(BRANCH);
        layout.street(start, out, length, top);
    }
    if layout.streets.len() < 2 {
        return None;
    }

    // the pieces, nearest the well first: the hall, smithy and tower among the first houses, the
    // farm and pen among the rest, and the rest of the houses and a second farm further out
    let houses = draws.range(HOUSES) as usize;
    let mut queue = vec![
        Role::Hall,
        Role::House,
        Role::Smithy,
        Role::House,
        Role::Farm,
        Role::Tower,
        Role::House,
        Role::Pen,
    ];
    queue.extend(std::iter::repeat_n(Role::House, houses.saturating_sub(3)));
    if draws.chance(0.5) {
        queue.push(Role::Farm);
    }
    let mut queue: Vec<usize> = queue
        .into_iter()
        .filter_map(|role| draws.pick(&layout.of(role)))
        .collect();
    queue.reverse();
    // where along each side of each street the next piece would go
    let mut spots: Vec<(usize, i64, i64)> = (0..layout.streets.len())
        .flat_map(|street| [(street, 1, 0), (street, -1, 0)])
        .collect();
    let mut failed = 0;
    while let Some(&piece) = queue.last() {
        let nearest = spots
            .iter_mut()
            .filter(|(street, _, along)| *along < layout.streets[*street].tops.len() as i64)
            .min_by_key(|(street, _, along)| {
                (layout.streets[*street].tile(*along) - middle)
                    .abs()
                    .max_element()
            });
        let Some((street, side, along)) = nearest else {
            break;
        };
        let setback = draws.range(1..=2);
        match layout.beside(piece, *street, *side, *along, setback) {
            Some(reach) => {
                *along = reach + draws.range(1..=3);
                queue.pop();
                failed = 0;
            }
            None => {
                *along += 2;
                failed += 1;
                if failed > PATIENCE {
                    queue.pop();
                    failed = 0;
                }
            }
        }
    }
    if layout.pieces.len() - 1 < FEWEST_BUILDINGS {
        return None;
    }
    // and lamps along the streets, every so often, on one side or the other
    let lamps = layout.of(Role::Lamp);
    for street in 0..layout.streets.len() {
        let length = layout.streets[street].tops.len() as i64;
        let mut side = if draws.chance(0.5) { 1 } else { -1 };
        for along in (2..length).step_by(LAMP_EVERY as usize) {
            let Some(lamp) = draws.pick(&lamps) else {
                break;
            };
            let _ = layout
                .beside(lamp, street, side, along, 0)
                .or_else(|| layout.beside(lamp, street, -side, along, 0));
            side = -side;
        }
    }

    let rects = layout.taken.iter();
    let min = rects.clone().map(|r| r.0).reduce(I64Vec2::min).unwrap();
    let max = rects.map(|r| r.1).reduce(I64Vec2::max).unwrap();
    Some(Village {
        cell,
        style,
        pieces: layout.pieces,
        paths: layout.paths,
        min,
        max,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet, VecDeque};
    use std::sync::Arc;

    use super::*;
    use crate::chunks::generate::structures::Placements;

    // Twelve kilometres of seed 3's world round its spawn, and of seed 42's, with a few dozen
    // villages between them.
    fn villages(seed: u32) -> (NoiseGenerator, Vec<Arc<Village>>) {
        let noise = NoiseGenerator::new(seed);
        let (x, z, _) = noise.find_spawn();
        let spawn = I64Vec2::new(x, z);
        let found = Placements::default().villages_near(&noise, spawn - 6000, spawn + 5999);
        (noise, found)
    }

    // The way a piece turned `rotation` faces, out of its front.
    fn front(rotation: u8) -> I64Vec2 {
        [I64Vec2::Y, I64Vec2::NEG_X, I64Vec2::NEG_Y, I64Vec2::X][rotation as usize]
    }

    fn rect(placed: &Placed) -> Rect {
        (placed.min.xz(), placed.min.xz() + placed.footprint() - 1)
    }

    #[test]
    fn test_every_piece_loads_whole() {
        let styles: Vec<&str> = VILLAGES.styles.iter().map(|s| s.name.as_str()).collect();
        let biomes = crate::chunks::generate::biome::Biomes::new(3);
        for style in &VILLAGES.styles {
            for biome in &style.biomes {
                assert!(VILLAGES.placement.biomes.contains(biome), "{biome}");
                assert!(biome == BASE_TERRAIN || biomes.names().any(|b| b == biome));
            }
            // every role in every style
            for role in [
                Role::House,
                Role::Hall,
                Role::Smithy,
                Role::Farm,
                Role::Pen,
                Role::Tower,
                Role::Well,
                Role::Lamp,
            ] {
                let fits =
                    |p: &&Piece| p.piece.role == role && p.piece.styles.contains(&style.name);
                assert!(
                    PIECES.iter().any(|p| fits(&p)),
                    "no {role:?} in {}",
                    style.name
                );
            }
        }
        let names: HashSet<_> = PIECES.iter().map(|p| &p.name).collect();
        assert_eq!(PIECES.len(), names.len());
        for piece in PIECES.iter() {
            let template = &piece.template;
            let name = &piece.name;
            assert!(
                piece
                    .piece
                    .styles
                    .iter()
                    .all(|s| styles.contains(&s.as_str())),
                "{name}"
            );
            let markers = |kind: &str| -> Vec<I64Vec3> {
                let at = template.markers.iter().filter(|(k, _)| k == kind);
                at.map(|&(_, at)| at).collect()
            };
            let role = piece.piece.role;
            assert_eq!(
                role == Role::House,
                !markers(BED).is_empty(),
                "{name}'s beds"
            );
            assert_eq!(
                role == Role::Well,
                !markers(GATHER).is_empty(),
                "{name}'s meeting places"
            );
            assert_eq!(
                role == Role::Pen,
                !markers(ANIMAL).is_empty(),
                "{name}'s animals"
            );
            let works = matches!(
                role,
                Role::Hall | Role::Smithy | Role::Farm | Role::Pen | Role::Tower
            );
            assert_eq!(works, !markers(WORK).is_empty(), "{name}'s workplace");
            // one way in, on its front, standing on its ground with room to walk in
            let entrances = markers(ENTRANCE);
            if role == Role::Lamp {
                assert!(entrances.is_empty());
                assert!(template.size.z <= 2, "{name} stands in a street");
                continue;
            }
            let [entrance] = entrances[..] else {
                panic!("{name} has {} entrances", entrances.len());
            };
            assert_eq!(template.size.z - 1, entrance.z, "{name}'s entrance");
            assert_eq!(template.floor, entrance.y, "{name}'s entrance");
            for seed in 0..8 {
                let solid = |at: I64Vec3| {
                    template
                        .cell(0, at, seed)
                        .is_some_and(|b| b.kind.is_solid())
                };
                assert!(
                    solid(entrance - I64Vec3::Y),
                    "{name}'s entrance has no ground"
                );
                assert!(
                    !solid(entrance) && !solid(entrance + I64Vec3::Y),
                    "{name}'s entrance"
                );
            }
        }
    }

    #[test]
    fn test_villages_are_laid_out_the_same_whatever_asks_and_in_whatever_order() {
        let (noise, all) = villages(3);
        assert!(all.len() >= 10, "only {} villages", all.len());
        let (x, z, _) = noise.find_spawn();
        let spawn = I64Vec2::new(x, z);
        // a fresh store asked for windows of it, last first, lays out the same ones
        let placements = Placements::default();
        let mut found = Vec::new();
        for window in (0..9).rev() {
            let min = spawn - 6000 + I64Vec2::new(window % 3, window / 3) * 4000;
            for village in placements.villages_near(&noise, min, min + 3999) {
                let piece = |p: &Placed| p.overlaps(min, min + 3999);
                // and the structures there are its pieces and paths that reach in
                let near = placements.near(&noise, min, min + 3999);
                for placed in village
                    .pieces
                    .iter()
                    .chain(&village.paths)
                    .filter(|p| piece(p))
                {
                    assert!(near.contains(placed));
                }
                if !found.iter().any(|v: &Arc<Village>| v.cell == village.cell) {
                    found.push(village);
                }
            }
        }
        found.sort_by_key(|v| (v.cell.x, v.cell.y));
        let mut all = all.clone();
        all.sort_by_key(|v| (v.cell.x, v.cell.y));
        assert_eq!(all, found);
        // and another world others
        let (_, elsewhere) = villages(4);
        assert!(!elsewhere
            .iter()
            .any(|v| all.iter().any(|a| a.well() == v.well())));
    }

    #[test]
    fn test_villages_stand_in_their_styles_biomes_with_every_kind_of_piece() {
        let mut styles = HashSet::new();
        let mut roles = HashMap::new();
        let mut villages_seen = 0;
        for seed in [3, 42] {
            let (noise, all) = villages(seed);
            for village in &all {
                villages_seen += 1;
                let middle = village.well().min.xz() + village.well().footprint() / 2;
                let biome = noise.sample(middle.x as f64, middle.y as f64).biome;
                let biome = noise.biomes().name(biome).unwrap_or(BASE_TERRAIN);
                let style = &VILLAGES.styles[village.style];
                assert!(
                    style.biomes.iter().any(|b| b == biome),
                    "{style:?} in {biome}"
                );
                styles.insert(village.style);
                for placed in &village.pieces {
                    let Kind::Piece(piece) = placed.kind else {
                        panic!("{placed:?} isn't a piece");
                    };
                    assert!(PIECES[piece].piece.styles.contains(&style.name));
                    *roles.entry(PIECES[piece].piece.role).or_insert(0) += 1;
                }
                let houses = village.pieces.iter().filter(|p| role(p) == Role::House);
                assert!(houses.count() >= 2, "{village:?}");
            }
        }
        // timber and adobe near the spawns: taiga's lodges are far off
        assert!(styles.len() >= 2, "{styles:?}");
        for role in [
            Role::Hall,
            Role::Smithy,
            Role::Farm,
            Role::Pen,
            Role::Tower,
            Role::Lamp,
        ] {
            let count = roles.get(&role).copied().unwrap_or(0);
            assert!(
                count * 3 >= villages_seen,
                "{count} {role:?} in {villages_seen} villages"
            );
        }
        assert!(roles[&Role::House] >= villages_seen * 4, "{roles:?}");
    }

    fn role(placed: &Placed) -> Role {
        match placed.kind {
            Kind::Piece(piece) => PIECES[piece].piece.role,
            _ => panic!("not a piece"),
        }
    }

    #[test]
    fn test_pieces_never_overlap_and_stand_on_ground_levelled_for_them() {
        let (noise, all) = villages(3);
        for village in &all {
            let parts: Vec<&Placed> = village.pieces.iter().chain(&village.paths).collect();
            for (i, a) in parts.iter().enumerate() {
                for b in &parts[i + 1..] {
                    assert!(!meets(rect(a), rect(b)), "{a:?} and {b:?}");
                }
                assert!(village.overlaps(rect(a).0, rect(a).1));
            }
            let middle = village.well().min.xz() + village.well().footprint() / 2;
            for placed in &village.pieces {
                let (min, max) = rect(placed);
                assert!((min - middle).min_element() >= -VILLAGE_REACH - 4);
                assert!((max - middle).max_element() <= VILLAGE_REACH + 4);
                let mut heights: Vec<i64> = (min.x..=max.x)
                    .flat_map(|x| (min.y..=max.y).map(move |z| I64Vec2::new(x, z)))
                    .map(|at| height(&noise, at))
                    .collect();
                heights.sort_unstable();
                assert_eq!(ground_top(heights[heights.len() / 2]), placed.min.y);
                assert_eq!(
                    (heights[0], *heights.last().unwrap()),
                    (placed.lowest, placed.highest)
                );
                let rise = if role(placed) == Role::Well {
                    WELL_RISE
                } else {
                    PIECE_RISE
                };
                assert!(placed.highest - placed.lowest <= rise && placed.lowest > WATER_LEVEL);
            }
        }
    }

    #[test]
    fn test_paths_join_every_door_to_the_well_a_step_at_a_time() {
        let (_, all) = villages(3);
        for village in &all {
            // the surface over every column of path, and the well's
            let mut tops = HashMap::new();
            for placed in &village.paths {
                let Kind::Path(path) = placed.kind else {
                    panic!("{placed:?} isn't a path");
                };
                let (min, max) = rect(placed);
                for x in min.x..=max.x {
                    for z in min.y..=max.y {
                        let column = I64Vec2::new(x, z);
                        let top = path.top(placed.min, column);
                        assert!(tops.insert(column, top).is_none());
                    }
                }
            }
            let well = village.well();
            let (min, max) = rect(well);
            let mut reached = VecDeque::new();
            for x in min.x..=max.x {
                for z in min.y..=max.y {
                    tops.insert(I64Vec2::new(x, z), well.min.y);
                    reached.push_back(I64Vec2::new(x, z));
                }
            }
            // walked from the well along the paths, a block up or down at a time
            let mut seen: HashSet<I64Vec2> = reached.iter().copied().collect();
            while let Some(at) = reached.pop_front() {
                for step in [I64Vec2::X, I64Vec2::NEG_X, I64Vec2::Y, I64Vec2::NEG_Y] {
                    let next = at + step;
                    let climbs = tops
                        .get(&next)
                        .is_some_and(|top| (top - tops[&at]).abs() <= 1);
                    if climbs && seen.insert(next) {
                        reached.push_back(next);
                    }
                }
            }
            for placed in village.pieces.iter().skip(1) {
                let Some((_, entrance)) = placed.markers().find(|(kind, _)| *kind == ENTRANCE)
                else {
                    continue;
                };
                assert_eq!(placed.min.y, entrance.y);
                let outside = entrance.xz() + front(placed.rotation);
                assert!(!placed.covers(outside.x, outside.y));
                assert!(
                    seen.contains(&outside),
                    "{} cut off in {village:?}",
                    placed.name()
                );
                assert!(
                    (tops[&outside] - placed.min.y).abs() <= 1,
                    "{}",
                    placed.name()
                );
            }
        }
    }
}
