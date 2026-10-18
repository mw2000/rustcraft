//! Drawing the items lying in the world, which the server moves (`server::drops`): each a little
//! cube of its block, or a flat picture of an item that isn't one, spinning and bobbing over the
//! spot. Every one of an item shares a mesh, and all blocks share a material and all pictures
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
/// one, and a mesh for each item, made the first time one's drawn.
#[derive(Resource)]
pub(super) struct DropLooks {
    material: Handle<ModelMaterial>,
    sprites: Handle<ModelMaterial>,
    meshes: HashMap<Item, Handle<Mesh>>,
    grass: LinearRgba,
}

impl DropLooks {
    /// The mesh and material `item` is drawn with, `SIZE` across.
    pub(super) fn of(
        &mut self,
        item: Item,
        meshes: &mut Assets<Mesh>,
    ) -> (Handle<Mesh>, Handle<ModelMaterial>) {
        let grass = self.grass;
        let mesh = self
            .meshes
            .entry(item)
            .or_insert_with(|| match (item.block(), item.sprite()) {
                (Some(block), _) => meshes.add(cube(block, grass)),
                (None, sprite) => {
                    meshes.add(picture(sprite.expect("an item has a block or a sprite")))
                }
            })
            .clone();
        let material = if item.block().is_some() {
            self.material.clone()
        } else {
            self.sprites.clone()
        };
        (mesh, material)
    }
}

// The cube drawn for a dropped item, a child of it so the server's `Transform` is left alone,
// and where it is in its spin and bob.
#[derive(Component)]
struct Spinning {
    phase: f32,
}

fn make_material(mut commands: Commands, icons: Res<ItemIcons>, mut materials: ModelMaterials) {
    let material = materials.add(ModelLook {
        texture: Some(icons.atlas().clone()),
        // plants' tiles have holes
        alpha_mode: AlphaMode::Mask(0.5),
        ..ModelLook::lit()
    });
    let sprites = materials.add(ModelLook {
        texture: Some(icons.items().clone()),
        alpha_mode: AlphaMode::Mask(0.5),
        // seen from both sides as it spins
        double_sided: true,
        ..ModelLook::lit()
    });
    commands.insert_resource(DropLooks {
        material,
        sprites,
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
    let (mesh, material) = looks.of(dropped.stack.item, &mut meshes);
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

// A square `SIZE` across around its middle, upright, drawing tile `sprite` of the items' atlas.
fn picture(sprite: usize) -> Mesh {
    let half = SIZE / 2.0;
    let tiles = Item::sprites() as f32;
    let corners = [(-1.0, 1.0), (1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)];
    let positions: Vec<_> = corners.map(|(x, y)| [x * half, y * half, 0.0]).into();
    let uvs: Vec<_> = corners
        .map(|(x, y)| [(sprite as f32 + (x + 1.0) / 2.0) / tiles, (1.0 - y) / 2.0])
        .into();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U16(vec![0, 3, 2, 0, 2, 1]))
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
