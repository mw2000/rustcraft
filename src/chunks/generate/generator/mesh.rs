use super::buffers::{
    tiled_uvs, ChunkMeshes, GrassTint, MeshBuffers, MeshStyle, BOX_TEXTURE_MAX_CELL, CROSS, CUBE,
    TEXTURED, TEXTURED_SOLID, TORCH,
};
use super::greedy::mesh_opaque_greedy;
use super::shading::{
    vertex_shore_distance, vertex_shore_factor, vertex_water_depth, FACE_NORMAL_OFFSET, WATER_TOP,
};
use super::shapes::push_shape;
use super::source::{grid_index, BlockSource, BoxFaces, GridView};
use crate::block::Face;
use crate::block::RenderStyle;
use crate::block::Tile;
use crate::block::{Block, BlockType};
use crate::chunks::chunk::CHUNK_SIZE;
use crate::chunks::generate::light::Light;
use crate::chunks::generate::light::MAX_LIGHT;
use crate::chunks::generate::trees::hash;
use bevy::math::I64Vec3;
use bevy::math::IVec3;
use bevy::math::Vec3;
use std::collections::HashMap;
use std::ops::Deref;

// `cell_size` is how many blocks wide each cell of `source` is.
pub(super) fn mesh_blocks(
    source: &impl BlockSource,
    cell_size: i64,
    style: MeshStyle,
    grass_tint: &GrassTint,
    light: &impl Deref<Target = Light>,
) -> ChunkMeshes {
    let MeshStyle { textured, .. } = style;
    let mut opaque = MeshBuffers {
        untextured: !textured,
        ..Default::default()
    };
    let mut water = MeshBuffers::default();
    let mut inner_leaves = MeshBuffers {
        untextured: !textured,
        ..Default::default()
    };

    // water's surface sits below a full block's height, see `WATER_TOP`
    let water_top = 0.5 - (0.5 - WATER_TOP) / cell_size as f32;
    let water_faces = CUBE.map(|face| {
        face.map(|mut v| {
            if v.position[1] == 0.5 {
                v.position[1] = water_top;
            }
            v
        })
    });

    // columns' water depths, shared between the corners that sample them
    let mut depths = HashMap::new();
    let mut depth_of = |coord: IVec3| {
        *depths
            .entry(coord)
            .or_insert_with(|| source.water_depth_blocks(coord))
    };

    let size = CHUNK_SIZE as i32;
    let mut blocks = vec![BlockType::Air; (size * size * size) as usize];
    let non_air = source.non_air();
    for &(coord, block) in &non_air {
        blocks[grid_index(size, coord)] = block;
    }

    for &(block_coord, block) in &non_air {
        if block != BlockType::Water {
            continue;
        }

        for (i, offset) in FACE_NORMAL_OFFSET.into_iter().enumerate() {
            let neighbour = source.block(block_coord + offset);
            if !(matches!(neighbour, BlockType::Air | BlockType::Leaves)
                || neighbour.needs_ground())
            {
                continue;
            }
            // Only the top face carries the shore, depth, top flag and shore distance (R, G, B,
            // A), as foam and waves are surface-only. Waves are gated by the flag alone, so
            // neighbouring columns of different depths move their shared edge alike.
            let colors = water_faces[i].map(|v| {
                if i != 4 {
                    return [0.0, 0.0, 0.0, 1.0];
                }
                [
                    vertex_shore_factor(source, block_coord, i, v.position),
                    vertex_water_depth(&mut depth_of, block_coord, i, v.position)
                        / CHUNK_SIZE as f32,
                    1.0,
                    vertex_shore_distance(source, block_coord, i, v.position),
                ]
            });
            water.push_face(
                &water_faces[i],
                block_coord.as_vec3(),
                Tile::Water.atlas_cell(),
                &colors,
            );
        }
    }

    mesh_opaque_greedy(
        &GridView {
            size,
            blocks: &blocks,
            source,
        },
        &CUBE,
        &mut opaque,
        &mut inner_leaves,
        style,
        if textured { cell_size as f32 } else { 1.0 },
        grass_tint,
        light,
        cell_size > 1,
    );
    // plants and torches are too small to make out from afar, so coarse LODs leave them out
    if cell_size == 1 {
        let plants = opaque.vertices.len();
        for &(coord, block) in non_air.iter().filter(|(_, block)| block.is_plant()) {
            push_cross(&mut opaque, coord, block, style.origin, grass_tint, light);
        }
        opaque.plants = plants..opaque.vertices.len();
        for &(coord, block) in &non_air {
            if block.style() == RenderStyle::Torch {
                push_torch(&mut opaque, coord, block, light);
            }
        }
        // shapes cut their holes out of their shadows too, as glass panes and ladders have them
        let cutouts = opaque.vertices.len();
        for &(coord, kind) in non_air.iter().filter(|(_, block)| block.style().is_shape()) {
            let state = source.state(coord);
            push_shape(
                &mut opaque,
                source,
                coord,
                Block { kind, state },
                grass_tint,
                light,
            );
        }
        opaque.cutouts = cutouts..opaque.vertices.len();
    }
    opaque.tint_grass_sides(grass_tint);
    // sky light there is rarely bright under a canopy, and these boxes are too small to shade smoothly
    const BOX_LIGHT: f32 = 0.8;
    // past them, so wide a box would show through to its back where leaves' holes line up, so
    // they're filled
    let box_holes = cell_size <= BOX_TEXTURE_MAX_CELL;
    opaque.boxes = Some((
        opaque.vertices.len(),
        if box_holes { TEXTURED } else { TEXTURED_SOLID },
    ));
    for lod_box in source.lod_boxes() {
        let (centre, extent) = ((lod_box.min + lod_box.max) / 2.0, lod_box.max - lod_box.min);
        let tint = if lod_box.block.lod_tile().tinted() {
            grass_tint.at(centre) * lod_box.tint
        } else {
            Vec3::ONE
        };
        let faces = match lod_box.faces {
            BoxFaces::All => &CUBE[..],
            BoxFaces::NoBottom => &CUBE[..5],
            BoxFaces::Sides => &CUBE[..4],
        };
        for face in faces {
            let tile = lod_box.block.tile(Face::of_normal_y(face[0].normal[1]));
            let positions = std::array::from_fn(|j| {
                (centre + Vec3::from(face[j].position) * extent).to_array()
            });
            let uvs = tiled_uvs(
                &positions,
                &std::array::from_fn(|j| face[j].uv),
                cell_size as f32,
            );
            opaque.push_tiled_quad(
                positions,
                face[0].normal,
                uvs,
                [tint.extend(tile.atlas_cell()).to_array(); 4],
                [[BOX_LIGHT, 0.0]; 4],
            );
        }
    }

    let water_mesh = if water.vertices.is_empty() {
        None
    } else {
        Some(water.build_mesh())
    };

    ChunkMeshes {
        opaque: opaque.build_mesh(),
        water: water_mesh,
        inner_leaves: (!inner_leaves.vertices.is_empty()).then(|| inner_leaves.build_mesh()),
    }
}

// How far a wild plant may stand off its cell's middle, and how much shorter than a block grasses
// and other wild plants may be: grasses vary far more, so a meadow isn't cut level. Crops stand
// in their rows at their stage's height.
const PLANT_JITTER: f32 = 0.12;
const GRASS_SHORTEN: f32 = 0.45;
const PLANT_SHORTEN: f32 = 0.2;
// How dark a plant is at its roots, to its tips, as the grass round it shades them.
const ROOT_SHADE: f32 = 0.65;

// A plant at `coord`, as two crossed quads lit by the light of its cell, tinted to the climate's
// grass if its tile is greyscale and darker at its roots. A wild plant stands a little off its
// cell's middle and a little shorter, by where it is in the world (`origin` is the mesh's), so
// it's the same every time it's meshed and neighbours don't line up.
fn push_cross(
    buffers: &mut MeshBuffers,
    coord: IVec3,
    block: BlockType,
    origin: I64Vec3,
    grass_tint: &GrassTint,
    light: &Light,
) {
    let tile = block.tile(Face::Side);
    let mut centre = coord.as_vec3();
    let mut top = 0.5;
    if !block.is_crop() {
        let world = origin + coord.as_i64vec3();
        let unit = |salt: u64| (hash(salt, world.x, world.z) >> 40) as f32 / (1u64 << 24) as f32;
        centre += Vec3::new(unit(1) - 0.5, 0.0, unit(2) - 0.5) * 2.0 * PLANT_JITTER;
        let shorten = if tile.tinted() && block != BlockType::Fern {
            GRASS_SHORTEN
        } else {
            PLANT_SHORTEN
        };
        top -= unit(3) * shorten;
    }
    let tint = if tile.tinted() {
        grass_tint.at(coord.as_vec3())
    } else {
        Vec3::ONE
    };
    let (tip, root) = (
        tint.extend(tile.atlas_cell()).to_array(),
        (tint * ROOT_SHADE).extend(tile.atlas_cell()).to_array(),
    );
    let levels = light.levels(coord).map(|l| l as f32 / MAX_LIGHT as f32);
    for quad in &CROSS {
        buffers.push_double_sided_quad(
            quad.map(|v| {
                let mut at = Vec3::from(v.position);
                if at.y > 0.0 {
                    at.y = top;
                }
                (at + centre).to_array()
            }),
            quad[0].normal,
            quad.map(|v| v.uv),
            // v is 0 at the tip
            quad.map(|v| if v.uv[1] == 0.0 { tip } else { root }),
            [levels; 4],
        );
    }
}

// A torch at `coord`, as a stick standing on the floor of its cell, lit by the light of its
// cell. It shows the middle of its tile, the flame at the top, see `TORCH`.
fn push_torch(buffers: &mut MeshBuffers, coord: IVec3, block: BlockType, light: &Light) {
    let tile = block.tile(Face::Side);
    let colour = Vec3::ONE.extend(tile.atlas_cell()).to_array();
    let levels = light.levels(coord).map(|l| l as f32 / MAX_LIGHT as f32);
    for face in &TORCH {
        buffers.push_tiled_quad(
            face.map(|v| (Vec3::from(v.position) + coord.as_vec3()).to_array()),
            face[0].normal,
            face.map(|v| v.uv),
            [colour; 4],
            [levels; 4],
        );
    }
}
