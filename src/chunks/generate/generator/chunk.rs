use super::buffers::{ChunkMeshes, GrassTint, MeshStyle};
use super::mesh::mesh_blocks;
use super::source::{BlockSource, ChunkNeighbourhood};
use super::terrain::{column_block, ground_top, under_surface};
use crate::block::BlockType;
use crate::chunks::chunk::ChunkCoordinate;
use crate::chunks::chunk::ChunkData;
use crate::chunks::chunk::CHUNK_SIZE;
use crate::chunks::generate::light::SkyLight;
use crate::chunks::generate::light::MAX_LIGHT;
use crate::chunks::generate::noise::NoiseGenerator;
use crate::chunks::generate::ores::place_ores;
use crate::chunks::generate::region_store::RegionStore;
use crate::chunks::generate::trees::grows_into;
use crate::chunks::generate::trees::trees_near;
use crate::chunks::generate::trees::TreeTints;
use crate::chunks::generate::trees::TREE_HEIGHT;
use crate::chunks::generate::WATER_LEVEL;
use bevy::math::I64Vec2;
use bevy::math::I64Vec3;
use bevy::math::IVec3;
use std::cell::LazyCell;
use std::sync::Arc;

pub fn generate_chunk(regions: Arc<RegionStore>, chunk_pos: ChunkCoordinate) -> ChunkData {
    let mut heights = regions.sampler();
    let (biomes, caves) = (regions.noise().biomes(), regions.noise().caves());
    // the same for every chunk stacked over these columns, so worked out once for them all
    let columns = regions.chunk_columns(&mut heights, chunk_pos.0.x, chunk_pos.0.z);
    let (lowest, highest) = (columns.lowest, columns.highest);
    let size = CHUNK_SIZE as i64;
    let origin = chunk_pos.0 * size;
    let world_y = origin.y;
    // cells in `ChunkData::from_cells` order, which is the order they're worked out in here
    let mut cells = Vec::with_capacity((size * size * size) as usize);

    for x in 0..size {
        for z in 0..size {
            let column_height = columns.height(x, z);
            // above the ground and the sea, the column is air here whatever its climate
            if world_y >= ground_top(column_height).max(WATER_LEVEL) {
                cells.extend(std::iter::repeat_n(BlockType::Air, size as usize));
                continue;
            }
            let (gradient, biome, temperature) = (
                columns.gradient(x, z),
                columns.biome(x, z),
                columns.temperature(x, z),
            );
            cells.extend((world_y..world_y + size).map(|y| {
                under_surface(
                    column_block(biomes, biome, y, column_height, gradient, temperature),
                    y,
                    column_height,
                    1,
                )
            }));
        }
    }
    if let Some(carved) = caves.chunk(origin, |x, z| columns.cave_top(x, z), columns.cave_highest) {
        carved.carve(&mut cells);
    }
    place_ores(
        regions.noise().seed(),
        origin,
        ground_top(highest),
        &mut cells,
    );
    let mut chunk_data = ChunkData::from_cells(cells);

    // the sky's light is kept out of the chunks below wherever it can't reach down past this one
    for x in 0..size {
        for z in 0..size {
            if world_y + size < columns.sky_reach(x, z) {
                chunk_data.roofed.insert(x as u16, z as u16);
            }
        }
    }

    if trees_may_reach(origin.y, origin.y + size, lowest, highest) {
        let min = I64Vec2::new(origin.x, origin.z);
        let trees = trees_near(regions.noise(), &mut heights, min, min + size - 1);
        for tree in &trees {
            tree.plant(
                origin,
                1,
                (IVec3::ZERO, IVec3::splat(size as i32 - 1)),
                |coord, block| {
                    let coord = coord.as_u16vec3();
                    if grows_into(chunk_data.get_block_at(coord), block) {
                        chunk_data.set_block_at(coord, block);
                    }
                },
            );
        }
        chunk_data.trees = TreeTints::new(&trees, origin, 1);
    }

    chunk_data
}

// Whether trees on ground between `lowest` and `highest` could reach into blocks `bottom..top`.
// Trees stand on gentle ground, so neighbouring ground just outside is close in height.
pub(super) fn trees_may_reach(bottom: i64, top: i64, lowest: i64, highest: i64) -> bool {
    const NEARBY_GROUND: i64 = 4;
    top > lowest - NEARBY_GROUND && bottom < highest + NEARBY_GROUND + TREE_HEIGHT
}

pub fn generate_chunk_mesh(
    chunk: Arc<ChunkData>,
    adjacent_chunks: Vec<Option<Arc<ChunkData>>>,
    coord: ChunkCoordinate,
    noise: &NoiseGenerator,
    solid_leaves: bool,
) -> ChunkMeshes {
    let source = ChunkNeighbourhood::new(&chunk, &adjacent_chunks);
    let tint = GrassTint::new(noise, (coord.0 * CHUNK_SIZE as i64).as_vec3(), 1.0);
    // worked out only once there's a face to light: buried and empty chunks have none
    let light = LazyCell::new(|| neighbourhood_light(&source, coord));
    // leaves drawn solid hide the leaves behind them, see `DebugSettings::solid_leaves`
    mesh_blocks(
        &source,
        1,
        MeshStyle {
            ambient_occlusion: true,
            textured: true,
            solid_leaves,
        },
        &tint,
        &light,
    )
}

// Light falling into the top of a box whose highest block is at `top`, in boxes of `cell`
// blocks: full sky, less any sea above it.
pub(super) fn sky_above(top: i64, cell: i64) -> u8 {
    let water_above = ((WATER_LEVEL - 1 - top) / cell).clamp(0, MAX_LIGHT as i64) as u8;
    MAX_LIGHT.saturating_sub(water_above.saturating_mul(BlockType::Water.light_opacity()))
}

// How far to each side of a chunk its light takes blocks into account: light that could
// only arrive from further out would already have faded to deep shade.
pub(super) const LIGHT_MARGIN: i32 = 8;

// The box of blocks a chunk's light is worked out over, as its minimum corner from the chunk's
// and its size: the chunk and a block around it, `LIGHT_MARGIN` to each side, and up to the top
// of the chunk above.
fn light_box() -> (IVec3, IVec3) {
    let size = CHUNK_SIZE as i32;
    let min = IVec3::new(-LIGHT_MARGIN, -1, -LIGHT_MARGIN);
    (
        min,
        IVec3::new(size + LIGHT_MARGIN, 2 * size, size + LIGHT_MARGIN) - min,
    )
}

/// The chunks whose meshes change when `block` does: those whose light takes it into account,
/// which takes in those whose faces, ambient occlusion and shores reach it across a border.
pub fn chunks_meshed_with(block: I64Vec3) -> impl Iterator<Item = ChunkCoordinate> {
    let (min, span) = light_box();
    let size = CHUNK_SIZE as i64;
    // each chunk whose box, starting at `chunk * size + min`, holds the block
    let from = block - min.as_i64vec3();
    let first = (from - span.as_i64vec3() + size).div_euclid(I64Vec3::splat(size));
    let last = from.div_euclid(I64Vec3::splat(size));
    (first.x..=last.x).flat_map(move |x| {
        (first.y..=last.y).flat_map(move |y| {
            (first.z..=last.z).map(move |z| ChunkCoordinate(I64Vec3::new(x, y, z)))
        })
    })
}

// Sky light for the chunk and a block around it, from the chunk and its 26 neighbours, as
// high as the top of the chunk above, with open sky above that unless the ground roofs it over.
pub(super) fn neighbourhood_light(source: &ChunkNeighbourhood, coord: ChunkCoordinate) -> SkyLight {
    let size = CHUNK_SIZE as i32;
    let (min, span) = light_box();
    let mut opacity = Vec::with_capacity((span.x * span.y * span.z) as usize);
    // in `SkyLight::index` order
    for x in 0..span.x {
        for z in 0..span.z {
            for y in 0..span.y {
                opacity.push(source.block(min + IVec3::new(x, y, z)).light_opacity());
            }
        }
    }
    let top = coord.0.y * size as i64 + (min.y + span.y) as i64 - 1;
    let sky = sky_above(top, 1);
    let sky = |x, z| {
        if source.roofed_above(min.x + x, min.z + z) {
            0
        } else {
            sky
        }
    };
    SkyLight::with_sky(min, span, sky, opacity)
}
