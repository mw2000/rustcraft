//! Lighting models as the land is lit where they are (§5.5): creatures, other players, what they
//! hold and wear, dropped items and thrown stones. Each part drawn with a `ModelMaterial` is lit
//! by the sky light and block light of the block its middle is in, worked out just as the land's
//! meshes are lit (`generator::chunk_light`), so it takes the same moonlight, the same warm
//! torchlight and the same dark in a cave as the ground under it, through the same shader
//! functions (`land_light.wesl`), bending with the land and fogged as it is.
//!
//! The light of each chunk with a model in it is worked out on the compute pool and kept while
//! models are in it, again whenever it or a chunk beside it changes. Each part looks its block up
//! as it moves into another, eases towards it, and carries it in its `MeshTag`, which is per
//! instance: every part of a kind still shares its mesh and material, so Bevy draws them batched.

use std::sync::{Arc, Weak};

use bevy::asset::Asset;
use bevy::ecs::system::SystemParam;
use bevy::math::I64Vec3;
use bevy::mesh::{MeshTag, MeshVertexBufferLayoutRef};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;
use bevy::tasks::{block_on, futures, AsyncComputeTaskPool, Task};
use bevy::transform::TransformSystems;

use super::animation::ease;
use crate::chunks::chunk::{ChunkCoordinate, ChunkData};
use crate::chunks::generate::generator::chunk_light;
use crate::chunks::generate::light::{ChunkLight, MAX_LIGHT};
use crate::chunks::material::{push_shader_defs, water_level};
use crate::clouds::CloudLayer;
use crate::curvature::Curvature;
use crate::world::World;

/// The bit of a part's `MeshTag` that says it carries light, matching `LIT` in `model.wesl`.
/// A part without it, not looked up yet, is drawn as if under the open sky.
pub const LIT: u32 = 0x10000;

// How quickly a part's light follows it into another block, as the fraction of the way left
// that's gone a second, so walking out of a torch's light dims rather than steps.
const LIGHT_EASING: f32 = 6.0;

// A chunk's light is put away once no model has looked at it for this many seconds.
const FORGET_AFTER: f32 = 4.0;

// Each part looks its block up again at least this often, in seconds, so the chunk it's in is
// kept while it stands still.
const LOOK_AGAIN: f32 = 1.0;

/// Lights `ModelMaterial`s. Needs the renderer, `CloudPlugin` and `CurvaturePlugin`.
pub struct ModelLightPlugin;

impl Plugin for ModelLightPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<ModelMaterial>::default())
            .init_resource::<ChunkLights>()
            .add_observer(light_part)
            .add_systems(
                PostUpdate,
                // where parts are this frame
                (light_chunks, light_models)
                    .chain()
                    .after(TransformSystems::Propagate),
            );
    }
}

/// How the parts of models are drawn: in their meshes' own colours, so the parts of every kind
/// share a few materials and are drawn together, or a picture from `texture`, tinted, and lit
/// as the land around them is unless they glow (§5.5).
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(ModelKey)]
pub struct ModelMaterial {
    #[uniform(0)]
    pub tint: LinearRgba,
    #[texture(1)]
    #[sampler(2)]
    pub texture: Option<Handle<Image>>,
    // the colour it's turned towards, and how far, in alpha
    #[uniform(3)]
    pub flash: LinearRgba,
    // the clouds, whose shadows fall on models as on the land, see `clouds::CloudLayer`
    #[texture(4)]
    #[sampler(5)]
    cloud_pattern: Handle<Image>,
    #[storage(6, read_only)]
    cloud_layer: Handle<ShaderBuffer>,
    // what the land bends around, see `curvature::Curvature`
    #[storage(8, read_only)]
    curvature: Handle<ShaderBuffer>,
    pub alpha_mode: AlphaMode,
    look: ModelKey,
}

/// What the parts of models drawn with a material look like, for `ModelMaterials::add`.
#[derive(Clone, Debug)]
pub struct ModelLook {
    // what their meshes' colours, or their pictures, are multiplied by: its alpha fades them
    pub tint: Color,
    // a colour they're turned towards, and how far, as creatures flash red when they're hurt
    pub flash: Option<(Color, f32)>,
    pub texture: Option<Handle<Image>>,
    pub alpha_mode: AlphaMode,
    // seen from both sides, as a picture spinning is
    pub double_sided: bool,
    // drawn as bright as they are whatever the light, as monsters' eyes are
    pub glows: bool,
}

impl ModelLook {
    /// In their meshes' colours, lit.
    pub fn lit() -> Self {
        Self {
            tint: Color::WHITE,
            flash: None,
            texture: None,
            alpha_mode: AlphaMode::Opaque,
            double_sided: false,
            glows: false,
        }
    }

    /// In their meshes' colours, as bright as they are whatever the light.
    pub fn glowing() -> Self {
        Self {
            glows: true,
            ..Self::lit()
        }
    }

    /// Faded to `alpha`, blended over what's behind.
    pub fn faded(self, alpha: f32) -> Self {
        Self {
            tint: self.tint.with_alpha(alpha),
            alpha_mode: AlphaMode::Blend,
            ..self
        }
    }

    /// Turned `amount` of the way towards `colour`.
    pub fn flashing(self, colour: Color, amount: f32) -> Self {
        Self {
            flash: Some((colour, amount)),
            ..self
        }
    }
}

/// `mesh` coloured `colour` all over, for a `ModelMaterial` to draw.
pub fn coloured(mesh: Mesh, colour: Color) -> Mesh {
    let count = mesh.count_vertices();
    let colour = colour.to_linear().to_f32_array();
    mesh.with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![colour; count])
}

// Compiled into a model's shaders, see `ModelMaterial::specialize`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ModelKey {
    textured: bool,
    cut_out: bool,
    double_sided: bool,
    glows: bool,
}

impl From<&ModelMaterial> for ModelKey {
    fn from(material: &ModelMaterial) -> Self {
        material.look
    }
}

impl ModelMaterial {
    /// Whether it's drawn as bright as it is whatever the light.
    pub fn glows(&self) -> bool {
        self.look.glows
    }
}

impl Material for ModelMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/model.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/model.wesl".into()
    }

    // bent with the land, so its shadows fall where it's drawn
    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/model_prepass.wesl".into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        "shaders/model_prepass.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let look = key.bind_group_data;
        if look.double_sided {
            descriptor.primitive.cull_mode = None;
        }
        let mut defs = vec![water_level()];
        for (on, def) in [
            (look.textured, "TEXTURED"),
            (look.cut_out, "CUT_OUT"),
            (look.glows, "GLOWS"),
        ] {
            if on {
                defs.push(def.into());
            }
        }
        push_shader_defs(descriptor, &defs);
        Ok(())
    }
}

/// Makes `ModelMaterial`s, with the clouds and curvature every one carries.
#[derive(SystemParam)]
pub struct ModelMaterials<'w> {
    materials: ResMut<'w, Assets<ModelMaterial>>,
    clouds: Res<'w, CloudLayer>,
    curvature: Res<'w, Curvature>,
}

impl ModelMaterials<'_> {
    pub fn add(&mut self, look: ModelLook) -> Handle<ModelMaterial> {
        let flash = look.flash.map_or(LinearRgba::NONE, |(colour, amount)| {
            colour.to_linear().with_alpha(amount)
        });
        let material = ModelMaterial {
            tint: look.tint.to_linear(),
            flash,
            look: ModelKey {
                textured: look.texture.is_some(),
                cut_out: matches!(look.alpha_mode, AlphaMode::Mask(_)),
                double_sided: look.double_sided,
                glows: look.glows,
            },
            texture: look.texture,
            cloud_pattern: self.clouds.pattern.clone(),
            cloud_layer: self.clouds.layer.clone(),
            curvature: self.curvature.buffer.clone(),
            alpha_mode: look.alpha_mode,
        };
        self.materials.add(material)
    }
}

/// The light a part of a model is drawn in, and where it last looked it up.
#[derive(Component, Default)]
pub struct Lit {
    block: Option<I64Vec3>,
    looked_at: f32,
    // sky light and block light, 0..1: what it's easing to, and what it's drawn in
    target: Vec2,
    light: Option<Vec2>,
}

/// The `MeshTag` that carries sky light and block light, 0..1, to `model.wesl`.
pub fn tag(light: Vec2) -> u32 {
    let level = |l: f32| (l.clamp(0.0, 1.0) * 255.0).round() as u32;
    LIT | level(light.x) | level(light.y) << 8
}

// Every part drawn with a `ModelMaterial` is lit, by the block it's in once it's been placed.
fn light_part(add: On<Add<MeshMaterial3d<ModelMaterial>>>, mut commands: Commands) {
    commands
        .entity(add.entity)
        .insert((Lit::default(), MeshTag::new(0)));
}

/// The light in the chunks models are in, as their meshes are lit (§5.5).
#[derive(Resource, Default)]
pub struct ChunkLights {
    chunks: HashMap<ChunkCoordinate, Lighting>,
    // the chunks whose light changed this frame, so the parts in them look theirs up again
    changed: Vec<ChunkCoordinate>,
}

// A chunk's light, and what it was worked out from: the chunk and its 26 neighbours, weakly, so
// an edit to one doesn't have to copy it, but enough to tell it's been changed.
struct Lighting {
    light: Option<ChunkLight>,
    working: Option<Task<ChunkLight>>,
    from: [Option<Weak<ChunkData>>; 27],
    used: f32,
}

impl Lighting {
    fn new(now: f32) -> Self {
        Self {
            light: None,
            working: None,
            from: std::array::from_fn(|_| None),
            used: now,
        }
    }

    // Whether the chunks it's from are still those in `world`.
    fn is_current(&self, world: &World, coord: ChunkCoordinate) -> bool {
        around(coord).zip(&self.from).all(|(chunk, from)| {
            match (world.get_chunk_data(chunk), from) {
                (None, None) => true,
                (Some(now), Some(from)) => Arc::as_ptr(&now) == Weak::as_ptr(from),
                _ => false,
            }
        })
    }
}

// The chunk at `coord` and the 26 round it, always in the same order.
fn around(coord: ChunkCoordinate) -> impl Iterator<Item = ChunkCoordinate> {
    (-1..=1).flat_map(move |x| {
        (-1..=1).flat_map(move |y| {
            (-1..=1).map(move |z| ChunkCoordinate(coord.0 + I64Vec3::new(x, y, z)))
        })
    })
}

impl ChunkLights {
    /// The sky light and block light at `block`, 0..1, once its chunk's light is worked out,
    /// which asking for it starts.
    pub fn at(&mut self, block: I64Vec3, now: f32) -> Option<Vec2> {
        let coord = ChunkCoordinate::containing(block);
        let lighting = self
            .chunks
            .entry(coord)
            .or_insert_with(|| Lighting::new(now));
        lighting.used = now;
        let local = ChunkCoordinate::local_block(block).as_ivec3();
        let [sky, block] = lighting.light.as_ref()?.levels(local);
        Some(Vec2::new(sky as f32, block as f32) / MAX_LIGHT as f32)
    }

    /// Works out the light of the chunks asked for, and again those whose blocks have changed
    /// since, and puts away those no longer looked at. `wait` waits for each to be worked out,
    /// for tests.
    pub fn update(&mut self, world: &World, now: f32, wait: bool) {
        let changed = &mut self.changed;
        changed.clear();
        self.chunks.retain(|&coord, lighting| {
            if now - lighting.used > FORGET_AFTER {
                return false;
            }
            if lighting.working.is_none() && !lighting.is_current(world, coord) {
                for (from, chunk) in lighting.from.iter_mut().zip(around(coord)) {
                    *from = world.get_chunk_data(chunk).as_ref().map(Arc::downgrade);
                }
                match world.get_chunk_data(coord) {
                    Some(chunk) => {
                        let adjacent = world.adjacent_chunk_data(coord);
                        lighting.working = Some(
                            AsyncComputeTaskPool::get()
                                .spawn(async move { chunk_light(&chunk, &adjacent, coord) }),
                        );
                    }
                    // gone, so nothing's in it to light
                    None => {
                        if lighting.light.take().is_some() {
                            changed.push(coord);
                        }
                    }
                }
            }
            if let Some(task) = &mut lighting.working {
                let ready = if wait {
                    Some(block_on(task))
                } else {
                    futures::check_ready(task)
                };
                if let Some(light) = ready {
                    lighting.light = Some(light);
                    lighting.working = None;
                    changed.push(coord);
                }
            }
            true
        });
    }
}

fn light_chunks(world: Option<Res<World>>, time: Res<Time>, mut lights: ResMut<ChunkLights>) {
    match world {
        Some(world) => lights.update(&world, time.elapsed_secs(), false),
        None if !lights.chunks.is_empty() => *lights = ChunkLights::default(),
        None => {}
    }
}

// Lights each part by the block its middle is in, easing towards the light there as it moves
// or the light changes. Until its chunk's light is worked out, a part new in the world is lit as
// if open to the sky above, or dark if it's under ground.
fn light_models(
    time: Res<Time>,
    world: Option<Res<World>>,
    mut lights: ResMut<ChunkLights>,
    mut parts: Query<(&GlobalTransform, &mut Lit, &mut MeshTag)>,
) {
    let Some(world) = world else {
        return;
    };
    let (now, dt) = (time.elapsed_secs(), time.delta_secs());
    for (at, mut lit, mut tag) in &mut parts {
        let block = at.translation().floor().as_i64vec3();
        let look = lit.block != Some(block)
            || now - lit.looked_at > LOOK_AGAIN
            || (!lights.changed.is_empty()
                && lights.changed.contains(&ChunkCoordinate::containing(block)));
        // lit as it should be already, which most are most of the time
        if !look && lit.light == Some(lit.target) {
            continue;
        }
        if look {
            lit.block = Some(block);
            lit.looked_at = now;
            match lights.at(block, now) {
                Some(light) => lit.target = light,
                None if lit.light.is_none() => {
                    let sky = if world.is_under_ground(block) {
                        0.0
                    } else {
                        1.0
                    };
                    lit.target = Vec2::new(sky, 0.0);
                }
                // as it was until this block's light is known
                None => {}
            }
        }
        let target = lit.target;
        let light = match lit.light {
            // there, once it's as near as the tag can tell
            Some(light) if (light - target).abs().max_element() < 0.5 / 255.0 => target,
            Some(light) => Vec2::new(
                ease(light.x, target.x, LIGHT_EASING, dt),
                ease(light.y, target.y, LIGHT_EASING, dt),
            ),
            None => target,
        };
        lit.light = Some(light);
        tag.set_if_neq(MeshTag::new(self::tag(light)));
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::{I64Vec3, Vec3Swizzles};
    use bevy::mesh::{MeshTag, VertexAttributeValues};
    use bevy::prelude::*;
    use bevy::tasks::{AsyncComputeTaskPool, TaskPool};

    use super::{
        light_chunks, light_models, light_part, tag, ChunkLights, Lit, ModelMaterial, LIT,
    };
    use crate::block::BlockType;
    use crate::chunks::chunk::{ChunkCoordinate, ChunkData, CHUNK_SIZE, MESH_OFFSET};
    use crate::chunks::generate::generator::generate_chunk_mesh;
    use crate::world::World;

    // A world of air round the chunk at height 40, with a floor of stone at `FLOOR` across it.
    const FLOOR: i64 = 40 * CHUNK_SIZE as i64 + 4;

    fn field() -> World {
        AsyncComputeTaskPool::get_or_init(TaskPool::new);
        let mut world = World::new(16);
        for x in -2..=2 {
            for y in 39..=41 {
                for z in -2..=2 {
                    let coord = ChunkCoordinate(I64Vec3::new(x, y, z));
                    world.insert_chunk(coord, ChunkData::default());
                }
            }
        }
        for x in -8..24 {
            for z in -8..24 {
                world.edit(I64Vec3::new(x, FLOOR, z), BlockType::Stone);
            }
        }
        world
    }

    fn light(lights: &mut ChunkLights, world: &World, block: I64Vec3) -> Vec2 {
        lights.at(block, 0.0);
        lights.update(world, 0.0, true);
        lights.at(block, 0.0).expect("worked out")
    }

    // The sky light and block light of the top face of the block under `block`, the mean of its
    // corners, as the land's mesh has them.
    fn floor_light(world: &World, block: I64Vec3) -> Vec2 {
        let coord = ChunkCoordinate::containing(block);
        let meshes = generate_chunk_mesh(
            world.get_chunk_data(coord).unwrap(),
            world.adjacent_chunk_data(coord),
            coord,
            world.region_store.noise(),
            false,
        );
        let mesh = &meshes.opaque;
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("no positions");
        };
        let Some(VertexAttributeValues::Float32x2(sky)) = mesh.attribute(Mesh::ATTRIBUTE_UV_1)
        else {
            panic!("no sky light");
        };
        let block_light = match mesh.attribute(Mesh::ATTRIBUTE_TANGENT) {
            Some(VertexAttributeValues::Float32x4(tangents)) => Some(tangents),
            _ => None,
        };
        let origin = (coord.0 * CHUNK_SIZE as i64).as_vec3() + MESH_OFFSET;
        let low = block.as_vec3();
        // the face's corners, or if it's merged into a bigger face, the corners of that
        let on_top = |p: Vec3| (p.y - low.y).abs() < 1e-4;
        let corner = |p: Vec3| {
            let off = (p - low).xz();
            (off.x == 0.0 || off.x == 1.0) && (off.y == 0.0 || off.y == 1.0)
        };
        let tops: Vec<(usize, Vec3)> = positions
            .iter()
            .map(|p| origin + Vec3::from_array(*p))
            .enumerate()
            .filter(|&(_, p)| on_top(p))
            .collect();
        let mut corners: Vec<usize> = tops
            .iter()
            .filter(|&&(_, p)| corner(p))
            .map(|&(i, _)| i)
            .collect();
        if corners.is_empty() {
            let middle = low + Vec3::new(0.5, 0.0, 0.5);
            let nearest = tops
                .iter()
                .min_by(|a, b| a.1.distance(middle).total_cmp(&b.1.distance(middle)))
                .expect("a floor");
            corners.push(nearest.0);
        }
        let n = corners.len() as f32;
        let sky: f32 = corners.iter().map(|&i| sky[i][0]).sum::<f32>() / n;
        let block = block_light.map_or(0.0, |tangents| {
            corners.iter().map(|&i| tangents[i][0]).sum::<f32>() / n
        });
        Vec2::new(sky, block)
    }

    // As the floor's lit, near enough: its corners take in the blocks round it too, no more
    // than a level brighter or darker.
    fn assert_lit_as_the_floor(world: &World, lights: &mut ChunkLights, block: I64Vec3) {
        let (model, floor) = (light(lights, world, block), floor_light(world, block));
        assert!(
            (model - floor).abs().max_element() <= 1.0 / 15.0 + 1e-4,
            "{model} where the floor has {floor}"
        );
    }

    #[test]
    fn test_models_are_lit_by_the_lands_own_light() {
        let read = |name: &str| std::fs::read_to_string(format!("assets/shaders/{name}.wesl"));
        for shader in ["world", "model"] {
            let text = read(shader).unwrap();
            assert!(
                text.contains("import super::land_light::land_light;"),
                "{shader}"
            );
            assert!(
                text.contains("land_light(in.world_position, normal,"),
                "{shader}"
            );
        }
    }

    #[test]
    fn test_a_tag_carries_light_to_the_shader() {
        assert_eq!(LIT | 255, tag(Vec2::new(1.0, 0.0)));
        assert_eq!(LIT | 128 | (51 << 8), tag(Vec2::new(0.5, 0.2)));
        // and never more than there is
        assert_eq!(tag(Vec2::ONE), tag(Vec2::splat(3.0)));
    }

    #[test]
    fn test_a_model_is_lit_as_the_ground_it_stands_on() {
        let mut world = field();
        let mut lights = ChunkLights::default();
        // out in the open
        let open = I64Vec3::new(8, FLOOR + 1, 8);
        assert_eq!(Vec2::new(1.0, 0.0), light(&mut lights, &world, open));
        assert_lit_as_the_floor(&world, &mut lights, open);

        // in a cave: a room roofed and walled over, in the dark
        for x in 2..=12 {
            for y in FLOOR + 1..=FLOOR + 4 {
                for z in 2..=12 {
                    let wall = x == 2 || x == 12 || z == 2 || z == 12 || y == FLOOR + 4;
                    if wall {
                        world.edit(I64Vec3::new(x, y, z), BlockType::Stone);
                    }
                }
            }
        }
        let inside = I64Vec3::new(7, FLOOR + 1, 7);
        assert_eq!(Vec2::ZERO, light(&mut lights, &world, inside));
        assert_lit_as_the_floor(&world, &mut lights, inside);

        // and lit by a torch in it, as the floor round it is
        world.edit(I64Vec3::new(4, FLOOR + 1, 4), BlockType::Torch);
        let by_torch = I64Vec3::new(5, FLOOR + 1, 4);
        let lit = light(&mut lights, &world, by_torch);
        assert_eq!(Vec2::new(0.0, 13.0 / 15.0), lit);
        assert_lit_as_the_floor(&world, &mut lights, by_torch);
        assert!(light(&mut lights, &world, inside).y > 0.0);
    }

    #[test]
    fn test_a_part_just_inside_a_wall_has_the_light_beside_it() {
        let world = field();
        let mut lights = ChunkLights::default();
        let floor = I64Vec3::new(8, FLOOR, 8);
        assert_eq!(Vec2::new(1.0, 0.0), light(&mut lights, &world, floor));
    }

    #[test]
    fn test_light_is_worked_out_again_as_the_chunk_changes_and_forgotten_once_unused() {
        let mut world = field();
        let mut lights = ChunkLights::default();
        let block = I64Vec3::new(8, FLOOR + 1, 8);
        assert_eq!(0.0, light(&mut lights, &world, block).y);
        // a torch in the next chunk over still lights it
        world.edit(I64Vec3::new(17, FLOOR + 1, 8), BlockType::Torch);
        lights.update(&world, 0.5, true);
        assert!(lights.changed.contains(&ChunkCoordinate::containing(block)));
        assert_eq!(5.0 / 15.0, lights.at(block, 0.5).unwrap().y);
        // nothing changed, nothing to do
        lights.update(&world, 1.0, true);
        assert!(lights.changed.is_empty());
        lights.update(&world, 10.0, true);
        assert!(lights.chunks.is_empty());
    }

    // A part drawn with a model's material, in an App lighting it.
    fn lighting(world: World) -> (App, Entity) {
        use std::time::Duration;

        use bevy::time::TimeUpdateStrategy;
        use bevy::transform::TransformSystems;

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                50,
            )))
            .insert_resource(world)
            .init_resource::<ChunkLights>()
            .add_observer(light_part)
            .add_systems(
                PostUpdate,
                (light_chunks, light_models)
                    .chain()
                    .after(TransformSystems::Propagate),
            );
        let at = Vec3::new(8.5, FLOOR as f32 + 1.5, 8.5);
        let part = app
            .world_mut()
            .spawn((
                Transform::from_translation(at),
                MeshMaterial3d::<ModelMaterial>(Handle::default()),
            ))
            .id();
        (app, part)
    }

    // Updates until the part's light settles, once its chunk's is worked out, and its tag.
    fn settle(app: &mut App, part: Entity) -> u32 {
        for _ in 0..400 {
            app.update();
            let working = app
                .world()
                .resource::<ChunkLights>()
                .chunks
                .values()
                .any(|lighting| lighting.working.is_some() || lighting.light.is_none());
            let lit = app.world().get::<Lit>(part).unwrap();
            if !working && lit.light == Some(lit.target) {
                return app.world().get::<MeshTag>(part).unwrap().value;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("never settled");
    }

    #[test]
    fn test_a_part_carries_the_light_where_it_is_and_eases_into_another() {
        let mut world = field();
        world.edit(I64Vec3::new(30, FLOOR + 1, 8), BlockType::Torch);
        let (mut app, part) = lighting(world);
        assert_eq!(tag(Vec2::new(1.0, 0.0)), settle(&mut app, part));
        // walked up to the torch, into its light: not all at once
        app.world_mut()
            .get_mut::<Transform>(part)
            .unwrap()
            .translation
            .x = 29.5;
        let block = |tag: u32| (tag >> 8) & 0xff;
        let lit = |app: &App| app.world().get::<MeshTag>(part).unwrap().value;
        for _ in 0..400 {
            app.update();
            if block(lit(&app)) > 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let first = block(lit(&app));
        assert!(
            first > 0 && first < block(tag(Vec2::new(1.0, 13.0 / 15.0))),
            "{first}"
        );
        assert_eq!(tag(Vec2::new(1.0, 13.0 / 15.0)), settle(&mut app, part));
    }

    #[test]
    fn test_a_part_lights_up_as_a_torch_is_put_down_by_it() {
        let mut world = field();
        // roofed over, in the dark
        for x in 0..16 {
            for z in 0..16 {
                world.edit(I64Vec3::new(x, FLOOR + 3, z), BlockType::Stone);
            }
        }
        let (mut app, part) = lighting(world);
        let dark = settle(&mut app, part);
        assert!(dark & 0xff < 255 / 2, "{:x}", dark);
        app.world_mut()
            .resource_mut::<World>()
            .edit(I64Vec3::new(9, FLOOR + 1, 8), BlockType::Torch);
        let lit = settle(&mut app, part);
        assert_eq!(13 * 17, (lit >> 8) & 0xff, "{:x}", lit);
    }
}
