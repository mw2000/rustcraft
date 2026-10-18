use super::buffers::{ChunkMeshes, GrassTint, MeshStyle};
use super::mesh::mesh_blocks;
use super::source::{BlockSource, ChunkNeighbourhood};
use super::terrain::{column_block, ground_top, under_surface};
use super::FACE_NORMAL_OFFSET;
use crate::block::{BlockState, BlockType};
use crate::chunks::chunk::ChunkCoordinate;
use crate::chunks::chunk::ChunkData;
use crate::chunks::chunk::CHUNK_SIZE;
use crate::chunks::generate::light::{
    BlockLight, ChunkLight, Light, SkyLight, BLOCK_LIGHT_REACH, MAX_LIGHT,
};
use crate::chunks::generate::noise::NoiseGenerator;
use crate::chunks::generate::ores::place_ores;
use crate::chunks::generate::plants::plant_on;
use crate::chunks::generate::region_store::RegionStore;
use crate::chunks::generate::structures::{clear_trees, STRUCTURE_DEPTH, STRUCTURE_HEIGHT};
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
    // the trees reaching into the chunk, which are planted once its ground is, and whose crowns
    // shade ferns in. Any ground in the chunk is in their reach.
    let min = I64Vec2::new(origin.x, origin.z);
    let giants = regions
        .noise()
        .landmarks()
        .giant_trees_near(min, min + size - 1);
    let mut trees = Vec::new();
    let trees_reach = trees_may_reach(origin.y, origin.y + size, lowest, highest);
    if trees_reach {
        trees = trees_near(regions.noise(), &mut heights, min, min + size - 1);
        clear_trees(&mut trees, &columns.structures, &giants);
    }

    for x in 0..size {
        for z in 0..size {
            let column_height = columns.height(x, z);
            let (gradient, biome, temperature) = (
                columns.gradient(x, z),
                columns.biome(x, z),
                columns.temperature(x, z),
            );
            let start = cells.len();
            // above the ground and the sea, the column is air here whatever its climate
            if world_y >= ground_top(column_height).max(WATER_LEVEL) {
                cells.extend(std::iter::repeat_n(BlockType::Air, size as usize));
            } else {
                cells.extend((world_y..world_y + size).map(|y| {
                    under_surface(
                        column_block(biomes, biome, y, column_height, gradient, temperature),
                        y,
                        column_height,
                        1,
                    )
                }));
            }
            // a plant stands over the ground, unless a cave mouth has taken the ground away
            let over = column_height - world_y;
            if (0..size).contains(&over) && columns.sky_reach(x, z) == ground_top(column_height) {
                let ground = column_block(
                    biomes,
                    biome,
                    column_height - 1,
                    column_height,
                    gradient,
                    temperature,
                );
                let (world_x, world_z) = (origin.x + x, origin.z + z);
                let shaded = || {
                    let (x, z) = (world_x as f64 + 0.5, world_z as f64 + 0.5);
                    trees.iter().any(|tree| tree.shades(x, z))
                };
                if let Some(plant) =
                    plant_on(regions.noise(), world_x, world_z, ground, gradient, shaded)
                {
                    cells[start + over as usize] = plant;
                }
            }
        }
    }
    if let Some(carved) = caves.chunk(origin, |x, z| columns.cave_top(x, z), columns.cave_highest) {
        carved.carve(&mut cells);
    }
    place_ores(
        regions.noise().seed(),
        origin,
        ground_top(highest),
        |x, z| biomes.name(columns.biome(x, z)),
        &mut cells,
    );
    // the few blocks of structures with states, such as stairs, which are put in once the
    // chunk's runs are made, so the cells of everything else are its blocks alone
    let mut states = Vec::new();
    for placed in &columns.structures {
        placed.build(
            origin,
            1,
            (IVec3::ZERO, IVec3::splat(size as i32 - 1)),
            |x, z| columns.height(x - origin.x, z - origin.z),
            |coord, block| {
                let at = coord.as_i64vec3();
                cells[((at.x * size + at.z) * size + at.y) as usize] = block.kind;
                if block.state != BlockState::default() {
                    states.push((coord.as_u16vec3(), block));
                }
            },
        );
    }
    let mut chunk_data = ChunkData::from_cells(cells);
    for (coord, block) in states {
        // unless another structure's built over it since
        if chunk_data.get_block_at(coord) == block.kind {
            chunk_data.set_block_at(coord, block);
        }
    }

    // the sky's light is kept out of the chunks below wherever it can't reach down past this one
    for x in 0..size {
        for z in 0..size {
            if world_y + size < columns.sky_reach(x, z) {
                chunk_data.roofed.insert(x as u16, z as u16);
            }
        }
    }

    if trees_reach {
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
    for giant in &giants {
        giant.plant(
            origin,
            1,
            (IVec3::ZERO, IVec3::splat(size as i32 - 1)),
            |coord, block| chunk_data.set_block_at(coord.as_u16vec3(), block),
        );
    }

    chunk_data
}

// Ground just outside an area is near in height to the ground in it, where trees and structures
// stand: they need gentle ground.
const NEARBY_GROUND: i64 = 4;

// Whether trees on ground between `lowest` and `highest` could reach into blocks `bottom..top`.
pub(super) fn trees_may_reach(bottom: i64, top: i64, lowest: i64, highest: i64) -> bool {
    top > lowest - NEARBY_GROUND && bottom < highest + NEARBY_GROUND + TREE_HEIGHT
}

// Whether structures on ground between `lowest` and `highest` could reach into blocks
// `bottom..top`. Wherever trees could reach too, so the structures they keep clear of are known.
pub(super) fn structures_may_reach(bottom: i64, top: i64, lowest: i64, highest: i64) -> bool {
    top > lowest - NEARBY_GROUND - STRUCTURE_DEPTH
        && bottom < highest + NEARBY_GROUND + STRUCTURE_HEIGHT
}

const _: () = assert!(STRUCTURE_HEIGHT >= TREE_HEIGHT);

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
            origin: coord.0 * CHUNK_SIZE as i64,
        },
        &tint,
        &light,
    )
}

/// The light in each block of the chunk at `coord`, from it and its 26 neighbours as meshing
/// has them, worked out just as its meshes are lit, for models drawn in it (§5.5).
pub fn chunk_light(
    chunk: &ChunkData,
    adjacent_chunks: &[Option<Arc<ChunkData>>],
    coord: ChunkCoordinate,
) -> ChunkLight {
    let source = ChunkNeighbourhood::new(chunk, adjacent_chunks);
    let light = neighbourhood_light(&source, coord);
    // as `vertex_light` leaves solid cells out
    let open = |cell: IVec3| !source.terrain(cell).is_opaque();
    let size = CHUNK_SIZE as i32;
    let mut levels = Vec::with_capacity((size * size * size) as usize);
    for x in 0..size {
        for z in 0..size {
            for y in 0..size {
                let cell = IVec3::new(x, y, z);
                if open(cell) {
                    levels.push(light.levels(cell));
                    continue;
                }
                let beside = FACE_NORMAL_OFFSET
                    .into_iter()
                    .map(|offset| cell + offset)
                    .filter(|&next| open(next))
                    .map(|next| light.levels(next));
                levels.push(beside.fold([0, 0], |a, b| [a[0].max(b[0]), a[1].max(b[1])]));
            }
        }
    }
    ChunkLight::new(levels.into_boxed_slice())
}

// Light falling into the top of a box whose highest block is at `top`, in boxes of `cell`
// blocks: full sky, less any sea above it.
pub(super) fn sky_above(top: i64, cell: i64) -> u8 {
    let water_above = ((WATER_LEVEL - 1 - top) / cell).clamp(0, MAX_LIGHT as i64) as u8;
    MAX_LIGHT.saturating_sub(water_above.saturating_mul(BlockType::Water.light_opacity()))
}

// How far to each side of a chunk its sky light takes blocks into account: light that could
// only arrive from further out would already have faded to deep shade.
pub(super) const LIGHT_MARGIN: i32 = 8;

// The box of blocks a chunk's sky light is worked out over, as its minimum corner from the
// chunk's and its size: the chunk and a block around it, `LIGHT_MARGIN` to each side, and up to
// the top of the chunk above.
fn light_box() -> (IVec3, IVec3) {
    let size = CHUNK_SIZE as i32;
    let min = IVec3::new(-LIGHT_MARGIN, -1, -LIGHT_MARGIN);
    (
        min,
        IVec3::new(size + LIGHT_MARGIN, 2 * size, size + LIGHT_MARGIN) - min,
    )
}

// The box of blocks a chunk's block light is worked out for, as `light_box`: the chunk and a
// block around it, which meshing reads the light of. The blocks that can light it are as far
// again as block light reaches, inside the chunk's 26 neighbours.
fn block_light_box() -> (IVec3, IVec3) {
    (IVec3::splat(-1), IVec3::splat(CHUNK_SIZE as i32 + 2))
}

/// The chunks whose meshes change when `block` does: those whose sky light takes it in, which
/// takes in those whose faces, ambient occlusion and shores reach it across a border, and if
/// the change `relights` block light (see `World::take_relit`), those that light could reach.
pub fn chunks_meshed_with(block: I64Vec3, relights: bool) -> impl Iterator<Item = ChunkCoordinate> {
    let (mut min, span) = light_box();
    let mut max = min + span;
    if relights {
        // the blocks whose block light meshing reads, and as far round them as it comes from
        let (block_min, block_span) = block_light_box();
        let reach = IVec3::splat(BLOCK_LIGHT_REACH);
        min = min.min(block_min - reach);
        max = max.max(block_min + block_span + reach);
    }
    let size = CHUNK_SIZE as i64;
    // each chunk whose box, starting at `chunk * size + min`, holds the block
    let from = block - min.as_i64vec3();
    let first = (from - (max - min).as_i64vec3() + size).div_euclid(I64Vec3::splat(size));
    let last = from.div_euclid(I64Vec3::splat(size));
    (first.x..=last.x).flat_map(move |x| {
        (first.y..=last.y).flat_map(move |y| {
            (first.z..=last.z).map(move |z| ChunkCoordinate(I64Vec3::new(x, y, z)))
        })
    })
}

// Light for the chunk and a block around it, from the chunk and its 26 neighbours: sky light
// as high as the top of the chunk above, with open sky above that unless the ground roofs it
// over, and the light of any blocks near enough to give it some.
pub(super) fn neighbourhood_light(source: &ChunkNeighbourhood, coord: ChunkCoordinate) -> Light {
    let size = CHUNK_SIZE as i32;
    let (min, span) = light_box();
    let mut opacity = Vec::with_capacity((span.x * span.y * span.z) as usize);
    // the stairs and slabs among them, which keep light out without filling their cells
    let mut shapes = Vec::new();
    // in `SkyLight::index` order
    for x in 0..span.x {
        for z in 0..span.z {
            for y in 0..span.y {
                let coord = min + IVec3::new(x, y, z);
                let block = source.block(coord);
                opacity.push(block.light_opacity());
                if source.light_shapes && block.is_shape() && block.blocks_light() {
                    shapes.push(coord);
                }
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
    let mut sky = SkyLight::with_sky(min, span, sky, opacity);
    let mut block = if source.emitters.is_empty() {
        BlockLight::default()
    } else {
        let (min, span) = block_light_box();
        BlockLight::new(min, span, &source.emitters, |c| {
            source.block(c).light_opacity()
        })
    };
    if !shapes.is_empty() {
        sky.light_shapes(&shapes);
        if !source.emitters.is_empty() {
            block.light_shapes(&shapes);
        }
    }
    Light { sky, block }
}
