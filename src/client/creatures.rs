//! Creatures as this client sees them (§5.8): each where its poses put it, drawn a little behind
//! time as other players are (`client::others`), as boxes of its kind's model whose legs swing
//! as it walks, flashing red as it's hurt, smaller while it's young, without its fleece once
//! it's shorn, and with a heart over it while it's in love. Every creature of a kind shares its
//! meshes and materials, so Bevy draws them batched.

use std::f32::consts::TAU;

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use super::others::{Pose, Poses};
use crate::collision::{ray_distance, Aabb};
use crate::creatures::{Condition, Creature, CreatureKind, CreaturePose, Flinches, YOUNG_SCALE};
use crate::player::PlayerLook;

// How long a creature flashes red when it's hurt, in seconds.
const FLASH: f32 = 0.3;
// How much redder it's drawn then.
const FLASH_RED: f32 = 0.6;
// How far the legs swing at a walk, in radians, and how fast they swing fully, in blocks a
// second, about a creature's walk.
const SWING: f32 = 0.7;
const FULL_SWING_SPEED: f32 = 1.4;
// Radians of stride a block walked, for a leg a block long: a player's 0.75 swings 2.2.
const STRIDE: f32 = 1.65;
// How far over a creature's model the heart it shows in love is, in blocks.
const HEART_OVER: f32 = 0.35;
const HEART: Color = Color::srgb(0.9, 0.18, 0.26);

/// A creature as this client sees it, drawn where its poses put it, on an entity of its own,
/// which goes with it. Its look's pitch is always level.
#[derive(Component)]
#[relationship(relationship_target = CreatureSeen)]
#[require(Poses, PlayerLook, Transform, Visibility = Visibility::Hidden)]
pub struct SeenCreature(pub Entity);

/// On a creature, what this client sees of it.
#[derive(Component)]
#[relationship_target(relationship = SeenCreature, linked_spawn)]
pub struct CreatureSeen(Entity);

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

/// Sees each creature that arrives, as the server spawns it or it's shown to this client, from
/// where it is then.
pub(super) fn see_creatures(
    arrived: Query<(Entity, &CreaturePose), Added<Creature>>,
    mut commands: Commands,
) {
    for (creature, now) in &arrived {
        let mut poses = Poses::default();
        poses.add(pose(now));
        commands.spawn((SeenCreature(creature), poses));
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
        app.add_systems(Startup, make_looks)
            .add_observer(draw_creature)
            .add_systems(Update, (walk, flash, show_condition));
    }
}

/// What each kind of creature's drawn with: a mesh for each box of its model, where it turns
/// from, and its material, as it is and flashing, and the heart shown over those in love.
#[derive(Resource)]
struct CreatureLooks {
    kinds: HashMap<CreatureKind, Vec<PartLook>>,
    heart: Handle<Mesh>,
    heart_colour: Handle<StandardMaterial>,
}

struct PartLook {
    mesh: Handle<Mesh>,
    joint: Vec3,
    swing: f32,
    fleece: bool,
    material: Handle<StandardMaterial>,
    hurt: Handle<StandardMaterial>,
}

// A creature's boxes, with which of them are legs, which way each swings, and where it is in
// its stride, which are its fleece, its heart, and the condition they were last shown for.
#[derive(Component)]
struct Model {
    parts: Vec<Entity>,
    legs: Vec<(Entity, f32)>,
    fleece: Vec<Entity>,
    heart: Entity,
    stride_per_block: f32,
    stride: f32,
    swing: f32,
    last: Vec3,
    flashing: bool,
    shown: Option<Condition>,
}

fn make_looks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut colours: HashMap<[u32; 3], (Handle<StandardMaterial>, Handle<StandardMaterial>)> =
        HashMap::new();
    let mut looks = HashMap::new();
    for &kind in CreatureKind::ALL {
        let parts = kind.properties().model.iter().map(|part| {
            // a leg turns about its top, and anything else about its middle
            let joint = part.centre + Vec3::Y * part.size.y / 2.0 * part.swing.abs();
            let mesh = Mesh::from(Cuboid::from_size(part.size)).translated_by(part.centre - joint);
            let [r, g, b] = part.colour;
            let (material, hurt) = colours
                .entry(part.colour.map(f32::to_bits))
                .or_insert_with(|| {
                    let mut plain = |colour: Color| {
                        materials.add(StandardMaterial {
                            base_color: colour,
                            perceptual_roughness: 1.0,
                            ..default()
                        })
                    };
                    let colour = Color::srgb(r, g, b);
                    let red = colour.mix(&Color::srgb(1.0, 0.1, 0.1), FLASH_RED);
                    (plain(colour), plain(red))
                })
                .clone();
            PartLook {
                mesh: meshes.add(mesh),
                joint,
                swing: part.swing,
                fleece: part.fleece,
                material,
                hurt,
            }
        });
        looks.insert(kind, parts.collect());
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
        kinds: looks,
        heart: meshes.add(heart),
        heart_colour: materials.add(StandardMaterial {
            base_color: HEART,
            unlit: true,
            ..default()
        }),
    });
}

fn draw_creature(
    add: On<Add<SeenCreature>>,
    seen: Query<&SeenCreature>,
    creatures: Query<&Creature>,
    looks: Res<CreatureLooks>,
    mut commands: Commands,
) {
    let Ok(Creature(kind)) = seen.get(add.entity).and_then(|seen| creatures.get(seen.0)) else {
        return;
    };
    let (mut parts, mut legs, mut fleece) = (vec![], vec![], vec![]);
    for look in &looks.kinds[kind] {
        let part = commands
            .spawn((
                Mesh3d(look.mesh.clone()),
                MeshMaterial3d(look.material.clone()),
                Transform::from_translation(look.joint),
                ChildOf(add.entity),
            ))
            .id();
        parts.push(part);
        if look.swing != 0.0 {
            legs.push((part, look.swing));
        }
        if look.fleece {
            fleece.push(part);
        }
    }
    let model = kind.properties().model;
    let top = model.iter().map(|part| part.centre.y + part.size.y / 2.0);
    let heart = commands
        .spawn((
            Mesh3d(looks.heart.clone()),
            MeshMaterial3d(looks.heart_colour.clone()),
            Transform::from_xyz(0.0, top.fold(0.0, f32::max) + HEART_OVER, 0.0),
            Visibility::Hidden,
            ChildOf(add.entity),
        ))
        .id();
    let leg = model
        .iter()
        .find(|part| part.swing != 0.0)
        .map_or(1.0, |part| part.size.y);
    commands.entity(add.entity).insert(Model {
        parts,
        legs,
        fleece,
        heart,
        stride_per_block: STRIDE / leg,
        stride: 0.0,
        swing: 0.0,
        last: Vec3::ZERO,
        flashing: false,
        shown: None,
    });
}

// Swings legs the further a creature's gone across, more the faster, as players' do.
fn walk(
    time: Res<Time>,
    mut models: Query<(&Transform, &mut Model)>,
    mut parts: Query<&mut Transform, Without<Model>>,
) {
    let dt = time.delta_secs();
    for (body, mut model) in &mut models {
        let moved = (body.translation - model.last).xz().length();
        model.last = body.translation;
        let speed = if dt > 0.0 { moved / dt } else { 0.0 };
        let swing = (speed / FULL_SWING_SPEED).min(1.0);
        model.swing += (swing - model.swing) * (dt * 10.0).min(1.0);
        model.stride = (model.stride + moved.min(1.0) * model.stride_per_block).rem_euclid(TAU);
        let angle = model.stride.sin() * model.swing * SWING;
        for &(leg, side) in &model.legs {
            if let Ok(mut leg) = parts.get_mut(leg) {
                leg.rotation = Quat::from_rotation_x(angle * side);
            }
        }
    }
}

// Draws creatures hurt redder for a moment.
fn flash(
    time: Res<Time>,
    mut flashing: Query<(Entity, &SeenCreature, &mut Model, Option<&mut Flash>)>,
    creatures: Query<&Creature>,
    looks: Res<CreatureLooks>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut commands: Commands,
) {
    for (entity, seen, mut model, flash) in &mut flashing {
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
        let Ok(Creature(kind)) = creatures.get(seen.0) else {
            continue;
        };
        for (&part, look) in model.parts.iter().zip(&looks.kinds[kind]) {
            if let Ok(mut material) = materials.get_mut(part) {
                material.0 = if red { &look.hurt } else { &look.material }.clone();
            }
        }
    }
}

// Shows each creature as its condition has it: smaller while it's young, without its fleece
// once it's shorn, and with a heart over it in love.
fn show_condition(
    mut models: Query<(&SeenCreature, &mut Model, &mut Transform)>,
    creatures: Query<&Condition>,
    mut visibilities: Query<&mut Visibility>,
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
        let shown = |on: bool| {
            if on {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            }
        };
        for &part in &model.fleece {
            if let Ok(mut visibility) = visibilities.get_mut(part) {
                *visibility = shown(!condition.shorn);
            }
        }
        if let Ok(mut visibility) = visibilities.get_mut(model.heart) {
            *visibility = shown(condition.in_love);
        }
    }
}
