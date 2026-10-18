//! Drawing other players: a blocky body of boxes, its head turning the way they look and its
//! legs and arms swinging as they go, holding what they hold, and their name over their head.
//! Every player shares the meshes and materials, so Bevy draws them batched.

use std::f32::consts::TAU;
use std::hash::{Hash, Hasher};

use bevy::prelude::*;
use bevy::transform::TransformSystems;

use super::drops::DropLooks;
use super::others::SeenPlayer;
use crate::player::name::PlayerName;
use crate::player::PlayerLook;
use crate::server::HeldItem;

// The body's parts, in blocks, two blocks tall in all as the player is: legs and arms a
// quarter of a block square, the head half a block across.
const LIMB: Vec3 = Vec3::new(0.25, 0.75, 0.25);
const TORSO: Vec3 = Vec3::new(0.5, 0.75, 0.25);
const HEAD: f32 = 0.5;
const HIPS: f32 = 0.75;
const SHOULDERS: f32 = 1.5;
// Radians a block walked, and how far the legs swing at a walk, the arms a little less.
const STRIDE: f32 = 2.2;
const SWING: f32 = 0.6;
const ARM_SWING: f32 = 0.8;
// The speed a stride swings fully at, in blocks a second, about a walk.
const FULL_SWING_SPEED: f32 = 4.0;
// Labels further off than this, in blocks, aren't drawn.
const LABEL_DISTANCE: f32 = 64.0;
// How far over the feet a name's drawn.
const LABEL_HEIGHT: f32 = 2.4;

const SKIN: Color = Color::srgb(0.86, 0.66, 0.52);
const HAIR: Color = Color::srgb(0.27, 0.18, 0.11);
const EYES: Color = Color::srgb(0.1, 0.1, 0.16);
const TROUSERS: Color = Color::srgb(0.22, 0.24, 0.45);
// One for each player by their name, to tell them apart.
const SHIRTS: [Color; 6] = [
    Color::srgb(0.16, 0.55, 0.58),
    Color::srgb(0.66, 0.22, 0.2),
    Color::srgb(0.32, 0.55, 0.25),
    Color::srgb(0.75, 0.6, 0.2),
    Color::srgb(0.5, 0.3, 0.6),
    Color::srgb(0.85, 0.85, 0.82),
];

/// Draws other players. Needs the renderer, and `DrawDropsPlugin` for what they hold.
pub struct DrawPlayersPlugin;

impl Plugin for DrawPlayersPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, make_looks)
            .add_observer(draw_player)
            .add_systems(Update, (walk, hold))
            // where the camera's moved to this frame
            .add_systems(PostUpdate, place_labels.after(TransformSystems::Propagate));
    }
}

/// What players are drawn with.
#[derive(Resource)]
struct PlayerLooks {
    limb: Handle<Mesh>,
    torso: Handle<Mesh>,
    head: Handle<Mesh>,
    hair: Handle<Mesh>,
    eye: Handle<Mesh>,
    skin: Handle<StandardMaterial>,
    hair_colour: Handle<StandardMaterial>,
    eyes: Handle<StandardMaterial>,
    trousers: Handle<StandardMaterial>,
    shirts: Vec<Handle<StandardMaterial>>,
}

// A player's parts that move, and where they are in their stride.
#[derive(Component)]
struct Model {
    head: Entity,
    arms: [Entity; 2],
    legs: [Entity; 2],
    held: Entity,
    stride: f32,
    swing: f32,
    last: Vec3,
}

// A player's name over their head, a UI node placed where it's seen, for the player seen.
#[derive(Component)]
struct NameLabel(Entity);

fn make_looks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // each about the joint it turns on: limbs hang from it, the head sits on it
    let mut cuboid =
        |size: Vec3, at: Vec3| meshes.add(Mesh::from(Cuboid::from_size(size)).translated_by(at));
    let limb = cuboid(LIMB, Vec3::Y * -LIMB.y / 2.0);
    let torso = cuboid(TORSO, Vec3::Y * TORSO.y / 2.0);
    let head = cuboid(Vec3::splat(HEAD), Vec3::Y * HEAD / 2.0);
    let hair = cuboid(Vec3::new(HEAD, 0.12, HEAD) * 1.04, Vec3::Y * (HEAD - 0.05));
    let eye = cuboid(Vec3::new(0.08, 0.06, 0.02), Vec3::ZERO);
    let mut plain = |colour: Color| {
        materials.add(StandardMaterial {
            base_color: colour,
            perceptual_roughness: 1.0,
            ..default()
        })
    };
    let looks = PlayerLooks {
        limb,
        torso,
        head,
        hair,
        eye,
        skin: plain(SKIN),
        hair_colour: plain(HAIR),
        eyes: plain(EYES),
        trousers: plain(TROUSERS),
        shirts: SHIRTS.map(&mut plain).into(),
    };
    commands.insert_resource(looks);
}

fn draw_player(
    add: On<Add<SeenPlayer>>,
    seen: Query<&SeenPlayer>,
    names: Query<&PlayerName>,
    looks: Res<PlayerLooks>,
    mut commands: Commands,
) {
    let Ok(name) = seen.get(add.entity).and_then(|seen| names.get(seen.0)) else {
        return;
    };
    let mut hash = std::hash::DefaultHasher::new();
    name.as_str().hash(&mut hash);
    let shirt = looks.shirts[hash.finish() as usize % looks.shirts.len()].clone();
    let mut part = |mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, at: Vec3| {
        commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(at),
                ChildOf(add.entity),
            ))
            .id()
    };
    let legs = [-1.0, 1.0].map(|side| {
        let hip = Vec3::new(side * LIMB.x / 2.0, HIPS, 0.0);
        part(&looks.limb, &looks.trousers, hip)
    });
    part(&looks.torso, &shirt, Vec3::Y * HIPS);
    let arms = [-1.0, 1.0].map(|side| {
        let shoulder = Vec3::new(side * (TORSO.x + LIMB.x) / 2.0, SHOULDERS, 0.0);
        part(&looks.limb, &looks.skin, shoulder)
    });
    let head = part(&looks.head, &looks.skin, Vec3::Y * SHOULDERS);
    let mut on_head = |mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, at: Vec3| {
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(at),
            ChildOf(head),
        ));
    };
    on_head(&looks.hair, &looks.hair_colour, Vec3::ZERO);
    // on the face, which looks along -z as the player does
    for side in [-1.0, 1.0] {
        let at = Vec3::new(side * 0.11, 0.3, -HEAD / 2.0);
        on_head(&looks.eye, &looks.eyes, at);
    }
    // in the right hand, a little forward of it
    let held = commands
        .spawn((
            Transform::from_xyz(0.0, -LIMB.y, -0.1),
            Visibility::Hidden,
            ChildOf(arms[1]),
        ))
        .id();
    commands.entity(add.entity).insert(Model {
        head,
        arms,
        legs,
        held,
        stride: 0.0,
        swing: 0.0,
        last: Vec3::ZERO,
    });
    commands.spawn((
        NameLabel(add.entity),
        Text::new(name.as_str()),
        TextFont {
            font_size: bevy::text::FontSize::Px(14.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(Val::Px(4.0), Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.4)),
        Visibility::Hidden,
    ));
}

// Turns heads the way their players look, and swings legs and arms the further they've gone
// across, more the faster.
fn walk(
    time: Res<Time>,
    mut models: Query<(&Transform, &PlayerLook, &mut Model)>,
    mut parts: Query<&mut Transform, Without<Model>>,
) {
    let dt = time.delta_secs();
    for (body, look, mut model) in &mut models {
        let moved = (body.translation - model.last).xz().length();
        model.last = body.translation;
        let speed = if dt > 0.0 { moved / dt } else { 0.0 };
        let swing = (speed / FULL_SWING_SPEED).min(1.0);
        // easing in and out of a stride, so a step's worth of pose doesn't jerk the legs
        model.swing += (swing - model.swing) * (dt * 10.0).min(1.0);
        model.stride = (model.stride + moved.min(1.0) * STRIDE).rem_euclid(TAU);
        let angle = model.stride.sin() * model.swing * SWING;
        for (leg, side) in model.legs.into_iter().zip([1.0, -1.0]) {
            if let Ok(mut leg) = parts.get_mut(leg) {
                leg.rotation = Quat::from_rotation_x(angle * side);
            }
        }
        for (arm, side) in model.arms.into_iter().zip([-1.0, 1.0]) {
            if let Ok(mut arm) = parts.get_mut(arm) {
                arm.rotation = Quat::from_rotation_x(angle * side * ARM_SWING);
            }
        }
        if let Ok(mut head) = parts.get_mut(model.head) {
            head.rotation = Quat::from_rotation_x(look.pitch);
        }
    }
}

// Puts what each player holds in their hand as it changes.
fn hold(
    models: Query<(&SeenPlayer, Ref<Model>)>,
    held: Query<Ref<HeldItem>>,
    mut looks: ResMut<DropLooks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    for (seen, model) in &models {
        let Ok(item) = held.get(seen.0) else {
            continue;
        };
        if !model.is_added() && !item.is_changed() {
            continue;
        }
        let mut hand = commands.entity(model.held);
        match item.0 {
            Some(item) => {
                let (mesh, material) = looks.of(item, &mut meshes);
                hand.insert((
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    Visibility::Inherited,
                ));
            }
            None => {
                hand.insert(Visibility::Hidden);
            }
        }
    }
}

// Places each name over its player's head where the camera sees it, hidden if it's behind the
// camera or far off, and takes it away once they've gone.
fn place_labels(
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    seen: Query<(&GlobalTransform, &InheritedVisibility), With<SeenPlayer>>,
    mut labels: Query<(
        Entity,
        &NameLabel,
        &ComputedNode,
        &mut Node,
        &mut Visibility,
    )>,
    mut commands: Commands,
) {
    let Ok((camera, eye)) = camera.single() else {
        return;
    };
    for (label, NameLabel(player), size, mut node, mut visibility) in &mut labels {
        let Ok((body, shown)) = seen.get(*player) else {
            commands.entity(label).despawn();
            continue;
        };
        let head = body.translation() + Vec3::Y * LABEL_HEIGHT;
        let on_screen = camera
            .world_to_viewport(eye, head)
            .ok()
            .filter(|_| shown.get() && eye.translation().distance(head) < LABEL_DISTANCE);
        let Some(at) = on_screen else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let size = size.size() * size.inverse_scale_factor();
        node.left = Val::Px(at.x - size.x / 2.0);
        node.top = Val::Px(at.y - size.y);
        visibility.set_if_neq(Visibility::Inherited);
    }
}
