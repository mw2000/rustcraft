// Times each stage of building terrain around spawn, one thread, for comparing before and after a
// change and for profiling. Run with:
//   cargo run --release --example bench -- [seed, default 1] [lod, to time only LOD meshes]
//   [torches, to stand a torch every 4 blocks on every floor before meshing chunks, or shapes,
//   to stand stairs, slabs, fences, panes, lanterns and the like every 2 blocks on them]
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bevy::math::{I64Vec3, U16Vec3};
use rustcraft::block::{Block, BlockType};
use rustcraft::chunks::chunk::{ChunkCoordinate, ChunkData, CHUNK_SIZE};
use rustcraft::chunks::generate::generator::{
    generate_chunk, generate_chunk_mesh, generate_lod_mesh,
};
use rustcraft::chunks::generate::noise::NoiseGenerator;
use rustcraft::chunks::generate::region_store::RegionStore;
use rustcraft::chunks::lod::LodKey;

// Chunks across the area generated, and how many layers of them up from below the surface.
const ACROSS: i64 = 20;
const LAYERS: i64 = 8;
// LOD nodes meshed at each level, in a square round spawn.
const NODES_ACROSS: i64 = 4;
const LEVELS: std::ops::RangeInclusive<u8> = 1..=8;

fn timed<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = f();
    println!("{label:>16}: {:>8.1?}", start.elapsed());
    result
}

fn main() {
    let seed = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let noise = Arc::new(NoiseGenerator::new(seed));
    let (sx, sz, sh) = noise.find_spawn();
    let spawn = ChunkCoordinate::containing(I64Vec3::new(sx, sh as i64, sz)).0;
    let regions = Arc::new(RegionStore::new(noise.clone()));
    let mode = std::env::args().nth(2);
    if mode.as_deref() != Some("lod") {
        chunks(&regions, spawn, mode.as_deref());
    }
    lods(&regions, spawn);
}

fn chunks(regions: &Arc<RegionStore>, spawn: I64Vec3, mode: Option<&str>) {
    let coords: Vec<ChunkCoordinate> = (-ACROSS / 2..ACROSS / 2)
        .flat_map(|x| {
            (-LAYERS / 2..LAYERS / 2).flat_map(move |y| {
                (-ACROSS / 2..ACROSS / 2)
                    .map(move |z| ChunkCoordinate(spawn + I64Vec3::new(x, y, z)))
            })
        })
        .collect();
    timed("regions", || {
        for c in &coords {
            let size = CHUNK_SIZE as i64;
            regions.sampler().height(c.0.x * size, c.0.z * size);
        }
    });
    let data: HashMap<ChunkCoordinate, Arc<ChunkData>> = timed("generate chunks", || {
        coords
            .iter()
            .map(|&c| (c, Arc::new(generate_chunk(regions.clone(), c))))
            .collect()
    });
    let stand: Option<fn(&ChunkData) -> ChunkData> = match mode {
        Some("torches") => Some(stand_torches),
        Some("shapes") => Some(stand_shapes),
        _ => None,
    };
    let data = match stand {
        Some(stand) => data
            .into_iter()
            .map(|(c, chunk)| (c, Arc::new(stand(&chunk))))
            .collect(),
        None => data,
    };
    // the inner chunks, which have all their neighbours
    let meshed: Vec<ChunkCoordinate> = coords
        .iter()
        .copied()
        .filter(|c| c.neighbors_26().iter().all(|n| data.contains_key(n)))
        .collect();
    let mut best = Duration::MAX;
    let (mut vertices, mut inner) = (0, 0);
    for _ in 0..3 {
        let start = Instant::now();
        (vertices, inner) = (0, 0);
        for &c in &meshed {
            let neighbours = c
                .neighbors_26()
                .iter()
                .map(|n| data.get(n).cloned())
                .collect();
            let meshes =
                generate_chunk_mesh(data[&c].clone(), neighbours, c, regions.noise(), false);
            vertices += meshes.opaque.count_vertices();
            inner += meshes.inner_vertices();
        }
        best = best.min(start.elapsed());
    }
    println!(
        "{:>16}: {best:>8.1?} best of 3, {} chunks, {vertices} vertices, {inner} between leaves",
        "mesh chunks",
        meshed.len()
    );
}

// A torch on every floor, open or in a cave, every 4 blocks each way.
fn stand_torches(chunk: &ChunkData) -> ChunkData {
    let mut chunk = chunk.clone();
    for x in (0..CHUNK_SIZE).step_by(4) {
        for z in (0..CHUNK_SIZE).step_by(4) {
            for y in 1..CHUNK_SIZE {
                let (under, at) = (U16Vec3::new(x, y - 1, z), U16Vec3::new(x, y, z));
                if chunk.get_block_at(under).is_solid() && chunk.get_block_at(at) == BlockType::Air
                {
                    chunk.set_block_at(at, BlockType::Torch);
                }
            }
        }
    }
    chunk
}

// Shapes on every floor, open or in a cave, every 2 blocks each way, a different one at each.
fn stand_shapes(chunk: &ChunkData) -> ChunkData {
    const SHAPES: [&str; 8] = [
        "stone_brick_stairs[facing=east]",
        "planks_slab[half=top]",
        "planks_fence",
        "glass_pane",
        "cobblestone_wall",
        "lantern",
        "glass",
        "log[axis=x]",
    ];
    let mut chunk = chunk.clone();
    for x in (0..CHUNK_SIZE).step_by(2) {
        for z in (0..CHUNK_SIZE).step_by(2) {
            for y in 1..CHUNK_SIZE {
                let (under, at) = (U16Vec3::new(x, y - 1, z), U16Vec3::new(x, y, z));
                if chunk.get_block_at(under).is_solid() && chunk.get_block_at(at) == BlockType::Air
                {
                    let shape = SHAPES[((x / 2 + z / 2 * 3 + y) % 8) as usize];
                    chunk.set_block_at(at, shape.parse::<Block>().unwrap());
                }
            }
        }
    }
    chunk
}

// Best of 3 at each level, after a first pass that fills the regions and column grids, which
// nodes share with those near them.
fn lods(regions: &Arc<RegionStore>, spawn: I64Vec3) {
    let keys = |level: u8| {
        let centre = LodKey::containing(ChunkCoordinate(spawn), level).coord;
        (-NODES_ACROSS / 2..NODES_ACROSS / 2).flat_map(move |x| {
            (-NODES_ACROSS / 2..NODES_ACROSS / 2).flat_map(move |z| {
                (-1..=1).map(move |y| LodKey {
                    level,
                    coord: centre + I64Vec3::new(x, y, z),
                })
            })
        })
    };
    let mesh = |level: u8| {
        keys(level)
            .filter_map(|key| generate_lod_mesh(regions.clone(), key, None))
            .map(|meshes| (meshes.opaque.count_vertices(), meshes.inner_vertices()))
            .fold((0, 0), |(a, b), (c, d)| (a + c, b + d))
    };
    for level in LEVELS {
        mesh(level);
    }
    let (mut total, mut vertices, mut inner) = (Duration::ZERO, 0, 0);
    for level in LEVELS {
        let mut best = Duration::MAX;
        let mut counts = (0, 0);
        for _ in 0..3 {
            let start = Instant::now();
            counts = mesh(level);
            best = best.min(start.elapsed());
        }
        total += best;
        (vertices, inner) = (vertices + counts.0, inner + counts.1);
        println!(
            "{:>16}: {best:>8.1?}, {} vertices, {} between leaves",
            format!("lod level {level}"),
            counts.0,
            counts.1
        );
    }
    println!(
        "{:>16}: {total:>8.1?}, {vertices} vertices, {inner} between leaves",
        "lod total",
    );
}
