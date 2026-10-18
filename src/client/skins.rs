//! Players' models as their skins draw them (M-A4): each part a box in the skin layout's
//! proportions, a pixel a sixteenth of a block, so a player's two blocks tall, with its overlay
//! a little bigger round it, both mapped onto the layout. The parts' meshes are made once for
//! each arm style, and a material once for each skin worn: the default skins, and each of
//! players' own as it's first seen.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::model_light::{ModelLook, ModelMaterial, ModelMaterials};
use crate::player::name::PlayerName;
use crate::skin::{decode, default_skin, ArmStyle, Skin, SkinPixels, DEFAULT_SKINS, SKIN_SIZE};

/// A skin's pixel, in blocks.
pub const PIXEL: f32 = 1.0 / 16.0;

/// A part of a player's model, in the skin's pixels: its box, by its corner towards -x, -y and
/// -z from the feet and its size, where its texture and its overlay's are in the layout, how
/// much bigger the overlay's drawn each way, and the joint it turns about. The player faces -z,
/// their right towards +x.
#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub min: Vec3,
    pub size: Vec3,
    pub base: UVec2,
    pub overlay: UVec2,
    pub grow: f32,
    pub joint: Vec3,
}

pub const HEAD: Part = Part {
    min: Vec3::new(-4.0, 24.0, -4.0),
    size: Vec3::splat(8.0),
    base: UVec2::new(0, 0),
    overlay: UVec2::new(32, 0),
    grow: 0.5,
    joint: Vec3::new(0.0, 24.0, 0.0),
};

pub const BODY: Part = Part {
    min: Vec3::new(-4.0, 12.0, -2.0),
    size: Vec3::new(8.0, 12.0, 4.0),
    base: UVec2::new(16, 16),
    overlay: UVec2::new(16, 32),
    grow: 0.25,
    // where it breathes from
    joint: Vec3::new(0.0, 12.0, 0.0),
};

/// The legs, left and then right.
pub const LEGS: [Part; 2] = [
    Part {
        min: Vec3::new(-4.0, 0.0, -2.0),
        size: Vec3::new(4.0, 12.0, 4.0),
        base: UVec2::new(16, 48),
        overlay: UVec2::new(0, 48),
        grow: 0.25,
        joint: Vec3::new(-2.0, 12.0, 0.0),
    },
    Part {
        min: Vec3::new(0.0, 0.0, -2.0),
        size: Vec3::new(4.0, 12.0, 4.0),
        base: UVec2::new(0, 16),
        overlay: UVec2::new(0, 32),
        grow: 0.25,
        joint: Vec3::new(2.0, 12.0, 0.0),
    },
];

/// The arms, left and then right, which hang from a little under the top of the shoulders: 4
/// pixels wide, or 3 for slim arms, against the body.
pub fn arms(style: ArmStyle) -> [Part; 2] {
    let width = match style {
        ArmStyle::Classic => 4.0,
        ArmStyle::Slim => 3.0,
    };
    let arm = |side: f32, base, overlay| Part {
        min: Vec3::new(if side > 0.0 { 4.0 } else { -4.0 - width }, 12.0, -2.0),
        size: Vec3::new(width, 12.0, 4.0),
        base,
        overlay,
        grow: 0.25,
        joint: Vec3::new(side * 5.0, 22.0, 0.0),
    };
    [
        arm(-1.0, UVec2::new(32, 48), UVec2::new(48, 48)),
        arm(1.0, UVec2::new(40, 16), UVec2::new(40, 32)),
    ]
}

impl Part {
    /// Where its joint is from the feet, in blocks.
    pub fn joint(&self) -> Vec3 {
        self.joint * PIXEL
    }

    /// Its box and its overlay's about its joint, in blocks, mapped onto the skin layout.
    pub fn mesh(&self) -> Mesh {
        let mut faces = Faces::default();
        faces.add(self.min, self.size, self.base, 0.0, self.joint);
        faces.add(self.min, self.size, self.overlay, self.grow, self.joint);
        faces.mesh()
    }

    /// A plain box over `rows` of it, counted down from its top, `grow` pixels bigger each way,
    /// about its joint, in blocks: armour over it.
    pub fn cover(&self, rows: std::ops::Range<f32>, grow: f32) -> Mesh {
        let top = self.min.y + self.size.y;
        let min = self.min.with_y(top - rows.end) - grow;
        let size = self.size.with_y(rows.end - rows.start) + grow * 2.0;
        let middle = (min + size / 2.0 - self.joint) * PIXEL;
        Mesh::from(Cuboid::from_size(size * PIXEL)).translated_by(middle)
    }
}

// The faces of a box as the layout lays its texture out: its top and bottom side by side over
// its right side, front, left side and back, each a rectangle by its corner and size, with the
// box's corners at its top left, top right, bottom right and bottom left as seen from outside,
// each corner as whether it's the box's far side on x, y and z. Tops and bottoms are seen with
// the front at the bottom of the picture.
type Corner = (bool, bool, bool);
type Face = (Vec3, [Corner; 4], fn(Vec3) -> [f32; 4]);
const FACES: [Face; 6] = [
    (
        Vec3::X,
        [
            (true, true, true),
            (true, true, false),
            (true, false, false),
            (true, false, true),
        ],
        |s| [0.0, s.z, s.z, s.y],
    ),
    (
        Vec3::NEG_Z,
        [
            (true, true, false),
            (false, true, false),
            (false, false, false),
            (true, false, false),
        ],
        |s| [s.z, s.z, s.x, s.y],
    ),
    (
        Vec3::NEG_X,
        [
            (false, true, false),
            (false, true, true),
            (false, false, true),
            (false, false, false),
        ],
        |s| [s.z + s.x, s.z, s.z, s.y],
    ),
    (
        Vec3::Z,
        [
            (false, true, true),
            (true, true, true),
            (true, false, true),
            (false, false, true),
        ],
        |s| [2.0 * s.z + s.x, s.z, s.x, s.y],
    ),
    (
        Vec3::Y,
        [
            (true, true, true),
            (false, true, true),
            (false, true, false),
            (true, true, false),
        ],
        |s| [s.z, 0.0, s.x, s.z],
    ),
    (
        Vec3::NEG_Y,
        [
            (true, false, true),
            (false, false, true),
            (false, false, false),
            (true, false, false),
        ],
        |s| [s.z + s.x, 0.0, s.x, s.z],
    ),
];

#[derive(Default)]
struct Faces {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u16>,
}

impl Faces {
    // A box at `min`, `size` pixels, its texture at `uv`, drawn `grow` pixels bigger each way,
    // about `joint`.
    fn add(&mut self, min: Vec3, size: Vec3, uv: UVec2, grow: f32, joint: Vec3) {
        let (low, high) = (
            (min - grow - joint) * PIXEL,
            (min + size + grow - joint) * PIXEL,
        );
        let pick = |(x, y, z): Corner| {
            Vec3::new(
                if x { high.x } else { low.x },
                if y { high.y } else { low.y },
                if z { high.z } else { low.z },
            )
        };
        for (normal, corners, rect) in FACES {
            let [x, y, width, height] = rect(size);
            let (u0, v0) = (uv.x as f32 + x, uv.y as f32 + y);
            let (u1, v1) = (u0 + width, v0 + height);
            let start = self.positions.len() as u16;
            for (corner, (u, v)) in
                corners
                    .into_iter()
                    .zip([(u0, v0), (u1, v0), (u1, v1), (u0, v1)])
            {
                self.positions.push(pick(corner).to_array());
                self.normals.push(normal.to_array());
                self.uvs.push([u / SKIN_SIZE as f32, v / SKIN_SIZE as f32]);
            }
            // seen from outside, top left to top right to bottom right is clockwise
            self.indices.extend([0, 2, 1, 0, 3, 2].map(|i| start + i));
        }
    }

    fn mesh(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
        .with_inserted_indices(Indices::U16(self.indices))
    }
}

/// The meshes of the parts of players' models.
pub struct SkinMeshes {
    pub head: Handle<Mesh>,
    pub body: Handle<Mesh>,
    pub legs: [Handle<Mesh>; 2],
    // classic arms and then slim
    arms: [[Handle<Mesh>; 2]; 2],
}

impl SkinMeshes {
    fn new(meshes: &mut Assets<Mesh>) -> Self {
        let mut part = |part: &Part| meshes.add(part.mesh());
        Self {
            head: part(&HEAD),
            body: part(&BODY),
            legs: LEGS.each_ref().map(&mut part),
            arms: [ArmStyle::Classic, ArmStyle::Slim]
                .map(|style| arms(style).each_ref().map(&mut part)),
        }
    }

    /// The arms, left and right.
    pub fn arms(&self, style: ArmStyle) -> &[Handle<Mesh>; 2] {
        &self.arms[style as usize]
    }
}

/// What a player's drawn wearing: one of the default skins, or their own, by its hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wearing {
    Default(usize),
    Own(u64),
}

impl Wearing {
    pub fn of(name: &PlayerName, skin: Option<&Skin>) -> Self {
        skin.map_or(Wearing::Default(default_skin(name)), |skin| {
            Wearing::Own(skin.hash())
        })
    }
}

/// The meshes players' models are drawn with, and the material of each skin worn, which every
/// model wearing it shares.
#[derive(Resource)]
pub struct SkinLooks {
    pub meshes: SkinMeshes,
    defaults: Vec<(Handle<ModelMaterial>, Handle<Image>)>,
    own: HashMap<u64, (Handle<ModelMaterial>, Handle<Image>)>,
}

impl SkinLooks {
    /// How the player called `name` wearing `skin`, if they've one of their own, is drawn,
    /// their skin's material made the first time it's worn.
    pub fn dress(
        &mut self,
        name: &PlayerName,
        skin: Option<&Skin>,
        images: &mut Assets<Image>,
        materials: &mut ModelMaterials,
    ) -> Dressed {
        let ((material, image), arms) = match skin {
            Some(skin) => {
                let looks = self
                    .own
                    .entry(skin.hash())
                    .or_insert_with(|| material(&skin.pixels(), images, materials));
                (looks.clone(), skin.arms())
            }
            None => {
                let index = default_skin(name);
                (self.defaults[index].clone(), DEFAULT_SKINS[index].1)
            }
        };
        Dressed {
            wearing: Wearing::of(name, skin),
            material,
            image,
            arms,
        }
    }
}

/// How a player's drawn: what they wear, its material and picture, and their arms.
pub struct Dressed {
    pub wearing: Wearing,
    pub material: Handle<ModelMaterial>,
    pub image: Handle<Image>,
    pub arms: ArmStyle,
}

// A skin's picture, sharp as its pixels are, and the material drawing it, its overlay cut out
// where it's clear.
fn material(
    pixels: &SkinPixels,
    images: &mut Assets<Image>,
    materials: &mut ModelMaterials,
) -> (Handle<ModelMaterial>, Handle<Image>) {
    let mut image = Image::new(
        Extent3d {
            width: SKIN_SIZE as u32,
            height: SKIN_SIZE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels.rgba().to_vec(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    let image = images.add(image);
    let material = materials.add(ModelLook {
        texture: Some(image.clone()),
        alpha_mode: AlphaMode::Mask(0.5),
        ..ModelLook::lit()
    });
    (material, image)
}

pub(super) fn load_skins(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ModelMaterials,
) {
    let defaults = DEFAULT_SKINS
        .iter()
        .map(|(name, _)| {
            let path = crate::settings::asset_path(&format!("assets/textures/skins/{name}.png"));
            let bytes = std::fs::read(&path)
                .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
            let (pixels, _) =
                decode(&bytes).unwrap_or_else(|e| panic!("{} isn't a skin: {e}", path.display()));
            material(&pixels, &mut images, &mut materials)
        })
        .collect();
    commands.insert_resource(SkinLooks {
        meshes: SkinMeshes::new(&mut meshes),
        defaults,
        own: HashMap::new(),
    });
}

#[cfg(test)]
mod tests {
    use bevy::mesh::VertexAttributeValues;

    use super::*;
    use crate::skin::tests::png_of;

    // The texel, as pixels across and down the layout, that the face of `mesh` turned to `normal`
    // shows just inside its corner at `corner`, in blocks about the part's joint: the face's
    // nearest vertex's texture coordinate, a quarter of a pixel in towards the face's middle.
    fn texel(mesh: &Mesh, normal: Vec3, corner: Vec3) -> (usize, usize) {
        let attribute = |name| match mesh.attribute(name) {
            Some(VertexAttributeValues::Float32x3(values)) => values.clone(),
            _ => unreachable!(),
        };
        let (positions, normals) = (
            attribute(Mesh::ATTRIBUTE_POSITION),
            attribute(Mesh::ATTRIBUTE_NORMAL),
        );
        let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0)
        else {
            unreachable!()
        };
        // the face's four corners, the first of the base box's and the overlay's that has one there
        let face: Vec<usize> = (0..positions.len())
            .filter(|&i| Vec3::from(normals[i]) == normal)
            .collect();
        let nearest = *face
            .iter()
            .min_by(|&&a, &&b| {
                let distance = |i: usize| Vec3::from(positions[i]).distance(corner);
                distance(a).total_cmp(&distance(b))
            })
            .unwrap();
        let quad = &face[(face.iter().position(|&i| i == nearest).unwrap() / 4) * 4..][..4];
        let middle = quad.iter().map(|&i| Vec2::from(uvs[i])).sum::<Vec2>() / 4.0;
        let uv = Vec2::from(uvs[nearest]);
        let inside = (uv + (middle - uv).signum() * 0.25 / SKIN_SIZE as f32) * SKIN_SIZE as f32;
        (inside.x as usize, inside.y as usize)
    }

    // A part's box's corner, towards `towards` on each axis, about its joint, in blocks.
    fn corner(part: &Part, towards: Vec3) -> Vec3 {
        let middle = part.min + part.size / 2.0;
        (middle + part.size / 2.0 * towards - part.joint) * PIXEL
    }

    #[test]
    fn test_each_face_shows_its_part_of_the_layout_the_right_way_round() {
        let head = HEAD.mesh();
        // the face's top left, as it's looked at, is the player's right of their forehead
        let right_brow = corner(&HEAD, Vec3::new(1.0, 1.0, -1.0));
        assert_eq!((8, 8), texel(&head, Vec3::NEG_Z, right_brow));
        assert_eq!(
            (15, 15),
            texel(
                &head,
                Vec3::NEG_Z,
                corner(&HEAD, Vec3::new(-1.0, -1.0, -1.0))
            )
        );
        // the right side runs from the back to the front, and the left from the front back
        assert_eq!(
            (0, 8),
            texel(&head, Vec3::X, corner(&HEAD, Vec3::new(1.0, 1.0, 1.0)))
        );
        assert_eq!((7, 8), texel(&head, Vec3::X, right_brow));
        assert_eq!(
            (16, 8),
            texel(
                &head,
                Vec3::NEG_X,
                corner(&HEAD, Vec3::new(-1.0, 1.0, -1.0))
            )
        );
        // the back's top left is the player's left, seen from behind
        assert_eq!(
            (24, 8),
            texel(&head, Vec3::Z, corner(&HEAD, Vec3::new(-1.0, 1.0, 1.0)))
        );
        // the top has the front at the bottom, the right on the left, and the bottom the same
        assert_eq!((8, 7), texel(&head, Vec3::Y, right_brow));
        assert_eq!(
            (15, 0),
            texel(&head, Vec3::Y, corner(&HEAD, Vec3::new(-1.0, 1.0, 1.0)))
        );
        assert_eq!(
            (16, 7),
            texel(
                &head,
                Vec3::NEG_Y,
                corner(&HEAD, Vec3::new(1.0, -1.0, -1.0))
            )
        );
        // the body's front, and the right leg's, from the right
        let body = BODY.mesh();
        assert_eq!(
            (20, 20),
            texel(&body, Vec3::NEG_Z, corner(&BODY, Vec3::new(1.0, 1.0, -1.0)))
        );
        let leg = LEGS[1].mesh();
        assert_eq!(
            (4, 20),
            texel(
                &leg,
                Vec3::NEG_Z,
                corner(&LEGS[1], Vec3::new(1.0, 1.0, -1.0))
            )
        );
        let leg = LEGS[0].mesh();
        assert_eq!(
            (20, 52),
            texel(
                &leg,
                Vec3::NEG_Z,
                corner(&LEGS[0], Vec3::new(1.0, 1.0, -1.0))
            )
        );
    }

    #[test]
    fn test_arms_are_mapped_for_their_width() {
        for (style, width) in [(ArmStyle::Classic, 4), (ArmStyle::Slim, 3)] {
            let [left, right] = arms(style);
            let mesh = right.mesh();
            // the right arm's front, from its outer edge in, then its inner side and its back
            assert_eq!(
                (44, 20),
                texel(
                    &mesh,
                    Vec3::NEG_Z,
                    corner(&right, Vec3::new(1.0, 1.0, -1.0))
                )
            );
            let inner = corner(&right, Vec3::new(-1.0, 1.0, -1.0));
            assert_eq!((43 + width, 20), texel(&mesh, Vec3::NEG_Z, inner));
            assert_eq!((44 + width, 20), texel(&mesh, Vec3::NEG_X, inner));
            let back = corner(&right, Vec3::new(-1.0, 1.0, 1.0));
            assert_eq!((48 + width, 20), texel(&mesh, Vec3::Z, back));
            assert_eq!(
                (47 + 2 * width, 31),
                texel(&mesh, Vec3::Z, corner(&right, Vec3::new(1.0, -1.0, 1.0)))
            );
            // the top is as wide as the arm, the bottom beside it
            assert_eq!((43 + width, 16), texel(&mesh, Vec3::Y, back));
            assert_eq!(
                (44 + width, 16),
                texel(
                    &mesh,
                    Vec3::NEG_Y,
                    corner(&right, Vec3::new(1.0, -1.0, 1.0))
                )
            );
            // the left arm against the body on the other side
            assert!(left.min.x + left.size.x == -4.0 && right.min.x == 4.0);
            let mesh = left.mesh();
            assert_eq!(
                (36, 52),
                texel(&mesh, Vec3::NEG_Z, corner(&left, Vec3::new(1.0, 1.0, -1.0)))
            );
        }
    }

    #[test]
    fn test_overlays_are_bigger_boxes_over_their_own_part_of_the_layout() {
        let head = HEAD.mesh();
        // the hat's corner is half a pixel out from the head's
        let hat =
            corner(&HEAD, Vec3::new(1.0, 1.0, -1.0)) + Vec3::new(1.0, 1.0, -1.0) * 0.5 * PIXEL;
        assert_eq!((40, 8), texel(&head, Vec3::NEG_Z, hat));
        let [_, right] = arms(ArmStyle::Slim);
        let sleeve =
            corner(&right, Vec3::new(1.0, 1.0, -1.0)) + Vec3::new(1.0, 1.0, -1.0) * 0.25 * PIXEL;
        assert_eq!((44, 36), texel(&right.mesh(), Vec3::NEG_Z, sleeve));
    }

    #[test]
    fn test_an_old_skins_left_leg_shows_its_right_legs_front_turned_about() {
        let png = png_of(64, 32, |x, y| [x as u8, y as u8, 9, 255]);
        let (pixels, _) = decode(&png).unwrap();
        let mesh = LEGS[0].mesh();
        // the left leg's outer front corner and the right leg's
        let (x, y) = texel(
            &mesh,
            Vec3::NEG_Z,
            corner(&LEGS[0], Vec3::new(-1.0, 1.0, -1.0)),
        );
        assert_eq!([4, 20, 9, 255], pixels.at(x, y));
        let (x, y) = texel(
            &mesh,
            Vec3::NEG_Z,
            corner(&LEGS[0], Vec3::new(1.0, -1.0, -1.0)),
        );
        assert_eq!([7, 31, 9, 255], pixels.at(x, y));
    }

    #[test]
    fn test_armour_covers_its_rows_of_a_part() {
        let boot = LEGS[1].cover(8.0..12.0, 1.0);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            boot.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            unreachable!()
        };
        let lowest = positions.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        let highest = positions
            .iter()
            .map(|p| p[1])
            .fold(f32::NEG_INFINITY, f32::max);
        // from the joint at the hip, 12 pixels down to a pixel under the foot
        assert!((lowest - -13.0 * PIXEL).abs() < 1e-6, "{lowest}");
        assert!((highest - -7.0 * PIXEL).abs() < 1e-6, "{highest}");
    }
}
