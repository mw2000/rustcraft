//! Creatures as this client sees them (§5.8): each where its poses put it, drawn a little behind
//! time as other players are (`client::others`), as boxes of its kind's model, smaller while
//! it's young, without its fleece once it's shorn, and with a heart over it while it's in love.
//! They live as they go (M-A2, `client::animation`): turning smoothly, legs swinging and easing
//! with their speed, breathing, blinking, looking at players near them, grazing, swaying their
//! tails, flapping as they flutter, flinching and flashing red as they're hurt, and tipping over
//! and fading as they die. Monsters' eyes glow, they rear back and raise their arms as they wind
//! up a blow and lunge into it, they hang upside down from ceilings, and those that vanish
//! rather than die just fade (M3.3); their missiles fly where they were thrown. Every creature
//! of a kind shares its meshes and materials, so Bevy draws them batched, and each frame changes
//! only their parts' transforms.

use std::f32::consts::{PI, TAU};

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use super::animation::{
    blinking, breath, death, ease, ease_angle, flap, flinch, grazing, hash, head_look,
    stride_swing, tail, GRAZE_PITCH,
};
use super::model_light::{coloured, ModelLook, ModelMaterial, ModelMaterials};
use super::others::{move_seen, Pose, Poses, SeenPlayer};
use super::player::LocalPlayer;
use crate::clouds::spawn_clouds;
use crate::collision::{ray_distance, Aabb};
use crate::creatures::{
    Condition, Creature, CreatureKind, CreaturePose, Flinches, Joint, Look, Missile, Part, Stance,
    Vanishing, YOUNG_SCALE,
};
use crate::player::{PlayerLook, EYE_HEIGHT};
use crate::server::is_authority;

// How long a creature flashes red when it's hurt, in seconds.
const FLASH: f32 = 0.3;
// How much redder it's drawn then, and the red.
const FLASH_RED: f32 = 0.6;
const HURT_RED: Color = Color::srgb(1.0, 0.1, 0.1);
// How far the legs swing at a walk, in radians, and how fast they swing fully, in blocks a
// second, about a creature's walk.
const SWING: f32 = 0.7;
const FULL_SWING_SPEED: f32 = 1.4;
// Radians of stride a block walked, for a leg a block long: a player's 0.75 swings 2.2.
const STRIDE: f32 = 1.65;
// How quickly a body turns to the way it's going, and a head to what it looks at, as the
// fraction of the way left that's turned a second (`animation::ease`).
const TURNING: f32 = 10.0;
const LOOKING: f32 = 6.0;
// How near a player must be, in blocks, for a creature to look at them.
const LOOK_RANGE: f32 = 6.0;
// How far a head and body bob as it walks, in radians and blocks, at a full stride.
const HEAD_BOB: f32 = 0.08;
const BODY_BOB: f32 = 0.03;
// A young creature's head is drawn this much bigger for its size.
const YOUNG_HEAD: f32 = 1.3;
// How fast a creature falls, in blocks a second, before its wings flap, and how fast it runs
// before they half do.
const FLUTTERING: f32 = 0.5;
const FLAPPING_RUN: f32 = 2.0;
// How far over a creature's model the heart it shows in love is, in blocks, and how far it
// bobs.
const HEART_OVER: f32 = 0.35;
const HEART_BOB: f32 = 0.05;
const HEART: Color = Color::srgb(0.9, 0.18, 0.26);
// A creature that goes while it's this near this client's player, in blocks, has died, as
// nothing else takes one away so near: they sleep and leave a client's sight beyond 96.
const DEATH_RANGE: f32 = 48.0;
// How many steps a dying creature fades out in, each a material of its own.
const FADE_STEPS: usize = 4;
// How much of a leg's swing an arm swings, against it, as a creature walks.
const ARM_SWING: f32 = 0.6;
// Winding up a blow: how far the body rears back and the arms rise, in radians, and how
// quickly, as `animation::ease` takes it; and the blow itself, how far the body lunges into it
// and the arms come down past hanging, and how long it takes, in seconds.
const WIND_REAR: f32 = 0.3;
const WIND_ARMS: f32 = 2.5;
const WINDING: f32 = 8.0;
const STRIKE_LUNGE: f32 = 0.35;
const STRIKE_ARMS: f32 = 0.5;
const STRIKE: f32 = 0.25;
// How big a missile's drawn, in blocks, and its colour, a stone's.
const MISSILE: f32 = 0.2;
const STONE: Color = Color::srgb(0.48, 0.47, 0.46);

/// A creature as this client sees it, drawn where its poses put it, on an entity of its own.
/// Its look's pitch is always level.
#[derive(Component)]
#[relationship(relationship_target = CreatureSeen)]
#[require(Poses, PlayerLook, Transform, Visibility = Visibility::Hidden)]
pub struct SeenCreature(pub Entity);

/// On a creature, what this client sees of it. When the creature goes, what's seen of it does
/// too, unless it died near enough to be seen dying (`Dying`).
#[derive(Component)]
#[relationship_target(relationship = SeenCreature)]
pub struct CreatureSeen(Entity);

/// What's seen of a creature that's died, and how many seconds ago.
#[derive(Component, Debug, Default)]
pub struct Dying(pub f32);

/// On what's seen of a creature, its kind, which stays as it dies.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeenKind(pub CreatureKind);

/// On what's seen of a creature that's vanishing rather than dying (`Vanishing`), so it only
/// fades away where it was, without tipping over or a cry.
#[derive(Component, Debug, Default)]
pub struct Fading;

/// A missile this client's drawing, and how many seconds ago it saw it thrown.
#[derive(Component, Debug, Default)]
pub struct Thrown(f32);

/// A creature seen that's just been hurt, and for how much longer it flashes.
#[derive(Component)]
struct Flash(f32);

fn pose(pose: &CreaturePose) -> Pose {
    Pose {
        at: pose.at.into(),
        position: pose.position,
        yaw: pose.yaw,
        pitch: 0.0,
    }
}

// Creatures just arrived, unless they're already vanishing.
type Arrived = (Added<Creature>, Without<Vanishing>);

/// Sees each creature that arrives, as the server spawns it or it's shown to this client, from
/// where it is then, unless it's already vanishing.
pub(super) fn see_creatures(
    arrived: Query<(Entity, &Creature, &CreaturePose), Arrived>,
    mut commands: Commands,
) {
    for (creature, &Creature(kind), now) in &arrived {
        let mut poses = Poses::default();
        poses.add(pose(now));
        commands.spawn((SeenCreature(creature), SeenKind(kind), poses));
    }
}

/// Marks what's seen of a creature that's vanishing as only fading, once it goes.
pub(super) fn see_vanish(
    vanishing: On<Add<Vanishing>>,
    creatures: Query<&CreatureSeen>,
    mut commands: Commands,
) {
    if let Ok(seen) = creatures.get(vanishing.entity) {
        commands.entity(seen.0).try_insert(Fading);
    }
}

/// Keeps the poses of the creatures seen as they come, and flashes those hurt.
pub(super) fn follow_creatures(
    creatures: Query<(Ref<CreaturePose>, Ref<Flinches>, &CreatureSeen)>,
    mut poses: Query<&mut Poses>,
    mut commands: Commands,
) {
    for (now, flinches, seen) in &creatures {
        if now.is_changed() && !now.is_added() {
            if let Ok(mut poses) = poses.get_mut(seen.0) {
                poses.add(pose(&now));
            }
        }
        if flinches.is_changed() && !flinches.is_added() {
            commands.entity(seen.0).insert(Flash(FLASH));
        }
    }
}

/// Lets what's seen of a creature that's gone die where it was, or fade if it vanished, if it
/// was near this client's player, and otherwise takes it away with it.
pub(super) fn see_off(
    gone: On<Remove<SeenCreature>>,
    seen: Query<&Transform>,
    player: Query<&Transform, With<LocalPlayer>>,
    mut commands: Commands,
) {
    let near = match (seen.get(gone.entity), player.single()) {
        (Ok(body), Ok(player)) => body.translation.distance(player.translation) < DEATH_RANGE,
        _ => false,
    };
    if near {
        commands.entity(gone.entity).try_insert(Dying::default());
    } else {
        commands.entity(gone.entity).try_despawn();
    }
}

/// Takes away what's seen of each dead creature once it's had time to die.
pub(super) fn die_away(
    time: Res<Time>,
    mut dying: Query<(Entity, &mut Dying)>,
    mut commands: Commands,
) {
    for (entity, mut dying) in &mut dying {
        dying.0 += time.delta_secs();
        if death(dying.0).is_none() {
            commands.entity(entity).despawn();
        }
    }
}

/// The nearest creature seen that the ray from `origin` along `direction` passes through within
/// `reach`, as the creature itself, and how far along the ray it is. Each is seen with the body
/// it has.
pub fn creature_hit<'a>(
    origin: Vec3,
    direction: Vec3,
    reach: f32,
    seen: impl IntoIterator<Item = (&'a SeenCreature, &'a Transform, Aabb)>,
) -> Option<(Entity, f32)> {
    seen.into_iter()
        .filter_map(|(seen, body, aabb)| {
            let distance = ray_distance(origin, direction, body.translation, aabb)?;
            (distance <= reach).then_some((seen.0, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// Draws creatures. Needs the renderer.
pub struct DrawCreaturesPlugin;

impl Plugin for DrawCreaturesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_looks.after(spawn_clouds))
            .add_observer(draw_creature)
            .add_systems(
                Update,
                (
                    (animate, show_death).after(move_seen),
                    (flash, show_condition),
                    (show_missiles, fly_missiles.run_if(not(is_authority))).chain(),
                ),
            );
    }
}

/// What each kind of creature's drawn with, and the heart shown over those in love.
#[derive(Resource)]
struct CreatureLooks {
    kinds: HashMap<CreatureKind, KindLook>,
    heart: Handle<Mesh>,
    heart_colour: Handle<ModelMaterial>,
    missile: Handle<Mesh>,
    stone: Handle<ModelMaterial>,
}

// A kind's parts, where the middle of its body breathes from, and how far over its feet the
// heart goes.
struct KindLook {
    parts: Vec<PartLook>,
    trunk: Vec3,
    heart: f32,
}

// A part's mesh, about the point it turns on, and that point from what it hangs from: the
// body's middle for the body, the neck for the head, and the feet for the rest. Its materials
// are as it is, flashing, and fading in steps as it dies.
struct PartLook {
    mesh: Handle<Mesh>,
    at: Vec3,
    joint: Joint,
    look: Look,
    material: Handle<ModelMaterial>,
    hurt: Handle<ModelMaterial>,
    fading: [Handle<ModelMaterial>; FADE_STEPS],
}

/// A box of a creature drawn, what its boxes hang from, or the heart over it.
#[derive(Component)]
struct CreaturePart;

// A creature drawn: its parts in its model's order, those that move by themselves, and how
// they're moving, as of the last frame.
#[derive(Component)]
struct Model {
    kind: CreatureKind,
    parts: Vec<Entity>,
    trunk: Entity,
    head: Entity,
    legs: Vec<(Entity, f32)>,
    arms: Vec<(Entity, f32)>,
    tails: Vec<Entity>,
    wings: Vec<(Entity, f32)>,
    eyes: Vec<Entity>,
    fleece: Vec<Entity>,
    heart: Entity,
    heart_at: f32,
    // what makes it unlike the rest
    seed: u32,
    stride_per_block: f32,
    stride: f32,
    swing: f32,
    flutter: f32,
    // how far it's wound up a blow, from 0 to 1, whether it was winding one up last frame, and
    // how long ago the blow it wound up came
    wound: f32,
    winding: bool,
    struck: Option<f32>,
    last: Vec3,
    // which way its body's shown facing, and its head turned from that, across and up
    yaw: Option<f32>,
    look: Vec2,
    blinking: bool,
    flashing: bool,
    faded: usize,
    shown: Option<Condition>,
}

// Where a part turns: a body's or a head's box about its middle, a leg from its top, a tail
// from where it meets the body, and a wing from its top along the body.
fn pivot(part: &Part) -> Vec3 {
    let half = part.size / 2.0;
    part.centre
        + match part.joint {
            Joint::Body | Joint::Head => Vec3::ZERO,
            Joint::Leg(_) | Joint::Arm(_) => Vec3::Y * half.y,
            Joint::Tail => Vec3::new(0.0, half.y, -half.z),
            Joint::Wing(side) => Vec3::new(-side * half.x, half.y, 0.0),
        }
}

// A part's materials: as it is, flashing, and fading in steps as it dies.
type Materials = (
    Handle<ModelMaterial>,
    Handle<ModelMaterial>,
    [Handle<ModelMaterial>; FADE_STEPS],
);

fn make_looks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ModelMaterials,
) {
    // every kind's parts are drawn with these few, lit as the land is where they are or, for
    // those that glow, as bright as they are whatever the light, so they're drawn together:
    // the colours are their meshes'
    let mut shades = |look: ModelLook| -> Materials {
        let fading = std::array::from_fn(|step| {
            let left = 1.0 - (step + 1) as f32 / (FADE_STEPS + 1) as f32;
            materials.add(look.clone().faded(left))
        });
        (
            materials.add(look.clone()),
            materials.add(look.flashing(HURT_RED, FLASH_RED)),
            fading,
        )
    };
    let (lit, glowing) = (shades(ModelLook::lit()), shades(ModelLook::glowing()));
    let mut kinds = HashMap::new();
    for &kind in CreatureKind::ALL {
        let properties = kind.properties();
        // the body breathes from the middle of its biggest box
        let volume = |part: &&Part| part.size.element_product();
        let trunk = properties
            .model
            .iter()
            .filter(|part| part.joint == Joint::Body)
            .max_by(|a, b| volume(a).total_cmp(&volume(b)))
            .map_or(Vec3::ZERO, |part| part.centre);
        let mut parts = Vec::new();
        for part in properties.model {
            let pivot = pivot(part);
            let hangs_from = match part.joint {
                Joint::Body => trunk,
                Joint::Head => properties.neck,
                _ => Vec3::ZERO,
            };
            let [r, g, b] = part.colour;
            let mesh = Mesh::from(Cuboid::from_size(part.size)).translated_by(part.centre - pivot);
            let mesh = coloured(mesh, Color::srgb(r, g, b));
            let (material, hurt, fading) = if part.look == Look::Glow {
                glowing.clone()
            } else {
                lit.clone()
            };
            parts.push(PartLook {
                mesh: meshes.add(mesh),
                at: pivot - hangs_from,
                joint: part.joint,
                look: part.look,
                material,
                hurt,
                fading,
            });
        }
        let top = properties
            .model
            .iter()
            .map(|part| part.centre.y + part.size.y / 2.0);
        let heart = top.fold(0.0, f32::max) + HEART_OVER;
        kinds.insert(
            kind,
            KindLook {
                parts,
                trunk,
                heart,
            },
        );
    }
    // two lobes over a square stood on its corner, a fifth of a block across
    let lobe = |x: f32| Mesh::from(Cuboid::from_length(0.1)).translated_by(Vec3::new(x, 0.04, 0.0));
    let mut heart = Mesh::from(Cuboid::from_length(0.1))
        .rotated_by(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4))
        .translated_by(Vec3::Y * -0.02);
    for x in [-0.045, 0.045] {
        heart.merge(&lobe(x)).expect("boxes merge");
    }
    commands.insert_resource(CreatureLooks {
        kinds,
        heart: meshes.add(coloured(heart, HEART)),
        heart_colour: glowing.0,
        missile: meshes.add(coloured(Cuboid::from_length(MISSILE).into(), STONE)),
        stone: lit.0,
    });
}

fn draw_creature(
    add: On<Add<SeenCreature>>,
    seen: Query<&SeenCreature>,
    creatures: Query<&Creature>,
    looks: Res<CreatureLooks>,
    mut commands: Commands,
) {
    let Ok(&Creature(kind)) = seen.get(add.entity).and_then(|seen| creatures.get(seen.0)) else {
        return;
    };
    let look = &looks.kinds[&kind];
    let properties = kind.properties();
    let mut joint = |at: Vec3, parent: Entity| {
        let transform = Transform::from_translation(at);
        let joint = (
            CreaturePart,
            transform,
            Visibility::Inherited,
            ChildOf(parent),
        );
        commands.spawn(joint).id()
    };
    let trunk = joint(look.trunk, add.entity);
    let head = joint(properties.neck, add.entity);
    let heart = joint(Vec3::Y * look.heart, add.entity);
    let leg = properties.model.iter().find_map(|part| match part.joint {
        Joint::Leg(_) => Some(part.size.y),
        _ => None,
    });
    let bits = add.entity.to_bits();
    let mut model = Model {
        kind,
        parts: Vec::with_capacity(look.parts.len()),
        trunk,
        head,
        legs: Vec::new(),
        arms: Vec::new(),
        tails: Vec::new(),
        wings: Vec::new(),
        eyes: Vec::new(),
        fleece: Vec::new(),
        heart,
        heart_at: look.heart,
        seed: hash(bits as u32, (bits >> 32) as u32).to_bits(),
        stride_per_block: STRIDE / leg.unwrap_or(1.0),
        stride: 0.0,
        swing: 0.0,
        flutter: 0.0,
        wound: 0.0,
        winding: false,
        struck: None,
        last: Vec3::ZERO,
        yaw: None,
        look: Vec2::ZERO,
        blinking: false,
        flashing: false,
        faded: 0,
        shown: None,
    };
    commands.entity(heart).insert((
        Mesh3d(looks.heart.clone()),
        MeshMaterial3d(looks.heart_colour.clone()),
        Visibility::Hidden,
    ));
    for part in &look.parts {
        let parent = match part.joint {
            Joint::Body => trunk,
            Joint::Head => head,
            _ => add.entity,
        };
        let entity = commands
            .spawn((
                CreaturePart,
                Mesh3d(part.mesh.clone()),
                MeshMaterial3d(part.material.clone()),
                Transform::from_translation(part.at),
                ChildOf(parent),
            ))
            .id();
        model.parts.push(entity);
        match part.joint {
            Joint::Leg(side) => model.legs.push((entity, side)),
            Joint::Arm(side) => model.arms.push((entity, side)),
            Joint::Tail => model.tails.push(entity),
            Joint::Wing(side) => model.wings.push((entity, side)),
            Joint::Body | Joint::Head => {}
        }
        match part.look {
            Look::Eye | Look::Glow => model.eyes.push(entity),
            Look::Fleece => model.fleece.push(entity),
            Look::Plain => {}
        }
    }
    commands.entity(add.entity).insert(model);
}

fn shown(on: bool) -> Visibility {
    if on {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

// Those a creature looks at.
type Watchers = Or<(With<LocalPlayer>, With<SeenPlayer>)>;

type PartMut<'a> = (&'a mut Transform, &'a mut Visibility);

type Live<'a> = (
    &'a SeenCreature,
    &'a mut Model,
    &'a mut Transform,
    &'a PlayerLook,
    Option<&'a Flash>,
);

// Moves each creature's parts as it lives: turning its body smoothly the way it faces, swinging
// its legs as it walks, more the faster, bobbing as it steps, breathing, blinking, looking at
// the nearest player near enough, or else grazing now and then if it grazes, swaying its tail,
// flapping its wings as it flutters down or runs, and rearing back as it flinches. A monster
// rears back and raises its arms as it winds up a blow, and lunges into it as it comes, and
// one hanging from a ceiling is drawn upside down from it.
fn animate(
    time: Res<Time>,
    watchers: Query<&GlobalTransform, Watchers>,
    stances: Query<&Stance>,
    mut models: Query<Live, Without<CreaturePart>>,
    mut parts: Query<PartMut, (With<CreaturePart>, Without<Model>)>,
) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    for (seen, mut model, mut body, look, flash) in &mut models {
        let properties = model.kind.properties();
        let seed = model.seed;
        let at = body.translation;
        let moved = at - model.last;
        model.last = at;
        let (across, falling) = if dt > 0.0 {
            (moved.xz().length() / dt, -moved.y / dt)
        } else {
            (0.0, 0.0)
        };
        let swing = ease(
            model.swing,
            stride_swing(across, FULL_SWING_SPEED),
            10.0,
            dt,
        );
        model.swing = swing;
        let stride = model.stride + moved.xz().length().min(1.0) * model.stride_per_block;
        model.stride = stride.rem_euclid(TAU);
        let stride = model.stride;
        let yaw = ease_angle(model.yaw.unwrap_or(look.yaw), look.yaw, TURNING, dt);
        model.yaw = Some(yaw);

        // winding up a blow, and the blow once it comes
        let stance = stances.get(seen.0).copied().unwrap_or_default();
        if model.winding && !stance.winding {
            model.struck = Some(0.0);
        }
        model.winding = stance.winding;
        let wanted = if stance.winding { 1.0 } else { 0.0 };
        model.wound = ease(model.wound, wanted, WINDING, dt);
        let strike = match model.struck {
            Some(since) if since < STRIKE => {
                model.struck = Some(since + dt);
                (since / STRIKE * PI).sin()
            }
            _ => {
                model.struck = None;
                0.0
            }
        };
        let rear = WIND_REAR * model.wound - STRIKE_LUNGE * strike;

        let flinched = flash.map_or(0.0, |flash| flinch(FLASH - flash.0));
        body.rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(flinched + rear);
        body.translation.y += BODY_BOB * stride.sin().abs() * swing;
        if stance.clinging {
            // its back to the ceiling, its feet where its top would be
            body.rotation *= Quat::from_rotation_z(PI);
            body.translation.y += properties.height * body.scale.y;
        }

        let angle = stride.sin() * swing * SWING;
        for &(leg, side) in &model.legs {
            if let Ok((mut leg, _)) = parts.get_mut(leg) {
                leg.rotation = Quat::from_rotation_x(angle * side);
            }
        }
        let raised = WIND_ARMS * model.wound - STRIKE_ARMS * strike;
        for &(arm, side) in &model.arms {
            if let Ok((mut arm, _)) = parts.get_mut(arm) {
                arm.rotation = Quat::from_rotation_x(raised - angle * side * ARM_SWING);
            }
        }
        if let Ok((mut trunk, _)) = parts.get_mut(model.trunk) {
            let breathing = breath(t, seed);
            let across = 1.0 + (breathing - 1.0) / 2.0;
            trunk.scale = Vec3::new(across, breathing, across);
        }
        for &tail_part in &model.tails {
            if let Ok((mut tail_part, _)) = parts.get_mut(tail_part) {
                tail_part.rotation = Quat::from_rotation_y(tail(t, seed));
            }
        }
        let flutter = if falling > FLUTTERING {
            1.0
        } else if across > FLAPPING_RUN {
            0.5
        } else {
            0.0
        };
        model.flutter = ease(model.flutter, flutter, 8.0, dt);
        for &(wing, side) in &model.wings {
            if let Ok((mut wing, _)) = parts.get_mut(wing) {
                wing.rotation = Quat::from_rotation_z(side * flap(t, model.flutter));
            }
        }

        // at the nearest player near enough, or grazing, or ahead
        let eye = at + Vec3::Y * properties.neck.y * body.scale.y;
        let watcher = watchers
            .iter()
            .map(|watcher| watcher.translation() + Vec3::Y * EYE_HEIGHT)
            .filter(|watcher| watcher.distance(eye) <= LOOK_RANGE)
            .min_by(|a, b| a.distance(eye).total_cmp(&b.distance(eye)));
        let watched = watcher.filter(|_| !stance.clinging);
        let wanted = match watched.and_then(|watcher| head_look(yaw, eye, watcher)) {
            Some(looking) => looking,
            None if properties.grazes => {
                Vec2::new(0.0, grazing(t, seed) * (1.0 - swing) * GRAZE_PITCH)
            }
            None => Vec2::ZERO,
        };
        model.look = Vec2::new(
            ease_angle(model.look.x, wanted.x, LOOKING, dt),
            ease(model.look.y, wanted.y, LOOKING, dt),
        );
        if let Ok((mut head, _)) = parts.get_mut(model.head) {
            let bob = HEAD_BOB * (stride * 2.0).sin() * swing;
            head.rotation =
                Quat::from_rotation_y(model.look.x) * Quat::from_rotation_x(model.look.y + bob);
        }

        let blink = blinking(t, seed);
        if blink != model.blinking {
            model.blinking = blink;
            for &eye in &model.eyes {
                if let Ok((_, mut visibility)) = parts.get_mut(eye) {
                    *visibility = shown(!blink);
                }
            }
        }
        if model.shown.is_some_and(|shown| shown.in_love) {
            if let Ok((mut heart, _)) = parts.get_mut(model.heart) {
                heart.translation.y = model.heart_at + HEART_BOB * (t * 4.0).sin();
            }
        }
    }
}

// Tips each dying creature onto its side, its eyes shut, and then fades it out, or just fades
// one vanishing.
fn show_death(
    looks: Res<CreatureLooks>,
    mut dying: Query<(&mut Model, &Dying, &mut Transform, Has<Fading>), Without<CreaturePart>>,
    mut parts: Query<(&mut MeshMaterial3d<ModelMaterial>, &mut Visibility), With<CreaturePart>>,
) {
    for (mut model, dying, mut body, fading) in &mut dying {
        let Some((tipped, left)) = death(dying.0) else {
            continue;
        };
        let tipped = if fading { 0.0 } else { tipped };
        let yaw = model.yaw.unwrap_or_default();
        body.rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_z(tipped);
        // onto its side, rather than into the ground
        let half = model.kind.properties().width / 2.0 * body.scale.x;
        body.translation.y += half * tipped.sin();
        for &part in model.eyes.iter().chain([&model.heart]) {
            if let Ok((_, mut visibility)) = parts.get_mut(part) {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
        let step = (((1.0 - left) * FADE_STEPS as f32) as usize).min(FADE_STEPS);
        if step == model.faded {
            continue;
        }
        model.faded = step;
        for (&part, look) in model.parts.iter().zip(&looks.kinds[&model.kind].parts) {
            if let Ok((mut material, _)) = parts.get_mut(part) {
                material.0 = look.fading[step.max(1) - 1].clone();
            }
        }
    }
}

// Draws creatures hurt redder for a moment.
fn flash(
    time: Res<Time>,
    mut flashing: Query<(Entity, &mut Model, Option<&mut Flash>), With<SeenCreature>>,
    looks: Res<CreatureLooks>,
    mut materials: Query<&mut MeshMaterial3d<ModelMaterial>>,
    mut commands: Commands,
) {
    for (entity, mut model, flash) in &mut flashing {
        let red = match flash {
            Some(mut flash) => {
                flash.0 -= time.delta_secs();
                if flash.0 <= 0.0 {
                    commands.entity(entity).remove::<Flash>();
                }
                flash.0 > 0.0
            }
            None => false,
        };
        if red == model.flashing {
            continue;
        }
        model.flashing = red;
        for (&part, look) in model.parts.iter().zip(&looks.kinds[&model.kind].parts) {
            if let Ok(mut material) = materials.get_mut(part) {
                material.0 = if red { &look.hurt } else { &look.material }.clone();
            }
        }
    }
}

// Shows each creature as its condition has it: smaller while it's young, its head bigger for
// its size, without its fleece once it's shorn, and with a heart over it in love.
fn show_condition(
    mut models: Query<(&SeenCreature, &mut Model, &mut Transform), Without<CreaturePart>>,
    creatures: Query<&Condition>,
    mut parts: Query<(&mut Transform, &mut Visibility), With<CreaturePart>>,
) {
    for (seen, mut model, mut body) in &mut models {
        let Ok(&condition) = creatures.get(seen.0) else {
            continue;
        };
        if model.shown == Some(condition) {
            continue;
        }
        model.shown = Some(condition);
        body.scale = Vec3::splat(if condition.young { YOUNG_SCALE } else { 1.0 });
        if let Ok((mut head, _)) = parts.get_mut(model.head) {
            head.scale = Vec3::splat(if condition.young { YOUNG_HEAD } else { 1.0 });
        }
        for &part in &model.fleece {
            if let Ok((_, mut visibility)) = parts.get_mut(part) {
                *visibility = shown(!condition.shorn);
            }
        }
        if let Ok((_, mut visibility)) = parts.get_mut(model.heart) {
            *visibility = shown(condition.in_love);
        }
    }
}

// Draws each missile as it's first seen, a small stone, from where it was thrown.
fn show_missiles(
    thrown: Query<(Entity, &Missile), Added<Missile>>,
    looks: Res<CreatureLooks>,
    mut commands: Commands,
) {
    for (entity, missile) in &thrown {
        commands.entity(entity).insert((
            Thrown::default(),
            Mesh3d(looks.missile.clone()),
            MeshMaterial3d(looks.stone.clone()),
            Transform::from_translation(missile.from),
        ));
    }
}

/// Flies each missile along its arc from where it was thrown, by this client's clock since it
/// was seen thrown, until the server takes it away. A host's are its server's, which move them.
pub(super) fn fly_missiles(
    time: Res<Time>,
    mut thrown: Query<(&Missile, &mut Thrown, &mut Transform)>,
) {
    for (missile, mut thrown, mut body) in &mut thrown {
        thrown.0 += time.delta_secs();
        body.translation = missile.at(thrown.0);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::prelude::*;
    use bevy::time::TimeUpdateStrategy;

    use super::{die_away, see_off, see_vanish, Dying, Fading, SeenCreature};
    use crate::client::animation::DEATH;
    use crate::client::player::LocalPlayer;
    use crate::creatures::Vanishing;

    #[test]
    fn test_a_creature_gone_near_dies_where_it_was_and_then_goes_and_one_far_off_just_goes() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                50,
            )))
            .add_observer(see_off)
            .add_systems(Update, die_away);
        app.world_mut().spawn((LocalPlayer, Transform::default()));
        let seen = |app: &mut App, x: f32| {
            let creature = app.world_mut().spawn_empty().id();
            let at = Transform::from_xyz(x, 0.0, 0.0);
            let seen = app.world_mut().spawn((SeenCreature(creature), at)).id();
            (creature, seen)
        };
        let (near, dying) = seen(&mut app, 3.0);
        let (far, gone) = seen(&mut app, 100.0);
        app.update();
        app.world_mut().despawn(near);
        app.world_mut().despawn(far);
        app.update();
        assert!(app.world().get_entity(gone).is_err());
        assert!(app.world().get::<Dying>(dying).is_some());
        // still there until its death's over, and then gone
        let frames = (DEATH / 0.05) as usize;
        for _ in 0..frames - 2 {
            app.update();
        }
        assert!(app.world().get_entity(dying).is_ok());
        for _ in 0..3 {
            app.update();
        }
        assert!(app.world().get_entity(dying).is_err());
    }

    #[test]
    fn test_a_creature_that_vanishes_near_only_fades() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_observer(see_off)
            .add_observer(see_vanish);
        app.world_mut().spawn((LocalPlayer, Transform::default()));
        let creature = app.world_mut().spawn_empty().id();
        let at = Transform::from_xyz(3.0, 0.0, 0.0);
        let seen = app.world_mut().spawn((SeenCreature(creature), at)).id();
        app.update();
        app.world_mut().entity_mut(creature).insert(Vanishing);
        app.update();
        assert!(app.world().get::<Fading>(seen).is_some());
        app.world_mut().despawn(creature);
        app.update();
        // fading away where it was, which `show_death` draws without tipping it over
        assert!(app.world().get::<Dying>(seen).is_some());
        assert!(app.world().get::<Fading>(seen).is_some());
    }

    #[test]
    fn test_every_kind_shares_a_few_materials_and_only_eyes_that_glow_are_unlit() {
        use bevy::ecs::system::RunSystemOnce;
        use bevy::mesh::VertexAttributeValues;
        use bevy::platform::collections::HashSet;
        use bevy::render::storage::ShaderBuffer;

        use super::{make_looks, CreatureLooks, ModelMaterial};
        use crate::clouds::CloudLayer;
        use crate::creatures::{CreatureKind, Look};
        use crate::curvature::Curvature;

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset::<Image>()
            .init_asset::<ShaderBuffer>()
            .init_asset::<ModelMaterial>()
            .init_resource::<Curvature>()
            .insert_resource(CloudLayer {
                pattern: Handle::default(),
                layer: Handle::default(),
            });
        app.world_mut().run_system_once(make_looks).unwrap();
        let world = app.world();
        let looks = world.resource::<CreatureLooks>();
        let (materials, meshes) = (
            world.resource::<Assets<ModelMaterial>>(),
            world.resource::<Assets<Mesh>>(),
        );
        let mut shared = HashSet::new();
        for &kind in CreatureKind::ALL {
            let parts = kind.properties().model;
            for (part, look) in parts.iter().zip(&looks.kinds[&kind].parts) {
                shared.insert(look.material.id());
                let glows = part.look == Look::Glow;
                for material in [&look.material, &look.hurt].into_iter().chain(&look.fading) {
                    assert_eq!(glows, materials.get(material).unwrap().glows());
                }
                // its colour is its mesh's
                let Some(VertexAttributeValues::Float32x4(colours)) = meshes
                    .get(&look.mesh)
                    .unwrap()
                    .attribute(Mesh::ATTRIBUTE_COLOR)
                else {
                    panic!("{kind:?}'s parts are coloured");
                };
                let [r, g, b] = part.colour;
                let colour = Color::srgb(r, g, b).to_linear().to_f32_array();
                assert!(colours.iter().all(|&c| c == colour));
            }
        }
        // one lit as the land is, and one for eyes that glow, however many kinds and colours
        assert_eq!(2, shared.len());
    }
}
