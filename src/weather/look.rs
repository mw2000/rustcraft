//! How the weather looks, on the client: the sun, moon and clouds darkening under a storm, rain
//! falling round the camera, lightning forking down in the heaviest of it, lighting everything
//! for a moment, and a rainbow opposite the sun once it clears. Ported from rornic/rustcraft
//! #80, by Aymen Qader. Its thunder is heard through `client::sound`, as `Thunderclap`s.
//!
//! Our own on top: what falls is snow where it's cold, as `weather::precipitation` has it by
//! the climate over the camera, and none falls under a roof, in a house or a cave: a map of the
//! roofs over the columns round the camera (`RoofMap`) hides the streaks under them. How much
//! is drawn is the player's "Rain and Snow" setting (`Particles`).

use std::f32::consts::{PI, TAU};

use bevy::{
    asset::RenderAssetUsages,
    camera::{visibility::NoFrustumCulling, Exposure},
    light::{NotShadowCaster, NotShadowReceiver},
    math::ops,
    math::{I64Vec2, I64Vec3, U16Vec3},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    reflect::TypePath,
    render::view::ColorGrading,
    render::{
        mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology},
        render_resource::{AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError},
    },
    shader::ShaderRef,
};

use super::{precipitation, Weather, STRIKE_RAIN};
use crate::block::BlockType;
use crate::chunks::chunk::{linear_index, ChunkCoordinate, CELLS, CHUNK_SIZE};
use crate::chunks::generate::WATER_LEVEL;
use crate::client::critters::AmbientLife;
use crate::client::player::WorldCamera;
use crate::clock::TimeOfDay;
use crate::clouds::{CloudMaterial, CLOUD_BASE};
use crate::preferences::{Particles, Preferences};
use crate::sky::{camera_exposure, moon_illuminance, sun_direction, sun_illuminance, Moon, Sun};
use crate::tonemap::dry_color_grading;
use crate::world::World;

/// The weather drawn, for the game's App.
pub struct WeatherLookPlugin;

impl Plugin for WeatherLookPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            MaterialPlugin::<RainMaterial>::default(),
            MaterialPlugin::<RainbowMaterial>::default(),
        ))
        .add_message::<Thunderclap>()
        .init_resource::<Overhead>()
        .init_resource::<Gloom>()
        .init_resource::<RoofMap>()
        .add_systems(Startup, spawn_weather)
        .add_systems(
            Update,
            (
                look_up,
                map_roofs,
                (darken_sky, fall_rain, strike_lightning, show_rainbow),
            )
                .chain(),
        );
    }
}

/// Lightning struck `distance` blocks off, whose thunder is heard after as long as sound takes
/// to come that far, see `client::sound`.
#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub struct Thunderclap {
    pub distance: f32,
}

/// How deep a storm's gloom is over the camera, 0 to 1: what greys the view, closes the fog in
/// (`horizon::update_horizon`) and keeps the sun's direct light off the land as the overcast
/// does (`clouds`, `cloud_shadow.wesl`). None above the clouds.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct Gloom(pub f32);

/// The weather as it is over the camera this frame.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
struct Overhead {
    /// How hard the storm is, 0 to 1, which darkens the sky wherever the camera is.
    storm: f32,
    /// How hard it's raining and snowing round the camera, as the climate has it, which is none
    /// under water or above the clouds.
    rain: f32,
    snow: f32,
    /// Whether the camera's in a cave, where no lightning's seen.
    in_cave: bool,
    /// Whether the camera's under water, whose own look the storm leaves be.
    underwater: bool,
    /// How strong the rainbow is, 0 to 1.
    rainbow: f32,
}

// How dark the storm's clouds get at the heaviest rain, and how much of the sun's and moon's
// light is left, which dims the sky and the light it sheds as well as theirs. The overcast
// takes most of what's left of their direct light off the land too, see `cloud_shadow.wesl`,
// so shadows all but go.
const STORM_CLOUD: f32 = 0.42;
const STORM_SUN: f32 = 0.45;
// How grey and how much darker the view goes in the heaviest storm: the share of its colour
// kept, and stops darker.
const STORM_SATURATION: f32 = 0.45;
const STORM_EXPOSURE: f32 = -0.35;
// How far under the ground's surface, in blocks, the camera must be for a cave.
const CAVE_DEPTH: f32 = 3.0;

fn look_up(
    weather: Res<Weather>,
    clock: Res<TimeOfDay>,
    world: Option<Res<World>>,
    camera: Query<&GlobalTransform, WorldCamera>,
    life: Option<ResMut<AmbientLife>>,
    mut overhead: ResMut<Overhead>,
    mut gloom: ResMut<Gloom>,
) {
    let storm = weather.rain(*clock);
    let eye = camera
        .single()
        .map_or(Vec3::ZERO, GlobalTransform::translation);
    let block = eye.floor().as_i64vec3();
    let (mut now, mut sheltered) = (
        Overhead {
            storm,
            rainbow: weather.rainbow(*clock),
            ..default()
        },
        true,
    );
    if let Some(world) = world.filter(|_| storm > 0.0) {
        let noise = world.region_store.noise();
        let (x, z) = (eye.x as f64, eye.z as f64);
        let falling = precipitation(storm, noise.climate(x, z), eye.y - WATER_LEVEL as f32);
        // none under water, nor once high above the clouds' base
        now.underwater = world.get_block_at(block) == BlockType::Water;
        let dry = now.underwater || eye.y > CLOUD_BASE;
        if !dry {
            (now.rain, now.snow) = (falling.rain, falling.snow);
        }
        sheltered = world.is_under_ground(block);
        let ground = noise.sample(x, z).height as f32;
        now.in_cave = sheltered && eye.y < ground - CAVE_DEPTH;
    }
    overhead.set_if_neq(now);
    gloom.set_if_neq(Gloom(if eye.y > CLOUD_BASE { 0.0 } else { storm }));
    // birds and insects shelter from it, unless they're under a roof too
    if let Some(mut life) = life {
        let rain = if sheltered { 0.0 } else { now.rain + now.snow };
        if life.rain != rain {
            life.rain = rain;
        }
    }
}

// The roof map's columns across, round the camera's, matching `ROOFS` in `rain.wesl`: the rain
// box's width.
const ROOF_SPAN: i64 = 48;
const ROOF_COLUMNS: usize = (ROOF_SPAN * ROOF_SPAN) as usize;
// What the roof map holds for a column open to the sky through the rain box, and for one roofed
// over above it.
const OPEN: f32 = -1.0e9;
const ROOFED: f32 = 1.0e9;
// How often the roofs are looked at again while the camera stays in one block, for edits, in
// seconds.
const ROOF_INTERVAL: f32 = 0.5;

/// Over each column round the camera, where what falls stops: the top of the highest block in
/// it that rain can't fall through, in the rain box's height, `ROOFED` if there's one above the
/// box and `OPEN` if there's none, x then z, from `origin`.
#[derive(Resource)]
struct RoofMap {
    origin: I64Vec2,
    roofs: Vec<f32>,
}

impl Default for RoofMap {
    fn default() -> Self {
        Self {
            origin: I64Vec2::ZERO,
            roofs: vec![OPEN; ROOF_COLUMNS],
        }
    }
}

// What rain and snow can't fall through: anything solid, and water, where they meet it.
fn stops_rain(block: BlockType) -> bool {
    block.is_solid() || block == BlockType::Water
}

/// Maps the roofs round `eye` in `world`, over the columns `ROOF_SPAN` across round it, between
/// `low` and `high`: chunk by chunk, each decoded once, from the top of the one at `high` down.
fn map_roofs_round(world: &World, eye: I64Vec3, low: i64, high: i64) -> (I64Vec2, Vec<f32>) {
    let origin = eye.xz() - I64Vec2::splat(ROOF_SPAN / 2);
    let mut roofs = vec![OPEN; ROOF_COLUMNS];
    let size = CHUNK_SIZE as i64;
    let first = origin.div_euclid(I64Vec2::splat(size));
    let last = (origin + I64Vec2::splat(ROOF_SPAN - 1)).div_euclid(I64Vec2::splat(size));
    let (bottom, top) = (low.div_euclid(size), high.div_euclid(size));
    let mut cells = Box::new([BlockType::Air; CELLS]);
    for cx in first.x..=last.x {
        for cz in first.y..=last.y {
            // the columns of this chunk column in the map, as local and map coordinates
            let columns: Vec<(u16, u16, usize)> = (0..size)
                .flat_map(|lx| (0..size).map(move |lz| (lx, lz)))
                .filter_map(|(lx, lz)| {
                    let at = I64Vec2::new(cx * size + lx, cz * size + lz) - origin;
                    let inside = at.x >= 0 && at.y >= 0 && at.x < ROOF_SPAN && at.y < ROOF_SPAN;
                    let index = (at.x * ROOF_SPAN + at.y) as usize;
                    inside.then_some((lx as u16, lz as u16, index))
                })
                .collect();
            for cy in (bottom..=top).rev() {
                let coord = ChunkCoordinate(I64Vec3::new(cx, cy, cz));
                let Some(data) = world.get_chunk_data(coord) else {
                    continue;
                };
                if cy == top {
                    for &(lx, lz, index) in &columns {
                        if data.roofed.contains(lx, lz) {
                            roofs[index] = ROOFED;
                        }
                    }
                }
                if data.empty() {
                    continue;
                }
                data.decode_into(&mut cells);
                for &(lx, lz, index) in &columns {
                    if roofs[index] != OPEN {
                        continue;
                    }
                    let lowest = (low - cy * size).clamp(0, size - 1) as u16;
                    for ly in (lowest..CHUNK_SIZE).rev() {
                        if stops_rain(cells[linear_index(U16Vec3::new(lx, ly, lz))]) {
                            roofs[index] = (cy * size + ly as i64 + 1) as f32;
                            break;
                        }
                    }
                }
            }
        }
    }
    (origin, roofs)
}

// Maps the roofs round the camera while anything's falling, as it moves to another block and
// every `ROOF_INTERVAL` for edits.
fn map_roofs(
    time: Res<Time>,
    overhead: Res<Overhead>,
    world: Option<Res<World>>,
    camera: Query<&GlobalTransform, WorldCamera>,
    mut map: ResMut<RoofMap>,
    // the camera's block as it was last mapped, and how long ago
    mut mapped: Local<Option<(I64Vec3, f32)>>,
) {
    let (Some(world), Ok(camera)) = (world, camera.single()) else {
        return;
    };
    if overhead.rain + overhead.snow <= 0.0 {
        *mapped = None;
        return;
    }
    let eye = camera.translation().floor().as_i64vec3();
    if let Some((at, since)) = mapped.as_mut() {
        *since += time.delta_secs();
        if *at == eye && *since < ROOF_INTERVAL {
            return;
        }
    }
    *mapped = Some((eye, 0.0));
    let half = (RAIN_BOX.y / 2.0).ceil() as i64;
    let (origin, roofs) = map_roofs_round(&world, eye, eye.y - half, eye.y + half);
    *map = RoofMap { origin, roofs };
}

// Lightning's flash, in stops brighter: held, then dying away, in seconds, with a fainter
// flicker after.
const FLASH_STOPS: f32 = 3.2;
const FLASH_HOLD_SECONDS: f32 = 0.08;
const FLASH_SECONDS: f32 = 0.5;
const FLICKER_STOPS: f32 = 2.2;

#[derive(Component)]
struct Flash {
    // seconds since the strike, and when the second, fainter flicker comes
    age: f32,
    flicker: f32,
}

impl Flash {
    // How many stops brighter it lights everything now.
    fn stops(&self) -> f32 {
        let dying = |since: f32| {
            let t = ((since - FLASH_HOLD_SECONDS) / FLASH_SECONDS).clamp(0.0, 1.0);
            if since < 0.0 {
                0.0
            } else {
                (1.0 - t).powf(2.5)
            }
        };
        (FLASH_STOPS * dying(self.age)).max(FLICKER_STOPS * dying(self.age - self.flicker))
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn darken_sky(
    overhead: Res<Overhead>,
    gloom: Res<Gloom>,
    time: Res<Time>,
    mut lights: Query<(&mut DirectionalLight, Has<Sun>), Or<(With<Sun>, With<Moon>)>>,
    mut clouds: ResMut<Assets<CloudMaterial>>,
    mut cameras: Query<(&mut Exposure, &mut ColorGrading), WorldCamera>,
    mut flashes: Query<(Entity, &mut Flash)>,
    mut commands: Commands,
    mut last_shade: Local<Option<f32>>,
) {
    if overhead.is_changed() {
        let shade = 1.0 - (1.0 - STORM_SUN) * overhead.storm;
        for (mut light, is_sun) in &mut lights {
            let clear = if is_sun {
                sun_illuminance()
            } else {
                moon_illuminance()
            };
            light.illuminance = shade * clear;
        }
        let cloud_shade = 1.0 - (1.0 - STORM_CLOUD) * overhead.storm;
        if last_shade.is_none_or(|last| (cloud_shade - last).abs() > 0.002) {
            *last_shade = Some(cloud_shade);
            for (_, cloud) in clouds.iter_mut() {
                cloud.set_shade(cloud_shade);
            }
        }
    }

    // lightning lights everything the moment it strikes, holds, dies away and flickers once
    let mut stops = 0.0f32;
    for (entity, mut flash) in &mut flashes {
        flash.age += time.delta_secs();
        stops = stops.max(flash.stops());
        if flash.age > flash.flicker + FLASH_HOLD_SECONDS + FLASH_SECONDS {
            commands.entity(entity).despawn();
        }
    }
    let ev100 = camera_exposure().ev100 - stops;
    // greyer and darker as the storm deepens, but under water its own look holds
    let mut graded = dry_color_grading();
    graded.global.post_saturation = 1.0 - (1.0 - STORM_SATURATION) * gloom.0;
    graded.global.exposure += STORM_EXPOSURE * gloom.0;
    for (mut exposure, mut grading) in &mut cameras {
        if exposure.ev100 != ev100 {
            exposure.ev100 = ev100;
        }
        if !overhead.underwater
            && (grading.global.post_saturation != graded.global.post_saturation
                || grading.global.exposure != graded.global.exposure)
        {
            grading.global.post_saturation = graded.global.post_saturation;
            grading.global.exposure = graded.global.exposure;
        }
    }
}

// A see-through effect drawn from both sides by its own shaders, which casts nothing and
// takes one uniform: what it needs to know this frame.
macro_rules! effect_material {
    ($(#[$doc:meta])* $material:ident, vertex: $vertex:expr, fragment: $fragment:literal) => {
        $(#[$doc])*
        #[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
        pub struct $material {
            #[uniform(0)]
            params: Vec4,
        }

        impl Material for $material {
            fn vertex_shader() -> ShaderRef {
                $vertex
            }

            fn fragment_shader() -> ShaderRef {
                $fragment.into()
            }

            fn alpha_mode(&self) -> AlphaMode {
                AlphaMode::Premultiplied
            }

            fn enable_prepass() -> bool {
                false
            }

            fn enable_shadows() -> bool {
                false
            }

            fn specialize(
                _pipeline: &MaterialPipeline,
                descriptor: &mut RenderPipelineDescriptor,
                _layout: &MeshVertexBufferLayoutRef,
                _key: MaterialPipelineKey<Self>,
            ) -> Result<(), SpecializedMeshPipelineError> {
                descriptor.primitive.cull_mode = None;
                Ok(())
            }
        }
    };
}

/// The streaks of rain and flakes of snow, falling in `rain.wesl`.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct RainMaterial {
    // how much falls, 0..1, how bright the light on it is, and the wind it slants in
    #[uniform(0)]
    params: Vec4,
    // how much of what falls is snow, and the roof map's origin
    #[uniform(1)]
    look: Vec4,
    // the roof map, `RoofMap::roofs` four to an element
    #[uniform(2)]
    roofs: [Vec4; ROOF_COLUMNS / 4],
}

impl Material for RainMaterial {
    fn vertex_shader() -> ShaderRef {
        "shaders/rain.wesl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/rain.wesl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

effect_material!(
    /// The rainbow's band, coloured in `rainbow.wesl`.
    RainbowMaterial,
    vertex: ShaderRef::Default,
    fragment: "shaders/rainbow.wesl"
);

// The box of rain round the camera, in blocks, matching `BOX` in `rain.wesl`, and how many
// streaks fill it in the heaviest rain.
const RAIN_BOX: Vec3 = Vec3::new(48.0, 32.0, 48.0);
const STREAKS: usize = 14_000;
// The wind the rain slants in, in blocks a second.
const RAIN_WIND: Vec2 = Vec2::new(3.0, 1.5);

#[derive(Resource)]
struct WeatherHandles {
    rain: Handle<RainMaterial>,
    rainbow: Handle<RainbowMaterial>,
    bolt: Handle<StandardMaterial>,
    glow: Handle<StandardMaterial>,
}

#[derive(Component)]
struct Rainbow;

/// The streaks of rain round the camera, hidden while it's dry.
#[derive(Component)]
struct RainCurtain;

// Shows or hides an effect, leaving it be when it's already so, as change detection would
// otherwise re-extract it every frame.
fn show_if(visibility: &mut Mut<Visibility>, shown: bool) {
    visibility.set_if_neq(if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
}

fn spawn_weather(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut rains: ResMut<Assets<RainMaterial>>,
    mut rainbows: ResMut<Assets<RainbowMaterial>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
) {
    let rain = rains.add(RainMaterial {
        params: Vec4::ZERO,
        look: Vec4::ZERO,
        roofs: [Vec4::splat(OPEN); ROOF_COLUMNS / 4],
    });
    let rainbow = rainbows.add(RainbowMaterial { params: Vec4::ZERO });
    commands.spawn((
        RainCurtain,
        Mesh3d(meshes.add(rain_mesh())),
        MeshMaterial3d(rain.clone()),
        Transform::default(),
        Visibility::Hidden,
        NoFrustumCulling,
        NotShadowCaster,
        NotShadowReceiver,
    ));
    commands.spawn((
        Rainbow,
        Mesh3d(meshes.add(rainbow_mesh())),
        MeshMaterial3d(rainbow.clone()),
        Transform::default(),
        Visibility::Hidden,
        NoFrustumCulling,
        NotShadowCaster,
        NotShadowReceiver,
    ));
    // a white-hot core, and a faint blue glow round it added over the sky
    let bolt = standard.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: LinearRgba::rgb(9.0, 9.5, 12.0),
        emissive_exposure_weight: 0.0,
        fog_enabled: false,
        cull_mode: None,
        ..default()
    });
    let glow = standard.add(StandardMaterial {
        base_color: Color::linear_rgba(0.5, 0.6, 1.0, 0.35),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        fog_enabled: false,
        cull_mode: None,
        ..default()
    });
    commands.insert_resource(WeatherHandles {
        rain,
        rainbow,
        bolt,
        glow,
    });
}

// Streaks scattered through the box, each four corners at one spot, told apart by their uv.
fn rain_mesh() -> Mesh {
    let mut positions = Vec::with_capacity(STREAKS * 4);
    let mut uvs = Vec::with_capacity(STREAKS * 4);
    let mut indices = Vec::with_capacity(STREAKS * 6);
    for i in 0..STREAKS {
        let spot = Vec3::new(
            rand::random_range(-0.5..0.5),
            rand::random_range(-0.5..0.5),
            rand::random_range(-0.5..0.5),
        ) * RAIN_BOX;
        let start = (i * 4) as u32;
        for uv in [[-1.0, 0.0], [1.0, 0.0], [-1.0, 1.0], [1.0, 1.0]] {
            positions.push(spot.to_array());
            uvs.push(uv);
        }
        indices.extend([0, 2, 1, 1, 2, 3].map(|k| start + k));
    }
    let count = positions.len();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    // `rain.wesl` passes its own values along in the colour
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0f32; 4]; count])
    .with_inserted_indices(Indices::U32(indices))
}

// A unit ring in its xy plane, its band from 0.95 to 1 of the way out, uv.x across the band.
fn rainbow_mesh() -> Mesh {
    const SEGMENTS: usize = 128;
    const INNER: f32 = 0.95;
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    for i in 0..=SEGMENTS {
        let angle = i as f32 / SEGMENTS as f32 * TAU;
        let (sin, cos) = ops::sin_cos(angle);
        positions.push([cos * INNER, sin * INNER, 0.0]);
        positions.push([cos, sin, 0.0]);
        uvs.push([0.0, 0.0]);
        uvs.push([1.0, 0.0]);
        if i < SEGMENTS {
            let k = (i * 2) as u32;
            indices.extend([k, k + 1, k + 2, k + 2, k + 1, k + 3]);
        }
    }
    let count = positions.len();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; count])
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// How bright the daylight is, 0..1, for things that aren't lit by the world's shaders.
fn unlit_light(clock: &TimeOfDay) -> f32 {
    ((sun_direction(clock.days()).y + 0.1) / 0.4).clamp(0.05, 1.0)
}

// Writes a material's uniform only when it changes, as writing marks it for the GPU again.
fn set_params<M: Asset + HasParams>(materials: &mut Assets<M>, handle: &Handle<M>, params: Vec4) {
    if materials
        .get(handle)
        .is_some_and(|material| material.params() != params)
    {
        if let Some(mut material) = materials.get_mut(handle) {
            material.set_params(params);
        }
    }
}

trait HasParams {
    fn params(&self) -> Vec4;
    fn set_params(&mut self, params: Vec4);
}

impl HasParams for RainbowMaterial {
    fn params(&self) -> Vec4 {
        self.params
    }

    fn set_params(&mut self, params: Vec4) {
        self.params = params;
    }
}

fn fall_rain(
    overhead: Res<Overhead>,
    clock: Res<TimeOfDay>,
    preferences: Option<Res<Preferences>>,
    map: Res<RoofMap>,
    handles: Option<Res<WeatherHandles>>,
    mut rains: ResMut<Assets<RainMaterial>>,
    mut curtains: Query<&mut Visibility, With<RainCurtain>>,
) {
    let Some(handles) = handles else {
        return;
    };
    let density = preferences.map_or(Particles::default(), |p| p.video.particles);
    let falling = overhead.rain + overhead.snow;
    let amount = falling * density.density();
    for mut visibility in &mut curtains {
        show_if(&mut visibility, amount > 0.0);
    }
    if amount <= 0.0 {
        return;
    }
    let light = unlit_light(&clock) * (1.0 - 0.5 * overhead.storm);
    let params = Vec4::new(amount, light, RAIN_WIND.x, RAIN_WIND.y);
    let snow = overhead.snow / falling;
    let look = Vec4::new(snow, map.origin.x as f32, map.origin.y as f32, 0.0);
    let Some(material) = rains.get(&handles.rain) else {
        return;
    };
    let (params_changed, look_changed) = (material.params != params, material.look != look);
    if !(params_changed || look_changed || map.is_changed()) {
        return;
    }
    if let Some(mut material) = rains.get_mut(&handles.rain) {
        material.params = params;
        material.look = look;
        if map.is_changed() {
            for (element, roofs) in material.roofs.iter_mut().zip(map.roofs.as_chunks::<4>().0) {
                *element = Vec4::from_array(*roofs);
            }
        }
    }
}

// Lightning strikes this often in the heaviest rain, at the least and most, in seconds, from
// this far off, in blocks (a metre each), and its thunder rolls in at the speed of sound.
const STRIKE_SECONDS: (f32, f32) = (7.0, 22.0);
pub(crate) const STRIKE_DISTANCE: (f32, f32) = (30.0, 520.0);
pub(crate) const THUNDER_SPEED: f32 = 343.0;
// Now and then a strike lands close, within this many blocks; the rest are farther off.
const CLOSE_STRIKE_CHANCE: f64 = 0.25;
const CLOSE_STRIKE_WITHIN: f32 = 110.0;
// How long a bolt shows, in seconds: a stroke, a moment dark, and a fainter return stroke.
const BOLT_SECONDS: f32 = 0.2;
const BOLT_GAP: (f32, f32) = (0.06, 0.1);
// How wide a bolt's core is for each block it is away, so it's a line a pixel or two across
// however far it strikes, and how much wider its glow is.
const BOLT_WIDTH: f32 = 0.0012;
const GLOW_WIDTH: f32 = 5.0;

#[derive(Component)]
struct Bolt {
    age: f32,
}

// Lightning keeps real time, not the game's, or a storm the clock's wound through would strobe.
#[allow(clippy::too_many_arguments)]
fn strike_lightning(
    mut commands: Commands,
    time: Res<Time>,
    overhead: Res<Overhead>,
    world: Option<Res<World>>,
    handles: Option<Res<WeatherHandles>>,
    camera: Query<&GlobalTransform, WorldCamera>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut bolts: Query<(Entity, &mut Bolt, &mut Visibility)>,
    mut claps: MessageWriter<Thunderclap>,
    mut strike_in: Local<Option<f32>>,
) {
    let dt = time.delta_secs();
    for (entity, mut bolt, mut visibility) in &mut bolts {
        bolt.age += dt;
        // it goes dark for a moment between its strokes
        let gap = (BOLT_GAP.0..BOLT_GAP.1).contains(&bolt.age);
        *visibility = if bolt.age < BOLT_SECONDS && !gap {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if bolt.age > BOLT_SECONDS {
            commands.entity(entity).despawn();
        }
    }

    let (Some(handles), Some(world), Ok(camera)) = (handles, world, camera.single()) else {
        return;
    };
    // only where it's falling, not in a desert's thin rain, and never seen in a cave
    if overhead.storm < STRIKE_RAIN || overhead.rain + overhead.snow < 0.3 || overhead.in_cave {
        return;
    }
    let left = strike_in.get_or_insert(6.0);
    *left -= dt;
    // one bolt at a time
    if *left > 0.0 || !bolts.is_empty() {
        return;
    }
    *left = rand::random_range(STRIKE_SECONDS.0..STRIKE_SECONDS.1);

    let eye = camera.translation();
    // mostly somewhere ahead, where it's seen
    let facing = camera.forward().with_y(0.0).normalize_or(Vec3::Z);
    let angle = ops::atan2(facing.z, facing.x) + rand::random_range(-0.55..0.55);
    let distance = if rand::random_bool(CLOSE_STRIKE_CHANCE) {
        rand::random_range(STRIKE_DISTANCE.0..CLOSE_STRIKE_WITHIN)
    } else {
        rand::random_range(CLOSE_STRIKE_WITHIN..STRIKE_DISTANCE.1)
    };
    let foot = eye + Vec3::new(ops::cos(angle), 0.0, ops::sin(angle)) * distance;
    // from the noise, as the region there may not be worked out, and working it out here
    // would stall the frame the bolt strikes on
    let ground = world
        .region_store
        .noise()
        .sample(foot.x.floor() as f64, foot.z.floor() as f64)
        .height
        .max(crate::chunks::generate::WATER_LEVEL as f64) as f32;
    let foot = foot.with_y(ground);
    let top = foot.with_y(CLOUD_BASE);
    let (core, glow) = bolt_meshes(top, foot, eye, distance);
    for (mesh, material) in [(core, &handles.bolt), (glow, &handles.glow)] {
        commands.spawn((
            Bolt { age: 0.0 },
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(material.clone()),
            Transform::default(),
            Visibility::Visible,
            NoFrustumCulling,
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }
    commands.spawn(Flash {
        age: 0.0,
        flicker: rand::random_range(0.12..0.22),
    });
    claps.write(Thunderclap { distance });
}

// A forked bolt from `top` down to `foot`, `distance` blocks from `eye`, as quads turned to face
// it: a trunk that jags at every scale, as each half of it is bent off the straight in turn, and
// a branch or two off it, thinner. Its core is a line a pixel or two across, and its glow a few
// times wider, both as wide for the distance, so near it isn't a pole and far it doesn't vanish.
fn bolt_meshes(top: Vec3, foot: Vec3, eye: Vec3, distance: f32) -> (Mesh, Mesh) {
    let width = (distance * BOLT_WIDTH).max(0.06);
    let trunk = jag(top, foot, 6, 0.22);
    let mut strokes = Vec::new();
    for _ in 0..rand::random_range(1..3) {
        let from = trunk[rand::random_range(trunk.len() / 4..trunk.len() * 2 / 3)];
        let reach = (foot.y - top.y).abs() * rand::random_range(0.2..0.4);
        let heading = rand::random_range(0.0..TAU);
        let to = from
            + Vec3::new(
                ops::cos(heading) * reach,
                -reach * 1.2,
                ops::sin(heading) * reach,
            );
        strokes.push((jag(from, to, 4, 0.25), width * 0.6));
    }
    strokes.push((trunk, width));
    let quads = |scale: f32| {
        let mut positions = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        for (points, width) in &strokes {
            for pair in points.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                let along = (b - a).normalize_or(Vec3::Y);
                let across = along
                    .cross((eye - a).normalize_or(Vec3::Z))
                    .normalize_or(Vec3::X)
                    * width
                    * scale;
                let k = positions.len() as u32;
                positions
                    .extend([a - across, a + across, b - across, b + across].map(|p| p.to_array()));
                indices.extend([0, 2, 1, 1, 2, 3].map(|i| k + i));
            }
        }
        let count = positions.len();
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count])
        .with_inserted_indices(Indices::U32(indices))
    };
    (quads(1.0), quads(GLOW_WIDTH))
}

// The points of a jagged line from `from` to `to`: each stretch's middle pushed sideways by up
// to `roughness` of its length, then each half's, `depth` times over.
fn jag(from: Vec3, to: Vec3, depth: u32, roughness: f32) -> Vec<Vec3> {
    let mut points = vec![from, to];
    for _ in 0..depth {
        let mut finer = Vec::with_capacity(points.len() * 2);
        for pair in points.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let length = a.distance(b);
            let push = Vec3::new(
                rand::random_range(-1.0..1.0),
                rand::random_range(-0.3..0.3),
                rand::random_range(-1.0..1.0),
            ) * length
                * roughness;
            finer.push(a);
            finer.push(a.lerp(b, 0.5) + push);
        }
        finer.push(*points.last().unwrap());
        points = finer;
    }
    points
}

// How far off the rainbow hangs, in blocks: near enough to be in front of the far hills'
// fog, and how wide it is from the point opposite the sun, as real ones are.
const RAINBOW_DISTANCE: f32 = 700.0;
const RAINBOW_ANGLE: f32 = 42.0 * PI / 180.0;

fn show_rainbow(
    overhead: Res<Overhead>,
    clock: Res<TimeOfDay>,
    handles: Option<Res<WeatherHandles>>,
    camera: Query<&GlobalTransform, WorldCamera>,
    mut bows: Query<(&mut Transform, &mut Visibility), With<Rainbow>>,
    mut materials: ResMut<Assets<RainbowMaterial>>,
) {
    let (Some(handles), Ok(camera)) = (handles, camera.single()) else {
        return;
    };
    let eye = camera.translation();
    let away = -sun_direction(clock.days());
    // only by day, with the sun low enough that some of the bow clears the horizon: once
    // it's higher than the bow is wide, all of it's below
    let sun_height = -away.y;
    let strength = overhead.rainbow
        * smoothstep(-0.02, 0.08, sun_height)
        * (1.0 - smoothstep(0.6, 0.68, sun_height));
    if let Ok((mut transform, mut visibility)) = bows.single_mut() {
        show_if(&mut visibility, strength > 0.0);
        if strength > 0.0 {
            *transform = Transform::from_translation(eye + away * RAINBOW_DISTANCE)
                .looking_to(away, Vec3::Y)
                .with_scale(Vec3::splat(RAINBOW_DISTANCE * ops::tan(RAINBOW_ANGLE)));
        }
    }
    let params = Vec4::new(strength, unlit_light(&clock), 0.0, 0.0);
    set_params(&mut materials, &handles.rainbow, params);
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::chunk::ChunkData;
    use crate::edits::Edits;

    // A world of seed 1 with only air loaded round the origin, 4 chunks up and down from 512.
    fn open_world() -> World {
        let mut world = World::from_save(1, Edits::default(), 16);
        for x in -3..=3 {
            for z in -3..=3 {
                for y in 28..=36 {
                    let coord = ChunkCoordinate(I64Vec3::new(x, y, z));
                    world.insert_chunk(coord, ChunkData::default());
                }
            }
        }
        world
    }

    fn roof_over(world: &World, eye: I64Vec3, column: I64Vec2) -> f32 {
        let (origin, roofs) = map_roofs_round(world, eye, eye.y - 16, eye.y + 16);
        let at = column - origin;
        roofs[(at.x * ROOF_SPAN + at.y) as usize]
    }

    #[test]
    fn test_rain_stops_under_a_roof_and_on_the_ground_but_not_in_the_open() {
        let mut world = open_world();
        let eye = I64Vec3::new(5, 520, 5);
        // open sky
        assert_eq!(OPEN, roof_over(&world, eye, I64Vec2::new(5, 5)));
        // a roof of planks over the camera, and glass over the column beside it
        world.edit(I64Vec3::new(5, 524, 5), BlockType::Planks);
        world.edit(I64Vec3::new(6, 524, 5), BlockType::Glass);
        assert_eq!(525.0, roof_over(&world, eye, I64Vec2::new(5, 5)));
        assert_eq!(525.0, roof_over(&world, eye, I64Vec2::new(6, 5)));
        // the ground under the camera: what falls stops on it
        world.edit(I64Vec3::new(-10, 510, 12), BlockType::Stone);
        assert_eq!(511.0, roof_over(&world, eye, I64Vec2::new(-10, 12)));
        // the columns round about stay open, out to the map's edge
        assert_eq!(OPEN, roof_over(&world, eye, I64Vec2::new(5 + 23, 5 - 24)));
    }

    #[test]
    fn test_a_roof_far_above_the_rain_box_covers_it_as_a_cave_does() {
        let mut world = open_world();
        let eye = I64Vec3::new(0, 520, 0);
        // stone in the chunk above the box, which roofs the chunk the box's top is in over
        world.edit(I64Vec3::new(2, 570, 3), BlockType::Stone);
        assert_eq!(ROOFED, roof_over(&world, eye, I64Vec2::new(2, 3)));
        assert_eq!(OPEN, roof_over(&world, eye, I64Vec2::new(3, 3)));
    }

    #[test]
    fn test_the_particle_setting_sets_how_much_is_drawn() {
        assert_eq!(0.0, Particles::Off.density());
        assert!(Particles::Low.density() > 0.0 && Particles::Low.density() < 0.5);
        assert_eq!(1.0, Particles::High.density());
        assert_eq!(Particles::High, Particles::default());
    }
}
