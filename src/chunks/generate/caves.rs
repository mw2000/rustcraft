use bevy::math::{I64Vec2, I64Vec3, IVec3, Vec3};
use noise::{NoiseFn, Perlin};

use super::biome::smoothstep;
use super::generator::ground_top;
use super::region_store::HeightSampler;
use super::WATER_LEVEL;
use crate::block::BlockType;
use crate::chunks::chunk::CHUNK_SIZE;

const SEED_OFFSET: u32 = 3000;

// The noise is sampled every this many blocks each way and blended in between: it changes
// slowly enough that the blend can't be told apart from the real thing, at a sixty-fourth of
// the samples.
const GRID: i64 = 4;
const CELLS_ACROSS: i64 = CHUNK_SIZE as i64 / GRID;
const NODES_ACROSS: i64 = CELLS_ACROSS + 1;

// Tunnels run where two of three sheets meet, each sheet where its noise crosses zero, so that
// tunnels on a shared sheet cross and join up into networks. The first sheet is stretched
// vertically into a gently rolling floor, so tunnels along it climb gently enough to walk, and
// those along the other two wind up and down between them.
const TUNNEL_WAVELENGTH: f64 = 120.0;
// Each sheet's scale, vertical stretch and offset: Perlin noise is zero at every point of its
// lattice, and sheets sharing a lattice would all meet at each of its points.
const SHEETS: [(f64, f64, [f64; 3]); 3] = [
    (1.0, 2.5, [0.31, 0.77, 0.53]),
    (1.13, 1.4, [0.62, 0.18, 0.91]),
    (0.89, 1.4, [0.47, 0.39, 0.12]),
];
// How near both sheets a block must be to be in a tunnel, in noise.
const TUNNEL_WIDTH: f32 = 0.04;

// Caverns open up where all three sheets meet, as near all three as this, which grows the
// deeper they are: from a widening where tunnels join near the surface to halls far below.
const CAVERN_SHALLOW: (i64, f64) = (WATER_LEVEL, 0.05);
const CAVERN_DEEP: (i64, f64) = (96, 0.16);

// Nothing is carved below this, so the world keeps a floor.
const CAVE_FLOOR: i64 = 8;
// Caves stay this many blocks under the ground, and narrow to nothing over this many more
// below that.
const ROOF: i64 = 5;
const ROOF_FADE: i64 = 8;

// Nothing simulates water, so caves stay this far under the floor of any sea within SEA_REACH
// blocks, rising SEA_SLOPE blocks for each block away from it. The roof is thicker than the
// slope, so no water is ever beside a cave, nor over one.
const SEA_ROOF: i64 = 12;
const SEA_SLOPE: i64 = 2;
const SEA_REACH: i64 = 16;

// Caves break the surface on dry land where the mouth noise is high, coming up through it at
// their full size.
const MOUTH_WAVELENGTH: f64 = 150.0;
const MOUTH_THRESHOLD: f64 = 0.45;
// Mouths open over this range of the noise, so they shrink to nothing at their edges.
const MOUTH_BLEND: f64 = 0.1;
const MOUTH_ABOVE_SEA: i64 = 4;

// Slack for the bounds `may_carve` rules cells out by, against rounding in the blend.
const BOUND_SLACK: f32 = 1e-4;

/// The noise caves are carved out of: winding tunnels, caverns that open up deeper down, and
/// the mouths where they reach the surface.
pub struct Caves {
    sheets: [Perlin; 3],
    mouths: Perlin,
}

impl Caves {
    pub fn new(seed: u32) -> Self {
        let seed = seed.wrapping_add(SEED_OFFSET);
        Self {
            sheets: [0, 1, 2].map(|i| Perlin::new(seed.wrapping_add(i))),
            mouths: Perlin::new(seed.wrapping_add(3)),
        }
    }

    // The three sheets' noise at a point of the grid.
    fn sample(&self, point: I64Vec3) -> [f32; 3] {
        let [x, y, z] = point.as_dvec3().to_array();
        let sheet = |i: usize| {
            let (scale, stretch, offset) = SHEETS[i];
            let frequency = scale / TUNNEL_WAVELENGTH;
            self.sheets[i].get([
                x * frequency + offset[0],
                y * frequency * stretch + offset[1],
                z * frequency + offset[2],
            ]) as f32
        };
        [sheet(0), sheet(1), sheet(2)]
    }

    /// Whether `block` is carved out, in a column whose caves reach up to `top`, see
    /// `column_tops`. The same as `Caves::chunk` finds for the chunk holding it.
    pub fn carves(&self, block: I64Vec3, top: i64) -> bool {
        if !(CAVE_FLOOR..top).contains(&block.y) {
            return false;
        }
        let base = block.div_euclid(I64Vec3::splat(GRID)) * GRID;
        let corners = CORNERS.map(|corner| self.sample(base + corner.as_i64vec3() * GRID));
        let t = (block - base).as_vec3() / GRID as f32;
        carved(blend(&corners, t), block.y, top)
    }

    /// How far down the sky reaches straight down column `x`, `z`, whose ground tops out at
    /// `ground` and whose caves reach up to `top`: the lowest block of a shaft a cave mouth
    /// opens there, or `ground` where there's none.
    pub fn sky_reach(&self, x: i64, z: i64, ground: i64, top: i64) -> i64 {
        let mut reach = ground;
        while self.carves(I64Vec3::new(x, reach - 1, z), top) {
            reach -= 1;
        }
        reach
    }

    /// What caves carve out of the chunk whose lowest block is `origin`, with `top(x, z)` the
    /// height caves reach up to in each of its columns, and `highest` the highest of those.
    /// None if they carve nothing.
    pub fn chunk(
        &self,
        origin: I64Vec3,
        top: impl Fn(i64, i64) -> i64,
        highest: i64,
    ) -> Option<ChunkCaves> {
        let size = CHUNK_SIZE as i64;
        if origin.y >= highest || origin.y + size <= CAVE_FLOOR {
            return None;
        }
        let samples: Vec<_> = cube(NODES_ACROSS as i32)
            .map(|node| self.sample(origin + node.as_i64vec3() * GRID))
            .collect();
        let node = |node: IVec3| {
            let across = NODES_ACROSS as i32;
            samples[((node.x * across + node.y) * across + node.z) as usize]
        };

        let mut caves = ChunkCaves([0; 64]);
        for cell in cube(CELLS_ACROSS as i32) {
            let corners = CORNERS.map(|corner| node(cell + corner));
            let lowest = cell * GRID as i32;
            // most cells are nowhere near a cave
            if !may_carve(&corners, origin.y + lowest.y as i64) {
                continue;
            }
            for offset in cube(GRID as i32) {
                let local = (lowest + offset).as_i64vec3();
                let (y, top) = (origin.y + local.y, top(local.x, local.z));
                let t = offset.as_vec3() / GRID as f32;
                if (CAVE_FLOOR..top).contains(&y) && carved(blend(&corners, t), y, top) {
                    caves.insert(local);
                }
            }
        }
        caves.0.iter().any(|&bits| bits != 0).then_some(caves)
    }

    /// How high caves can reach in each column of the chunks at chunk `x`, `z`, in
    /// `ChunkColumns` order: a few blocks under the ground, well under any sea near it, and up
    /// through the surface at a cave mouth.
    pub fn column_tops(&self, heights: &mut HeightSampler, x: i64, z: i64) -> Vec<i64> {
        let size = CHUNK_SIZE as i64;
        let (min_x, min_z) = (x * size, z * size);
        let mut ground = |x: i64, z: i64| ground_top(heights.height(x, z).round() as i64);

        let mut tops = Vec::with_capacity((size * size) as usize);
        for cx in 0..size {
            for cz in 0..size {
                let (x, z) = (min_x + cx, min_z + cz);
                let ground = ground(x, z);
                let mouth = if ground >= WATER_LEVEL + MOUTH_ABOVE_SEA {
                    let noise = self
                        .mouths
                        .get([x as f64 / MOUTH_WAVELENGTH, z as f64 / MOUTH_WAVELENGTH]);
                    smoothstep(MOUTH_THRESHOLD, MOUTH_THRESHOLD + MOUTH_BLEND, noise)
                } else {
                    0.0
                };
                tops.push(ground - ROOF + (mouth * (ROOF + ROOF_FADE) as f64).round() as i64);
            }
        }

        // inland, with no sea near, that's all
        let (min, max) = (
            I64Vec2::new(min_x, min_z) - SEA_REACH,
            I64Vec2::new(min_x, min_z) + size - 1 + SEA_REACH,
        );
        if heights.lowest(min, max).round() as i64 >= WATER_LEVEL {
            return tops;
        }
        // the lowest of the sea floors nearby plus the slope up from each, along x then z
        let span = size + 2 * SEA_REACH;
        let mut floors = Vec::with_capacity((span * span) as usize);
        for px in 0..span {
            for pz in 0..span {
                let ground = ground_top(heights.height(min.x + px, min.y + pz).round() as i64);
                floors.push(if ground < WATER_LEVEL {
                    ground
                } else {
                    i64::MAX
                });
            }
        }
        let nearest = |floor: &dyn Fn(i64) -> i64| {
            (-SEA_REACH..=SEA_REACH)
                .map(|d| floor(d).saturating_add(SEA_SLOPE * d.abs()))
                .min()
                .unwrap()
        };
        let mut along_x = Vec::with_capacity((size * span) as usize);
        for cx in 0..size {
            for pz in 0..span {
                along_x.push(nearest(&|d| {
                    floors[((cx + SEA_REACH + d) * span + pz) as usize]
                }));
            }
        }
        for cx in 0..size {
            for cz in 0..size {
                let floor = nearest(&|d| along_x[(cx * span + cz + SEA_REACH + d) as usize]);
                let top = &mut tops[(cx * size + cz) as usize];
                *top = (*top).min(floor.saturating_sub(SEA_ROOF));
            }
        }
        tops
    }
}

/// The blocks of a chunk caves carve out, a bit each.
pub struct ChunkCaves([u64; 64]);

impl ChunkCaves {
    fn bit(local: I64Vec3) -> (usize, u64) {
        let size = CHUNK_SIZE as i64;
        let index = ((local.x * size + local.z) * size + local.y) as usize;
        (index / 64, 1 << (index % 64))
    }

    fn insert(&mut self, local: I64Vec3) {
        let (word, bit) = Self::bit(local);
        self.0[word] |= bit;
    }

    /// Turns the blocks carved out of `cells` to air, in `ChunkData::from_cells` order.
    pub fn carve(&self, cells: &mut [BlockType]) {
        for (word, &bits) in self.0.iter().enumerate() {
            let mut bits = bits;
            while bits != 0 {
                cells[word * 64 + bits.trailing_zeros() as usize] = BlockType::Air;
                bits &= bits - 1;
            }
        }
    }
}

// A cell's corners from its lowest, in the order `blend` takes them.
const CORNERS: [IVec3; 8] = [
    IVec3::new(0, 0, 0),
    IVec3::new(0, 0, 1),
    IVec3::new(0, 1, 0),
    IVec3::new(0, 1, 1),
    IVec3::new(1, 0, 0),
    IVec3::new(1, 0, 1),
    IVec3::new(1, 1, 0),
    IVec3::new(1, 1, 1),
];

// Every point of a cube `across` wide from the origin, x outermost and z innermost.
fn cube(across: i32) -> impl Iterator<Item = IVec3> {
    (0..across.pow(3))
        .map(move |i| IVec3::new(i / (across * across), i / across % across, i % across))
}

// The noise `t` of the way across a cell from its lowest corner.
fn blend(corners: &[[f32; 3]; 8], t: Vec3) -> [f32; 3] {
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    std::array::from_fn(|field| {
        let along_z = |i: usize| lerp(corners[i][field], corners[i + 1][field], t.z);
        let along_y = |i: usize| lerp(along_z(i), along_z(i + 2), t.y);
        lerp(along_y(0), along_y(4), t.x)
    })
}

// How near all three sheets a block at height `y` must be to be in a cavern, in noise.
fn cavern_size(y: i64) -> f32 {
    let depth = smoothstep(CAVERN_SHALLOW.0 as f64, CAVERN_DEEP.0 as f64, y as f64);
    (CAVERN_SHALLOW.1 + (CAVERN_DEEP.1 - CAVERN_SHALLOW.1) * depth) as f32
}

// Whether a block at height `y` this near the sheets is carved out, under a column whose caves
// reach up to `top`.
fn carved(sheets: [f32; 3], y: i64, top: i64) -> bool {
    let fade = smoothstep(0.0, ROOF_FADE as f64, (top - y) as f64) as f32;
    let (tunnel, cavern) = nearness(sheets.map(|sheet| sheet * sheet));
    let size = cavern_size(y);
    tunnel < TUNNEL_WIDTH * TUNNEL_WIDTH * fade || cavern < size * size * fade
}

// Whether any block in a cell with these corners and its bottom at `bottom` could be carved
// out: the blend never strays outside its corners, and caverns grow downwards.
fn may_carve(corners: &[[f32; 3]; 8], bottom: i64) -> bool {
    let nearest = |sheet: usize| {
        let (low, high) = corners
            .iter()
            .fold((f32::MAX, f32::MIN), |(low, high), corner| {
                (low.min(corner[sheet]), high.max(corner[sheet]))
            });
        if low <= 0.0 && high >= 0.0 {
            0.0
        } else {
            (low.abs().min(high.abs()) - BOUND_SLACK).max(0.0)
        }
    };
    let (tunnel, cavern) = nearness([0, 1, 2].map(nearest).map(|near| near * near));
    let size = cavern_size(bottom);
    tunnel < TUNNEL_WIDTH * TUNNEL_WIDTH || cavern < size * size
}

// How near the nearest two sheets are, for tunnels, and all three, for caverns, from how near
// each one is, all squared.
fn nearness([a, b, c]: [f32; 3]) -> (f32, f32) {
    let all = a + b + c;
    (all - a.max(b).max(c), all)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::chunks::generate::noise::NoiseGenerator;
    use crate::chunks::generate::region_store::RegionStore;

    fn regions(seed: u32) -> RegionStore {
        RegionStore::new(Arc::new(NoiseGenerator::new(seed)))
    }

    #[test]
    fn test_chunks_carve_what_each_block_alone_would() {
        let regions = regions(4);
        let caves = regions.noise().caves();
        let (spawn_x, spawn_z, _) = regions.noise().find_spawn();
        let size = CHUNK_SIZE as i64;
        let mut checked = 0;
        for i in 0..24 {
            let (x, y, z) = (spawn_x / size + i % 5, 2 + i, spawn_z / size - i % 3);
            let columns = regions.chunk_columns(&mut regions.sampler(), x, z);
            let origin = I64Vec3::new(x, y, z) * size;
            let top = |x, z| columns.cave_top(x, z);
            let chunk = caves.chunk(origin, top, columns.cave_highest);
            for local in cube(CHUNK_SIZE as i32).map(|local| local.as_i64vec3()) {
                let alone = caves.carves(origin + local, top(local.x, local.z));
                let (word, bit) = ChunkCaves::bit(local);
                let in_chunk = chunk.as_ref().is_some_and(|chunk| chunk.0[word] & bit != 0);
                assert_eq!(alone, in_chunk, "block {local} of the chunk at {origin}");
                checked += alone as usize;
            }
        }
        assert!(checked > 100, "only {checked} blocks carved");
    }

    #[test]
    fn test_caverns_open_up_deeper_down() {
        let caves = Caves::new(4);
        // blocks in a cavern but not a tunnel, at each height
        let caverns = |y: i64| {
            (0..40_000)
                .filter(|i| {
                    let block = I64Vec3::new(i % 200 * 7, y, i / 200 * 7);
                    let (tunnel, cavern) = nearness(caves.sample(block).map(|s| s * s));
                    tunnel >= TUNNEL_WIDTH * TUNNEL_WIDTH
                        && cavern < cavern_size(y) * cavern_size(y)
                })
                .count()
        };
        let (deep, shallow) = (caverns(100), caverns(420));
        assert!(
            deep > 5 * shallow.max(1),
            "{deep} deep and {shallow} shallow"
        );
    }

    #[test]
    fn test_cave_mouths_open_only_on_dry_land() {
        let regions = regions(1);
        let (spawn_x, spawn_z, _) = regions.noise().find_spawn();
        let size = CHUNK_SIZE as i64;
        let mut mouths = 0;
        for x in -20..20 {
            for z in -20..20 {
                let (x, z) = (spawn_x / size + x, spawn_z / size + z);
                let columns = regions.chunk_columns(&mut regions.sampler(), x, z);
                for (cx, cz) in (0..size).flat_map(|cx| (0..size).map(move |cz| (cx, cz))) {
                    let ground = ground_top(columns.height(cx, cz));
                    if columns.sky_reach(cx, cz) < ground {
                        assert!(ground >= WATER_LEVEL + MOUTH_ABOVE_SEA);
                        mouths += 1;
                    }
                }
            }
        }
        assert!(mouths > 0, "no cave mouths near spawn");
    }
}
