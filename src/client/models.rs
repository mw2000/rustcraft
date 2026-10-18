//! Drawing other players: their models as their skins draw them (`client::skins`, M-A4),
//! breathing, their heads turning smoothly the way they look and their bodies after them once
//! they've turned far enough or they walk, their legs and arms swinging as they go and an arm as
//! they break or place, holding what they hold, wearing the armour they wear (M3.3), and their
//! names over their heads (M-A2, `client::animation`). Every player shares the meshes, and
//! everyone wearing a skin its material, so Bevy draws them batched.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::transform::TransformSystems;

use super::animation::{self, arm_swing, breath, ease, ease_angle, stride_swing, wrap};
use super::drops::DropLooks;
use super::model_light::{coloured, ModelLook, ModelMaterial, ModelMaterials};
use super::others::{move_seen, ArmSwing, SeenPlayer};
use super::skins::{self, load_skins, Part, SkinLooks, Wearing, BODY, HEAD, LEGS, PIXEL};
use crate::clouds::spawn_clouds;
use crate::items::{Item, Piece};
use crate::player::name::PlayerName;
use crate::player::PlayerLook;
use crate::server::drops::SIZE;
use crate::server::{HeldItem, Worn};
use crate::skin::{ArmStyle, Skin};

// Radians a block walked, and how far the legs swing at a walk, the arms a little less.
const STRIDE: f32 = 2.2;
const SWING: f32 = 0.6;
const ARM_SWING: f32 = 0.8;
// The speed a stride swings fully at, in blocks a second, about a walk.
const FULL_SWING_SPEED: f32 = 4.0;
// How far a head turns from the body before the body turns after it, in radians, and how
// quickly each turns, as the fraction of the way left that's turned a second.
const HEAD_YAW: f32 = 0.8;
const BODY_TURNING: f32 = 8.0;
const HEAD_TURNING: f32 = 15.0;
// Labels further off than this, in blocks, aren't drawn.
const LABEL_DISTANCE: f32 = 64.0;
// How far over the feet a name's drawn.
const LABEL_HEIGHT: f32 = 2.4;
// How many pixels bigger than what it covers armour's drawn each way: over the overlay, and
// boots and helmets a little more.
const ARMOUR_GROW: f32 = 0.75;
const BOOT_GROW: f32 = 1.0;

/// Draws other players. Needs the renderer, and `DrawDropsPlugin` for what they hold.
pub struct DrawPlayersPlugin;

impl Plugin for DrawPlayersPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (load_skins, make_looks).after(spawn_clouds))
            .add_observer(draw_player)
            .add_systems(Update, (walk.after(move_seen), dress, hold, wear))
            // where the camera's moved to this frame
            .add_systems(PostUpdate, place_labels.after(TransformSystems::Propagate));
    }
}

/// What players' armour is drawn with: one material, lit as the land is where they are, and
/// each piece's boxes about what they cover, in each armour's colour.
#[derive(Resource)]
struct PlayerLooks {
    material: Handle<ModelMaterial>,
    armour: HashMap<(Item, Cover), Handle<Mesh>>,
}

// The boxes armour's drawn in: sleeves for each arm style, and each side's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Cover {
    Helmet,
    Chestplate,
    Sleeve(ArmStyle, usize),
    Legging(usize),
    Boot(usize),
}

impl Cover {
    const ALL: [Cover; 10] = [
        Cover::Helmet,
        Cover::Chestplate,
        Cover::Sleeve(ArmStyle::Classic, 0),
        Cover::Sleeve(ArmStyle::Classic, 1),
        Cover::Sleeve(ArmStyle::Slim, 0),
        Cover::Sleeve(ArmStyle::Slim, 1),
        Cover::Legging(0),
        Cover::Legging(1),
        Cover::Boot(0),
        Cover::Boot(1),
    ];

    // Its box, about the joint of the part it covers.
    fn mesh(self) -> Mesh {
        match self {
            Cover::Helmet => HEAD.cover(0.0..5.0, BOOT_GROW),
            Cover::Chestplate => BODY.cover(0.0..12.0, ARMOUR_GROW),
            Cover::Sleeve(style, side) => skins::arms(style)[side].cover(0.0..5.0, ARMOUR_GROW),
            Cover::Legging(side) => LEGS[side].cover(0.0..7.0, ARMOUR_GROW),
            Cover::Boot(side) => LEGS[side].cover(8.0..12.0, BOOT_GROW),
        }
    }
}

// A player's parts that move, what they're dressed in, where they are in their stride, which
// way their body's shown facing and their head turned from it, across and up, and what makes
// their breathing unlike the rest's.
#[derive(Component)]
struct Model {
    // whose model it is, for their skin, what they hold and what they wear
    player: Entity,
    head: Entity,
    torso: Entity,
    arms: [Entity; 2],
    legs: [Entity; 2],
    held: Entity,
    // the boxes of each piece of armour, in the order of the armour slots
    armour: [Vec<(Entity, Cover)>; 4],
    wearing: Option<Wearing>,
    stride: f32,
    swing: f32,
    last: Vec3,
    yaw: Option<f32>,
    look: Vec2,
    seed: u32,
}

// A player's name over their head, a UI node placed where it's seen, for the player seen.
#[derive(Component)]
struct NameLabel(Entity);

fn make_looks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ModelMaterials,
) {
    let looks = PlayerLooks {
        material: materials.add(ModelLook::lit()),
        armour: Item::all()
            .filter_map(|item| Some((item, item.armour()?.colour)))
            .flat_map(|(item, [r, g, b])| {
                Cover::ALL.map(|cover| ((item, cover), Color::srgb(r, g, b)))
            })
            .map(|(key, colour)| (key, meshes.add(coloured(key.1.mesh(), colour))))
            .collect(),
    };
    commands.insert_resource(looks);
}

fn draw_player(
    add: On<Add<SeenPlayer>>,
    seen: Query<&SeenPlayer>,
    names: Query<&PlayerName>,
    skins: Res<SkinLooks>,
    looks: Res<PlayerLooks>,
    mut commands: Commands,
) {
    let Ok((player, name)) = seen
        .get(add.entity)
        .and_then(|seen| Ok((seen.0, names.get(seen.0)?)))
    else {
        return;
    };
    let model = spawn_model(add.entity, player, &skins, &looks, &mut commands);
    commands.entity(add.entity).insert(model);
    commands.spawn((
        NameLabel(add.entity),
        Text::new(name.as_str()),
        crate::menu::font(14.0),
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

// The parts of `player`'s model, on `body`, in classic arms until it's dressed.
fn spawn_model(
    body: Entity,
    player: Entity,
    skins: &SkinLooks,
    looks: &PlayerLooks,
    commands: &mut Commands,
) -> Model {
    let mut part = |mesh: &Handle<Mesh>, part: &Part| {
        commands
            .spawn((
                Mesh3d(mesh.clone()),
                Transform::from_translation(part.joint()),
                ChildOf(body),
            ))
            .id()
    };
    let meshes = &skins.meshes;
    let legs = [0, 1].map(|side| part(&meshes.legs[side], &LEGS[side]));
    let torso = part(&meshes.body, &BODY);
    let arm_parts = skins::arms(ArmStyle::Classic);
    let arms = [0, 1].map(|side| part(&meshes.arms(ArmStyle::Classic)[side], &arm_parts[side]));
    let head = part(&meshes.head, &HEAD);
    // armour over what it covers, hidden until it's worn, when it takes the mesh in its colour
    let mut armour = |cover: Cover, on: Entity| {
        let entity = commands
            .spawn((
                MeshMaterial3d(looks.material.clone()),
                Transform::default(),
                Visibility::Hidden,
                ChildOf(on),
            ))
            .id();
        (entity, cover)
    };
    let classic = ArmStyle::Classic;
    let armour = [
        vec![armour(Cover::Helmet, head)],
        vec![
            armour(Cover::Chestplate, torso),
            armour(Cover::Sleeve(classic, 0), arms[0]),
            armour(Cover::Sleeve(classic, 1), arms[1]),
        ],
        [0, 1]
            .map(|side| armour(Cover::Legging(side), legs[side]))
            .into(),
        [0, 1]
            .map(|side| armour(Cover::Boot(side), legs[side]))
            .into(),
    ];
    // in the right hand, at the bottom of the arm
    let held = commands
        .spawn((Transform::default(), Visibility::Hidden, ChildOf(arms[1])))
        .id();
    Model {
        player,
        head,
        torso,
        arms,
        legs,
        held,
        armour,
        wearing: None,
        stride: 0.0,
        swing: 0.0,
        last: Vec3::ZERO,
        yaw: None,
        look: Vec2::ZERO,
        seed: animation::hash(body.to_bits() as u32, 1).to_bits(),
    }
}

// Dresses each model in its player's skin as it changes, or the default one their name gives
// them if they've none of their own, with their arms as it has them.
fn dress(
    mut models: Query<&mut Model>,
    players: Query<(&PlayerName, Option<&Skin>, Option<&Worn>)>,
    mut skins: ResMut<SkinLooks>,
    looks: Res<PlayerLooks>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ModelMaterials,
    mut commands: Commands,
) {
    for mut model in &mut models {
        let Ok((name, skin, worn)) = players.get(model.player) else {
            continue;
        };
        if model.wearing == Some(Wearing::of(name, skin)) {
            continue;
        }
        let dressed = skins.dress(name, skin, &mut images, &mut materials);
        model.wearing = Some(dressed.wearing);
        let arms = skins.meshes.arms(dressed.arms);
        let chestplate = worn.and_then(|worn| worn.0[Piece::Chestplate.slot()]);
        for (side, arm) in arms.iter().enumerate() {
            commands
                .entity(model.arms[side])
                .insert(Mesh3d(arm.clone()));
            let sleeve = &mut model.armour[1][side + 1];
            sleeve.1 = Cover::Sleeve(dressed.arms, side);
            if let Some(mesh) = chestplate.and_then(|item| looks.armour.get(&(item, sleeve.1))) {
                commands.entity(sleeve.0).insert(Mesh3d(mesh.clone()));
            }
        }
        for part in [model.head, model.torso, model.legs[0], model.legs[1]]
            .into_iter()
            .chain(model.arms)
        {
            commands
                .entity(part)
                .insert(MeshMaterial3d(dressed.material.clone()));
        }
    }
}

// Turns heads smoothly the way their players look, and bodies after them once a head's turned
// as far as it goes or they're walking; swings legs and arms the further they've gone across,
// more the faster, and the right arm as they break or place; and breathes.
fn walk(
    time: Res<Time>,
    mut models: Query<(&mut Transform, &PlayerLook, &mut ArmSwing, &mut Model)>,
    mut parts: Query<&mut Transform, Without<Model>>,
) {
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    for (mut body, look, mut swung, mut model) in &mut models {
        let moved = (body.translation - model.last).xz().length();
        model.last = body.translation;
        let speed = if dt > 0.0 { moved / dt } else { 0.0 };
        model.swing = ease(model.swing, stride_swing(speed, FULL_SWING_SPEED), 10.0, dt);
        model.stride = (model.stride + moved.min(1.0) * STRIDE).rem_euclid(TAU);
        let angle = model.stride.sin() * model.swing * SWING;

        // the body follows the head as they walk, and is pulled round once it's turned too far
        let mut yaw = model.yaw.unwrap_or(look.yaw);
        yaw = ease_angle(yaw, look.yaw, BODY_TURNING * model.swing, dt);
        let turned = wrap(look.yaw - yaw);
        if turned.abs() > HEAD_YAW {
            yaw = look.yaw - turned.signum() * HEAD_YAW;
        }
        model.yaw = Some(yaw);
        body.rotation = Quat::from_rotation_y(yaw);
        model.look = Vec2::new(
            ease_angle(model.look.x, wrap(look.yaw - yaw), HEAD_TURNING, dt),
            ease(model.look.y, look.pitch, HEAD_TURNING, dt),
        );

        for (leg, side) in model.legs.into_iter().zip([1.0, -1.0]) {
            if let Ok(mut leg) = parts.get_mut(leg) {
                leg.rotation = Quat::from_rotation_x(angle * side);
            }
        }
        swung.0 += dt;
        let raised = [0.0, arm_swing(swung.0)];
        for ((arm, side), raised) in model.arms.into_iter().zip([-1.0, 1.0]).zip(raised) {
            if let Ok(mut arm) = parts.get_mut(arm) {
                arm.rotation = Quat::from_rotation_x(angle * side * ARM_SWING + raised);
            }
        }
        if let Ok(mut head) = parts.get_mut(model.head) {
            head.rotation =
                Quat::from_rotation_y(model.look.x) * Quat::from_rotation_x(model.look.y);
        }
        if let Ok(mut torso) = parts.get_mut(model.torso) {
            let breathing = breath(t, model.seed);
            torso.scale = Vec3::new(1.0 + (breathing - 1.0) / 2.0, breathing, breathing);
        }
    }
}

// Puts what each player holds in their hand as it changes: a block in their grip, and a tool or
// another item upright along their arm, pointing forward, as a sword's held.
fn hold(
    models: Query<Ref<Model>>,
    held: Query<Ref<HeldItem>>,
    mut looks: ResMut<DropLooks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    for model in &models {
        let Ok(item) = held.get(model.player) else {
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
                    held_in_hand(item),
                    Visibility::Inherited,
                ));
            }
            None => {
                hand.insert(Visibility::Hidden);
            }
        }
    }
}

/// Where `item`'s held in the hand, from the bottom of the arm, which hangs down along -y.
pub(super) fn held_in_hand(item: Item) -> Transform {
    let grip = Vec3::new(1.0, -10.0, -1.0) * PIXEL;
    if item.block().is_some() {
        return Transform::from_translation(grip + Vec3::NEG_Y * SIZE / 2.0);
    }
    // its picture's handle in the hand and its head up and forward, facing out sideways
    let turned = Quat::from_rotation_y(FRAC_PI_2);
    Transform::from_translation(grip + turned * Vec3::new(SIZE, SIZE, 0.0) * 0.35)
        .with_rotation(turned)
}

// Shows the armour each player wears as it changes, in its material's colour.
fn wear(
    models: Query<Ref<Model>>,
    worn: Query<Ref<Worn>>,
    looks: Res<PlayerLooks>,
    mut commands: Commands,
) {
    for model in &models {
        let Ok(worn) = worn.get(model.player) else {
            continue;
        };
        if !model.is_added() && !worn.is_changed() {
            continue;
        }
        for (piece, boxes) in Piece::ALL.into_iter().zip(&model.armour) {
            let item = worn.0[piece.slot()];
            for &(part, cover) in boxes {
                let mesh = item.and_then(|item| looks.armour.get(&(item, cover)));
                let mut part = commands.entity(part);
                match mesh {
                    Some(mesh) => part.insert((Mesh3d(mesh.clone()), Visibility::Inherited)),
                    None => part.insert(Visibility::Hidden),
                };
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
