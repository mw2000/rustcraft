//! Butterflies flitting over the flowers by day, and fireflies drifting and blinking by water and
//! in the woods at night, round wherever the camera is near the ground. Both leave under water.

use bevy::{
    asset::RenderAssetUsages,
    image::Image,
    light::NotShadowCaster,
    math::{ops, I64Vec3},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use rand::RngExt;

use super::wings::{spawn_winged, wing_mesh, Flap, WingedLook};
use super::{daylight, ease, eye, in_world, AmbientCritter, AmbientLife, CritterDice};
use crate::block::BlockType;
use crate::clock::TimeOfDay;
use crate::world::World;

pub(super) struct InsectPlugin;

impl Plugin for InsectPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_assets).add_systems(
            Update,
            (spawn_insects, fly_butterflies, fly_fireflies)
                .chain()
                .run_if(in_world),
        );
    }
}

// How many of each are about at most, and how far from the camera they're found and let go.
pub(super) const BUTTERFLIES: usize = 14;
pub(super) const FIREFLIES: usize = 60;
const NEAR: f32 = 26.0;
const FAR: f32 = 44.0;
// How many spots round the camera are looked at for a new one each frame.
const SEARCHES: usize = 3;
// How long one takes to appear, or to fade away once it's the wrong time of day, in seconds.
const FADE_SECONDS: f32 = 1.5;
// How much of the day it is, see `daylight`, when butterflies come out and fireflies leave.
const BUTTERFLY_DAY: f32 = 0.55;
const FIREFLY_NIGHT: f32 = 0.35;
// How far from water, across, fireflies are found outside the woods.
const FIREFLY_WATER_REACH: i64 = 6;

#[derive(Resource)]
struct InsectAssets {
    wing: Handle<Mesh>,
    body: Handle<Mesh>,
    body_material: Handle<StandardMaterial>,
    // one for each kind of butterfly
    wings: Vec<Handle<StandardMaterial>>,
    firefly: Handle<Mesh>,
    glow: Handle<StandardMaterial>,
}

/// One of them: where it keeps to, wandering round it, and how far it's faded in.
#[derive(Component)]
pub(super) struct Insect {
    home: Vec3,
    // where it's drifting its home to, see `fly_butterflies`
    heading_for: Vec3,
    phase: f32,
    life: f32,
    leaving: bool,
}

#[derive(Component)]
pub(super) struct Butterfly {
    // how fast it circles and flaps, so no two keep time
    pace: f32,
    // how long until it picks another flower to go to
    restless: f32,
}

/// A firefly, and how brightly it's lit up just now, 0..1, which its light follows, see `glow`.
#[derive(Component, Default)]
pub(super) struct Firefly {
    pub lit: f32,
}

// Butterflies' colours: the wing texture's white takes them, its black edges stay black.
const BUTTERFLY_COLOURS: [Color; 5] = [
    Color::srgb(1.0, 0.55, 0.1),
    Color::srgb(0.98, 0.97, 0.92),
    Color::srgb(0.35, 0.6, 1.0),
    Color::srgb(1.0, 0.88, 0.25),
    Color::srgb(0.85, 0.45, 0.9),
];

fn make_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let texture = images.add(wing_texture());
    let wings = BUTTERFLY_COLOURS
        .iter()
        .map(|&colour| {
            materials.add(StandardMaterial {
                base_color: colour,
                base_color_texture: Some(texture.clone()),
                alpha_mode: AlphaMode::Mask(0.5),
                double_sided: true,
                cull_mode: None,
                perceptual_roughness: 0.9,
                ..default()
            })
        })
        .collect();
    commands.insert_resource(InsectAssets {
        // from the hinge on the body out to the tip, its leading edge forward at -z
        wing: meshes.add(wing_mesh([
            [0.0, 0.0, -0.3],
            [0.45, 0.0, -0.3],
            [0.0, 0.0, 0.26],
            [0.45, 0.0, 0.26],
        ])),
        body: meshes.add(Cuboid::new(0.045, 0.045, 0.3)),
        body_material: materials.add(StandardMaterial {
            base_color: Color::srgb(0.08, 0.06, 0.05),
            perceptual_roughness: 0.9,
            ..default()
        }),
        wings,
        firefly: meshes.add(Sphere::new(0.06).mesh().ico(1).unwrap()),
        // bright past white, so the bloom haloes it, and as bright whatever the exposure
        glow: materials.add(StandardMaterial {
            base_color: Color::BLACK,
            emissive: LinearRgba::rgb(1.8, 3.2, 0.25),
            emissive_exposure_weight: 0.0,
            ..default()
        }),
    });
}

// A 16px wing, fore and hind lobes, white to take the butterfly's colour, with dark edges and
// a pale spot. u runs from the body out, v from front to back.
fn wing_texture() -> Image {
    const SIZE: usize = 16;
    let mut data = vec![0u8; SIZE * SIZE * 4];
    // centre and radii, in texels, of the two lobes
    let lobes = [((8.5, 5.0), (7.5, 5.0)), ((6.0, 11.5), (5.5, 4.0))];
    let inside = |x: f32, y: f32, grow: f32| {
        lobes.iter().any(|&((cx, cy), (rx, ry))| {
            let (dx, dy) = ((x - cx) / (rx + grow), (y - cy) / (ry + grow));
            dx * dx + dy * dy <= 1.0
        })
    };
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            if !inside(fx, fy, 0.0) {
                continue;
            }
            let edge = !inside(fx, fy, -1.3);
            let spot = (fx - 12.0).powi(2) + (fy - 4.0).powi(2) < 2.2;
            // veins from the body darken the wing a little toward it
            let vein = 0.82 + 0.18 * (fx / SIZE as f32);
            let shade = if spot {
                1.0
            } else if edge {
                0.08
            } else {
                vein
            };
            let v = (shade * 255.0) as u8;
            let at = (y * SIZE + x) * 4;
            data[at..at + 4].copy_from_slice(&[v, v, v, 255]);
        }
    }
    Image::new(
        Extent3d {
            width: SIZE as u32,
            height: SIZE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// A spot on the ground near `eye` that `wanted` takes, given its top block and where it is,
/// anywhere round it or mostly ahead along `facing`, so new ones turn up where they'll be seen.
/// At the middle of the top of the block.
pub(super) fn find_spot(
    world: &World,
    dice: &mut CritterDice,
    eye: Vec3,
    facing: Option<Vec3>,
    wanted: impl Fn(&World, BlockType, I64Vec3) -> bool,
) -> Option<Vec3> {
    let angle = match facing {
        Some(facing) => ops::atan2(facing.z, facing.x) + dice.range(-1.1..1.1),
        None => dice.angle(),
    };
    let reach = dice.range(3.0..NEAR);
    let (x, z) = (
        (eye.x + ops::cos(angle) * reach).floor() as i64,
        (eye.z + ops::sin(angle) * reach).floor() as i64,
    );
    let top = eye.y.floor() as i64 + 12;
    for y in (top - 52..=top).rev() {
        let at = I64Vec3::new(x, y, z);
        let block = world.get_block_at(at);
        if block == BlockType::Air {
            continue;
        }
        return wanted(world, block, at)
            .then(|| Vec3::new(x as f32 + 0.5, y as f32, z as f32 + 0.5));
    }
    None
}

/// The name of the biome owning the column at `x`, `z`, `base` for the base terrain's.
pub(super) fn biome_at(world: &World, x: f32, z: f32) -> &'static str {
    let noise = world.region_store.noise();
    let sample = noise.sample(x.into(), z.into());
    noise.biomes().name(sample.biome).unwrap_or("base")
}

/// Butterflies keep to flowers, wherever they grow.
pub(super) fn is_flower(_: &World, block: BlockType, _: I64Vec3) -> bool {
    matches!(
        block,
        BlockType::Poppy | BlockType::Dandelion | BlockType::Cornflower
    )
}

/// Fireflies keep to grass and plants in the woods, or anywhere mild near water.
pub(super) fn is_firefly_ground(world: &World, block: BlockType, at: I64Vec3) -> bool {
    if !(block == BlockType::Grass || (block.is_plant() && !block.is_crop())) {
        return false;
    }
    match biome_at(world, at.x as f32, at.z as f32) {
        "forest" | "jungle" => true,
        "desert" | "glacier" => false,
        _ => near_water(world, at),
    }
}

// Whether there's water at about the level of `at` within `FIREFLY_WATER_REACH` across: a few
// rings of blocks looked at, as a pond or a shore needn't be found exactly.
fn near_water(world: &World, at: I64Vec3) -> bool {
    const AROUND: [(i64, i64); 8] = [
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
        (0, -1),
        (1, -1),
    ];
    (2..=FIREFLY_WATER_REACH).step_by(2).any(|reach| {
        AROUND.iter().any(|&(dx, dz)| {
            (-3..=0).any(|dy| {
                let near = at + I64Vec3::new(dx * reach, dy, dz * reach);
                world.get_block_at(near) == BlockType::Water
            })
        })
    })
}

#[allow(clippy::too_many_arguments)]
fn spawn_insects(
    mut commands: Commands,
    assets: Option<Res<InsectAssets>>,
    world: Res<World>,
    clock: Res<TimeOfDay>,
    life: Res<AmbientLife>,
    mut dice: ResMut<CritterDice>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut insects: Query<(Entity, &mut Insect, &Transform, Has<Butterfly>)>,
) {
    let (Some(assets), Some(camera)) = (assets, eye(&camera)) else {
        return;
    };
    let eye = camera.translation();
    let ahead = Some(camera.forward().with_y(0.0));
    let day = daylight(&clock);
    let underwater = world.get_block_at(eye.floor().as_i64vec3()) == BlockType::Water;
    let unwanted = !life.wanted() || underwater;
    let (mut butterflies, mut fireflies) = (0, 0);
    for (entity, mut insect, transform, is_butterfly) in &mut insects {
        let wrong_time = if is_butterfly {
            day < BUTTERFLY_DAY
        } else {
            day > FIREFLY_NIGHT
        };
        if wrong_time || unwanted {
            insect.leaving = true;
        }
        if transform.translation.distance(eye) > FAR || (insect.leaving && insect.life <= 0.0) {
            commands.entity(entity).despawn();
            continue;
        }
        if is_butterfly {
            butterflies += 1;
        } else {
            fireflies += 1;
        }
    }
    if unwanted {
        return;
    }
    for _ in 0..SEARCHES {
        if day >= BUTTERFLY_DAY && butterflies < BUTTERFLIES {
            if let Some(flower) = find_spot(&world, &mut dice, eye, ahead, is_flower) {
                spawn_butterfly(&mut commands, &assets, &mut dice, flower + Vec3::Y * 1.2);
                butterflies += 1;
            }
        }
        if day <= FIREFLY_NIGHT && fireflies < FIREFLIES {
            if let Some(grass) = find_spot(&world, &mut dice, eye, ahead, is_firefly_ground) {
                let home = grass + Vec3::Y * dice.range(1.0..2.6);
                let insect = insect(home, &mut dice);
                commands.spawn((
                    insect,
                    Firefly::default(),
                    AmbientCritter,
                    Mesh3d(assets.firefly.clone()),
                    MeshMaterial3d(assets.glow.clone()),
                    Transform::from_translation(home).with_scale(Vec3::ZERO),
                    NotShadowCaster,
                ));
                fireflies += 1;
            }
        }
    }
}

fn insect(home: Vec3, dice: &mut CritterDice) -> Insect {
    Insect {
        home,
        heading_for: home,
        phase: dice.angle(),
        life: 0.0,
        leaving: false,
    }
}

fn spawn_butterfly(
    commands: &mut Commands,
    assets: &InsectAssets,
    dice: &mut CritterDice,
    home: Vec3,
) {
    let kind = dice.0.random_range(0..assets.wings.len());
    let pace = dice.range(0.8..1.25);
    let butterfly = (
        insect(home, dice),
        Butterfly {
            pace,
            restless: dice.range(4.0..12.0),
        },
    );
    let phase = dice.angle();
    spawn_winged(
        commands,
        butterfly,
        home,
        WingedLook {
            body: (&assets.body, &assets.body_material),
            wing: (&assets.wing, &assets.wings[kind]),
        },
        // quick, deep beats, wings held up a little between them
        Flap {
            beat: 19.0 * pace,
            sweep: 0.95,
            lift: 0.25,
            glides: false,
        },
        phase,
    );
}

// Fades an insect in, or out once it's leaving, and gives how big it is for that, 0..1.
fn fade(insect: &mut Insect, dt: f32) -> f32 {
    let step = dt / FADE_SECONDS;
    insect.life = if insect.leaving {
        (insect.life - step).max(0.0)
    } else {
        (insect.life + step).min(1.0)
    };
    ease(insect.life)
}

/// Where a butterfly is at `t` seconds: looping round its home, bobbing and dipping to land.
pub(super) fn butterfly_at(home: Vec3, phase: f32, pace: f32, t: f32) -> Vec3 {
    let t = t * pace;
    let angle = phase + t * 0.9 + 0.6 * ops::sin(t * 0.37 + phase);
    let radius = 1.2 + 0.8 * ops::sin(t * 0.53 + phase * 2.0);
    let bob = 0.35 * ops::sin(t * 2.6 + phase) + 0.12 * ops::sin(t * 7.3)
        - 0.5 * ops::sin(t * 0.21 + phase).max(0.0).powi(4);
    home + Vec3::new(
        ops::cos(angle) * radius,
        bob,
        ops::sin(angle * 1.3) * radius * 0.8,
    )
}

fn fly_butterflies(
    time: Res<Time>,
    world: Res<World>,
    mut dice: ResMut<CritterDice>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut butterflies: Query<(&mut Insect, &mut Butterfly, &mut Transform)>,
) {
    let (t, dt) = (time.elapsed_secs(), time.delta_secs());
    let eye = eye(&camera).map(|c| c.translation()).unwrap_or_default();
    for (mut insect, mut butterfly, mut transform) in &mut butterflies {
        // now and then it's off to another flower nearby
        butterfly.restless -= dt;
        if butterfly.restless <= 0.0 {
            butterfly.restless = dice.range(6.0..16.0);
            if let Some(flower) = find_spot(&world, &mut dice, eye, None, is_flower) {
                if flower.distance(insect.home) < 12.0 {
                    insect.heading_for = flower + Vec3::Y * 1.2;
                }
            }
        }
        let drift = 1.0 - ops::exp(-dt * 0.4);
        insect.home = insect.home.lerp(insect.heading_for, drift);

        let size = fade(&mut insect, dt);
        let (here, ahead) = (
            butterfly_at(insect.home, insect.phase, butterfly.pace, t),
            butterfly_at(insect.home, insect.phase, butterfly.pace, t + 0.1),
        );
        transform.translation = here;
        let course = (ahead - here).with_y(0.0);
        if course.length_squared() > 1e-6 {
            transform.look_to(course, Vec3::Y);
        }
        transform.scale = Vec3::splat(size);
    }
}

/// Where a firefly is at `t` seconds: a slow, lazy drift round its home, rising and sinking.
pub(super) fn firefly_at(home: Vec3, phase: f32, t: f32) -> Vec3 {
    let p = phase;
    home + Vec3::new(
        1.6 * ops::sin(t * 0.23 + p) + 0.4 * ops::sin(t * 0.9 + p * 3.0),
        0.5 * ops::sin(t * 0.31 + p * 2.0),
        1.6 * ops::cos(t * 0.19 + p * 1.7) + 0.4 * ops::cos(t * 0.8 + p),
    )
}

/// How lit up a firefly is at `t` seconds, 0..1: each glows up and fades in its own time, dark
/// more than lit.
pub(super) fn firefly_blink(phase: f32, t: f32) -> f32 {
    ops::sin(t * (0.9 + 0.3 * ops::sin(phase)) + phase * 5.0)
        .max(0.0)
        .powi(3)
}

fn fly_fireflies(
    time: Res<Time>,
    mut fireflies: Query<(&mut Insect, &mut Firefly, &mut Transform)>,
) {
    let (t, dt) = (time.elapsed_secs(), time.delta_secs());
    for (mut insect, mut firefly, mut transform) in &mut fireflies {
        let size = fade(&mut insect, dt);
        transform.translation = firefly_at(insect.home, insect.phase, t);
        let blink = firefly_blink(insect.phase, t);
        transform.scale = Vec3::splat(size * (0.15 + 0.85 * blink));
        firefly.lit = size * blink;
    }
}
