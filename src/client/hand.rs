//! The player's view of themselves (M-A4): the view key going round first person and third
//! person behind and in front (`player::input::follow_look` puts the camera there), and in first
//! person the player's own arm, in their skin, and what they hold, low on the right. They're
//! drawn by a camera of their own over the world's, with a depth of its own, so they never go
//! into a wall and the world's depth of field doesn't blur them, lit by the light where the
//! player stands (`ModelLook::in_view`). The arm swings as they break, place, hit or use
//! something, bobs as they walk, drops out of sight and comes back with what's in a slot newly
//! chosen, and brings food to their mouth as they eat. The same camera draws the UI, over it.

use std::f32::consts::PI;

use bevy::app::{HierarchyPropagatePlugin, Propagate};
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, Hdr};
use bevy::core_pipeline::tonemapping::DebandDither;
use bevy::prelude::*;
use bevy::render::render_resource::BlendState;

use super::animation::{ease, smoothstep};
use super::drops::DropLooks;
use super::model_light::{ModelMaterial, ModelMaterials};
use super::player::{LocalPlayer, ModelCamera, WorldCamera};
use super::skins::{skin_look, SkinLooks, Wearing};
use crate::clock::TimeOfDay;
use crate::controls::{Action, Controls};
use crate::items::Item;
use crate::menu::GameState;
use crate::player::name::PlayerName;
use crate::player::{Movement, Player};
use crate::preferences::{Preferences, View};
use crate::protocol::{Eat, Swing};
use crate::server::HeldItem;
use crate::skin::Skin;
use crate::sky::sun_direction;
use crate::tonemap::{dry_color_grading, TONEMAPPING};

// The layer the hand's drawn on, by its camera alone.
const HAND_LAYER: usize = 1;
// In view, x right, y up and z back from the eye: where the shoulder is, out of sight below and
// to the right, and where the hand reaches to, low on the right and a little in.
const SHOULDER: Vec3 = Vec3::new(0.36, -0.44, 0.08);
const HAND: Vec3 = Vec3::new(0.24, -0.18, -0.6);
// How far the hand is from the shoulder, down the arm, as the skin's model has it.
const ARM_LENGTH: f32 = 10.0 / 16.0;
// Where what's held is from the hand, and how it's turned and how big it's drawn: a block
// turned to show three faces, and a picture such as a tool upright and turned half towards
// the eye, as it's gripped.
const BLOCK_HELD: Vec3 = Vec3::new(-0.03, 0.05, -0.06);
const BLOCK_SCALE: f32 = 0.6;
const PICTURE_HELD: Vec3 = Vec3::new(-0.06, 0.13, -0.06);
const PICTURE_SCALE: f32 = 0.9;
// How long a swing takes, in seconds, and how far it chops down and across, in radians, and
// moves the hand in and up, in blocks.
const SWING_SECONDS: f32 = 0.3;
const SWING_DOWN: f32 = 0.2;
const SWING_ACROSS: f32 = 0.25;
const SWING_IN: f32 = 0.08;
const SWING_UP: f32 = 0.08;
// Swapping what's held lowers the hand out of sight over this long, in seconds, and raises it
// again with the new over as long, this far down.
const SWAP_SECONDS: f32 = 0.12;
const SWAP_DROP: f32 = 0.6;
// Walking bobs the hand this far across and down, in blocks, once a stride, radians a block.
const BOB_ACROSS: f32 = 0.02;
const BOB_DOWN: f32 = 0.025;
const BOB_STRIDE: f32 = 2.2;
// Eating brings the hand this far towards the mouth, turned towards it, chewing as quickly as
// this many radians a second.
const EAT_TOWARDS: Vec3 = Vec3::new(-0.24, 0.14, 0.12);
const EAT_TURN: f32 = 0.6;
const CHEW_RATE: f32 = 18.0;
const CHEW: f32 = 0.025;
// The open sky's light on the hand by day and by night, and the sun's height, over the
// horizon, over which one turns into the other.
const DAYLIGHT: Vec3 = Vec3::new(1.0, 0.97, 0.9);
const NIGHT_LIGHT: Vec3 = Vec3::new(0.07, 0.08, 0.13);
const DUSK: (f32, f32) = (-0.08, 0.25);

/// Draws the player's own arm in first person, and lets them change how they're seen. Needs the
/// renderer, `DrawPlayersPlugin` and `DrawDropsPlugin`.
pub struct HandPlugin;

impl Plugin for HandPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<HierarchyPropagatePlugin<RenderLayers>>() {
            app.add_plugins(HierarchyPropagatePlugin::<RenderLayers>::new(PostUpdate));
        }
        app.init_resource::<HandMotion>()
            .add_systems(PostStartup, spawn_hand)
            .add_systems(
                Update,
                (
                    cycle_view.run_if(in_state(GameState::Playing)),
                    (dress_hand, move_hand, light_hand),
                    match_view,
                ),
            );
    }
}

/// The camera that draws the hand, and the UI over it.
#[derive(Component)]
struct HandCamera;

// What the hand's arm and what it holds turn about, at the shoulder, and their entities.
#[derive(Component)]
struct Hand {
    arm: Entity,
    held: Entity,
}

// What the hand's arm is drawn with, as the player's skin has it, and how they're dressed.
#[derive(Component, Default)]
struct HandLook {
    wearing: Option<Wearing>,
    material: Option<Handle<ModelMaterial>>,
}

/// Where the hand is in its moves: how long since it last swung, what it's showing and how far
/// it's lowered to change it, where it is in a stride and how much it's bobbing, and how far
/// it's brought to the mouth, and when.
#[derive(Resource, Debug)]
pub struct HandMotion {
    since_swing: f32,
    shown: Option<Item>,
    lowered: f32,
    stride: f32,
    bobbing: f32,
    eating: f32,
    time: f32,
}

impl Default for HandMotion {
    fn default() -> Self {
        Self {
            since_swing: f32::INFINITY,
            shown: None,
            lowered: 0.0,
            stride: 0.0,
            bobbing: 0.0,
            eating: 0.0,
            time: 0.0,
        }
    }
}

impl HandMotion {
    /// Moves on by `dt` seconds, with `held` held, having walked `walked` blocks across, swung
    /// if `swung`, and eating if `eating`.
    pub fn step(&mut self, dt: f32, held: Option<Item>, walked: f32, swung: bool, eating: bool) {
        self.time += dt;
        self.since_swing = if swung { 0.0 } else { self.since_swing + dt };
        // down with the old, swapped at the bottom, and up with the new
        self.lowered = if held != self.shown {
            (self.lowered + dt / SWAP_SECONDS).min(1.0)
        } else {
            (self.lowered - dt / SWAP_SECONDS).max(0.0)
        };
        if self.lowered >= 1.0 {
            self.shown = held;
        }
        self.stride = (self.stride + walked * BOB_STRIDE).rem_euclid(2.0 * PI);
        let walking = if dt > 0.0 && walked / dt > 0.5 {
            1.0
        } else {
            0.0
        };
        self.bobbing = ease(self.bobbing, walking, 8.0, dt);
        self.eating = ease(self.eating, if eating { 1.0 } else { 0.0 }, 12.0, dt);
    }

    /// What the hand shows, which only changes once it's out of sight.
    pub fn shown(&self) -> Option<Item> {
        self.shown
    }

    /// The hand's place and turn, about the shoulder, from where it rests.
    pub fn pose(&self) -> Transform {
        let swing = (self.since_swing / SWING_SECONDS).clamp(0.0, 1.0);
        let (chop, sweep) = if self.since_swing < SWING_SECONDS {
            ((swing * PI).sin(), (swing.sqrt() * PI).sin())
        } else {
            (0.0, 0.0)
        };
        let bob = Vec3::new(
            BOB_ACROSS * self.stride.sin(),
            -BOB_DOWN * self.stride.sin().abs(),
            0.0,
        ) * self.bobbing;
        let eat = smoothstep(self.eating);
        let chew = Vec3::Y * CHEW * (self.time * CHEW_RATE).sin() * eat;
        let translation = Vec3::new(-SWING_IN * sweep, SWING_UP * sweep, -SWING_IN * chop)
            + bob
            + Vec3::NEG_Y * SWAP_DROP * smoothstep(self.lowered)
            + EAT_TOWARDS * eat
            + chew;
        let rotation = Quat::from_rotation_y(SWING_ACROSS * sweep + EAT_TURN * eat)
            * Quat::from_rotation_x(-SWING_DOWN * chop + EAT_TURN * 0.5 * eat);
        Transform::from_translation(translation).with_rotation(rotation)
    }
}

/// Whether the hand's drawn: in first person, while the game's being played, not under a menu,
/// the inventory or the map, nor while dead.
pub fn hand_shown(state: &GameState, view: View) -> bool {
    *state == GameState::Playing && view == View::FirstPerson
}

// The hand's camera, hung from the world's at its eye, drawing after it, and the arm and what
// it holds, which take their meshes as the player's dressed and holds things.
fn spawn_hand(cameras: Query<(Entity, &Projection), WorldCamera>, mut commands: Commands) {
    let Ok((eye, projection)) = cameras.single() else {
        return;
    };
    let layer = RenderLayers::layer(HAND_LAYER);
    let camera = commands
        .spawn((
            HandCamera,
            ModelCamera,
            Camera3d::default(),
            Camera {
                order: 1,
                // its own picture, clear but for the hand and the UI, laid over the world's
                clear_color: ClearColorConfig::Custom(Color::NONE),
                output_mode: CameraOutputMode::Write {
                    blend_state: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    clear_color: ClearColorConfig::None,
                },
                ..default()
            },
            // toned as the world is, its own textures as it's multisampled and the world's
            // camera isn't, and undithered, as dithering would speckle all it's laid over
            Hdr,
            Msaa::Sample4,
            TONEMAPPING,
            dry_color_grading(),
            DebandDither::Disabled,
            // which the model's shaders declare, though the hand's never in it
            DistanceFog::default(),
            projection.clone(),
            IsDefaultUiCamera,
            layer.clone(),
            Transform::default(),
            ChildOf(eye),
        ))
        .id();
    let shoulder = commands
        .spawn((
            Transform::from_translation(SHOULDER),
            Visibility::Hidden,
            Propagate(layer),
            ChildOf(camera),
        ))
        .id();
    let reach = HAND - SHOULDER;
    let arm = commands
        .spawn((
            Transform::from_rotation(Quat::from_rotation_arc(Vec3::NEG_Y, reach.normalize())),
            Visibility::Inherited,
            ChildOf(shoulder),
        ))
        .id();
    let held = commands
        .spawn((Transform::default(), Visibility::Hidden, ChildOf(shoulder)))
        .id();
    commands
        .entity(shoulder)
        .insert((Hand { arm, held }, HandLook::default()));
}

// Dresses the hand's arm in the local player's skin as it changes, with their arms, in a
// material of its own, lit as it's drawn in view.
fn dress_hand(
    players: Query<(&PlayerName, Option<&Skin>), With<LocalPlayer>>,
    mut hands: Query<(&Hand, &mut HandLook)>,
    mut skins: ResMut<SkinLooks>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ModelMaterials,
    mut commands: Commands,
) {
    let (Ok((name, skin)), Ok((hand, mut look))) = (players.single(), hands.single_mut()) else {
        return;
    };
    if look.wearing == Some(Wearing::of(name, skin)) {
        return;
    }
    let dressed = skins.dress(name, skin, &mut images, &mut materials);
    let material = materials.add(skin_look(&dressed.image, true));
    let right = &skins.meshes.arms(dressed.arms)[1];
    commands
        .entity(hand.arm)
        .insert((Mesh3d(right.clone()), MeshMaterial3d(material.clone())));
    look.wearing = Some(dressed.wearing);
    look.material = Some(material);
}

// Moves the hand as the player swings, walks, changes what they hold and eats, shows what they
// hold once it's out of sight, and hides it all but in first person while playing.
#[allow(clippy::too_many_arguments)]
fn move_hand(
    time: Res<Time>,
    state: Res<State<GameState>>,
    preferences: Res<Preferences>,
    players: Query<(&Transform, &Player, Option<&HeldItem>), With<LocalPlayer>>,
    mut hands: Query<(&Hand, &mut Transform, &mut Visibility), Without<LocalPlayer>>,
    mut swings: MessageReader<Swing>,
    mut eats: MessageReader<Eat>,
    mut motion: ResMut<HandMotion>,
    mut looks: ResMut<DropLooks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut last: Local<Option<Vec3>>,
    mut eating: Local<bool>,
    mut commands: Commands,
) {
    let (Ok((feet, player, held)), Ok((hand, mut at, mut visibility))) =
        (players.single(), hands.single_mut())
    else {
        *last = None;
        return;
    };
    if let Some(Eat(slot)) = eats.read().last() {
        *eating = slot.is_some();
    }
    let walked = match player.movement {
        Movement::Walk => last.map_or(0.0, |last| (feet.translation - last).xz().length()),
        Movement::Fly => 0.0,
    };
    *last = Some(feet.translation);
    let was = motion.shown();
    let swung = swings.read().count() > 0;
    let held = held.and_then(|held| held.0);
    motion.step(time.delta_secs(), held, walked.min(1.0), swung, *eating);
    *at = Transform::from_translation(SHOULDER) * motion.pose();
    visibility.set_if_neq(if hand_shown(state.get(), preferences.view) {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    if motion.shown() == was {
        return;
    }
    let mut shown = commands.entity(hand.held);
    match motion.shown() {
        Some(item) => {
            let (mesh, material) = looks.of(item, true, &mut meshes);
            shown.insert((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                held_in_view(item),
                Visibility::Inherited,
            ));
        }
        None => {
            shown.insert(Visibility::Hidden);
        }
    }
}

// Where `item`'s held in view, from the shoulder.
fn held_in_view(item: Item) -> Transform {
    let hand = (HAND - SHOULDER).normalize() * ARM_LENGTH;
    if item.block().is_some() {
        Transform::from_translation(hand + BLOCK_HELD)
            .with_rotation(Quat::from_rotation_y(PI / 4.0) * Quat::from_rotation_x(0.35))
            .with_scale(Vec3::splat(BLOCK_SCALE))
    } else {
        Transform::from_translation(hand + PICTURE_HELD)
            .with_rotation(Quat::from_rotation_y(-1.15) * Quat::from_rotation_z(0.1))
            .with_scale(Vec3::splat(PICTURE_SCALE))
    }
}

// Lights the hand's materials with the open sky's light as the day has it, as the hand's camera
// has none of the world's lights.
fn light_hand(
    time: Res<TimeOfDay>,
    hands: Query<&HandLook>,
    looks: Res<DropLooks>,
    mut materials: ResMut<Assets<ModelMaterial>>,
) {
    let day = smoothstep((sun_direction(time.days()).y - DUSK.0) / (DUSK.1 - DUSK.0));
    let light = NIGHT_LIGHT.lerp(DAYLIGHT, day);
    let light = LinearRgba::rgb(light.x, light.y, light.z);
    let skins = hands.iter().filter_map(|hand| hand.material.as_ref());
    for handle in skins.chain(&looks.in_view) {
        // only as it changes, so the materials aren't sent to the GPU again every frame
        if materials.get(handle).is_some_and(|m| m.daylight != light) {
            if let Some(mut material) = materials.get_mut(handle) {
                material.daylight = light;
            }
        }
    }
}

// The view key goes round first person, behind and in front.
fn cycle_view(controls: Controls, mut preferences: ResMut<Preferences>) {
    if controls.just_pressed(Action::CameraView) {
        preferences.view = preferences.view.next();
    }
}

// The hand's seen through as wide a lens as the world.
fn match_view(
    world: Query<&Projection, (WorldCamera, Changed<Projection>)>,
    mut hand: Query<&mut Projection, (With<HandCamera>, With<ModelCamera>)>,
) {
    if let (Ok(world), Ok(mut hand)) = (world.single(), hand.single_mut()) {
        *hand = world.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::BlockType;

    const FRAME: f32 = 1.0 / 60.0;

    #[test]
    fn test_the_hand_is_only_drawn_in_first_person_while_playing() {
        assert!(hand_shown(&GameState::Playing, View::FirstPerson));
        for view in [View::Behind, View::InFront] {
            assert!(!hand_shown(&GameState::Playing, view));
        }
        for state in [
            GameState::Title,
            GameState::Loading,
            GameState::Paused,
            GameState::Inventory,
            GameState::Map,
            GameState::Dead,
        ] {
            assert!(!hand_shown(&state, View::FirstPerson), "{state:?}");
        }
    }

    #[test]
    fn test_the_hand_lowers_to_change_what_it_holds_and_raises_it() {
        let mut motion = HandMotion::default();
        let (dirt, pick) = (Item::Block(BlockType::Dirt), Item::WoodenPickaxe);
        let rest = motion.pose().translation;
        let mut frames = 0;
        // the old is shown until the hand's out of sight
        while motion.shown() != Some(dirt) {
            motion.step(FRAME, Some(dirt), 0.0, false, false);
            frames += 1;
            assert!(frames < 60, "never swapped");
        }
        let low = motion.pose().translation;
        assert!(low.y < rest.y - SWAP_DROP * 0.9, "{low}");
        // and comes back up with the new
        for _ in 0..30 {
            motion.step(FRAME, Some(dirt), 0.0, false, false);
        }
        assert!(motion.pose().translation.distance(rest) < 1e-4);
        // following the slot as it changes again
        for _ in 0..30 {
            motion.step(FRAME, Some(pick), 0.0, false, false);
        }
        assert_eq!(Some(pick), motion.shown());
        for _ in 0..30 {
            motion.step(FRAME, None, 0.0, false, false);
        }
        assert_eq!(None, motion.shown());
    }

    #[test]
    fn test_the_hand_swings_bobs_and_eats_and_comes_back_to_rest() {
        let mut motion = HandMotion::default();
        let rest = motion.pose();
        motion.step(FRAME, None, 0.0, true, false);
        for _ in 0..8 {
            motion.step(FRAME, None, 0.0, false, false);
        }
        let swung = motion.pose();
        assert!(swung.rotation.angle_between(rest.rotation) > 0.15);
        for _ in 0..30 {
            motion.step(FRAME, None, 0.0, false, false);
        }
        assert!(motion.pose().rotation.angle_between(rest.rotation) < 1e-4);
        // walking a block every quarter second bobs it, flying doesn't
        let mut lowest: f32 = 0.0;
        for _ in 0..60 {
            motion.step(FRAME, None, 4.0 * FRAME, false, false);
            lowest = lowest.min(motion.pose().translation.y);
        }
        assert!(lowest < -BOB_DOWN * 0.5, "{lowest}");
        // eating brings it towards the mouth, up and in
        for _ in 0..60 {
            motion.step(FRAME, None, 0.0, false, true);
        }
        let eating = motion.pose().translation;
        assert!(eating.x < -0.15 && eating.y > 0.05, "{eating}");
    }
}
