//! Birds: a murmuration of starlings wheeling over open land at dusk, and by day now and then a
//! skein of geese migrating high over in a V, every skein of a day the same way.

use crate::client::player::WorldCamera;
use bevy::{math::ops, prelude::*};
use rand::RngExt;

use super::insects::biome_at;
use super::wings::{spawn_winged, wing_mesh, Flap, WingedLook};
use super::{daylight, dusk, ease, eye, in_world, AmbientLife, CritterDice};
use crate::chunks::generate::WATER_LEVEL;
use crate::clock::TimeOfDay;
use crate::world::World;

pub(super) struct BirdPlugin;

impl Plugin for BirdPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Murmuration>()
            .add_systems(Startup, make_bird_assets)
            .add_systems(
                Update,
                (gather_flock, flock, fly_geese).chain().run_if(in_world),
            );
    }
}

// How many starlings are in the flock, how far round the camera it wheels, in blocks, and how
// high over the ground or sea.
pub(super) const STARLINGS: usize = 160;
const FLOCK_RANGE: f32 = 55.0;
const FLOCK_HEIGHT: f32 = 34.0;
// The starlings' speeds, in blocks a second, and how near they keep with and clear of others.
pub(super) const SPEED: (f32, f32) = (9.0, 15.0);
const NEIGHBOURS: f32 = 7.0;
const PERSONAL_SPACE: f32 = 3.2;
// How hard each starling steers to keep with its neighbours' heading, toward their middle,
// away from those too near, and toward where the flock's heading.
const ALIGN: f32 = 1.4;
const COHERE: f32 = 0.25;
const SEPARATE: f32 = 9.0;
const HOME: f32 = 0.55;
// How long the flock takes to gather or leave, in seconds.
const FADE_SECONDS: f32 = 3.0;
// How often whether the land round the camera is open is looked at again, in seconds.
const LOOK_ROUND_SECONDS: f32 = 1.0;
// How much of the day it is, see `daylight`, that geese fly in.
const GEESE_DAY: f32 = 0.5;

#[derive(Resource)]
struct BirdAssets {
    starling_wing: Handle<Mesh>,
    starling_body: Handle<Mesh>,
    starling: Handle<StandardMaterial>,
    goose_wing: Handle<Mesh>,
    goose_body: Handle<Mesh>,
    goose: Handle<StandardMaterial>,
    goose_wings: Handle<StandardMaterial>,
}

/// The flock, and how far it's gathered, 0..1.
#[derive(Resource, Default)]
pub(super) struct Murmuration {
    gathered: f32,
    // whether it's the time and place for starlings, or they're leaving
    about: bool,
    // whether the land round the camera is open, as last looked at, and when to look again
    open: bool,
    look_round: f32,
    // where it's heading round the camera, which wanders on so the flock never settles
    clock: f32,
}

#[derive(Component)]
pub(super) struct Starling {
    velocity: Vec3,
}

#[derive(Component)]
pub(super) struct Goose {
    pub velocity: Vec3,
    travelled: f32,
    phase: f32,
}

fn make_bird_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let matte = |materials: &mut Assets<StandardMaterial>, colour: Color| {
        materials.add(StandardMaterial {
            base_color: colour,
            perceptual_roughness: 0.95,
            double_sided: true,
            cull_mode: None,
            ..default()
        })
    };
    commands.insert_resource(BirdAssets {
        starling_wing: meshes.add(swept_wing(0.42, 0.16)),
        starling_body: meshes.add(Cuboid::new(0.1, 0.08, 0.32)),
        starling: matte(&mut materials, Color::srgb(0.05, 0.05, 0.06)),
        goose_wing: meshes.add(swept_wing(0.95, 0.32)),
        goose_body: meshes.add(Cuboid::new(0.22, 0.2, 0.9)),
        goose: matte(&mut materials, Color::srgb(0.32, 0.28, 0.24)),
        goose_wings: matte(&mut materials, Color::srgb(0.42, 0.38, 0.33)),
    });
}

// A swept-back wing `span` long and `chord` deep at the body.
fn swept_wing(span: f32, chord: f32) -> Mesh {
    wing_mesh([
        [0.0, 0.0, -chord * 0.5],
        [span * 0.55, 0.0, -chord * 0.15],
        [0.0, 0.0, chord * 0.5],
        [span, 0.0, chord * 0.55],
    ])
}

/// Whether the land round `eye` is open, as starlings like it: grassland, foothills or
/// savanna, or the sea, and not under ground.
pub(super) fn open_land(world: &World, eye: Vec3) -> bool {
    if in_a_cave(world, eye) {
        return false;
    }
    matches!(
        biome_at(world, eye.x, eye.z),
        "base" | "foothills" | "savanna"
    )
}

/// Whether `eye` is in a cave, as the ambience has it: under cover and 3 blocks below the
/// generated surface, so not just under a tree.
pub(super) fn in_a_cave(world: &World, eye: Vec3) -> bool {
    let surface = world
        .region_store
        .get_height(eye.x.floor() as i64, eye.z.floor() as i64) as f32;
    eye.y < surface - 3.0 && world.is_under_ground(eye.floor().as_i64vec3())
}

// The flock gathers round the camera at dusk over open land, and leaves once it's over or the
// land isn't open any more.
#[allow(clippy::too_many_arguments)]
fn gather_flock(
    mut commands: Commands,
    time: Res<Time>,
    clock: Res<TimeOfDay>,
    world: Res<World>,
    life: Res<AmbientLife>,
    mut dice: ResMut<CritterDice>,
    assets: Option<Res<BirdAssets>>,
    mut murmuration: ResMut<Murmuration>,
    camera: Query<&GlobalTransform, WorldCamera>,
    starlings: Query<Entity, With<Starling>>,
) {
    let (Some(assets), Some(camera)) = (assets, eye(&camera)) else {
        return;
    };
    let eye = camera.translation();
    let dt = time.delta_secs();
    murmuration.look_round -= dt;
    if murmuration.look_round <= 0.0 {
        murmuration.look_round = LOOK_ROUND_SECONDS;
        murmuration.open = open_land(&world, eye);
    }
    let about = life.wanted() && dusk(&clock) > 0.0 && murmuration.open;
    murmuration.about = about;
    let step = dt / FADE_SECONDS;
    murmuration.gathered =
        (murmuration.gathered + if about { step } else { -step }).clamp(0.0, 1.0);
    if about && starlings.is_empty() {
        // they arrive together, from somewhere off to one side
        let angle = dice.angle();
        let centre = eye
            + Vec3::new(ops::cos(angle), 0.0, ops::sin(angle)) * FLOCK_RANGE
            + Vec3::Y * FLOCK_HEIGHT;
        let heading = Vec3::new(-ops::sin(angle), 0.0, ops::cos(angle));
        for _ in 0..STARLINGS {
            let at = centre
                + Vec3::new(
                    dice.range(-4.0..4.0),
                    dice.range(-2.0..2.0),
                    dice.range(-4.0..4.0),
                );
            // quick beats, now and then gliding with their wings out
            let flap = Flap {
                beat: dice.range(16.0..20.0),
                sweep: 0.8,
                lift: 0.1,
                glides: true,
            };
            let phase = dice.angle();
            spawn_winged(
                &mut commands,
                Starling {
                    velocity: heading * SPEED.0,
                },
                at,
                WingedLook {
                    body: (&assets.starling_body, &assets.starling),
                    wing: (&assets.starling_wing, &assets.starling),
                },
                flap,
                phase,
            );
        }
    } else if !about && murmuration.gathered <= 0.0 {
        for starling in &starlings {
            commands.entity(starling).despawn();
        }
    }
}

/// Where the flock heads at `t` seconds, wandering round `eye` far enough off to be seen whole.
pub(super) fn flock_heading(eye: Vec3, t: f32) -> Vec3 {
    let a = t * 0.11 + 0.8 * ops::sin(t * 0.05);
    let radius = FLOCK_RANGE * (0.75 + 0.25 * ops::sin(t * 0.17));
    eye + Vec3::new(ops::cos(a) * radius, 0.0, ops::sin(a) * radius)
}

/// The flock a step of `dt` seconds on, each bird's place and velocity, heading for `target`
/// and keeping 8 blocks over `ground`, with how hard each steered.
pub(super) fn step_flock(
    birds: &mut [(Vec3, Vec3)],
    target: Vec3,
    ground: f32,
    dt: f32,
) -> Vec<Vec3> {
    // every bird steers by where the rest were, then they all move
    let steers: Vec<Vec3> = (0..birds.len())
        .map(|i| steering(i, birds, target))
        .collect();
    for ((here, velocity), steer) in birds.iter_mut().zip(&steers) {
        let mut moving = *velocity + *steer * dt;
        let speed = moving.length().clamp(SPEED.0, SPEED.1);
        moving = moving.normalize_or(Vec3::X) * speed;
        // never down into the land
        if here.y < ground + 8.0 {
            moving.y = moving.y.max(3.0);
        }
        *here += moving * dt;
        *velocity = moving;
    }
    steers
}

// Each starling heads with its neighbours, toward their middle and out of their way, while
// all of them steer after the flock's heading. Nothing leads, so it wheels and folds.
fn flock(
    time: Res<Time>,
    world: Res<World>,
    mut murmuration: ResMut<Murmuration>,
    camera: Query<&GlobalTransform, WorldCamera>,
    mut starlings: Query<(&mut Starling, &mut Transform)>,
) {
    let Some(camera) = eye(&camera) else {
        return;
    };
    if starlings.is_empty() {
        return;
    }
    let dt = time.delta_secs().min(0.05);
    murmuration.clock += dt;
    let eye = camera.translation();
    let heading = flock_heading(eye, murmuration.clock);
    let ground = world
        .region_store
        .get_height(heading.x.floor() as i64, heading.z.floor() as i64)
        .max(WATER_LEVEL as f64) as f32;
    let mut target = heading
        .with_y(ground.max(eye.y - 20.0) + FLOCK_HEIGHT + 6.0 * ops::sin(murmuration.clock * 0.3));
    // leaving, they make off together, up and away, fading as they go
    if !murmuration.about {
        target = eye + (heading - eye).with_y(0.0).normalize_or(Vec3::X) * 400.0 + Vec3::Y * 120.0;
    }

    let mut birds: Vec<(Vec3, Vec3)> = starlings
        .iter()
        .map(|(starling, transform)| (transform.translation, starling.velocity))
        .collect();
    let steers = step_flock(&mut birds, target, ground, dt);
    let size = ease(murmuration.gathered);
    for (i, (mut starling, mut transform)) in starlings.iter_mut().enumerate() {
        let (here, velocity) = birds[i];
        starling.velocity = velocity;
        transform.translation = here;
        transform.look_to(velocity, Vec3::Y);
        // banking into its turns
        let bank = steers[i].dot(transform.right().as_vec3()) * 0.04;
        transform.rotate_local_z(-bank.clamp(-0.9, 0.9));
        transform.scale = Vec3::splat(size);
    }
}

/// How hard bird `i` steers, in blocks a second squared, given every bird's place and velocity.
pub(super) fn steering(i: usize, birds: &[(Vec3, Vec3)], target: Vec3) -> Vec3 {
    let (here, velocity) = birds[i];
    let (mut heading, mut middle, mut away, mut count) = (Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, 0.0);
    for (j, &(there, their_velocity)) in birds.iter().enumerate() {
        if j == i {
            continue;
        }
        let between = there - here;
        let distance_squared = between.length_squared();
        if distance_squared < NEIGHBOURS * NEIGHBOURS {
            heading += their_velocity;
            middle += there;
            count += 1.0;
            if distance_squared < PERSONAL_SPACE * PERSONAL_SPACE {
                away -= between / distance_squared.max(0.01);
            }
        }
    }
    let mut steer = (target - here).normalize_or_zero() * SPEED.1 * HOME;
    if count > 0.0 {
        steer += (heading / count - velocity) * ALIGN;
        steer += (middle / count - here) * COHERE;
    }
    steer + away * SEPARATE
}

// How often a skein of geese flies over, in seconds, and how high over the ground (and at least
// over the camera), how fast and how far, in blocks.
const SKEIN_SECONDS: (f32, f32) = (45.0, 110.0);
const FIRST_SKEIN_IN: f32 = 12.0;
const SKEIN_HEIGHT: f32 = 64.0;
const SKEIN_OVER_EYE: f32 = 36.0;
const GOOSE_SPEED: f32 = 11.0;
const SKEIN_REACH: f32 = 230.0;
// How far each skein's way strays from the day's, in radians.
const SKEIN_STRAY: f32 = 0.25;

/// The way every skein of geese flies on day `day` of the world seeded `seed`, as they would
/// migrating: the same all day, so they're seen passing over one after another the same way.
pub(super) fn migration_course(seed: u32, day: i64) -> Vec3 {
    // splitmix64, as any good mix of the two will do
    let mut h = (seed as u64) ^ (day as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h = (h ^ (h >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h = (h ^ (h >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    h ^= h >> 31;
    let angle = (h >> 40) as f32 / (1u64 << 24) as f32 * std::f32::consts::TAU;
    Vec3::new(ops::cos(angle), 0.0, ops::sin(angle))
}

/// Where the `k`th goose of a skein flies, from the leader's place, for a skein flying along
/// `course`: the leader at the point, the rest down either arm of the V behind it.
pub(super) fn skein_place(k: u32, course: Vec3) -> Vec3 {
    let side = Vec3::new(-course.z, 0.0, course.x);
    let rank = k.div_ceil(2) as f32;
    let arm = if k.is_multiple_of(2) { 1.0 } else { -1.0 };
    -course * rank * 2.4 + side * arm * rank * 2.0 + Vec3::Y * rank * 0.3
}

#[allow(clippy::too_many_arguments)]
fn fly_geese(
    mut commands: Commands,
    time: Res<Time>,
    clock: Res<TimeOfDay>,
    world: Res<World>,
    life: Res<AmbientLife>,
    mut dice: ResMut<CritterDice>,
    assets: Option<Res<BirdAssets>>,
    camera: Query<&GlobalTransform, WorldCamera>,
    mut geese: Query<(Entity, &mut Goose, &mut Transform)>,
    mut next: Local<Option<f32>>,
) {
    let (Some(assets), Some(camera)) = (assets, eye(&camera)) else {
        return;
    };
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    for (entity, mut goose, mut transform) in &mut geese {
        goose.travelled += GOOSE_SPEED * dt;
        transform.translation += goose.velocity * dt;
        // a gentle rise and fall, each in its own time
        transform.translation.y += ops::sin(t * 1.1 + goose.phase) * 0.15 * dt;
        let grown = (goose.travelled / 20.0).min(1.0);
        let gone = ((2.0 * SKEIN_REACH - goose.travelled) / 20.0).clamp(0.0, 1.0);
        transform.scale = Vec3::splat(grown.min(gone));
        transform.look_to(goose.velocity, Vec3::Y);
        if goose.travelled > 2.0 * SKEIN_REACH {
            commands.entity(entity).despawn();
        }
    }

    let due = next.get_or_insert(t + FIRST_SKEIN_IN);
    if t < *due {
        return;
    }
    *due = t + dice.range(SKEIN_SECONDS.0..SKEIN_SECONDS.1);
    let eye = camera.translation();
    if !life.wanted() || daylight(&clock) < GEESE_DAY || in_a_cave(&world, eye) {
        return;
    }
    // the day's way, passing over somewhere near the camera, to one side or the other
    let day = clock.day() as i64;
    let course = Quat::from_rotation_y(dice.range(-SKEIN_STRAY..SKEIN_STRAY))
        * migration_course(world.seed(), day);
    let side = Vec3::new(-course.z, 0.0, course.x);
    let passing = eye + side * dice.range(-70.0..70.0);
    let ground = world
        .region_store
        .get_height(passing.x.floor() as i64, passing.z.floor() as i64)
        .max(WATER_LEVEL as f64) as f32;
    let passing = passing.with_y((ground + SKEIN_HEIGHT).max(eye.y + SKEIN_OVER_EYE));
    let start = passing - course * SKEIN_REACH;
    let count: u32 = dice.0.random_range(7..12);
    for k in 0..count {
        // slow, deep strokes
        let flap = Flap {
            beat: dice.range(5.5..6.5),
            sweep: 0.55,
            lift: 0.1,
            glides: true,
        };
        let phase = dice.angle();
        spawn_winged(
            &mut commands,
            Goose {
                velocity: course * GOOSE_SPEED,
                travelled: 0.0,
                phase,
            },
            start + skein_place(k, course),
            WingedLook {
                body: (&assets.goose_body, &assets.goose),
                wing: (&assets.goose_wing, &assets.goose_wings),
            },
            flap,
            phase,
        );
    }
}
