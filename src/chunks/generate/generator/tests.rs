use std::sync::Arc;

use bevy::math::{I64Vec3, IVec3, U16Vec3};
use bevy::math::{Vec3, Vec3Swizzles};
use bevy::mesh::{Indices, Mesh, VertexAttributeValues};
use bevy::platform::collections::HashMap;

use crate::block::{BlockType, Tile};

use super::buffers::{ChunkMeshes, GrassTint, MeshStyle};
use super::chunk::{chunks_meshed_with, generate_chunk, generate_chunk_mesh};
use super::lod::LodBlocks;
use super::shading::{vertex_ao, vertex_water_depth, AO_STRENGTH};
use super::source::{water_depth, BlockSource, ChunkNeighbourhood};
use super::terrain::{freeze_cell, terrain_block, under_surface, FREEZE_TEMPERATURE};
use crate::chunks::chunk::{neighbor_26_index, ChunkCoordinate, ChunkData, CELLS, CHUNK_SIZE};
use crate::chunks::generate::light::{Light, SkyLight, MAX_LIGHT};
use crate::chunks::generate::noise::NoiseGenerator;
use crate::chunks::generate::region_store::RegionStore;
use crate::chunks::generate::WATER_LEVEL;
use crate::chunks::lod::LodKey;

// solid leaves, as every LOD draws them
fn style(ambient_occlusion: bool, textured: bool) -> MeshStyle {
    MeshStyle {
        ambient_occlusion,
        textured,
        solid_leaves: true,
    }
}

fn no_neighbors() -> [Option<Arc<ChunkData>>; 26] {
    std::array::from_fn(|_| None)
}

#[test]
fn test_neighbourhood_within_chunk() {
    let mut chunk = ChunkData::default();
    chunk.set_block_at(U16Vec3::new(4, 4, 4), BlockType::Stone);

    let block = ChunkNeighbourhood::new(&chunk, &no_neighbors()).block(IVec3::new(4, 4, 4));
    assert_eq!(BlockType::Stone, block);
}

#[test]
fn test_neighbourhood_crosses_single_axis_into_adjacent_chunk() {
    let chunk = ChunkData::default();
    let mut adjacent = ChunkData::default();
    adjacent.set_block_at(U16Vec3::new(0, 4, 4), BlockType::Stone);

    let mut neighbors = no_neighbors();
    neighbors[neighbor_26_index(1, 0, 0)] = Some(Arc::new(adjacent));
    let block =
        ChunkNeighbourhood::new(&chunk, &neighbors).block(IVec3::new(CHUNK_SIZE as i32, 4, 4));
    assert_eq!(BlockType::Stone, block);
}

#[test]
fn test_neighbourhood_crosses_two_axes_into_diagonal_chunk() {
    let chunk = ChunkData::default();
    let mut diagonal = ChunkData::default();
    diagonal.set_block_at(U16Vec3::new(0, 0, 4), BlockType::Stone);

    let mut neighbors = no_neighbors();
    neighbors[neighbor_26_index(1, 1, 0)] = Some(Arc::new(diagonal));
    let block = ChunkNeighbourhood::new(&chunk, &neighbors).block(IVec3::new(
        CHUNK_SIZE as i32,
        CHUNK_SIZE as i32,
        4,
    ));
    assert_eq!(BlockType::Stone, block);
}

#[test]
fn test_neighbourhood_missing_diagonal_chunk_is_air() {
    let chunk = ChunkData::default();
    let block = ChunkNeighbourhood::new(&chunk, &no_neighbors()).block(IVec3::new(
        CHUNK_SIZE as i32,
        CHUNK_SIZE as i32,
        4,
    ));
    assert_eq!(BlockType::Air, block);
}

#[test]
fn test_vertex_ao_no_occluders_is_unlit() {
    let chunk = ChunkData::default();
    let neighbors = no_neighbors();
    let source = ChunkNeighbourhood::new(&chunk, &neighbors);
    let ao = vertex_ao(&source, IVec3::new(4, 4, 4), 4, [0.5, 0.5, -0.5]);
    assert_eq!(1.0, ao);
}

#[test]
fn test_vertex_ao_fully_occluded_corner() {
    let mut chunk = ChunkData::default();
    // Top face (index 4) at (4,4,4): occluders are the y+1 layer's tangent/diagonal cells.
    chunk.set_block_at(U16Vec3::new(5, 5, 4), BlockType::Stone);
    chunk.set_block_at(U16Vec3::new(4, 5, 5), BlockType::Stone);
    chunk.set_block_at(U16Vec3::new(5, 5, 5), BlockType::Stone);

    let neighbors = no_neighbors();
    let source = ChunkNeighbourhood::new(&chunk, &neighbors);
    let ao = vertex_ao(&source, IVec3::new(4, 4, 4), 4, [0.5, 0.5, 0.5]);
    assert_eq!(1.0 - AO_STRENGTH, ao);
}

#[test]
fn test_neighbouring_water_columns_agree_on_shared_corner_depth() {
    let mut chunk = ChunkData::default();
    // two columns with their surface at y = 5: one 5 deep, its +x neighbour 2 deep
    chunk.set_block_at(U16Vec3::new(4, 0, 4), BlockType::Stone);
    for y in 1..=5 {
        chunk.set_block_at(U16Vec3::new(4, y, 4), BlockType::Water);
    }
    for y in 0..=3 {
        chunk.set_block_at(U16Vec3::new(5, y, 4), BlockType::Stone);
    }
    for y in 4..=5 {
        chunk.set_block_at(U16Vec3::new(5, y, 4), BlockType::Water);
    }
    let neighbors = no_neighbors();
    let source = ChunkNeighbourhood::new(&chunk, &neighbors);
    let mut depth_of = |coord: IVec3| water_depth(&source, coord) as f32;

    // the corner between them, on the +z side: seen from each column's top face
    let deep = vertex_water_depth(&mut depth_of, IVec3::new(4, 5, 4), 4, [0.5, 0.5, 0.5]);
    let shallow = vertex_water_depth(&mut depth_of, IVec3::new(5, 5, 4), 4, [-0.5, 0.5, 0.5]);
    assert_eq!(deep, shallow);
    // averaged with the two dry columns on the +z side
    assert_eq!((5.0 + 2.0) / 4.0, deep);
}

#[test]
fn test_desert_ground_is_sand_and_other_lowland_grass() {
    use bevy::math::I64Vec2;

    let regions = Arc::new(RegionStore::new(Arc::new(NoiseGenerator::new(3))));
    let lowland = |desert: bool| {
        (-60..60)
            .flat_map(|z| (-60..60).map(move |x| I64Vec2::new(x, z) * 128))
            .find(|p| {
                let sample = regions.noise().sample(p.x as f64, p.y as f64);
                let above_sea = sample.height - WATER_LEVEL as f64;
                (regions.noise().biomes().name(sample.biome) == Some("desert")) == desert
                    && (20.0..60.0).contains(&above_sea)
                    && regions.sampler().gradient(p.x, p.y).0.abs() < 0.5
                    && regions.sampler().gradient(p.x, p.y).1.abs() < 0.5
            })
            .expect("no lowland of that kind near the centre")
    };
    let surface = |p: I64Vec2| {
        let top = regions.get_height(p.x, p.y).round() as i64 - 1;
        let coord = ChunkCoordinate(I64Vec3::new(
            p.x.div_euclid(16),
            top.div_euclid(16),
            p.y.div_euclid(16),
        ));
        let local = U16Vec3::new(
            p.x.rem_euclid(16) as u16,
            top.rem_euclid(16) as u16,
            p.y.rem_euclid(16) as u16,
        );
        generate_chunk(regions.clone(), coord).get_block_at(local)
    };
    assert_eq!(BlockType::Sand, surface(lowland(true)));
    assert_eq!(BlockType::Grass, surface(lowland(false)));
}

#[test]
fn test_trees_across_chunk_borders_are_whole() {
    use crate::chunks::generate::trees::trees_near;
    use bevy::math::I64Vec2;

    let regions = Arc::new(RegionStore::new(Arc::new(NoiseGenerator::new(5))));
    let mut heights = regions.sampler();
    let trees = trees_near(
        regions.noise(),
        &mut heights,
        I64Vec2::splat(-800),
        I64Vec2::splat(800),
    );
    let chunk_of = |p: I64Vec3| p.div_euclid(I64Vec3::splat(16));
    let tree = trees
        .iter()
        .find(|t| {
            let (low, high) = t.bounds();
            chunk_of(low).x != chunk_of(high).x && chunk_of(low).z != chunk_of(high).z
        })
        .expect("no tree straddling a chunk corner");

    let mut chunks = HashMap::new();
    let (low, high) = tree.bounds();
    let mut checked = 0;
    tree.plant(
        low,
        1,
        (IVec3::ZERO, (high - low).as_ivec3()),
        |cell, block| {
            let p = low + cell.as_i64vec3();
            let coord = chunk_of(p);
            let chunk = chunks
                .entry(coord)
                .or_insert_with(|| generate_chunk(regions.clone(), ChunkCoordinate(coord)));
            let local = (p - coord * 16).as_u16vec3();
            let found = chunk.get_block_at(local);
            // only another tree's log may stand in for this one's leaves
            assert!(
                found == block || (block == BlockType::Leaves && found == BlockType::Log),
                "{block:?} at {p} is {found:?}"
            );
            checked += 1;
        },
    );
    assert!(
        chunks.len() >= 2 && checked > 20,
        "{} blocks over {} chunks",
        checked,
        chunks.len()
    );
}

#[test]
fn test_cold_sea_freezes_from_the_top_and_thicker_when_colder() {
    let ice_layers = |temperature| {
        (WATER_LEVEL - 10..WATER_LEVEL)
            .filter(|&y| terrain_block(y, 0, 0.0, temperature) == BlockType::Ice)
            .count()
    };
    assert_eq!(0, ice_layers(0.5));
    assert_eq!(0, ice_layers(FREEZE_TEMPERATURE));
    assert_eq!(1, ice_layers(FREEZE_TEMPERATURE - 0.01));
    assert_eq!(3, ice_layers(0.0));
    assert_eq!(BlockType::Ice, terrain_block(WATER_LEVEL - 1, 0, 0.0, 0.0));
    assert_eq!(
        BlockType::Water,
        terrain_block(WATER_LEVEL - 4, 0, 0.0, 0.0)
    );
    assert_eq!(BlockType::Air, terrain_block(WATER_LEVEL, 0, 0.0, 0.0));
}

#[test]
fn test_wide_cells_freeze_when_they_reach_the_ice() {
    let top = WATER_LEVEL;
    assert_eq!(BlockType::Ice, freeze_cell(BlockType::Water, 0.0, top, 32));
    assert_eq!(
        BlockType::Water,
        freeze_cell(BlockType::Water, 0.0, top - 32, 32)
    );
    assert_eq!(
        BlockType::Water,
        freeze_cell(BlockType::Water, 0.5, top, 32)
    );
    assert_eq!(
        BlockType::Stone,
        freeze_cell(BlockType::Stone, 0.0, top, 32)
    );
}

#[test]
fn test_polar_lowlands_are_snow_covered_and_deep_ice_gives_way_to_snow() {
    let ground = WATER_LEVEL + 20;
    assert_eq!(BlockType::Snow, terrain_block(ground - 1, ground, 0.0, 0.1));
    assert_eq!(
        BlockType::Grass,
        terrain_block(ground - 1, ground, 0.0, 0.5)
    );
    assert_eq!(
        BlockType::Snow,
        under_surface(BlockType::Ice, ground - 5, ground, 1)
    );
    assert_eq!(
        BlockType::Ice,
        under_surface(BlockType::Ice, ground - 1, ground, 1)
    );
}

#[test]
fn test_terrain_block_floor_and_water() {
    // below the solid floor even a zero-height column is solid
    assert_eq!(BlockType::Sand, terrain_block(5, 0, 0.0, 0.5));
    assert_eq!(BlockType::Water, terrain_block(20, 0, 0.0, 0.5));
    assert_eq!(BlockType::Air, terrain_block(WATER_LEVEL, 0, 0.0, 0.5));
    assert_eq!(
        BlockType::Grass,
        terrain_block(WATER_LEVEL + 10, WATER_LEVEL + 20, 0.0, 0.5)
    );
}

#[test]
fn test_snow_settles_on_steeper_ground_higher_up() {
    let at = |above_sea: i64, slope: f64| {
        let y = WATER_LEVEL + above_sea;
        terrain_block(y, y + 1, slope, 0.5)
    };
    assert_ne!(BlockType::Snow, at(200, 0.0));
    assert_eq!(BlockType::Snow, at(230, 1.0));
    assert_ne!(BlockType::Snow, at(230, 3.0));
    assert_eq!(BlockType::Snow, at(360, 3.0));
    // cliffs stay bare however high they are
    assert_ne!(BlockType::Snow, at(400, 6.0));
}

#[test]
fn test_snow_line_drops_in_the_cold_and_hot_ground_is_bare() {
    let at = |above_sea: i64, temperature: f32| {
        let y = WATER_LEVEL + above_sea;
        terrain_block(y, y + 1, 0.0, temperature)
    };
    assert_ne!(BlockType::Snow, at(100, 0.5));
    assert_eq!(BlockType::Snow, at(100, 0.35));
    assert_eq!(BlockType::Snow, at(300, 0.5));
    // too hot for snow however high, so the peaks are rock
    assert_eq!(BlockType::Stone, at(300, 0.8));
    assert_eq!(BlockType::Grass, at(100, 0.8));
}

#[test]
fn test_only_the_top_of_frozen_grass_is_snow() {
    let at = |y: i64, temperature: f32| terrain_block(y, WATER_LEVEL + 7, 0.0, temperature);
    let top = WATER_LEVEL + 6;
    assert_eq!(BlockType::Snow, at(top, 0.05));
    assert_eq!(BlockType::Grass, at(top, 0.3));
    assert_eq!(BlockType::Grass, at(top - 1, 0.05));
}

fn test_regions() -> Arc<RegionStore> {
    Arc::new(RegionStore::new(Arc::new(NoiseGenerator::new(3))))
}

#[test]
fn test_lod_level_zero_matches_full_detail_chunk() {
    let regions = test_regions();
    for coord in [
        I64Vec3::new(1, 1, 2),
        I64Vec3::new(-2, 0, 3),
        I64Vec3::new(0, 2, -1),
    ] {
        let chunk = generate_chunk(regions.clone(), ChunkCoordinate(coord));
        let lod = LodBlocks::sample(&regions, LodKey { level: 0, coord }, None);
        let columns = regions.chunk_columns(&mut regions.sampler(), coord.x, coord.z);
        for x in 0..16 {
            for y in 0..16 {
                for z in 0..16 {
                    // LODs leave out caves, which are hidden from afar
                    let block = coord * 16 + I64Vec3::new(x as i64, y as i64, z as i64);
                    let carved = regions
                        .noise()
                        .caves()
                        .carves(block, columns.cave_top(x as i64, z as i64));
                    let expected = lod.raw(IVec3::new(x as i32, y as i32, z as i32));
                    // and plants, which are too small to see from afar
                    let block = chunk.get_block_at(U16Vec3::new(x, y, z));
                    assert_eq!(
                        if carved { BlockType::Air } else { expected },
                        if block.is_plant() {
                            BlockType::Air
                        } else {
                            block
                        },
                        "chunk {coord} block ({x},{y},{z})"
                    );
                }
            }
        }
    }
}

fn flat_lod(surface_y: i32) -> LodBlocks {
    let size = 16;
    let padded = (size + 2) as usize;
    let mut cells = vec![BlockType::Air; padded * padded * padded];
    for x in -1..=size {
        for z in -1..=size {
            for y in -1..surface_y {
                cells[LodBlocks::index(size, IVec3::new(x, y, z))] = BlockType::Stone;
            }
        }
    }
    let tops = vec![surface_y as i64; padded * padded];
    LodBlocks::new(size, cells, tops, 0, 1)
}

// Grass two cells of 4 blocks deep, its ground 6 blocks up the node for x < 8 and 7 blocks
// beyond: both inside the second cell, so its top is lowered. Above the sea, under open sky.
fn sloped_lod() -> LodBlocks {
    let base_y = WATER_LEVEL + 64;
    let size = 16;
    let padded = (size + 2) as usize;
    let mut cells = vec![BlockType::Air; padded * padded * padded];
    let mut tops = vec![0; padded * padded];
    for x in -1..=size {
        for z in -1..=size {
            tops[LodBlocks::column(size, x, z)] = base_y + if x < 8 { 6 } else { 7 };
            for y in -1..=1 {
                cells[LodBlocks::index(size, IVec3::new(x, y, z))] = BlockType::Grass;
            }
        }
    }
    LodBlocks::new(size, cells, tops, base_y, 4)
}

// `lod`'s opaque mesh, lit as `generate_lod_mesh` lights it
fn mesh_lod(lod: &LodBlocks) -> Mesh {
    let top = lod.base_y + (lod.size as i64 + 1) * lod.cell;
    let opacity = lod.cells.iter().map(BlockType::light_opacity).collect();
    let light = Box::new(Light::sky(SkyLight::new(
        IVec3::splat(-1),
        IVec3::splat(lod.size + 2),
        super::chunk::sky_above(top, lod.cell),
        opacity,
    )));
    let tint = GrassTint::new(&NoiseGenerator::new(3), Vec3::ZERO, lod.cell as f32);
    super::mesh::mesh_blocks(lod, lod.cell, style(false, false), &tint, &light).opaque
}

// each face's normal and its vertices' positions
fn faces(mesh: &Mesh) -> Vec<([f32; 3], Vec<[f32; 3]>)> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("missing positions");
    };
    let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
    else {
        panic!("missing normals");
    };
    let mut faces: Vec<([f32; 3], Vec<[f32; 3]>)> = Vec::new();
    for (p, n) in positions.iter().zip(normals) {
        match faces
            .iter_mut()
            .find(|(normal, vs)| normal == n && vs.len() < 4)
        {
            Some((_, vs)) => vs.push(*p),
            None => faces.push((*n, vec![*p])),
        }
    }
    faces
}

#[test]
fn test_lod_surface_tops_out_at_the_grounds_height() {
    let lod = sloped_lod();
    // (6 - 0) / 4 - 0.5 and (7 - 0) / 4 - 0.5, in cell units centred on the cells
    assert_eq!(Some(1.0), lod.surface_top(IVec3::new(2, 1, 2)));
    assert_eq!(Some(1.25), lod.surface_top(IVec3::new(10, 1, 2)));
    assert_eq!(None, lod.surface_top(IVec3::new(2, 0, 2)));
}

#[test]
fn test_lod_surface_under_water_fills_its_cell() {
    let LodBlocks {
        size,
        mut cells,
        tops,
        base_y,
        cell,
        ..
    } = sloped_lod();
    cells[LodBlocks::index(16, IVec3::new(2, 2, 2))] = BlockType::Water;
    // nor does one beside water, which would show a gap under the water's side
    cells[LodBlocks::index(16, IVec3::new(6, 1, 2))] = BlockType::Water;
    let lod = LodBlocks::new(size, cells, tops, base_y, cell);
    assert_eq!(None, lod.surface_top(IVec3::new(2, 1, 2)));
    assert_eq!(None, lod.surface_top(IVec3::new(5, 1, 2)));
    assert_eq!(Some(1.0), lod.surface_top(IVec3::new(4, 1, 2)));
}

#[test]
fn test_lod_tops_are_drawn_at_the_grounds_height_with_a_step_between() {
    let faces = faces(&mesh_lod(&sloped_lod()));
    let tops: Vec<f32> = faces
        .iter()
        .filter(|(n, _)| *n == [0.0, 1.0, 0.0])
        .flat_map(|(_, vs)| vs.iter().map(|v| v[1]))
        .collect();
    assert!(tops.contains(&1.0) && tops.contains(&1.25));
    assert!(
        tops.iter().all(|&y| y == 1.0 || y == 1.25),
        "tops at {tops:?}"
    );
    // the higher ground's side, facing the lower, covers just the step between them
    let step: Vec<_> = faces
        .iter()
        .filter(|(n, vs)| *n == [-1.0, 0.0, 0.0] && vs.iter().all(|v| v[0] == 7.5))
        .collect();
    assert!(!step.is_empty());
    // one face along the whole contour, not one for each of its 16 cells
    let vertices: usize = step.iter().map(|(_, vs)| vs.len()).sum();
    assert!(vertices < 16 * 4, "{vertices} vertices along the step");
    for (_, vs) in step {
        assert!(
            vs.iter().all(|v| v[1] == 1.0 || v[1] == 1.25),
            "step at {vs:?}"
        );
    }
}

#[test]
fn test_lod_steps_are_lit_by_the_air_above_the_lower_ground() {
    let mesh = mesh_lod(&sloped_lod());
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("missing positions");
    };
    let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
    else {
        panic!("missing normals");
    };
    let Some(VertexAttributeValues::Float32x2(lights)) = mesh.attribute(Mesh::ATTRIBUTE_UV_1)
    else {
        panic!("missing light");
    };
    let step: Vec<f32> = (0..positions.len())
        .filter(|&i| {
            normals[i] == [-1.0, 0.0, 0.0] && positions[i][0] == 7.5 && positions[i][1] >= 1.0
        })
        .map(|i| lights[i][0])
        .collect();
    assert!(!step.is_empty());
    assert!(
        step.iter().all(|&l| l == 1.0),
        "steps under open sky lit {step:?}"
    );
}

fn mesh_chunk(chunk: ChunkData) -> ChunkMeshes {
    // well above the sea, under open sky
    let coord = ChunkCoordinate(I64Vec3::new(0, 40, 0));
    generate_chunk_mesh(
        Arc::new(chunk),
        vec![None; 26],
        coord,
        &NoiseGenerator::new(3),
        false,
    )
}

fn mesh_vertices(chunk: ChunkData) -> usize {
    mesh_chunk(chunk).opaque.count_vertices()
}

#[test]
fn test_single_block_has_six_quads() {
    let mut chunk = ChunkData::default();
    chunk.set_block_at(U16Vec3::new(4, 4, 4), BlockType::Stone);
    assert_eq!(24, mesh_vertices(chunk));
}

#[test]
fn test_flat_floor_merges_into_one_quad_per_face() {
    let mut chunk = ChunkData::default();
    for x in 0..16 {
        for z in 0..16 {
            chunk.set_block_at(U16Vec3::new(x, 0, z), BlockType::Stone);
        }
    }
    // each quad is a centre point plus a vertex per block step round its border: the top
    // is 16x16, the four sides 16x1. The underside shades as light creeps in under it.
    let mesh = mesh_chunk(chunk).opaque;
    let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
    else {
        panic!("missing normals");
    };
    let facing = |n: [f32; 3]| normals.iter().filter(|&&v| v == n).count();
    assert_eq!(1 + 64, facing([0.0, 1.0, 0.0]));
    for side in [
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ] {
        assert_eq!(1 + 34, facing(side));
    }
}

#[test]
fn test_different_block_types_do_not_merge() {
    let mut chunk = ChunkData::default();
    chunk.set_block_at(U16Vec3::new(4, 4, 4), BlockType::Stone);
    chunk.set_block_at(U16Vec3::new(5, 4, 4), BlockType::Grass);
    // 5 faces each, the shared one hidden
    assert_eq!(10 * 4, mesh_vertices(chunk));
}

fn floor_chunk() -> ChunkData {
    let mut chunk = ChunkData::default();
    for x in 0..16 {
        for z in 0..16 {
            chunk.set_block_at(U16Vec3::new(x, 0, z), BlockType::Stone);
        }
    }
    chunk
}

#[test]
fn test_merged_quads_tile_their_uvs_and_carry_the_atlas_cell() {
    let mesh = mesh_chunk(floor_chunk()).opaque;
    let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else {
        panic!("missing uvs");
    };
    let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
    else {
        panic!("missing colors");
    };
    let tiles = uvs.iter().flatten().fold(0.0f32, |m, &c| m.max(c));
    assert_eq!(16.0, tiles);
    assert!(colors.iter().all(|c| c[3] == Tile::Stone.atlas_cell()));
}

fn top_face_triangles(mesh: &Mesh) -> Vec<f32> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("missing positions");
    };
    let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
    else {
        panic!("missing normals");
    };
    let Some(Indices::U32(indices)) = mesh.indices() else {
        panic!("missing indices");
    };
    indices
        .chunks(3)
        .filter(|t| t.iter().all(|&i| normals[i as usize] == [0.0, 1.0, 0.0]))
        .map(|t| {
            let p = |k: usize| Vec3::from(positions[t[k] as usize]);
            (p(1) - p(0)).cross(p(2) - p(0)).y
        })
        .filter(|area| *area != 0.0)
        .collect()
}

#[test]
fn test_merged_quads_keep_the_winding_of_single_faces() {
    let mut single = ChunkData::default();
    single.set_block_at(U16Vec3::new(4, 4, 4), BlockType::Stone);
    let single = mesh_chunk(single).opaque;
    let merged = mesh_chunk(floor_chunk()).opaque;
    let expected = top_face_triangles(&single)[0].signum();
    assert!(top_face_triangles(&merged)
        .iter()
        .all(|area| area.signum() == expected));
}

#[test]
fn test_merged_quad_borders_share_vertices_with_unit_faces() {
    let mesh = mesh_chunk(floor_chunk()).opaque;
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("missing positions");
    };
    for step in 0..=16 {
        let edge = [step as f32 - 0.5, 0.5, -0.5];
        assert!(positions.contains(&edge), "no top-face vertex at {edge:?}");
    }
}

fn tints(mesh: &Mesh) -> Vec<Vec3> {
    let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
    else {
        panic!("missing colors");
    };
    colors.iter().map(|c| Vec3::new(c[0], c[1], c[2])).collect()
}

#[test]
fn test_only_grass_tops_and_sides_are_tinted() {
    let mut chunk = ChunkData::default();
    chunk.set_block_at(U16Vec3::new(4, 4, 4), BlockType::Stone);
    chunk.set_block_at(U16Vec3::new(10, 4, 10), BlockType::Grass);
    let colours = tints(&mesh_chunk(chunk).opaque);
    assert_eq!(28, colours.iter().filter(|c| **c == Vec3::ONE).count());
    assert_eq!(20, colours.iter().filter(|c| **c != Vec3::ONE).count());
}

#[test]
fn test_grass_sides_carry_their_untinted_shade() {
    let mut chunk = ChunkData::default();
    chunk.set_block_at(U16Vec3::new(10, 4, 10), BlockType::Grass);
    chunk.set_block_at(U16Vec3::new(11, 4, 10), BlockType::Grass);
    let mesh = mesh_chunk(chunk).opaque;
    let (
        Some(VertexAttributeValues::Float32x4(colors)),
        Some(VertexAttributeValues::Float32x2(modes)),
    ) = (
        mesh.attribute(Mesh::ATTRIBUTE_COLOR),
        mesh.attribute(Mesh::ATTRIBUTE_UV_1),
    )
    else {
        panic!("mesh lacks colours or uv_b");
    };
    let side = Tile::GrassSide.atlas_cell();
    let mut sides = 0;
    for (color, mode) in colors.iter().zip(modes) {
        if color[3] == side {
            sides += 1;
            // the shade is ambient occlusion here, so between dark and full
            assert!(mode[1] < 0.0 && mode[1] >= -1.0, "{mode:?}");
        } else {
            assert_eq!(super::buffers::TEXTURED, mode[1]);
        }
    }
    assert!(sides > 0);
}

#[test]
fn test_full_detail_ground_is_soil_a_few_blocks_deep_over_stone() {
    let under = |block, depth: i64| under_surface(block, 20 - 1 - depth, 20, 1);
    assert_eq!(BlockType::Grass, under(BlockType::Grass, 0));
    assert_eq!(BlockType::Dirt, under(BlockType::Grass, 3));
    assert_eq!(BlockType::Stone, under(BlockType::Grass, 4));
    assert_eq!(BlockType::Sand, under(BlockType::Sand, 3));
    assert_eq!(BlockType::Sandstone, under(BlockType::Sand, 4));
    assert_eq!(BlockType::Stone, under(BlockType::Sand, 8));
    assert_eq!(BlockType::Stone, under(BlockType::Gravel, 4));
    assert_eq!(BlockType::Snow, under(BlockType::Snow, 7));
    assert_eq!(BlockType::Stone, under(BlockType::Snow, 8));
    assert_eq!(BlockType::Stone, under(BlockType::Stone, 1));
    // wider cells are sampled mid-cell, and keep their surface
    assert_eq!(BlockType::Grass, under_surface(BlockType::Grass, 0, 20, 4));
}

#[test]
fn test_faces_pick_their_tile_and_merge_within_a_tile() {
    let mut chunk = ChunkData::default();
    chunk.set_block_at(U16Vec3::new(4, 4, 4), BlockType::Log);
    let mesh = mesh_chunk(chunk).opaque;
    let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
    else {
        panic!("missing colors");
    };
    let count = |tile: Tile| colors.iter().filter(|c| c[3] == tile.atlas_cell()).count();
    assert_eq!(8, count(Tile::LogEnd));
    assert_eq!(16, count(Tile::LogBark));
    let merged = mesh_chunk(floor_chunk()).opaque;
    let Some(VertexAttributeValues::Float32x4(colors)) = merged.attribute(Mesh::ATTRIBUTE_COLOR)
    else {
        panic!("missing colors");
    };
    assert!(colors.iter().all(|c| c[3] == Tile::Stone.atlas_cell()));
}

#[test]
fn test_grass_tint_matches_along_shared_edges() {
    let noise = NoiseGenerator::new(3);
    let chunk_tint =
        |x: i64, z: i64| GrassTint::new(&noise, Vec3::new(x as f32, 0.0, z as f32) * 16.0, 1.0);
    let (here, east, south) = (chunk_tint(5, 5), chunk_tint(6, 5), chunk_tint(5, 6));
    for step in 0..=16 {
        let along = step as f32 - 0.5;
        assert!(here
            .at(Vec3::new(15.5, 0.0, along))
            .abs_diff_eq(east.at(Vec3::new(-0.5, 0.0, along)), 1e-5));
        assert!(here
            .at(Vec3::new(along, 0.0, 15.5))
            .abs_diff_eq(south.at(Vec3::new(along, 0.0, -0.5)), 1e-5));
    }

    // a LOD node four chunks across, its local units 4 blocks wide, starting at chunk (4, 4)
    let node = GrassTint::new(&noise, Vec3::new(64.0, 0.0, 64.0) + Vec3::splat(1.5), 4.0);
    assert!(node
        .at(Vec3::splat(-0.5))
        .abs_diff_eq(chunk_tint(4, 4).at(Vec3::splat(-0.5)), 1e-5));
    assert!(node
        .at(Vec3::splat(15.5))
        .abs_diff_eq(chunk_tint(7, 7).at(Vec3::splat(15.5)), 1e-5));
}

// top-face sky light at each vertex, by position
fn top_lights(mesh: &Mesh) -> Vec<([f32; 3], f32)> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("missing positions");
    };
    let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
    else {
        panic!("missing normals");
    };
    let Some(VertexAttributeValues::Float32x2(lights)) = mesh.attribute(Mesh::ATTRIBUTE_UV_1)
    else {
        panic!("missing light");
    };
    (0..positions.len())
        .filter(|&i| normals[i] == [0.0, 1.0, 0.0])
        .map(|i| (positions[i], lights[i][0]))
        .collect()
}

#[test]
fn test_open_ground_is_fully_lit() {
    let lights = top_lights(&mesh_chunk(floor_chunk()).opaque);
    assert!(!lights.is_empty() && lights.iter().all(|(_, l)| *l == 1.0));
}

#[test]
fn test_ground_under_a_roof_is_shaded_smoothly() {
    let mut chunk = floor_chunk();
    // a stone roof over x 4..12, z 4..12, three blocks up
    for x in 4..12 {
        for z in 4..12 {
            chunk.set_block_at(U16Vec3::new(x, 3, z), BlockType::Stone);
        }
    }
    let lights = top_lights(&mesh_chunk(chunk).opaque);
    let floor: Vec<_> = lights.iter().filter(|(p, _)| p[1] == 0.5).collect();
    let (darkest, _) = floor.iter().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
    assert!(
        floor.iter().any(|(_, l)| *l == 1.0),
        "nowhere on the floor is in the open"
    );
    assert!(
        (4.5..=11.5).contains(&darkest[0]) && (4.5..=11.5).contains(&darkest[2]),
        "darkest at {darkest:?}"
    );
    let mut levels: Vec<f32> = floor.iter().map(|(_, l)| *l).collect();
    levels.sort_by(f32::total_cmp);
    levels.dedup();
    assert!(levels.len() >= 4, "light falls off in steps {levels:?}");
    // the roof's top is under open sky
    assert!(lights.iter().any(|(p, l)| p[1] == 3.5 && *l == 1.0));
}

#[test]
fn test_lod_ground_under_open_sky_is_lit_to_its_edges() {
    use bevy::math::I64Vec2;

    // a mountainside, where LOD skirts stand in for the solid ground beside the node
    let noise = Arc::new(NoiseGenerator::new(42));
    let p = (-40..40)
        .flat_map(|z| (-40..40).map(move |x| I64Vec2::new(x, z) * 256))
        .find(|p| (150.0..250.0).contains(&(noise.get(*p) - WATER_LEVEL as f64)))
        .expect("no mountainside near the centre");
    let height = noise.get(p) as i64;
    let chunk = ChunkCoordinate(I64Vec3::new(
        p.x.div_euclid(16),
        height.div_euclid(16),
        p.y.div_euclid(16),
    ));
    let regions = Arc::new(RegionStore::new(noise));
    let key = crate::chunks::lod::LodKey::containing(chunk, 1);
    let mesh = super::lod::generate_lod_mesh(regions, key, None)
        .expect("no mesh")
        .opaque;
    let lights = top_lights(&mesh);
    assert!(!lights.is_empty());
    let dark: Vec<_> = lights.iter().filter(|(_, l)| *l < 1.0).collect();
    assert!(dark.is_empty(), "shaded under open sky: {dark:?}");
}

// whether each vertex draws its block's mean colour instead of its texture, see `world.wesl`
fn untextured_flags(mesh: &Mesh) -> Vec<f32> {
    let Some(VertexAttributeValues::Float32x2(lights)) = mesh.attribute(Mesh::ATTRIBUTE_UV_1)
    else {
        panic!("missing light");
    };
    lights.iter().map(|l| l[1]).collect()
}

#[test]
fn test_chunks_are_textured() {
    let flags = untextured_flags(&mesh_chunk(floor_chunk()).opaque);
    assert!(!flags.is_empty() && flags.iter().all(|&f| f == 0.0));
}

#[test]
fn test_lods_wider_than_four_blocks_are_untextured() {
    use bevy::math::I64Vec2;

    let noise = Arc::new(NoiseGenerator::new(42));
    let height = noise.get(I64Vec2::ZERO) as i64;
    let chunk = ChunkCoordinate(I64Vec3::new(0, height.div_euclid(16), 0));
    let regions = Arc::new(RegionStore::new(noise));
    let key = crate::chunks::lod::LodKey::containing(chunk, 5);
    assert!(key.cell_size() > super::buffers::BOX_TEXTURE_MAX_CELL);
    let flags = untextured_flags(
        &super::lod::generate_lod_mesh(regions, key, None)
            .expect("no mesh")
            .opaque,
    );
    // but for tree boxes, textured with their leaves' holes filled, which come last
    let ground = flags
        .iter()
        .take_while(|&&f| f == super::buffers::UNTEXTURED)
        .count();
    assert!(
        ground > 0
            && flags[ground..]
                .iter()
                .all(|&f| f == super::buffers::TEXTURED_SOLID)
    );
}

#[test]
fn test_tree_boxes_stay_textured_up_to_eight_wide_cells_with_a_tile_a_block() {
    let regions = Arc::new(RegionStore::new(Arc::new(NoiseGenerator::new(5))));
    let mut heights = regions.sampler();
    let trees = crate::chunks::generate::trees::trees_near(
        regions.noise(),
        &mut heights,
        bevy::math::I64Vec2::splat(-300),
        bevy::math::I64Vec2::splat(300),
    );
    let tree = &trees[0];
    let crown = tree.base + I64Vec3::Y * (tree.trunk - 1);
    let key = LodKey::containing(ChunkCoordinate(crown.div_euclid(I64Vec3::splat(16))), 3);
    assert!(key.cell_size() <= super::buffers::BOX_TEXTURE_MAX_CELL);
    let opaque = super::lod::generate_lod_mesh(regions, key, None)
        .expect("no mesh")
        .opaque;
    let flags = untextured_flags(&opaque);
    let VertexAttributeValues::Float32x2(uvs) = opaque.attribute(Mesh::ATTRIBUTE_UV_0).unwrap()
    else {
        panic!()
    };
    // boxes come last: textured, and counting more than one tile across a crown
    let boxed = flags.len() - flags.iter().rev().take_while(|&&f| f == 0.0).count();
    assert!(boxed < flags.len(), "no textured tree boxes");
    assert!(uvs[boxed..].iter().any(|uv| uv[0] > 3.0 || uv[1] > 3.0));
    assert!(flags[..boxed].iter().all(|&f| f == 1.0));
}

#[test]
fn test_lods_up_to_four_blocks_are_textured_with_the_texture_repeating_over_each_cell() {
    use bevy::math::I64Vec2;

    let noise = Arc::new(NoiseGenerator::new(42));
    let height = noise.get(I64Vec2::ZERO) as i64;
    let chunk = ChunkCoordinate(I64Vec3::new(0, height.div_euclid(16), 0));
    let regions = Arc::new(RegionStore::new(noise));
    let key = crate::chunks::lod::LodKey::containing(chunk, 2);
    assert_eq!(4, key.cell_size());
    let mesh = super::lod::generate_lod_mesh(regions, key, None)
        .expect("no mesh")
        .opaque;
    let flags = untextured_flags(&mesh);
    // grass sides mark their shade instead, see `MeshBuffers::tint_grass_sides`
    assert!(!flags.is_empty() && flags.iter().all(|&f| f <= 0.0));
    // a face one cell wide spans four tiles, so none comes out a tile wide
    let uvs = mesh.attribute(Mesh::ATTRIBUTE_UV_0).unwrap();
    let bevy::mesh::VertexAttributeValues::Float32x2(uvs) = uvs else {
        panic!()
    };
    let tiles = |axis: usize| uvs.iter().map(|uv| uv[axis]).fold(0.0f32, f32::max);
    assert!(tiles(0) >= 4.0 || tiles(1) >= 4.0);
}

#[test]
fn test_ambient_occlusion_stops_faces_merging() {
    let mut chunk = floor_chunk();
    chunk.set_block_at(U16Vec3::new(8, 1, 8), BlockType::Stone);
    // one quad per face of the floor and of the pillar would be 44; the shaded ring around it splits the top
    assert!(mesh_vertices(chunk) > 44);
}

// A floor with a wall, a step up onto a ledge and a lone block on it, shaded every which way.
fn walled_chunk() -> ChunkData {
    let mut chunk = floor_chunk();
    for x in 2..14 {
        chunk.set_block_at(U16Vec3::new(x, 1, 4), BlockType::Stone);
        chunk.set_block_at(U16Vec3::new(x, 2, 4), BlockType::Stone);
    }
    for x in 0..16 {
        for z in 10..16 {
            chunk.set_block_at(U16Vec3::new(x, 1, z), BlockType::Stone);
        }
    }
    chunk.set_block_at(U16Vec3::new(7, 2, 12), BlockType::Stone);
    chunk
}

// Each vertex's position, normal, colour and light.
fn shaded_vertices(mesh: &Mesh) -> Vec<(Vec3, Vec3, Vec3, f32)> {
    let positions = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .unwrap()
        .as_float3()
        .unwrap();
    let normals = mesh
        .attribute(Mesh::ATTRIBUTE_NORMAL)
        .unwrap()
        .as_float3()
        .unwrap();
    let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
    else {
        panic!()
    };
    let Some(VertexAttributeValues::Float32x2(lights)) = mesh.attribute(Mesh::ATTRIBUTE_UV_1)
    else {
        panic!()
    };
    (0..positions.len())
        .map(|i| {
            (
                Vec3::from(positions[i]),
                Vec3::from(normals[i]),
                Vec3::new(colors[i][0], colors[i][1], colors[i][2]),
                lights[i][0],
            )
        })
        .collect()
}

#[test]
fn test_merging_shaded_faces_leaves_their_shading_as_it_was() {
    let unmerged = {
        super::greedy::NO_MERGING.set(true);
        let mesh = mesh_chunk(walled_chunk()).opaque;
        super::greedy::NO_MERGING.set(false);
        mesh
    };
    let merged = mesh_chunk(walled_chunk()).opaque;
    assert!(merged.count_vertices() < unmerged.count_vertices());
    let vertices = shaded_vertices(&merged);
    let Some(Indices::U32(indices)) = merged.indices() else {
        panic!()
    };
    // every corner of every face drawn alone is shaded the same by the merged quad over it
    for (p, normal, color, light) in shaded_vertices(&unmerged) {
        let found = indices.chunks(3).find_map(|tri| {
            let [a, b, c] = [0, 1, 2].map(|k| vertices[tri[k] as usize]);
            if a.1 != normal || (p - a.0).dot(normal).abs() > 1e-4 {
                return None;
            }
            // barycentric weights of p in the triangle
            let (v0, v1, v2) = (b.0 - a.0, c.0 - a.0, p - a.0);
            let (d00, d01, d11, d20, d21) =
                (v0.dot(v0), v0.dot(v1), v1.dot(v1), v2.dot(v0), v2.dot(v1));
            let denom = d00 * d11 - d01 * d01;
            let (wb, wc) = (
                (d11 * d20 - d01 * d21) / denom,
                (d00 * d21 - d01 * d20) / denom,
            );
            let wa = 1.0 - wb - wc;
            (wa.min(wb).min(wc) > -1e-4).then(|| {
                (
                    a.2 * wa + b.2 * wb + c.2 * wc,
                    a.3 * wa + b.3 * wb + c.3 * wc,
                )
            })
        });
        let (found_color, found_light) =
            found.unwrap_or_else(|| panic!("nothing drawn at {p} facing {normal}"));
        assert!(
            (found_color - color).abs().max_element() < 2e-3,
            "at {p} facing {normal}: {found_color} for {color}"
        );
        assert!(
            (found_light - light).abs() < 1e-4,
            "at {p} facing {normal}: light {found_light} for {light}"
        );
    }
}

// Open air over ground far below the node, with `blocks` set, in cells of 4 blocks.
fn lod_with(blocks: &[(IVec3, BlockType)]) -> LodBlocks {
    let size = 16;
    let padded = (size + 2) as usize;
    let mut cells = vec![BlockType::Air; padded * padded * padded];
    for &(coord, block) in blocks {
        cells[LodBlocks::index(size, coord)] = block;
    }
    LodBlocks::new(size, cells, vec![0; padded * padded], WATER_LEVEL + 64, 4)
}

// a 2x2x2 cube of leaves from `min` up
fn leaf_cube(min: i32) -> Vec<(IVec3, BlockType)> {
    let mut cube = Vec::new();
    for x in min..min + 2 {
        for y in min..min + 2 {
            for z in min..min + 2 {
                cube.push((IVec3::new(x, y, z), BlockType::Leaves));
            }
        }
    }
    cube
}

// how many vertices lie on faces in the plane at `plane` across their normal's axis
fn vertices_in_plane(mesh: &Mesh, plane: f32) -> usize {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("missing positions");
    };
    let Some(VertexAttributeValues::Float32x3(normals)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
    else {
        panic!("missing normals");
    };
    positions
        .iter()
        .zip(normals)
        .filter(|(p, n)| (0..3).any(|k| n[k] != 0.0 && p[k] == plane))
        .count()
}

#[test]
fn test_untextured_leaves_hide_the_leaves_behind_them() {
    let mesh = mesh_lod(&lod_with(&leaf_cube(7)));
    // cells 7 and 8 meet at 7.5; the shell is at 6.5 and 8.5
    assert_eq!(0, vertices_in_plane(&mesh, 7.5));
    assert!(vertices_in_plane(&mesh, 6.5) > 0 && vertices_in_plane(&mesh, 8.5) > 0);
    // each side of the shell merges into one quad, a centre point and 8 round its border
    assert_eq!(6 * 9, mesh.count_vertices());
}

#[test]
fn test_untextured_leaves_still_show_other_blocks() {
    let mut blocks = leaf_cube(7);
    blocks.push((IVec3::new(6, 7, 7), BlockType::Log));
    let faces = faces(&mesh_lod(&lod_with(&blocks)));
    // the log's face into the leaves stays: only leaves hide leaves
    assert!(faces
        .iter()
        .any(|(n, vs)| *n == [1.0, 0.0, 0.0] && vs.iter().all(|v| v[0] == 6.5)));
}

#[test]
fn test_untextured_leaves_on_the_nodes_edge_keep_their_faces() {
    // past the edge the leaves may be drawn at another detail, with none just there
    let lod = lod_with(&[
        (IVec3::new(15, 4, 4), BlockType::Leaves),
        (IVec3::new(16, 4, 4), BlockType::Leaves),
    ]);
    let faces = faces(&mesh_lod(&lod));
    assert!(faces
        .iter()
        .any(|(n, vs)| *n == [1.0, 0.0, 0.0] && vs.iter().all(|v| v[0] == 15.5)));
}

fn leaf_cube_chunk() -> ChunkData {
    let mut chunk = ChunkData::default();
    for (coord, block) in leaf_cube(4) {
        chunk.set_block_at(coord.as_u16vec3(), block);
    }
    chunk
}

#[test]
fn test_full_detail_leaves_show_the_leaves_behind_them() {
    // blocks 4 and 5 meet at 4.5, where the faces between them show through the holes
    let mesh = mesh_chunk(leaf_cube_chunk()).opaque;
    assert!(vertices_in_plane(&mesh, 4.5) > 0);
}

#[test]
fn test_full_detail_solid_leaves_hide_the_leaves_behind_them() {
    let coord = ChunkCoordinate(I64Vec3::new(0, 40, 0));
    let mesh = generate_chunk_mesh(
        Arc::new(leaf_cube_chunk()),
        vec![None; 26],
        coord,
        &NoiseGenerator::new(3),
        true,
    )
    .opaque;
    assert_eq!(0, vertices_in_plane(&mesh, 4.5));
    assert!(vertices_in_plane(&mesh, 3.5) > 0 && vertices_in_plane(&mesh, 5.5) > 0);
}

#[test]
fn test_lod_border_exposes_surface_cells_as_skirt() {
    let lod = flat_lod(8);
    // the border's top two solid cells read as open, so the edge draws a wall
    assert_eq!(BlockType::Air, lod.block(IVec3::new(-1, 7, 4)));
    assert_eq!(BlockType::Air, lod.block(IVec3::new(-1, 6, 4)));
    assert_eq!(BlockType::Air, lod.block(IVec3::new(4, 7, 16)));
    // deeper border cells and every interior cell read as they are
    assert_eq!(BlockType::Stone, lod.block(IVec3::new(-1, 5, 4)));
    assert_eq!(BlockType::Stone, lod.block(IVec3::new(4, 7, 4)));
    // below the node isn't a horizontal border, so no skirt
    assert_eq!(BlockType::Stone, lod.block(IVec3::new(4, -1, 4)));
}

#[test]
fn test_skirt_walls_are_lit_like_the_open_ground_they_hang_from() {
    // flat ground under open sky, cells a block across, and the cells beyond it solid
    let lod = flat_lod(8);
    let noise = NoiseGenerator::new(1);
    let tint = super::buffers::GrassTint::new(&noise, Vec3::ZERO, 1.0);
    let opacity = lod.cells.iter().map(BlockType::light_opacity).collect();
    let light = std::cell::LazyCell::new(|| {
        Light::sky(SkyLight::new(
            IVec3::splat(-1),
            IVec3::splat(18),
            MAX_LIGHT,
            opacity,
        ))
    });
    let mesh = super::mesh::mesh_blocks(&lod, 1, style(true, false), &tint, &light).opaque;
    let Some(bevy::mesh::VertexAttributeValues::Float32x2(lights)) =
        mesh.attribute(Mesh::ATTRIBUTE_UV_1)
    else {
        panic!("no light on the mesh");
    };
    // the top and the four walls, each from a corner to a corner of the node
    assert!(lights.len() >= 4 * 5);
    let darkest = lights.iter().map(|l| l[0]).fold(f32::MAX, f32::min);
    assert!(
        darkest > 0.6,
        "a vertex is lit {darkest}, so a skirt wall is dark"
    );
}

#[test]
fn test_lod_skirt_hangs_from_the_nodes_own_ground_not_its_neighbours() {
    // ground reaching the top of the node, where there's no air above to find
    let size = 16;
    let padded = (size + 2) as usize;
    let mut cells = vec![BlockType::Air; padded * padded * padded];
    for x in -1..=size {
        for z in -1..=size {
            for y in -1..=size {
                cells[LodBlocks::index(size, IVec3::new(x, y, z))] = BlockType::Stone;
            }
        }
    }
    // the ground's top is in the node above, 3 cells over the top row
    let lod = LodBlocks::new(
        size,
        cells.clone(),
        vec![size as i64 + 3; padded * padded],
        0,
        1,
    );
    assert_eq!(BlockType::Stone, lod.block(IVec3::new(-1, 8, 4)));
    // ground ending in the top row: the cells within two of it have walls, though the top row is solid
    let lod = LodBlocks::new(size, cells, vec![size as i64 + 1; padded * padded], 0, 1);
    assert_eq!(BlockType::Air, lod.block(IVec3::new(-1, 16, 4)));
    assert_eq!(BlockType::Air, lod.block(IVec3::new(-1, 15, 4)));
    assert_eq!(BlockType::Stone, lod.block(IVec3::new(-1, 14, 4)));
}

#[test]
fn test_lod_water_surface_cells_over_shallow_ground_are_water() {
    let regions = Arc::new(RegionStore::new(Arc::new(NoiseGenerator::new(102))));
    let mut checked = 0;
    for level in [3u8, 4, 5] {
        for x in -6..6i64 {
            for z in -6..6i64 {
                let width = 1i64 << level;
                let key = LodKey {
                    level,
                    coord: I64Vec3::new(x, (WATER_LEVEL / 16).div_euclid(width), z),
                };
                let lod = LodBlocks::sample(&regions, key, None);
                for cx in 0..lod.size {
                    for cz in 0..lod.size {
                        let column = LodBlocks::column(lod.size, cx, cz);
                        if lod.tops[column] >= WATER_LEVEL {
                            continue;
                        }
                        for cy in 0..lod.size {
                            let bottom = lod.base_y + cy as i64 * lod.cell;
                            if bottom < WATER_LEVEL && WATER_LEVEL <= bottom + lod.cell {
                                assert!(!super::terrain::is_solid(lod.raw(IVec3::new(cx, cy, cz))));
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(checked > 1000);
}

#[test]
fn test_water_depth_on_lod_border_ignores_skirts() {
    // ground up to y = 3 everywhere, water from 4 to 7
    let mut lod = flat_lod(4);
    for x in -1..=16 {
        for z in -1..=16 {
            for y in 4..8 {
                lod.cells[LodBlocks::index(16, IVec3::new(x, y, z))] = BlockType::Water;
            }
        }
    }
    // the border's top solid cells read as water for the skirt, but don't add depth
    assert_eq!(BlockType::Water, lod.block(IVec3::new(-1, 3, 4)));
    assert_eq!(4, water_depth(&lod, IVec3::new(4, 7, 4)));
    assert_eq!(4, water_depth(&lod, IVec3::new(-1, 7, 4)));
}

#[test]
fn test_lod_block_clamps_far_probes_to_border() {
    let lod = flat_lod(8);
    assert_eq!(
        lod.block(IVec3::new(-1, 3, 4)),
        lod.block(IVec3::new(-3, 3, 4))
    );
}

#[test]
fn test_buried_chunk_has_no_geometry() {
    let mut stone = ChunkData::default();
    for (x, y, z) in
        (0..16).flat_map(|x| (0..16).flat_map(move |y| (0..16).map(move |z| (x, y, z))))
    {
        stone.set_block_at(U16Vec3::new(x, y, z), BlockType::Stone);
    }
    let stone = Arc::new(stone);
    let meshes = generate_chunk_mesh(
        stone.clone(),
        vec![Some(stone); 26],
        ChunkCoordinate(I64Vec3::ZERO),
        test_regions().noise(),
        false,
    );
    assert!(!meshes.has_geometry());
}

#[test]
fn test_side_faces_texture_the_right_way_up_merged_or_not() {
    let mut chunk = ChunkData::default();
    for x in 4..7 {
        for y in 4..6 {
            chunk.set_block_at(U16Vec3::new(x, y, 4), BlockType::Grass);
        }
    }
    let mesh = mesh_chunk(chunk).opaque;
    let (
        Some(VertexAttributeValues::Float32x3(positions)),
        Some(VertexAttributeValues::Float32x3(normals)),
        Some(VertexAttributeValues::Float32x2(uvs)),
    ) = (
        mesh.attribute(Mesh::ATTRIBUTE_POSITION),
        mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
        mesh.attribute(Mesh::ATTRIBUTE_UV_0),
    )
    else {
        panic!("mesh lacks positions, normals or uvs");
    };
    // v counts blocks down from the wall's top on every side, so the tile's top row is at the top
    let mut sides = 0;
    for ((p, n), uv) in positions.iter().zip(normals).zip(uvs) {
        if n[1] == 0.0 {
            sides += 1;
            assert!(
                (p[1] + uv[1] - 5.5).abs() < 1e-4,
                "normal {n:?}: v {} at y {}",
                uv[1],
                p[1]
            );
        }
    }
    assert!(sides > 0);
}

#[test]
fn test_canopy_shade_darkens_toward_the_ground() {
    let padded = 18 * 18;
    let lod = LodBlocks {
        canopy_tops: vec![40; padded],
        ..flat_lod(8)
    };
    let at = |y: f32| lod.canopy_shade(Vec3::new(4.0, y, 4.0));
    // a leaf at the canopy's top is unshaded, and the foot of a canopy is the darkest
    assert_eq!(1.0, at(39.5));
    assert!((at(19.5) - (1.0 - super::canopy::CANOPY_SHADE)).abs() < 1e-5);
    assert!(at(30.0) < at(35.0) && at(35.0) < at(39.5));
    // below the span it stops getting darker
    assert_eq!(at(19.5), at(9.5));
}

#[test]
fn test_canopy_shade_leaves_bare_columns_and_other_lods_alone() {
    let padded = 18 * 18;
    let bare = LodBlocks {
        canopy_tops: vec![i64::MIN; padded],
        ..flat_lod(8)
    };
    assert_eq!(1.0, bare.canopy_shade(Vec3::new(4.0, 2.0, 4.0)));
    assert_eq!(1.0, flat_lod(8).canopy_shade(Vec3::new(4.0, 2.0, 4.0)));
}

#[test]
fn test_a_lone_tree_is_still_there_in_every_cell_up_to_thirty_two_wide() {
    use crate::chunks::generate::trees::trees_near;
    use bevy::math::I64Vec2;
    let regions = Arc::new(RegionStore::new(Arc::new(NoiseGenerator::new(5))));
    let mut heights = regions.sampler();
    let trees = trees_near(
        regions.noise(),
        &mut heights,
        I64Vec2::splat(-300),
        I64Vec2::splat(300),
    );
    let near = |a: &crate::chunks::generate::trees::Tree,
                b: &crate::chunks::generate::trees::Tree| {
        (a.base.x - b.base.x).abs() < 40 && (a.base.z - b.base.z).abs() < 40
    };
    let tree = trees
        .iter()
        .find(|t| trees.iter().filter(|o| near(t, o)).count() == 1)
        .expect("no lone tree");
    let crown = tree.base + I64Vec3::Y * (tree.trunk - 1);
    for level in [1u8, 2, 3, 4, 5] {
        let key = LodKey::containing(ChunkCoordinate(crown.div_euclid(I64Vec3::splat(16))), level);
        let blocks = LodBlocks::sample(&regions, key, None);
        let cell = key.cell_size();
        let leaf_cells = blocks
            .cells
            .iter()
            .filter(|&&c| c == BlockType::Leaves)
            .count();
        let crown_boxes = blocks
            .boxes
            .iter()
            .filter(|b| b.block == BlockType::Leaves)
            .count();
        if cell < super::canopy::BOX_CROWN_CELL {
            assert!(
                leaf_cells > 0,
                "no leaf cells at level {level}, cells {cell} blocks wide"
            );
        } else {
            // a crown is boxes of its real size, never a cell's worth of cube, and fewer the farther out
            assert_eq!(0, leaf_cells);
            let wanted = tree.crown_boxes(super::canopy::crown_layers(cell));
            assert!(
                !wanted.is_empty() && crown_boxes >= wanted.len(),
                "{crown_boxes} crown boxes of {} at level {level}",
                wanted.len()
            );
            for (min, max) in &wanted {
                let size = (*max - *min).as_vec3();
                let found = blocks
                    .boxes
                    .iter()
                    .filter(|b| b.block == BlockType::Leaves)
                    .any(|b| ((b.max - b.min) * cell as f32 - size).abs().max_element() < 1e-3);
                assert!(found, "no crown box of {size:?} at level {level}");
            }
            let wanted = wanted
                .iter()
                .fold(Vec3::ZERO, |w, (min, max)| w.max((*max - *min).as_vec3()));
            assert!(
                wanted.x < 20.0,
                "crown {wanted:?} wider than a crown should be"
            );
        }
    }
}

#[test]
fn test_a_lone_trees_trunk_is_a_thin_box_in_cells_four_to_sixteen_wide() {
    use crate::chunks::generate::trees::trees_near;
    use bevy::math::I64Vec2;
    let regions = Arc::new(RegionStore::new(Arc::new(NoiseGenerator::new(5))));
    let mut heights = regions.sampler();
    let trees = trees_near(
        regions.noise(),
        &mut heights,
        I64Vec2::splat(-300),
        I64Vec2::splat(300),
    );
    let near = |a: &crate::chunks::generate::trees::Tree,
                b: &crate::chunks::generate::trees::Tree| {
        (a.base.x - b.base.x).abs() < 40 && (a.base.z - b.base.z).abs() < 40
    };
    let tree = trees
        .iter()
        .find(|t| trees.iter().filter(|o| near(t, o)).count() == 1)
        .expect("no lone tree");
    for level in [2u8, 3, 4] {
        let key = LodKey::containing(
            ChunkCoordinate(tree.base.div_euclid(I64Vec3::splat(16))),
            level,
        );
        let blocks = LodBlocks::sample(&regions, key, None);
        let cell = key.cell_size() as f32;
        let at = |block: i64, min: i64| (block - min) as f32 / cell - 0.5;
        let (min_x, min_z) = (key.min_block().x, key.min_block().z);
        let trunk = blocks
            .boxes
            .iter()
            .filter(|b| b.block == BlockType::Log)
            .find(|t| {
                (t.min.x - at(tree.base.x, min_x)).abs() < 1e-4
                    && (t.min.z - at(tree.base.z, min_z)).abs() < 1e-4
                    && (t.max.x - t.min.x - tree.width() as f32 / cell).abs() < 1e-4
            });
        assert!(
            trunk.is_some(),
            "no trunk box at level {level}: {:?}",
            blocks.boxes
        );
        let trunk = trunk.unwrap();
        assert!(((trunk.max.y - trunk.min.y) * cell - tree.trunk as f32).abs() < 1e-3);
        // and no log cells, which would be a pillar several times too thick
        assert_eq!(
            0,
            blocks
                .cells
                .iter()
                .filter(|&&c| c == BlockType::Log)
                .count()
        );
        // meshed, the box is a few quads standing out of the ground's
        let lone = LodBlocks {
            boxes: vec![*trunk],
            ..LodBlocks::new(
                16,
                vec![BlockType::Air; 18 * 18 * 18],
                vec![0; 18 * 18],
                0,
                key.cell_size(),
            )
        };
        let tint = super::buffers::GrassTint::new(regions.noise(), Vec3::ZERO, 1.0);
        let light = std::cell::LazyCell::new(|| {
            Light::sky(SkyLight::new(
                IVec3::splat(-1),
                IVec3::splat(18),
                MAX_LIGHT,
                vec![0; 18 * 18 * 18],
            ))
        });
        let mesh =
            super::mesh::mesh_blocks(&lone, key.cell_size(), style(false, false), &tint, &light)
                .opaque;
        // its foot is in the ground and its top in the crown, so only its sides show
        assert_eq!(4 * 4, mesh.count_vertices());
    }
}

// The densest forest near the origin, and the trees in it.
fn forest(regions: &RegionStore) -> (I64Vec3, Vec<crate::chunks::generate::trees::Tree>) {
    use crate::chunks::generate::trees::trees_near;
    use bevy::math::I64Vec2;
    // the search is slow, and every caller's world is seed 5's
    static FOREST: std::sync::OnceLock<(I64Vec3, Vec<crate::chunks::generate::trees::Tree>)> =
        std::sync::OnceLock::new();
    assert_eq!(5, regions.noise().seed());
    FOREST
        .get_or_init(|| {
            let mut heights = regions.sampler();
            let corner = (-4..4)
                .flat_map(|x| (-4..4).map(move |z| I64Vec2::new(x, z) * 1024))
                .max_by_key(|&c| trees_near(regions.noise(), &mut heights, c, c + 255).len())
                .unwrap();
            let trees = trees_near(regions.noise(), &mut heights, corner, corner + 255);
            let ground = heights.height(corner.x + 128, corner.y + 128) as i64;
            (I64Vec3::new(corner.x + 128, ground, corner.y + 128), trees)
        })
        .clone()
}

#[test]
fn test_far_forests_are_a_few_canopy_boxes_not_a_box_a_tree() {
    let regions = Arc::new(RegionStore::new(Arc::new(NoiseGenerator::new(5))));
    let (middle, trees) = forest(&regions);
    assert!(
        trees.len() > 300,
        "only {} trees in the densest forest",
        trees.len()
    );
    for level in [4u8, 5] {
        let key = LodKey::containing(
            ChunkCoordinate(middle.div_euclid(I64Vec3::splat(16))),
            level,
        );
        let blocks = LodBlocks::sample(&regions, key, None);
        let cell = key.cell_size();
        let span = 16 * cell;
        let min = key.min_block();
        let inside = trees
            .iter()
            .filter(|t| {
                (min.x..min.x + span).contains(&t.base.x)
                    && (min.z..min.z + span).contains(&t.base.z)
            })
            .count();
        let leaf_boxes: Vec<_> = blocks
            .boxes
            .iter()
            .filter(|b| b.block == BlockType::Leaves)
            .collect();
        assert!(!leaf_boxes.is_empty(), "no canopy at level {level}");
        // each about as costly as one of the boxes a lone tree's crown is, where every tree was
        // three, and lone trees at the forest's edge keep theirs
        assert!(
            leaf_boxes.len() * 2 < inside,
            "{} leaf boxes for {inside} trees at level {level}",
            leaf_boxes.len()
        );
        // the forest's tree tops are under the canopy's, within a step of its rounding
        let step = (cell / super::canopy::CANOPY_STEPS) as f32;
        let highest = leaf_boxes
            .iter()
            .map(|b| min.y as f32 + (b.max.y + 0.5) * cell as f32)
            .fold(f32::MIN, f32::max);
        let corner = bevy::math::I64Vec2::new(min.x, min.z);
        let node_trees = crate::chunks::generate::trees::trees_near(
            regions.noise(),
            &mut regions.sampler(),
            corner,
            corner + span,
        );
        let tallest = node_trees
            .iter()
            .map(|t| t.crown_top() as f32)
            .fold(f32::MIN, f32::max);
        assert!(
            highest <= tallest + step,
            "canopy at {highest}, trees up to {tallest} at level {level}"
        );
        // the ground under it is darkened by how much of it is under crowns
        assert!(blocks.forest.iter().any(|&f| f > 0.9));
    }
}

#[test]
fn test_canopy_is_no_bigger_than_the_crowns_under_it() {
    let regions = Arc::new(RegionStore::new(Arc::new(NoiseGenerator::new(5))));
    let (middle, _) = forest(&regions);
    for level in [4u8, 5] {
        let key = LodKey::containing(
            ChunkCoordinate(middle.div_euclid(I64Vec3::splat(16))),
            level,
        );
        let blocks = LodBlocks::sample(&regions, key, None);
        let canopy: Vec<_> = blocks
            .boxes
            .iter()
            .filter(|b| b.faces == super::source::BoxFaces::NoBottom)
            .collect();
        let area = |b: &&super::source::LodBox| (b.max.x - b.min.x) * (b.max.z - b.min.z);
        let drawn: f32 = canopy.iter().map(area).sum();
        // what's under crowns over the ground this node draws
        let (min, span) = (key.min_block().y as f32, 16.0 * key.cell_size() as f32);
        let mut under = 0.0;
        for x in 0..16 {
            for z in 0..16 {
                let column = LodBlocks::column(16, x, z);
                if (min..min + span).contains(&blocks.ground_heights[column]) {
                    under += blocks.forest[column];
                }
            }
        }
        // its edges are drawn in to the crowns, so what's extra is the gaps between them inside
        // it, more of them the wider the columns
        assert!(
            drawn <= under * 1.4,
            "canopy {drawn} over {under} under crowns at level {level}"
        );
    }
}

#[test]
fn test_trees_vary_in_colour_the_same_at_every_level() {
    use crate::chunks::generate::trees::TreeTints;
    let regions = Arc::new(RegionStore::new(Arc::new(NoiseGenerator::new(5))));
    let (_, trees) = forest(&regions);
    let tints: Vec<Vec3> = trees.iter().map(|t| t.tint()).collect();
    let brightness = |t: &Vec3| t.x + t.y + t.z;
    let (dim, bright) = tints.iter().fold((f32::MAX, f32::MIN), |(lo, hi), t| {
        (lo.min(brightness(t)), hi.max(brightness(t)))
    });
    assert!(bright - dim > 0.4, "tints only span {dim}..{bright}");
    assert!(
        tints
            .iter()
            .all(|t| t.min_element() > 0.6 && t.max_element() < 1.5),
        "a tint strays too far"
    );
    // leaves take their own tree's tint, at full detail and in coarser cells
    let tree = &trees[0];
    let trunk_top = tree.base + I64Vec3::Y * tree.trunk;
    for cell in [1, 2] {
        let origin = trunk_top.div_euclid(I64Vec3::splat(16 * cell)) * 16 * cell;
        let at = (trunk_top - origin)
            .div_euclid(I64Vec3::splat(cell))
            .as_ivec3();
        assert_eq!(
            tree.tint(),
            TreeTints::new(&trees, origin, cell).at(at),
            "at cells {cell} wide"
        );
    }
}

#[test]
fn test_leaves_of_differently_tinted_trees_dont_merge() {
    let mut chunk = leaf_cube_chunk();
    let regions = Arc::new(RegionStore::new(Arc::new(NoiseGenerator::new(5))));
    let (_, trees) = forest(&regions);
    // two trees, one each side of the cube, so it's split down the middle
    let thin: Vec<_> = trees.iter().filter(|t| t.width() == 1).copied().collect();
    let mut pair = vec![thin[0], thin[1]];
    pair[0].base = I64Vec3::new(4, 0, 4);
    pair[1].base = I64Vec3::new(5, 0, 4);
    assert_ne!(pair[0].tint(), pair[1].tint());
    chunk.trees = crate::chunks::generate::trees::TreeTints::new(&pair, I64Vec3::ZERO, 1);
    let mesh = mesh_chunk(chunk).opaque;
    let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
    else {
        panic!()
    };
    let leaf = Tile::Leaves.atlas_cell();
    let mut shades: Vec<[u32; 3]> = colors
        .iter()
        .filter(|c| c[3] == leaf)
        .map(|c| [c[0], c[1], c[2]].map(|v| (v * 1000.0) as u32))
        .collect();
    shades.sort();
    shades.dedup();
    assert!(shades.len() >= 2, "leaves all one shade: {shades:?}");
}

#[test]
fn test_forest_tints_the_ground_darker_the_more_of_it_trees_cover() {
    let padded = 18 * 18;
    let mut forest = vec![0.0; padded];
    forest[LodBlocks::column(16, 4, 4)] = 0.5;
    forest[LodBlocks::column(16, 8, 8)] = 1.0;
    let lod = LodBlocks {
        forest,
        ..flat_lod(8)
    };
    let at = |x: f32, z: f32| lod.forest_shade(Vec3::new(x, 7.5, z));
    assert_eq!(1.0, at(1.0, 1.0));
    assert!(at(4.0, 4.0) < 1.0 && at(8.0, 8.0) < at(4.0, 4.0));
    assert!((at(8.0, 8.0) - super::canopy::FOREST_DARKEN).abs() < 1e-5);
    // real trees and bare ground leave the ground as it was
    assert_eq!(1.0, flat_lod(8).forest_shade(Vec3::new(4.0, 7.5, 4.0)));
}

#[test]
fn test_cells_wider_than_real_trees_tint_forest_instead_of_drawing_leaves() {
    use crate::chunks::generate::trees::trees_near;
    use bevy::math::I64Vec2;
    let regions = Arc::new(RegionStore::new(Arc::new(NoiseGenerator::new(5))));
    let mut heights = regions.sampler();
    let trees = trees_near(
        regions.noise(),
        &mut heights,
        I64Vec2::splat(-300),
        I64Vec2::splat(300),
    );
    let tree = trees.first().expect("no trees");
    let key = LodKey::containing(ChunkCoordinate(tree.base.div_euclid(I64Vec3::splat(16))), 6);
    assert!(key.cell_size() > super::canopy::TREE_MAX_CELL);
    let blocks = LodBlocks::sample(&regions, key, None);
    assert_eq!(
        0,
        blocks
            .cells
            .iter()
            .filter(|&&c| c == BlockType::Leaves || c == BlockType::Log)
            .count()
    );
    assert!(blocks.boxes.is_empty());
    assert!(
        blocks.forest.iter().any(|&f| f > 0.0)
            && blocks.forest.iter().all(|&f| (0.0..=1.0).contains(&f))
    );
}

type Spots = Vec<(i64, i64)>;

// Where trees grow within a node's own area, as full detail decides, and as the node's grid does.
fn tree_spots(regions: &Arc<RegionStore>, key: LodKey) -> (Spots, Spots) {
    use crate::chunks::generate::trees::trees_near;
    use bevy::math::I64Vec2;
    let (cell, base, size) = (key.cell_size(), key.min_block(), CHUNK_SIZE as i32);
    let span = (size + 1) as i64 * cell;
    let min = I64Vec2::new(base.x, base.z) - cell;
    let own = |t: &crate::chunks::generate::trees::Tree| {
        (base.x..base.x + size as i64 * cell).contains(&t.base.x)
            && (base.z..base.z + size as i64 * cell).contains(&t.base.z)
    };
    let mut heights = regions.sampler();
    let mut exact: Vec<_> = trees_near(regions.noise(), &mut heights, min, min + span + cell)
        .iter()
        .filter(|t| own(t))
        .map(|t| (t.base.x, t.base.z))
        .collect();
    let grid = regions.column_grid(key);
    let mut ground = super::lod::GridGround {
        grid: &grid,
        noise: regions.noise(),
        base: I64Vec2::new(base.x, base.z),
        cell,
        size,
        last: None,
    };
    let mut coarse: Vec<_> = trees_near(regions.noise(), &mut ground, min, min + span + cell)
        .iter()
        .filter(|t| own(t))
        .map(|t| (t.base.x, t.base.z))
        .collect();
    exact.sort();
    coarse.sort();
    (exact, coarse)
}

#[test]
fn test_every_level_grows_the_trees_full_detail_does() {
    use bevy::math::I64Vec2;
    let regions = Arc::new(RegionStore::new(Arc::new(NoiseGenerator::new(5))));
    for level in [2u8, 3, 4, 5] {
        // nodes over a forested stretch, found from the full-detail trees
        let mut heights = regions.sampler();
        let trees = crate::chunks::generate::trees::trees_near(
            regions.noise(),
            &mut heights,
            I64Vec2::splat(-500),
            I64Vec2::splat(500),
        );
        let mut checked = 0;
        for tree in trees.iter().step_by(trees.len() / 8 + 1) {
            let key = LodKey::containing(
                ChunkCoordinate(tree.base.div_euclid(I64Vec3::splat(16))),
                level,
            );
            let (exact, coarse) = tree_spots(&regions, key);
            assert_eq!(exact, coarse, "level {level} node {key:?}");
            checked += exact.len();
        }
        assert!(checked > 0, "no trees checked at level {level}");
    }
}

// Triangles of a node's opaque mesh in world space, placed as `lod_transform` does, less
// `MESH_OFFSET`.
fn world_triangles(regions: &Arc<RegionStore>, key: LodKey) -> Vec<[Vec3; 3]> {
    let Some(meshes) = super::lod::generate_lod_mesh(regions.clone(), key, None) else {
        return Vec::new();
    };
    let cell = key.cell_size() as f32;
    let origin = key.min_block().as_vec3() + Vec3::splat(0.5 * cell - 0.5);
    let positions = meshes
        .opaque
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .unwrap()
        .as_float3()
        .unwrap();
    let Some(Indices::U32(indices)) = meshes.opaque.indices() else {
        return Vec::new();
    };
    indices
        .chunks(3)
        .map(|t| [0, 1, 2].map(|k| origin + Vec3::from(positions[t[k] as usize]) * cell))
        .collect()
}

// Whether the ray from `from` along `direction` meets the triangle, either side.
fn ray_hits(from: Vec3, direction: Vec3, [a, b, c]: [Vec3; 3]) -> bool {
    let (e1, e2) = (b - a, c - a);
    let p = direction.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return false;
    }
    let t = from - a;
    let u = t.dot(p) / det;
    let q = t.cross(e1);
    let v = direction.dot(q) / det;
    u >= 0.0 && v >= 0.0 && u + v <= 1.0 && e2.dot(q) / det > 0.0
}

// How many oblique rays down across the seam between a node and its coarser neighbour pass
// through every triangle of both, seeing the sky through a crack.
fn seam_leaks(regions: &Arc<RegionStore>, fine: LodKey, coarse: LodKey) -> (usize, usize) {
    // the nodes above and below too, where the ground crosses into them
    let mut triangles = Vec::new();
    for dy in -1..=1 {
        for key in [fine, coarse] {
            triangles.extend(world_triangles(
                regions,
                LodKey {
                    coord: key.coord + I64Vec3::new(0, dy, 0),
                    ..key
                },
            ));
        }
    }
    let seam_x = coarse.min_block().x as f32;
    let (z0, z1) = (
        fine.min_block().z as f32,
        (fine.min_block().z + fine.width() * 16) as f32,
    );
    let mut sampler = regions.sampler();
    let (mut rays, mut leaks) = (0, 0);
    for direction in [
        Vec3::new(0.35, -1.0, 0.1),
        Vec3::new(-0.35, -1.0, 0.1),
        Vec3::new(0.0, -1.0, 0.45),
        Vec3::new(0.2, -1.0, -0.3),
    ] {
        let direction = direction.normalize();
        // clear of the node's z edges, past which the next node isn't loaded here
        let mut z = z0 + 8.713;
        while z < z1 - 8.0 {
            for dx in [-2.7f32, -1.77, -0.93, 0.21, 0.83, 1.61, 2.9] {
                let x = seam_x + dx;
                let y = sampler.height(x as i64, z as i64) as f32;
                // from well above any hill the ray might otherwise start inside of
                let from = Vec3::new(x, y, z) - direction * (300.0 / -direction.y);
                rays += 1;
                if !triangles.iter().any(|&t| ray_hits(from, direction, t)) {
                    leaks += 1;
                }
            }
            z += 2.07;
        }
    }
    (leaks, rays)
}

#[test]
fn test_nothing_is_seen_through_a_seam_between_lod_levels() {
    let regions = Arc::new(RegionStore::new(Arc::new(NoiseGenerator::new(5))));
    let mut sampler = regions.sampler();
    // stretches of land a few thousand blocks apart, with a fine node beside a coarser one
    let spots: Vec<(i64, i64)> = (0..8)
        .map(|i| {
            (
                -3000 + (i % 4) * 1900 + (i * 137) % 400,
                -3000 + (i / 4) * 1900 + (i * 211) % 400,
            )
        })
        .collect();
    for fine_level in 1..=4u8 {
        let (mut leaks, mut rays) = (0, 0);
        for &(bx, bz) in &spots {
            let h = sampler.height(bx, bz) as i64;
            let coarse = LodKey::containing(
                ChunkCoordinate(I64Vec3::new(bx, h, bz).div_euclid(I64Vec3::splat(16))),
                fine_level + 1,
            );
            let fine_chunk = I64Vec3::new(coarse.min_block().x - 8, h, coarse.min_block().z + 8)
                .div_euclid(I64Vec3::splat(16));
            let fine = LodKey::containing(ChunkCoordinate(fine_chunk), fine_level);
            let (l, r) = seam_leaks(&regions, fine, coarse);
            leaks += l;
            rays += r;
        }
        assert_eq!(
            0,
            leaks,
            "{leaks} of {rays} rays slipped through seams between levels {fine_level} and {}",
            fine_level + 1
        );
    }
}

#[test]
fn test_ground_a_block_lower_than_a_surface_cell_above_is_still_lidded() {
    // here full detail has the ground a block under the level 1 cell the node above draws it in
    let regions = Arc::new(RegionStore::new(Arc::new(NoiseGenerator::new(2775841323))));
    let key = LodKey {
        level: 1,
        coord: I64Vec3::new(6, 13, 4),
    };
    let meshes = super::lod::generate_lod_mesh(regions, key, None).unwrap();
    let origin = key.min_block().as_vec3() + Vec3::splat(0.5 * 2.0 - 0.5);
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) =
        meshes.opaque.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!()
    };
    let Some(Indices::U32(idx)) = meshes.opaque.indices() else {
        panic!()
    };
    let (px, pz) = (198.7f32, 127.7f32);
    let lidded = idx.chunks(3).any(|t| {
        let q = [0, 1, 2].map(|j| origin + Vec3::from(pos[t[j] as usize]) * 2.0);
        let e = |a: Vec3, b: Vec3| (px - b.x) * (a.z - b.z) - (a.x - b.x) * (pz - b.z);
        let d = [e(q[0], q[1]), e(q[1], q[2]), e(q[2], q[0])];
        q.iter().all(|v| v.y == 447.5)
            && !(d.iter().any(|&x| x < 0.0) && d.iter().any(|&x| x > 0.0))
    });
    assert!(lidded);
}

#[test]
fn test_lod_non_air_only_covers_interior() {
    let lod = flat_lod(8);
    let cells = lod.non_air();
    assert_eq!(16 * 16 * 8, cells.len());
    assert!(cells
        .iter()
        .all(|(c, _)| c.min_element() >= 0 && c.max_element() < 16));
}

#[test]
fn test_an_edit_remeshes_the_chunks_its_faces_and_light_reach() {
    let remeshed = |x, y, z| -> Vec<I64Vec3> {
        chunks_meshed_with(I64Vec3::new(x, y, z), false)
            .map(|c| c.0)
            .collect()
    };
    // inside a chunk: it, the chunk below, whose light falls through it, and those beside whose
    // light spreads in from as far
    let inside = remeshed(4, 4, 4);
    assert_eq!(8, inside.len());
    for chunk in [I64Vec3::ZERO, I64Vec3::NEG_Y, I64Vec3::new(-1, 0, -1)] {
        assert!(inside.contains(&chunk), "{chunk} in {inside:?}");
    }
    assert!(!inside.contains(&I64Vec3::Y) && !inside.contains(&I64Vec3::X));
    // in a top corner, the chunks it touches across that corner too
    let corner = remeshed(15, 15, 15);
    assert_eq!(12, corner.len());
    assert!(corner.contains(&I64Vec3::ONE) && corner.contains(&I64Vec3::new(0, -1, 0)));
    // and in a bottom corner, the chunks below and behind it
    let corner = remeshed(-16, -16, -16);
    assert_eq!(8, corner.len());
    assert!(corner.contains(&I64Vec3::splat(-1)) && corner.contains(&I64Vec3::splat(-2)));
}

#[test]
fn test_digging_a_shaft_through_the_chunk_above_lights_a_cave_below() {
    let stone = || ChunkData::from_cells(std::iter::repeat_n(BlockType::Stone, CELLS));
    // a pocket of air in the top of a chunk of stone, under another
    let mut below = stone();
    below.set_block_at(U16Vec3::new(8, 15, 8), BlockType::Air);
    let below = Arc::new(below);
    let coord = ChunkCoordinate(I64Vec3::new(0, 40, 0));
    let lit = |above: ChunkData| {
        let mut neighbours = vec![None; 26];
        neighbours[neighbor_26_index(0, 1, 0)] = Some(Arc::new(above));
        let mesh = generate_chunk_mesh(
            below.clone(),
            neighbours,
            coord,
            &NoiseGenerator::new(3),
            false,
        );
        let floor = top_lights(&mesh.opaque);
        assert!(!floor.is_empty());
        floor.iter().map(|(_, l)| *l).fold(0.0, f32::max)
    };
    let dark = lit(stone());
    let mut shaft = stone();
    for y in 0..16 {
        shaft.set_block_at(U16Vec3::new(8, y, 8), BlockType::Air);
    }
    let open = lit(shaft);
    assert!(
        dark < 0.1 && open > 0.9,
        "{dark} under stone, {open} under the shaft"
    );
    // the chunk below is one an edit in the shaft remeshes
    assert!(chunks_meshed_with(I64Vec3::new(8, 41 * 16 + 3, 8), false).any(|c| c == coord));
}

// Full-detail blocks anywhere, generating chunks as they're needed.
struct Generated {
    regions: Arc<RegionStore>,
    chunks: HashMap<I64Vec3, ChunkData>,
}

impl Generated {
    fn new(seed: u32) -> Self {
        Self {
            regions: Arc::new(RegionStore::new(Arc::new(NoiseGenerator::new(seed)))),
            chunks: HashMap::new(),
        }
    }

    fn chunk(&mut self, coord: I64Vec3) -> &ChunkData {
        let regions = &self.regions;
        self.chunks
            .entry(coord)
            .or_insert_with(|| generate_chunk(regions.clone(), ChunkCoordinate(coord)))
    }

    fn block(&mut self, block: I64Vec3) -> BlockType {
        let coord = block.div_euclid(I64Vec3::splat(16));
        self.chunk(coord)
            .get_block_at((block - coord * 16).as_u16vec3())
    }

    // The rounded height of the ground's top in each column of the chunks at chunk `x`, `z`.
    fn ground(&self, x: i64, z: i64) -> impl Fn(i64, i64) -> i64 {
        let columns = self
            .regions
            .chunk_columns(&mut self.regions.sampler(), x, z);
        move |cx, cz| super::ground_top(columns.height(cx, cz))
    }
}

#[test]
fn test_caves_are_the_same_whichever_order_chunks_come_in() {
    let coords: Vec<I64Vec3> = (0..60)
        .map(|i| I64Vec3::new(i % 4 - 2, 10 + i / 4 % 18, i / 12 - 3))
        .collect();
    let (mut forwards, mut backwards) = (Generated::new(7), Generated::new(7));
    for &coord in &coords {
        forwards.chunk(coord);
    }
    let mut carved = 0;
    for &coord in coords.iter().rev() {
        let (a, b) = (forwards.chunk(coord), backwards.chunk(coord));
        assert_eq!(a.roofed, b.roofed);
        let blocks = |chunk: &ChunkData| chunk.iter_non_air().collect::<Vec<_>>();
        assert_eq!(blocks(a), blocks(b), "chunk {coord}");
        carved += (a.iter_non_air().count() < 4096 && !a.empty()) as usize;
    }
    assert!(carved > 0);
}

#[test]
fn test_caves_open_up_underground() {
    let mut generated = Generated::new(1);
    let (x, z, _) = generated.regions.noise().find_spawn();
    let size = 16;
    let (mut air, mut blocks) = (0, 0);
    for cx in x / size - 4..x / size + 4 {
        for cz in z / size - 4..z / size + 4 {
            // from the ground's lowest point down, a few hundred blocks
            let ground = generated.ground(cx, cz);
            let lowest = (0..size).map(|c| ground(c, c)).min().unwrap() / size;
            for cy in lowest - 24..lowest - 1 {
                let chunk = generated.chunk(I64Vec3::new(cx, cy, cz));
                air += 4096 - chunk.iter_non_air().count();
                blocks += 4096;
            }
        }
    }
    let share = air as f64 / blocks as f64;
    assert!(
        (0.01..0.1).contains(&share),
        "{share} of the ground is caves"
    );
}

#[test]
fn test_no_cave_opens_under_or_beside_the_sea() {
    let mut generated = Generated::new(1);
    let (x, z, _) = generated.regions.noise().find_spawn();
    let size = 16;
    let sea_roof = 12;
    let (mut wet, mut carved) = (0, 0);
    for cx in x / size - 8..x / size + 8 {
        for cz in z / size - 8..z / size + 8 {
            let ground = generated.ground(cx, cz);
            for (lx, lz) in (0..size).flat_map(|lx| (0..size).map(move |lz| (lx, lz))) {
                let column_ground = ground(lx, lz);
                for y in WATER_LEVEL - 64..WATER_LEVEL {
                    let block = I64Vec3::new(cx * size + lx, y, cz * size + lz);
                    if generated.block(block) != BlockType::Air {
                        continue;
                    }
                    carved += 1;
                    if column_ground < WATER_LEVEL {
                        wet += 1;
                        assert!(
                            y < column_ground - sea_roof,
                            "{block} is near the sea floor"
                        );
                    }
                    for side in [IVec3::X, IVec3::NEG_X, IVec3::Z, IVec3::NEG_Z, IVec3::Y] {
                        let beside = generated.block(block + side.as_i64vec3());
                        assert!(
                            !matches!(beside, BlockType::Water | BlockType::Ice),
                            "{block} is beside {beside:?}"
                        );
                    }
                }
            }
        }
    }
    // the test is only worth anything with caves under the sea, and others beside it
    assert!(wet > 0 && carved > wet, "{wet} under the sea of {carved}");
}

#[test]
fn test_sky_light_stays_out_of_ground_with_caves_in_it() {
    // a chunk and the one over it, both hollow, under ground that roofs them or not, above the sea
    let light_inside = |roofed: bool| {
        let mut over = ChunkData::default();
        if roofed {
            for (x, z) in (0..16).flat_map(|x| (0..16).map(move |z| (x, z))) {
                over.roofed.insert(x, z);
            }
        }
        let over = Arc::new(over);
        let chunk = ChunkData::default();
        let mut neighbours = no_neighbors();
        for (dx, dz) in (-1..=1).flat_map(|dx| (-1..=1).map(move |dz| (dx, dz))) {
            neighbours[neighbor_26_index(dx, 1, dz)] = Some(over.clone());
        }
        let source = ChunkNeighbourhood::new(&chunk, &neighbours);
        let light =
            super::chunk::neighbourhood_light(&source, ChunkCoordinate(I64Vec3::new(0, 40, 0)));
        light.sky.level(IVec3::splat(8))
    };
    assert_eq!(MAX_LIGHT, light_inside(false));
    assert_eq!(0, light_inside(true));
}

#[test]
fn test_columns_are_roofed_only_under_ground_and_open_down_cave_mouths() {
    let mut generated = Generated::new(1);
    let (x, z, _) = generated.regions.noise().find_spawn();
    let size = 16;
    let mut mouths = 0;
    for cx in x / size - 20..x / size + 20 {
        for cz in z / size - 20..z / size + 20 {
            let columns = generated
                .regions
                .chunk_columns(&mut generated.regions.sampler(), cx, cz);
            for (lx, lz) in (0..size).flat_map(|lx| (0..size).map(move |lz| (lx, lz))) {
                let ground = super::ground_top(columns.height(lx, lz));
                if columns.sky_reach(lx, lz) == ground {
                    continue;
                }
                mouths += 1;
                // down from the surface, chunks are roofed once there's ground over them
                let (wx, wz) = (cx * size + lx, cz * size + lz);
                let mut covered = false;
                for y in (ground - 100..ground).rev() {
                    let block = generated.block(I64Vec3::new(wx, y, wz));
                    // trees can grow into a mouth, but aren't ground
                    covered |=
                        !matches!(block, BlockType::Air | BlockType::Leaves | BlockType::Log);
                    if y.rem_euclid(size) == 0 {
                        let below = I64Vec3::new(cx, y / size - 1, cz);
                        assert_eq!(
                            covered,
                            generated.chunk(below).roofed.contains(lx as u16, lz as u16),
                            "column ({wx}, {wz}) over chunk layer {}",
                            below.y
                        );
                    }
                }
            }
        }
    }
    assert!(mouths > 0);
}

#[test]
fn test_a_shaft_dug_down_into_a_cave_lights_it_until_its_roofed_over() {
    use crate::chunks::chunk::ColumnSet;
    use crate::world::World;
    // a cave in the top of a chunk 30 layers up, under two more layers of stone, all roofed as
    // generated ground would be, with stone all round
    let mut world = World::new(16);
    for (x, y, z) in
        (-1..=1).flat_map(|x| (29..=32).flat_map(move |y| (-1..=1).map(move |z| (x, y, z))))
    {
        let mut data = ChunkData::from_cells(std::iter::repeat_n(BlockType::Stone, CELLS));
        if y < 32 {
            data.roofed = ColumnSet::ALL;
        }
        if (x, y, z) == (0, 30, 0) {
            data.set_block_at(U16Vec3::new(8, 15, 8), BlockType::Air);
        }
        world.insert_chunk(ChunkCoordinate(I64Vec3::new(x, y, z)), data);
    }
    let cave = ChunkCoordinate(I64Vec3::new(0, 30, 0));
    let floor_light = |world: &World| {
        let mesh = generate_chunk_mesh(
            world.get_chunk_data(cave).unwrap(),
            world.adjacent_chunk_data(cave),
            cave,
            &NoiseGenerator::new(3),
            false,
        );
        top_lights(&mesh.opaque)
            .iter()
            .map(|(_, light)| *light)
            .fold(0.0, f32::max)
    };
    assert!(floor_light(&world) < 0.1);
    let in_the_cave = I64Vec3::new(8, 30 * 16 + 15, 8);
    assert!(world.is_under_ground(in_the_cave));

    let shaft = (31 * 16..33 * 16).map(|y| I64Vec3::new(8, y, 8));
    for block in shaft.clone().rev() {
        world.edit(block, BlockType::Air);
    }
    assert!(!world.get_chunk_data(cave).unwrap().roofed.contains(8, 8));
    // the chunks lit through the opened columns are the ones to mesh again
    assert!(world.take_reroofed().iter().any(|r| r.chunk == cave));
    assert!(floor_light(&world) > 0.9);
    assert!(!world.is_under_ground(in_the_cave));

    world.edit(I64Vec3::new(8, 33 * 16 - 1, 8), BlockType::Stone);
    assert!(world.get_chunk_data(cave).unwrap().roofed.contains(8, 8));
    assert!(floor_light(&world) < 0.1);
}

fn draws(mesh: &Mesh) -> Vec<f32> {
    let Some(VertexAttributeValues::Float32x2(modes)) = mesh.attribute(Mesh::ATTRIBUTE_UV_1) else {
        panic!("missing uv_b");
    };
    modes.iter().map(|m| m[1]).collect()
}

#[test]
fn test_a_plant_is_two_crossed_quads_seen_from_both_sides() {
    let mut chunk = ChunkData::default();
    chunk.set_block_at(U16Vec3::new(4, 4, 4), BlockType::Poppy);
    let mesh = mesh_chunk(chunk).opaque;
    assert_eq!(8, mesh.count_vertices());
    assert_eq!(Some(24), mesh.indices().map(Indices::len));
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        panic!("missing positions");
    };
    for p in positions {
        // on one diagonal of the cell or the other, from its floor to its ceiling
        let (dx, dz) = (p[0] - 4.0, p[2] - 4.0);
        assert!(
            (dx.abs() - dz.abs()).abs() < 1e-5 && dx.abs() < 0.5,
            "{p:?}"
        );
        assert!(p[1] == 3.5 || p[1] == 4.5, "{p:?}");
    }
    assert!(draws(&mesh).iter().all(|&d| d == super::buffers::PLANT));
    // each quad twice, wound both ways
    let Some(Indices::U32(indices)) = mesh.indices() else {
        panic!("missing indices");
    };
    let normal = |t: &[u32]| {
        let p = |i: u32| Vec3::from(positions[i as usize]);
        (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).normalize()
    };
    let windings: Vec<Vec3> = indices.chunks(6).map(normal).collect();
    assert_eq!(windings[0], -windings[1]);
    assert_eq!(windings[2], -windings[3]);
}

#[test]
fn test_plants_hide_none_of_their_neighbours_faces() {
    let mut stones = ChunkData::default();
    stones.set_block_at(U16Vec3::new(4, 4, 4), BlockType::Stone);
    stones.set_block_at(U16Vec3::new(6, 4, 4), BlockType::Stone);
    let mut planted = stones.clone();
    planted.set_block_at(U16Vec3::new(5, 4, 4), BlockType::TallGrass);
    planted.set_block_at(U16Vec3::new(4, 5, 4), BlockType::Dandelion);
    assert_eq!(2 * 24 + 2 * 8, mesh_vertices(planted));
    assert_eq!(2 * 24, mesh_vertices(stones));
}

#[test]
fn test_tall_grass_is_tinted_and_flowers_are_not() {
    for (plant, tinted) in [(BlockType::TallGrass, true), (BlockType::Cornflower, false)] {
        let mut chunk = ChunkData::default();
        chunk.set_block_at(U16Vec3::new(4, 4, 4), plant);
        let colours = tints(&mesh_chunk(chunk).opaque);
        assert!(
            colours.iter().all(|&c| (c != Vec3::ONE) == tinted),
            "{plant:?}"
        );
    }
}

#[test]
fn test_a_plant_takes_the_light_of_its_cell() {
    let mut chunk = ChunkData::default();
    chunk.set_block_at(U16Vec3::new(4, 4, 4), BlockType::TallGrass);
    let open = mesh_chunk(chunk.clone()).opaque;
    // and under a roof, in the shade
    for x in 0..16 {
        for z in 0..16 {
            chunk.set_block_at(U16Vec3::new(x, 6, z), BlockType::Stone);
        }
    }
    let roofed = mesh_chunk(chunk).opaque;
    let light = |mesh: &Mesh, plant: usize| {
        let Some(VertexAttributeValues::Float32x2(modes)) = mesh.attribute(Mesh::ATTRIBUTE_UV_1)
        else {
            panic!("missing uv_b");
        };
        let plants: Vec<f32> = modes
            .iter()
            .filter(|m| m[1] == super::buffers::PLANT)
            .map(|m| m[0])
            .collect();
        assert_eq!(plant, plants.len());
        plants[0]
    };
    assert_eq!(1.0, light(&open, 8));
    assert!(light(&roofed, 8) < 1.0);
}

#[test]
fn test_coarse_lods_leave_plants_out() {
    let plant = (IVec3::new(4, 4, 4), BlockType::TallGrass);
    assert_eq!(0, mesh_lod(&lod_with(&[plant])).count_vertices());
}

// The vertices drawing `tile`, by position, with their sky light and block light.
fn tile_vertices(mesh: &Mesh, tile: Tile) -> Vec<([f32; 3], [f32; 2])> {
    let attribute = |id| match mesh.attribute(id) {
        Some(VertexAttributeValues::Float32x3(values)) => {
            values.iter().map(|v| v.to_vec()).collect()
        }
        Some(VertexAttributeValues::Float32x4(values)) => {
            values.iter().map(|v| v.to_vec()).collect()
        }
        Some(VertexAttributeValues::Float32x2(values)) => {
            values.iter().map(|v| v.to_vec()).collect()
        }
        _ => Vec::<Vec<f32>>::new(),
    };
    let (positions, colours) = (
        attribute(Mesh::ATTRIBUTE_POSITION),
        attribute(Mesh::ATTRIBUTE_COLOR),
    );
    let (lights, blocks) = (
        attribute(Mesh::ATTRIBUTE_UV_1),
        attribute(Mesh::ATTRIBUTE_TANGENT),
    );
    (0..positions.len())
        .filter(|&i| colours[i][3] == tile.atlas_cell())
        .map(|i| {
            let block = blocks.get(i).map_or(0.0, |b| b[0]);
            (
                [positions[i][0], positions[i][1], positions[i][2]],
                [lights[i][0], block],
            )
        })
        .collect()
}

#[test]
fn test_a_torch_is_a_thin_stick_standing_on_the_floor_lit_by_its_own_flame() {
    let mut chunk = floor_chunk();
    chunk.set_block_at(U16Vec3::new(4, 1, 6), BlockType::Torch);
    let torch = tile_vertices(&mesh_chunk(chunk).opaque, Tile::Torch);
    // four sides and a top, but no bottom, which stands on the stone
    assert_eq!(5 * 4, torch.len());
    let (min, max) = torch
        .iter()
        .fold((Vec3::MAX, Vec3::MIN), |(min, max), (p, _)| {
            (min.min(Vec3::from(*p)), max.max(Vec3::from(*p)))
        });
    let half = 1.0 / 16.0;
    assert_eq!(Vec3::new(4.0 - half, 0.5, 6.0 - half), min);
    assert_eq!(Vec3::new(4.0 + half, 0.5 + 10.0 / 16.0, 6.0 + half), max);
    let lit = 14.0 / MAX_LIGHT as f32;
    assert!(
        torch.iter().all(|(_, light)| *light == [1.0, lit]),
        "{torch:?}"
    );
}

#[test]
fn test_coarse_lods_leave_torches_out() {
    let torch = (IVec3::new(4, 4, 4), BlockType::Torch);
    assert_eq!(0, mesh_lod(&lod_with(&[torch])).count_vertices());
}

#[test]
fn test_torchlight_crosses_into_the_next_chunk_alongside_sky_light() {
    // floors under open sky, with a torch on the one to +x, 2 blocks from the border
    let floor = || floor_chunk();
    let mut beside = floor();
    beside.set_block_at(U16Vec3::new(1, 1, 8), BlockType::Torch);
    let mesh = |beside: ChunkData| {
        let mut neighbours = vec![None; 26];
        neighbours[neighbor_26_index(1, 0, 0)] = Some(Arc::new(beside));
        let mesh = generate_chunk_mesh(
            Arc::new(floor()),
            neighbours,
            ChunkCoordinate(I64Vec3::new(0, 40, 0)),
            &NoiseGenerator::new(3),
            false,
        );
        mesh.opaque
    };
    let (unlit, lit) = (mesh(floor()), mesh(beside));
    // a mesh no light reaches carries none, so draws just as it did before torches
    assert!(!unlit.contains_attribute(Mesh::ATTRIBUTE_TANGENT));
    let floor = tile_vertices(&lit, Tile::Stone);
    let at = |x: f32, z: f32| {
        floor
            .iter()
            .find(|(p, _)| p[0] == x && p[1] == 0.5 && p[2] == z)
            .map(|(_, light)| *light)
            .unwrap_or_else(|| panic!("no vertex at {x}, {z}"))
    };
    // the corner nearest it averages the 4 cells over it, 1 to 3 blocks from the torch, and
    // the light falls away a level a block from there
    let level = |l: u8| l as f32 / MAX_LIGHT as f32;
    assert_eq!([1.0, level(14 - 2)], at(15.5, 8.5));
    assert_eq!([1.0, level(14 - 2 - 4)], at(11.5, 8.5));
    assert_eq!([1.0, 0.0], at(-0.5, 8.5));
    // while the sky still lights the open floor fully
    assert!(top_lights(&lit).iter().all(|(_, sky)| *sky == 1.0));
}

#[test]
fn test_plants_grow_on_the_grass_at_full_detail_and_never_in_lods() {
    let regions = test_regions();
    let (x, z, height) = regions.noise().find_spawn();
    let spawn = ChunkCoordinate::containing(I64Vec3::new(x, height as i64, z)).0;
    let mut plants = 0;
    for dx in -2..=2 {
        for dz in -2..=2 {
            for dy in -1..=1 {
                let coord = spawn + I64Vec3::new(dx, dy, dz);
                let chunk = generate_chunk(regions.clone(), ChunkCoordinate(coord));
                let below = generate_chunk(regions.clone(), ChunkCoordinate(coord - I64Vec3::Y));
                let lod = LodBlocks::sample(&regions, LodKey { level: 0, coord }, None);
                for (at, block) in chunk.iter_non_air().filter(|(_, b)| b.is_plant()) {
                    plants += 1;
                    let ground = match at.y {
                        0 => below.get_block_at(at.with_y(15)),
                        y => chunk.get_block_at(at.with_y(y - 1)),
                    };
                    assert_eq!(BlockType::Grass, ground, "{block:?} at {at} in {coord}");
                    assert_eq!(BlockType::Air, lod.raw(at.as_ivec3()));
                }
                // and generated the same again
                let again = generate_chunk(regions.clone(), ChunkCoordinate(coord));
                assert!(again.iter_non_air().eq(chunk.iter_non_air()));
            }
        }
    }
    assert!(plants > 20, "{plants} plants");
}

// The `count` structures nearest seed 3's spawn.
fn structures_near_spawn(
    regions: &RegionStore,
    count: usize,
) -> Vec<crate::chunks::generate::structures::Placed> {
    use bevy::math::{I64Vec2, Vec3Swizzles};
    let (x, z, _) = regions.noise().find_spawn();
    let spawn = I64Vec2::new(x, z);
    let mut found = regions.structures_near(spawn - 3000, spawn + 3000);
    found.sort_by_key(|p| (p.min.xz() - spawn).length_squared());
    found.truncate(count);
    assert_eq!(count, found.len());
    found
}

// Every chunk holding any of the blocks `min..max`.
fn chunks_over(min: I64Vec3, max: I64Vec3) -> Vec<I64Vec3> {
    let (first, last) = (
        min.div_euclid(I64Vec3::splat(16)),
        (max - 1).div_euclid(I64Vec3::splat(16)),
    );
    (first.x..=last.x)
        .flat_map(|x| {
            (first.y..=last.y)
                .flat_map(move |y| (first.z..=last.z).map(move |z| I64Vec3::new(x, y, z)))
        })
        .collect()
}

#[test]
fn test_structures_stand_on_their_foundations_cleared_inside_with_no_trees_or_plants() {
    let mut world = Generated::new(3);
    let regions = world.regions.clone();
    let mut filled = 0;
    for placed in structures_near_spawn(&regions, 6) {
        let template = &placed.structure().template;
        let corner = placed.min - I64Vec3::Y * template.floor;
        let footprint = placed.footprint();
        let top = placed.bounds().1.y;
        for x in 0..footprint.x {
            for z in 0..footprint.y {
                let column = corner + I64Vec3::new(x, 0, z);
                let (cx, cz) = (column.x.div_euclid(16), column.z.div_euclid(16));
                let ground = world.ground(cx, cz)(column.x.rem_euclid(16), column.z.rem_euclid(16));
                let wanted =
                    |y: i64| template.cell(placed.rotation, I64Vec3::new(x, y - corner.y, z));
                // filled from the ground up to its floor
                for y in ground.min(corner.y)..placed.min.y {
                    let block = world.block(column.with_y(y));
                    match wanted(y) {
                        Some(cell) => assert_eq!(cell, block, "{column} at {y}"),
                        None if y >= ground => {
                            assert_eq!(template.foundation, block, "{column} at {y}");
                            filled += 1;
                        }
                        None => {}
                    }
                }
                // and over it, its own blocks with nothing else among them: the ground and what
                // grew on it cleared away, and no tree reaching in
                for y in placed.min.y..top + 8 {
                    let block = world.block(column.with_y(y));
                    assert_eq!(
                        wanted(y).unwrap_or(BlockType::Air),
                        block,
                        "{column} at {y}"
                    );
                }
            }
        }
        // the clearing round it has no trees reaching into it
        for x in -2..footprint.x + 2 {
            for z in -2..footprint.y + 2 {
                for y in placed.min.y..top + 8 {
                    let block = world.block(corner + I64Vec3::new(x, y - corner.y, z));
                    assert!(
                        !matches!(block, BlockType::Leaves | BlockType::Log),
                        "{block:?}"
                    );
                }
            }
        }
    }
    // some stand where the ground falls away under their footing, and fill it
    assert!(filled > 0);
}

#[test]
fn test_structures_are_the_same_whichever_order_chunks_come_in() {
    let (mut forwards, mut backwards) = (Generated::new(3), Generated::new(3));
    let placed = structures_near_spawn(&forwards.regions, 1)[0];
    let (low, high) = placed.bounds();
    let coords = chunks_over(low - 16, high + 16);
    for &coord in &coords {
        forwards.chunk(coord);
    }
    for &coord in coords.iter().rev() {
        let (a, b) = (forwards.chunk(coord), backwards.chunk(coord));
        assert!(a.iter_non_air().eq(b.iter_non_air()), "chunk {coord}");
        assert_eq!(a.roofed, b.roofed);
    }
}

#[test]
fn test_lods_draw_structures_as_full_detail_has_them() {
    use super::lod::STRUCTURE_MAX_CELL;
    use super::terrain::is_solid;
    let mut world = Generated::new(3);
    let regions = world.regions.clone();
    for placed in structures_near_spawn(&regions, 4) {
        let (low, high) = placed.bounds();
        // level 0 is full detail itself, caves aside, and they keep under structures
        for coord in chunks_over(low, high) {
            let lod = LodBlocks::sample(&regions, LodKey { level: 0, coord }, None);
            for x in 0..16 {
                for z in 0..16 {
                    for y in 0..16 {
                        let at = coord * 16 + I64Vec3::new(x, y, z);
                        if placed.covers(at.x, at.z) && at.y >= low.y {
                            let cell = lod.raw(IVec3::new(x as i32, y as i32, z as i32));
                            assert_eq!(world.block(at), cell, "{at}");
                        }
                    }
                }
            }
        }
        // in cells two blocks wide, solid where at least half the blocks are, over the ground
        let above = placed.highest + 4;
        let mut solid_cells = 0;
        for coord in chunks_over(low, high)
            .into_iter()
            .map(|c| c.div_euclid(I64Vec3::splat(2)))
        {
            let key = LodKey { level: 1, coord };
            let lod = LodBlocks::sample(&regions, key, None);
            let base = key.min_block();
            for x in 0..16 {
                for z in 0..16 {
                    for y in 0..16 {
                        let min = base + I64Vec3::new(x, y, z) * 2;
                        let over = |c: i64, from: i64, to: i64| c + 2 > from && c < to;
                        if !(over(min.x, low.x, high.x) && over(min.z, low.z, high.z))
                            || min.y < above
                            || min.y >= high.y
                        {
                            continue;
                        }
                        let solid = (0..8)
                            .map(|i| min + I64Vec3::new(i & 1, (i >> 1) & 1, i >> 2))
                            .filter(|&b| is_solid(world.block(b)))
                            .count();
                        let cell = lod.raw(IVec3::new(x as i32, y as i32, z as i32));
                        assert_eq!(solid >= 4, is_solid(cell), "cell at {min} has {solid} of 8");
                        solid_cells += is_solid(cell) as usize;
                    }
                }
            }
        }
        assert!(solid_cells > 20, "only {solid_cells} solid cells");
        // and from cells four wide, as boxes as tall and as wide as it is
        let template = &placed.structure().template;
        let corner = placed.min - I64Vec3::Y * template.floor;
        let tallest = (0..template.size.y)
            .filter(|&y| {
                (0..template.size.x).any(|x| {
                    (0..template.size.z).any(|z| {
                        template
                            .cell(0, I64Vec3::new(x, y, z))
                            .is_some_and(|b| b != BlockType::Air)
                    })
                })
            })
            .max()
            .unwrap();
        let chunk = ChunkCoordinate::containing(placed.min);
        for level in 2..=6 {
            let key = LodKey::containing(chunk, level);
            let (base, cell) = (key.min_block().as_vec3(), key.cell_size() as f32);
            let lod = LodBlocks::sample(&regions, key, None);
            let boxes: Vec<_> = lod
                .boxes
                .iter()
                .filter(|b| matches!(b.block, BlockType::Cobblestone | BlockType::StoneBricks))
                .map(|b| ((b.min + 0.5) * cell + base, (b.max + 0.5) * cell + base))
                // a coarse node can hold another structure besides
                .filter(|(min, max)| {
                    let (low, high) = (placed.min.xz(), placed.min.xz() + placed.footprint());
                    min.x < high.x as f32
                        && max.x > low.x as f32
                        && min.z < high.y as f32
                        && max.z > low.y as f32
                })
                .collect();
            if key.cell_size() > STRUCTURE_MAX_CELL {
                assert!(boxes.is_empty(), "level {level}");
                continue;
            }
            let (min, max) = boxes
                .iter()
                .fold((Vec3::MAX, Vec3::MIN), |(lo, hi), (a, b)| {
                    (lo.min(*a), hi.max(*b))
                });
            let footprint = placed.footprint();
            let wanted_max = Vec3::new(
                (placed.min.x + footprint.x) as f32,
                (corner.y + tallest + 1) as f32,
                (placed.min.z + footprint.y) as f32,
            );
            assert!(
                (max - wanted_max).abs().max_element() < 1e-3,
                "level {level}: {max} not {wanted_max}"
            );
            assert!(
                (min.x - placed.min.x as f32).abs() < 1e-3
                    && (min.z - placed.min.z as f32).abs() < 1e-3
            );
            assert!(
                min.y <= corner.y as f32,
                "level {level} stands at {}",
                min.y
            );
        }
    }
}

#[test]
fn test_lods_draw_edits_as_full_detail_has_them_within_their_cells() {
    use crate::edits::Edits;
    use crate::world::World;
    let mut world = World::from_save(3, Edits::default(), 16);
    let regions = world.region_store.clone();
    let spawn = world.spawn().as_i64vec3();
    let ground = regions.sampler().height(spawn.x, spawn.z).round() as i64;
    // a brick tower from under the ground to high over it, not lined up with any cells, and a
    // pit dug beside it
    let tower = (spawn.with_y(ground - 10) + 5, I64Vec3::new(40, 130, 40));
    let pit = (
        spawn.with_y(ground - 34) + I64Vec3::new(-40, 0, 5),
        I64Vec3::new(34, 34, 34),
    );
    let blocks = |(min, size): (I64Vec3, I64Vec3)| {
        (0..size.x).flat_map(move |x| {
            (0..size.y).flat_map(move |y| (0..size.z).map(move |z| min + I64Vec3::new(x, y, z)))
        })
    };
    for block in blocks(tower) {
        world.edit(block, BlockType::Bricks);
    }
    for block in blocks(pit) {
        world.edit(block, BlockType::Air);
    }
    let inside = |(min, size): (I64Vec3, I64Vec3), low: I64Vec3, width: i64| {
        low.cmpge(min).all() && (low + width).cmple(min + size).all()
    };
    for level in 1..=4u8 {
        let mut drawn = (0, 0);
        let width = 1i64 << level;
        let corners = [tower.0, tower.0 + tower.1 - 1, pit.0, pit.0 + pit.1 - 1];
        let mut keys: Vec<LodKey> = corners
            .iter()
            .map(|&block| LodKey::containing(ChunkCoordinate::containing(block), level))
            .collect();
        keys.sort_by_key(|key| key.coord.to_array());
        keys.dedup();
        for key in keys {
            let edits = world.lod_edits().node(key);
            let lod = LodBlocks::sample(&regions, key, edits.as_deref());
            let unedited = LodBlocks::sample(&regions, key, None);
            for at in (0..16 * 16 * 16).map(|i| IVec3::new(i % 16, i / 16 % 16, i / 256)) {
                let low = key.min_block() + at.as_i64vec3() * width;
                let cell = lod.raw(at);
                if inside(tower, low, width) {
                    assert_eq!(BlockType::Bricks, cell, "level {level} at {low}");
                    drawn.0 += 1;
                } else if inside(pit, low, width) {
                    assert_eq!(BlockType::Air, cell, "level {level} at {low}");
                    drawn.1 += 1;
                } else if level == 1 {
                    // and on their edges, what most of the cell's blocks are, as edited
                    let (mut built, mut cleared) = (0, 0);
                    for block in blocks((low, I64Vec3::splat(2))) {
                        match world.edits().get(block) {
                            Some(BlockType::Bricks) => built += 1,
                            Some(_) => cleared += 1,
                            None => {}
                        }
                    }
                    let expected = if built >= 4 {
                        BlockType::Bricks
                    } else if built + cleared >= 4 {
                        BlockType::Air
                    } else {
                        unedited.raw(at)
                    };
                    assert_eq!(expected, cell, "level 1 at {low}");
                } else if !inside((tower.0 - width, tower.1 + 2 * width), low, width)
                    && !inside((pit.0 - width, pit.1 + 2 * width), low, width)
                {
                    // and further off, as generated
                    assert_eq!(unedited.raw(at), cell, "level {level} at {low}");
                }
            }
        }
        assert!(drawn.0 > 0 && drawn.1 > 0, "level {level}: {drawn:?}");
    }
}

// The spot on gentle lowland in `biome` nearest seed 3's centre, and the regions it's in.
fn lowland_in(biome: &str) -> (Arc<RegionStore>, bevy::math::I64Vec2) {
    use bevy::math::I64Vec2;
    let regions = test_regions();
    // out to the poles, where the taiga is
    let mut spots: Vec<_> = (-120..120)
        .flat_map(|z| (-120..120).map(move |x| I64Vec2::new(x, z) * 128))
        .collect();
    spots.sort_by_key(|p| p.length_squared());
    let spot = spots
        .into_iter()
        .find(|p| {
            // from the noise, as a region for every spot tried would take minutes
            let height = |dx: i64, dz: i64| regions.noise().get(*p + I64Vec2::new(dx, dz) * 4);
            let sample = regions.noise().sample(p.x as f64, p.y as f64);
            let above_sea = sample.height - WATER_LEVEL as f64;
            regions.noise().biomes().name(sample.biome) == Some(biome)
                && (8.0..60.0).contains(&above_sea)
                && (height(1, 0) - height(-1, 0)).abs() + (height(0, 1) - height(0, -1)).abs() < 4.0
        })
        .unwrap_or_else(|| panic!("no lowland {biome}"));
    (regions, spot)
}

const NEW_BIOMES: [&str; 3] = ["jungle", "savanna", "taiga"];

#[test]
fn test_new_biomes_grow_their_own_trees_the_same_whatever_asks() {
    use crate::chunks::generate::trees::{trees_near, TreeKind};
    use bevy::math::I64Vec2;
    for biome in NEW_BIOMES {
        let (regions, spot) = lowland_in(biome);
        let (min, max) = (spot - 200, spot + 200);
        let mut heights = regions.sampler();
        let all = trees_near(regions.noise(), &mut heights, min, max);
        let owned: Vec<_> = all
            .iter()
            .filter(|t| {
                let owner = regions
                    .noise()
                    .sample(t.base.x as f64, t.base.z as f64)
                    .biome;
                regions.noise().biomes().name(owner) == Some(biome)
            })
            .collect();
        assert!(owned.len() > 10, "only {} trees in {biome}", owned.len());
        let kinds: Vec<_> = owned.iter().map(|t| t.kind).collect();
        match biome {
            "jungle" => {
                assert!(kinds.contains(&TreeKind::Jungle));
                assert!(kinds
                    .iter()
                    .all(|k| matches!(k, TreeKind::Jungle | TreeKind::Broadleaf)));
            }
            "savanna" => assert!(kinds.iter().all(|&k| k == TreeKind::Acacia)),
            _ => assert!(kinds.iter().all(|&k| k == TreeKind::Pine)),
        }
        // quarters asked for in reverse order find the same trees, the same size
        let key =
            |t: &crate::chunks::generate::trees::Tree| (t.base.x, t.base.z, t.base.y, t.trunk);
        let mut quarters = Vec::new();
        for (dx, dz) in [(1, 1), (1, 0), (0, 1), (0, 0)] {
            let low = min + I64Vec2::new(dx, dz) * 200;
            quarters.extend(
                trees_near(regions.noise(), &mut regions.sampler(), low, low + 199)
                    .into_iter()
                    .filter(|t| t.base.x >= low.x && t.base.x < low.x + 200)
                    .filter(|t| t.base.z >= low.y && t.base.z < low.y + 200)
                    .map(|t| (key(&t), t.kind)),
            );
        }
        let mut whole: Vec<_> = all
            .iter()
            .filter(|t| (min.x..max.x).contains(&t.base.x) && (min.y..max.y).contains(&t.base.z))
            .map(|t| (key(t), t.kind))
            .collect();
        let order = |(k, _): &((i64, i64, i64, i64), TreeKind)| *k;
        whole.sort_by_key(order);
        quarters.sort_by_key(order);
        assert_eq!(whole, quarters, "{biome}");
    }
}

#[test]
fn test_new_biomes_look_the_same_from_every_level() {
    for biome in NEW_BIOMES {
        let (regions, spot) = lowland_in(biome);
        let height = regions.get_height(spot.x, spot.y) as i64;
        let chunk = ChunkCoordinate::containing(I64Vec3::new(spot.x, height, spot.y));
        // level 0 is full detail, but for plants, ores and caves
        for coord in [chunk.0, chunk.0 + I64Vec3::Y] {
            let full = generate_chunk(regions.clone(), ChunkCoordinate(coord));
            let lod = LodBlocks::sample(&regions, LodKey { level: 0, coord }, None);
            let columns = regions.chunk_columns(&mut regions.sampler(), coord.x, coord.z);
            for (x, y, z) in (0..16u16)
                .flat_map(|x| (0..16u16).flat_map(move |y| (0..16u16).map(move |z| (x, y, z))))
            {
                let at = coord * 16 + I64Vec3::new(x as i64, y as i64, z as i64);
                let carved = regions
                    .noise()
                    .caves()
                    .carves(at, columns.cave_top(x as i64, z as i64));
                // LODs draw no plants, and no ores, which are in the stone
                let block = match full.get_block_at(U16Vec3::new(x, y, z)) {
                    plant if plant.is_plant() => BlockType::Air,
                    BlockType::CoalOre | BlockType::CopperOre | BlockType::IronOre => {
                        BlockType::Stone
                    }
                    block => block,
                };
                let expected = lod.raw(IVec3::new(x as i32, y as i32, z as i32));
                if !carved {
                    assert_eq!(expected, block, "{biome} at {at}");
                }
            }
        }
        // and every coarser level grows the same trees
        let mut checked = 0;
        for level in [2u8, 3, 4, 5] {
            let (exact, coarse) = tree_spots(&regions, LodKey::containing(chunk, level));
            assert_eq!(exact, coarse, "{biome} at level {level}");
            checked += exact.len();
        }
        assert!(checked > 0, "no trees checked in {biome}");
    }
}

#[test]
fn test_ferns_grow_in_jungle_and_taiga_but_not_savanna() {
    use crate::chunks::generate::plants::plant_on;
    for biome in NEW_BIOMES {
        let (regions, spot) = lowland_in(biome);
        let plants: Vec<_> = (0..64)
            .flat_map(|x| (0..64).map(move |z| (x, z)))
            .filter_map(|(x, z)| {
                plant_on(
                    regions.noise(),
                    spot.x + x,
                    spot.y + z,
                    BlockType::Grass,
                    0.2,
                )
            })
            .collect();
        let ferns = plants.iter().filter(|&&p| p == BlockType::Fern).count();
        assert_eq!(biome != "savanna", ferns > 0, "{ferns} ferns in {biome}");
        assert!(
            plants.contains(&BlockType::TallGrass),
            "no grass in {biome}"
        );
    }
}
