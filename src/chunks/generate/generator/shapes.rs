//! Meshing shapes (`RenderStyle::is_shape`): each box of a stair, slab, fence and so on as the
//! faces of a cube shrunk to it, culled where a neighbour hides them, with each face showing
//! the part of its tile it covers, so a slab shows the bottom half of a block. Ladders and
//! vines are a quad against their wall, and lanterns map their boxes to their tile's parts.

use bevy::math::{IVec3, Vec3};

use super::buffers::{GrassTint, MeshBuffers, Vertex, CUBE};
use super::shading::{FACE_DIRECTIONS, FACE_NORMAL_OFFSET};
use super::source::BlockSource;
use crate::block::shape::{boxes, fills_face, joins, Use};
use crate::block::{Block, Direction, Face, RenderStyle};
use crate::chunks::generate::light::{Light, MAX_LIGHT};

// How far off its wall a ladder or vines are drawn, as shares of a block.
const OFF_WALL: f32 = 1.0 / 16.0;

/// Draws the shape `block` at `coord` into `buffers`.
pub(super) fn push_shape(
    buffers: &mut MeshBuffers,
    source: &impl BlockSource,
    coord: IVec3,
    block: Block,
    grass_tint: &GrassTint,
    light: &Light,
) {
    let style = block.kind.style();
    let colour = |tile: crate::block::Tile| {
        let tint = if tile.tinted() {
            grass_tint.at(coord.as_vec3())
        } else {
            Vec3::ONE
        };
        tint.extend(tile.atlas_cell()).to_array()
    };
    let levels = |cell: IVec3| light.levels(cell).map(|l| l as f32 / MAX_LIGHT as f32);
    match style {
        RenderStyle::Ladder | RenderStyle::Vine => {
            let facing = block.state.facing();
            let face = face_index(facing);
            let axis = facing.axis();
            let tile = block.kind.tile(Face::Side);
            let positions = CUBE[face].map(|v| {
                let mut p = Vec3::from(v.position);
                p[axis] = -p[axis] * (1.0 - 2.0 * OFF_WALL);
                (p + coord.as_vec3()).to_array()
            });
            buffers.push_double_sided_quad(
                positions,
                CUBE[face][0].normal,
                CUBE[face].map(|v| v.uv),
                [colour(tile); 4],
                [levels(coord); 4],
            );
        }
        RenderStyle::Lantern => push_lantern(buffers, coord, block, colour, levels(coord)),
        _ => {
            let get = |at: IVec3| Block {
                kind: source.block(at),
                state: source.state(at),
            };
            let joined = |side: Direction| joins(block.kind, source.block(coord + side.offset()));
            // a door's two blocks each draw their own half of its picture
            let door_tile = (style == RenderStyle::Door).then(|| {
                block.kind.tile(if block.state.top() {
                    Face::Top
                } else {
                    Face::Side
                })
            });
            let parts = boxes(block, Use::Drawing, joined);
            for (k, part) in parts.iter().enumerate() {
                for (i, template) in CUBE.iter().enumerate() {
                    let direction = FACE_DIRECTIONS[i];
                    let normal = FACE_NORMAL_OFFSET[i];
                    let axis = direction.axis();
                    let outward = normal[axis] > 0;
                    let edge = if outward {
                        part.max[axis] == 1.0
                    } else {
                        part.min[axis] == 0.0
                    };
                    let neighbour = coord + normal;
                    if edge && fills_face(get(neighbour), direction.opposite()) {
                        continue;
                    }
                    // a stair's step stands on its slab, so the face between them is hidden
                    let on_slab = if block.state.top() {
                        Direction::Up
                    } else {
                        Direction::Down
                    };
                    if style == RenderStyle::Stairs && k == 1 && direction == on_slab {
                        continue;
                    }
                    let tile = door_tile.unwrap_or(block.kind.face_tile(block.state, direction).0);
                    // lit by the cell it faces, or its own where that's solid
                    let lit_from = if source.block(neighbour).blocks_light() {
                        coord
                    } else {
                        neighbour
                    };
                    let corner = |v: &Vertex| {
                        part.min + (Vec3::from(v.position) + 0.5) * (part.max - part.min)
                    };
                    let positions =
                        template.map(|v| (corner(&v) - 0.5 + coord.as_vec3()).to_array());
                    let uvs = template.map(|v| cell_uv(template, corner(&v)));
                    buffers.push_tiled_quad(
                        positions,
                        template[0].normal,
                        uvs,
                        [colour(tile); 4],
                        [levels(lit_from); 4],
                    );
                }
            }
        }
    }
}

// Which of `CUBE`'s faces faces `direction`.
fn face_index(direction: Direction) -> usize {
    FACE_DIRECTIONS
        .iter()
        .position(|&d| d == direction)
        .expect("every way has a face")
}

// The uv of a point of a cube's face `template` at `at` within the cell (0 to 1 each way),
// showing as much of the tile as the face covers there: each of u and v follows the axis it
// follows across the cube's face, the same way round.
fn cell_uv(template: &[Vertex; 4], at: Vec3) -> [f32; 2] {
    std::array::from_fn(|k| {
        let follows = |axis: usize, rising: bool| {
            template
                .iter()
                .all(|v| (v.position[axis] > 0.0) == ((v.uv[k] > 0.5) == rising))
        };
        let axis = (0..3)
            .find(|&axis| follows(axis, true) || follows(axis, false))
            .expect("a face's uv follows its axes");
        if follows(axis, true) {
            at[axis]
        } else {
            1.0 - at[axis]
        }
    })
}

// A lantern's boxes, from its tile's parts, in sixteenths: (x0, y0, x1, y1) of its sides and of
// its top and bottom. Its body's glass glows unshaded, see `world.wesl`.
const LANTERN_BODY: ([f32; 4], [f32; 4]) = ([0.0, 9.0, 6.0, 16.0], [0.0, 2.0, 6.0, 8.0]);
const LANTERN_CAP: ([f32; 4], [f32; 4]) = ([6.0, 14.0, 10.0, 16.0], [6.0, 10.0, 10.0, 14.0]);
const LANTERN_CHAIN: [f32; 4] = [11.0, 10.0, 14.0, 16.0];

fn push_lantern(
    buffers: &mut MeshBuffers,
    coord: IVec3,
    block: Block,
    colour: impl Fn(crate::block::Tile) -> [f32; 4],
    levels: [f32; 2],
) {
    let tile = block.kind.tile(Face::Side);
    let parts = boxes(block, Use::Drawing, |_| false);
    let rect_uv = |rect: [f32; 4], uv: [f32; 2]| {
        [
            (rect[0] + uv[0] * (rect[2] - rect[0])) / 16.0,
            (rect[1] + uv[1] * (rect[3] - rect[1])) / 16.0,
        ]
    };
    for (part, (side, ends)) in parts.iter().zip([LANTERN_BODY, LANTERN_CAP]) {
        for face in &CUBE {
            let rect = if face[0].normal[1] == 0.0 { side } else { ends };
            let positions = face.map(|v| {
                let at = part.min + (Vec3::from(v.position) + 0.5) * (part.max - part.min);
                (at - 0.5 + coord.as_vec3()).to_array()
            });
            buffers.push_tiled_quad(
                positions,
                face[0].normal,
                face.map(|v| rect_uv(rect, v.uv)),
                [colour(tile); 4],
                [levels; 4],
            );
        }
    }
    // hanging, from a chain up to the block above, two quads crossing through it
    if block.state.top() {
        let top = parts[1].max.y;
        let width = (LANTERN_CHAIN[2] - LANTERN_CHAIN[0]) / 32.0;
        for axis in [0, 2] {
            let face = &CUBE[if axis == 0 { 0 } else { 1 }];
            let positions = face.map(|v| {
                let mut at = Vec3::from(v.position);
                at[axis] *= 2.0 * width;
                at[2 - axis] = 0.0;
                at.y = if at.y > 0.0 { 0.5 } else { top - 0.5 };
                (at + coord.as_vec3()).to_array()
            });
            buffers.push_double_sided_quad(
                positions,
                face[0].normal,
                face.map(|v| rect_uv(LANTERN_CHAIN, v.uv)),
                [colour(tile); 4],
                [levels; 4],
            );
        }
    }
}
