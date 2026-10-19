//! How the weather looks, on the client: the sun, moon and clouds darkening under a storm, rain
//! falling round the camera, lightning forking down in the heaviest of it, lighting everything
//! for a moment, and a rainbow opposite the sun once it clears. Ported from rornic/rustcraft
//! #80, by Aymen Qader. Its thunder is heard through `client::sound`, as `Thunderclap`s.

use std::f32::consts::{PI, TAU};

use bevy::{
    asset::RenderAssetUsages,
    camera::{visibility::NoFrustumCulling, Exposure},
    light::{NotShadowCaster, NotShadowReceiver},
    math::ops,
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

use super::{Weather, STRIKE_RAIN};
use crate::block::BlockType;
use crate::client::critters::AmbientLife;
use crate::client::player::WorldCamera;
use crate::clock::TimeOfDay;
use crate::clouds::{CloudMaterial, CLOUD_BASE};
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
        .add_systems(Startup, spawn_weather)
        .add_systems(
            Update,
            (
                look_up,
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
    /// How hard it's raining round the camera, which is none under water or above the clouds.
    rain: f32,
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
    let underwater = world
        .as_ref()
        .is_some_and(|world| world.get_block_at(eye.floor().as_i64vec3()) == BlockType::Water);
    // none under water, nor once high above the clouds' base
    let dry = world.is_none() || underwater || eye.y > CLOUD_BASE;
    let now = Overhead {
        storm,
        rain: if dry { 0.0 } else { storm },
        underwater,
        rainbow: weather.rainbow(*clock),
    };
    overhead.set_if_neq(now);
    gloom.set_if_neq(Gloom(if eye.y > CLOUD_BASE { 0.0 } else { storm }));
    // birds and insects shelter from it
    if let Some(mut life) = life {
        if life.rain != now.rain {
            life.rain = now.rain;
        }
    }
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

effect_material!(
    /// The streaks of rain, falling in `rain.wesl`.
    RainMaterial,
    vertex: "shaders/rain.wesl".into(),
    fragment: "shaders/rain.wesl"
);
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
    let rain = rains.add(RainMaterial { params: Vec4::ZERO });
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

impl HasParams for RainMaterial {
    fn params(&self) -> Vec4 {
        self.params
    }

    fn set_params(&mut self, params: Vec4) {
        self.params = params;
    }
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
    handles: Option<Res<WeatherHandles>>,
    mut rains: ResMut<Assets<RainMaterial>>,
    mut curtains: Query<&mut Visibility, With<RainCurtain>>,
) {
    let Some(handles) = handles else {
        return;
    };
    for mut visibility in &mut curtains {
        show_if(&mut visibility, overhead.rain > 0.0);
    }
    let light = unlit_light(&clock) * (1.0 - 0.5 * overhead.storm);
    let params = Vec4::new(overhead.rain, light, RAIN_WIND.x, RAIN_WIND.y);
    set_params(&mut rains, &handles.rain, params);
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
    if overhead.storm < STRIKE_RAIN {
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
