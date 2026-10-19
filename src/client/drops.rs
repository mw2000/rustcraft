//! Drawing the items lying in the world, which the server moves (`server::drops`): each a little
//! cube of its block, or the picture of an item that isn't one given a pixel's thickness,
//! spinning and bobbing over the spot. Every one of an item shares a mesh, and all blocks share a material and all pictures
//! another, so Bevy draws them batched.

use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};

use super::model_light::{ModelLook, ModelMaterial, ModelMaterials};
use crate::block::{BlockType, Face, TILE_COUNT};
use crate::clouds::spawn_clouds;
use crate::inventory::icons::{load_icons, ItemIcons};
use crate::items::Item;
use crate::server::drops::{DroppedItem, SIZE};

// Turns a second, and how far, in blocks, and how fast, in bobs a second, it rises and falls.
const SPIN_SPEED: f32 = 0.3;
const BOB_HEIGHT: f32 = 0.08;
const BOB_SPEED: f32 = 0.5;
// Texels in from a tile's edge its faces sample, so they never take in the next tile's.
const TILE_INSET: f32 = 0.5 / 16.0;

/// Draws dropped items. Needs the renderer, and `HotbarPlugin` for the block atlas.
pub struct DrawDropsPlugin;

impl Plugin for DrawDropsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_material.after(load_icons).after(spawn_clouds))
            .add_observer(draw_drop)
            .add_systems(Update, spin_and_bob);
    }
}

/// What dropped items, and those players hold, are drawn with: the block atlas and the items'
/// one, in the world and in view (the first-person hand's), which pixels of each item's
/// picture are drawn, and a mesh for each item, made the first time one's drawn.
#[derive(Resource)]
pub(super) struct DropLooks {
    material: Handle<ModelMaterial>,
    sprites: Handle<ModelMaterial>,
    pub(super) in_view: [Handle<ModelMaterial>; 2],
    // a row of bits for each row of each picture, from the top, the lowest bit leftmost
    drawn: Vec<[u16; 16]>,
    meshes: HashMap<Item, Handle<Mesh>>,
    grass: LinearRgba,
}

impl DropLooks {
    /// The mesh and material `item` is drawn with, `SIZE` across, in view or in the world.
    pub(super) fn of(
        &mut self,
        item: Item,
        in_view: bool,
        meshes: &mut Assets<Mesh>,
    ) -> (Handle<Mesh>, Handle<ModelMaterial>) {
        let (grass, drawn) = (self.grass, &self.drawn);
        let mesh = self
            .meshes
            .entry(item)
            .or_insert_with(|| match (item.block(), item.sprite()) {
                (Some(block), _) => meshes.add(cube(block, grass)),
                (None, sprite) => {
                    let sprite = sprite.expect("an item has a block or a sprite");
                    meshes.add(picture(sprite, &drawn[sprite]))
                }
            })
            .clone();
        let block = item.block().is_some();
        let material = match (in_view, block) {
            (false, true) => &self.material,
            (false, false) => &self.sprites,
            (true, true) => &self.in_view[0],
            (true, false) => &self.in_view[1],
        };
        (mesh, material.clone())
    }
}

// The cube drawn for a dropped item, a child of it so the server's `Transform` is left alone,
// and where it is in its spin and bob.
#[derive(Component)]
struct Spinning {
    phase: f32,
}

fn make_material(
    mut commands: Commands,
    icons: Res<ItemIcons>,
    images: Res<Assets<Image>>,
    mut materials: ModelMaterials,
) {
    // plants' tiles, and the items' pictures, have holes
    let mut look = |picture: &Handle<Image>, in_view| {
        materials.add(ModelLook {
            texture: Some(picture.clone()),
            alpha_mode: AlphaMode::Mask(0.5),
            in_view,
            ..ModelLook::lit()
        })
    };
    let (blocks, items) = (icons.atlas(), icons.items());
    let (material, sprites) = (look(blocks, false), look(items, false));
    let in_view = [look(blocks, true), look(items, true)];
    // read from the picture as it's loaded, before it's sent to the GPU and let go of here
    let picture = images.get(items).expect("the items' pictures are loaded");
    let width = picture.width() as usize;
    let data = picture
        .data
        .as_deref()
        .expect("the items' pictures are kept until drawn");
    let drawn = (0..Item::sprites())
        .map(|sprite| {
            std::array::from_fn(|y| {
                (0..16).fold(0, |row, x| {
                    let alpha = data[(y * width + sprite * 16 + x) * 4 + 3];
                    row | u16::from(alpha >= 128) << x
                })
            })
        })
        .collect();
    commands.insert_resource(DropLooks {
        material,
        sprites,
        in_view,
        drawn,
        meshes: HashMap::new(),
        grass: icons.grass().into(),
    });
}

fn draw_drop(
    add: On<Add<DroppedItem>>,
    items: Query<&DroppedItem>,
    mut looks: ResMut<DropLooks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    let Ok(dropped) = items.get(add.entity) else {
        return;
    };
    let (mesh, material) = looks.of(dropped.stack.item, false, &mut meshes);
    // the golden angle, so items dropped together don't spin and bob as one
    let phase = (add.entity.index_u32() as f32 * 2.39996).rem_euclid(TAU);
    commands
        .entity(add.entity)
        .insert(Visibility::default())
        .with_child((
            Spinning { phase },
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::default(),
        ));
}

fn spin_and_bob(time: Res<Time>, mut cubes: Query<(&Spinning, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (spinning, mut transform) in &mut cubes {
        let bob = (t * BOB_SPEED * TAU + spinning.phase).sin() * 0.5 + 0.5;
        transform.translation.y = SIZE / 2.0 + bob * BOB_HEIGHT;
        transform.rotation = Quat::from_rotation_y(t * SPIN_SPEED * TAU + spinning.phase);
    }
}

// A picture `SIZE` across around its middle, upright, facing +z, drawing tile `sprite` of the
// items' atlas, a pixel thick: its front and back, and a side along each edge of a pixel
// that's drawn against one that isn't, in that pixel's colour. `drawn` has a row of bits for
// each row of its pixels.
fn picture(sprite: usize, drawn: &[u16; 16]) -> Mesh {
    let pixel = SIZE / 16.0;
    let tiles = Item::sprites() as f32;
    let is_drawn = |x: i32, y: i32| {
        (0..16).contains(&x) && (0..16).contains(&y) && drawn[y as usize] >> x & 1 == 1
    };
    // a pixel's place, from its top left, x right and y down, to where it is
    let at = |x: f32, y: f32, z: f32| [(x - 8.0) * pixel, (8.0 - y) * pixel, z * pixel / 2.0];
    let uv = |x: f32, y: f32| [(sprite as f32 + x / 16.0) / tiles, y / 16.0];
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u16> = Vec::new();
    let mut quad = |corners: [[f32; 3]; 4], normal: Vec3, texels: [[f32; 2]; 4]| {
        let start = positions.len() as u16;
        positions.extend(corners);
        normals.extend([normal.to_array(); 4]);
        uvs.extend(texels);
        // wound anticlockwise as seen from outside
        let (a, b, c) = (
            Vec3::from(corners[0]),
            Vec3::from(corners[1]),
            Vec3::from(corners[2]),
        );
        let quad = if (b - a).cross(c - a).dot(normal) > 0.0 {
            [0, 1, 2, 0, 2, 3]
        } else {
            [0, 2, 1, 0, 3, 2]
        };
        indices.extend(quad.map(|i| start + i));
    };
    for z in [1.0, -1.0] {
        let corners = [(0.0, 0.0), (16.0, 0.0), (16.0, 16.0), (0.0, 16.0)];
        quad(
            corners.map(|(x, y)| at(x, y, z)),
            Vec3::Z * z,
            corners.map(|(x, y)| uv(x, y)),
        );
    }
    for y in 0..16 {
        for x in 0..16 {
            if !is_drawn(x, y) {
                continue;
            }
            let (fx, fy) = (x as f32, y as f32);
            let colour = [uv(fx + 0.5, fy + 0.5); 4];
            // each edge against a pixel not drawn, as its two ends and the way it faces
            let edges = [
                ((-1, 0), (fx, fy), (fx, fy + 1.0), Vec3::NEG_X),
                ((1, 0), (fx + 1.0, fy), (fx + 1.0, fy + 1.0), Vec3::X),
                ((0, -1), (fx, fy), (fx + 1.0, fy), Vec3::Y),
                ((0, 1), (fx, fy + 1.0), (fx + 1.0, fy + 1.0), Vec3::NEG_Y),
            ];
            for ((dx, dy), (x0, y0), (x1, y1), normal) in edges {
                if !is_drawn(x + dx, y + dy) {
                    let corners = [
                        at(x0, y0, 1.0),
                        at(x1, y1, 1.0),
                        at(x1, y1, -1.0),
                        at(x0, y0, -1.0),
                    ];
                    quad(corners, normal, colour);
                }
            }
        }
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U16(indices))
}

// A cube `SIZE` across around its middle, its faces drawing `block`'s tiles from the atlas,
// tinted as the terrain's are where the tile's greyscale.
fn cube(block: BlockType, grass: LinearRgba) -> Mesh {
    let half = SIZE / 2.0;
    let (mut positions, mut normals, mut uvs, mut colours) = (vec![], vec![], vec![], vec![]);
    let mut indices: Vec<u16> = vec![];
    for normal in [
        Vec3::X,
        Vec3::NEG_X,
        Vec3::Y,
        Vec3::NEG_Y,
        Vec3::Z,
        Vec3::NEG_Z,
    ] {
        // across and down the tile as it's seen from outside, upright on the sides
        let (across, down) = if normal.y == 0.0 {
            (Vec3::Y.cross(normal), Vec3::NEG_Y)
        } else {
            (Vec3::X, Vec3::Z * normal.y)
        };
        let tile = block.tile(Face::of_normal_y(normal.y));
        let colour = if tile.tinted() {
            grass
        } else {
            LinearRgba::WHITE
        };
        let start = positions.len() as u16;
        for (u, v) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
            let corner = normal + across * (2.0 * u - 1.0) + down * (2.0 * v - 1.0);
            positions.push((corner * half).to_array());
            normals.push(normal.to_array());
            let u = TILE_INSET + u * (1.0 - 2.0 * TILE_INSET);
            let v = TILE_INSET + v * (1.0 - 2.0 * TILE_INSET);
            uvs.push([(tile.atlas_cell() + u) / TILE_COUNT as f32, v]);
            colours.push(colour.to_f32_array());
        }
        // wound anticlockwise as seen from outside
        let quad = if across.cross(down).dot(normal) > 0.0 {
            [0, 1, 2, 0, 2, 3]
        } else {
            [0, 2, 1, 0, 3, 2]
        };
        indices.extend(quad.map(|i| start + i));
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colours)
    .with_inserted_indices(Indices::U16(indices))
}
