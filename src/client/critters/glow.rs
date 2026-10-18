//! Fireflies' light on the ground round them. They move, so they can't be block light, which is
//! worked out per mesh as it's built (§5.5): instead a few of Bevy's point lights follow the
//! nearest that are lit up, and `point_lights.wesl` adds their light to the world shader's.
//! There are never more than `MAX_FIREFLY_LIGHTS`, and none at all while no firefly's about.

use bevy::prelude::*;
use bevy::transform::TransformSystems;

use super::insects::Firefly;
use super::{eye, AmbientCritter};

pub(super) struct GlowPlugin;

impl Plugin for GlowPlugin {
    fn build(&self, app: &mut App) {
        // after the fireflies have moved, and before their lights' transforms propagate
        app.add_systems(
            PostUpdate,
            light_fireflies.before(TransformSystems::Propagate),
        );
    }
}

/// How many fireflies light what's round them at once, the nearest lit up: each light is looked
/// at by every fragment of the world in its cluster, so they're kept few.
pub const MAX_FIREFLY_LIGHTS: usize = 12;
// A firefly's glow on what's round it: its colour, how bright it is lit up, in lumens (the
// camera's exposed for sunlight, so it takes a lot), and how far it reaches, in blocks.
const FIREFLY_COLOUR: Color = Color::srgb(0.75, 1.0, 0.3);
const FIREFLY_LUMENS: f32 = 40_000.0;
const FIREFLY_LIGHT_RANGE: f32 = 4.0;
// Dimmer than this a firefly gives no light worth one.
const LIT: f32 = 0.02;

/// One of the lights that follow the fireflies.
#[derive(Component)]
pub struct FireflyLight;

/// The `count` nearest to `eye` of the fireflies at `at` lit up `lit`, nearest first, of those
/// lit up at all.
pub(super) fn nearest_lit(
    eye: Vec3,
    fireflies: impl Iterator<Item = (Vec3, f32)>,
    count: usize,
) -> Vec<(Vec3, f32)> {
    let mut lit: Vec<(f32, Vec3, f32)> = fireflies
        .filter(|&(_, lit)| lit > LIT)
        .map(|(at, lit)| (at.distance_squared(eye), at, lit))
        .collect();
    lit.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    lit.into_iter()
        .take(count)
        .map(|(_, at, lit)| (at, lit))
        .collect()
}

#[allow(clippy::type_complexity)]
fn light_fireflies(
    mut commands: Commands,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    fireflies: Query<(&Firefly, &Transform), Without<FireflyLight>>,
    mut lights: Query<
        (Entity, &mut PointLight, &mut Transform, &mut Visibility),
        With<FireflyLight>,
    >,
) {
    let eye = eye(&camera).map_or(Vec3::ZERO, |eye| eye.translation());
    if fireflies.is_empty() {
        // by day there are none to pay for
        for (light, ..) in &lights {
            commands.entity(light).despawn();
        }
        return;
    }
    let lit = nearest_lit(
        eye,
        fireflies
            .iter()
            .map(|(firefly, at)| (at.translation, firefly.lit)),
        MAX_FIREFLY_LIGHTS,
    );
    let mut lit = lit.into_iter();
    let mut have = 0;
    for (_, mut light, mut transform, mut visibility) in &mut lights {
        have += 1;
        match lit.next() {
            Some((at, brightness)) => {
                transform.translation = at;
                light.intensity = FIREFLY_LUMENS * brightness;
                *visibility = Visibility::Inherited;
            }
            None => {
                light.intensity = 0.0;
                *visibility = Visibility::Hidden;
            }
        }
    }
    for _ in have..MAX_FIREFLY_LIGHTS {
        let (at, brightness) = lit.next().unwrap_or((eye, 0.0));
        commands.spawn((
            FireflyLight,
            AmbientCritter,
            PointLight {
                color: FIREFLY_COLOUR,
                intensity: FIREFLY_LUMENS * brightness,
                range: FIREFLY_LIGHT_RANGE,
                radius: 0.05,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::from_translation(at),
            if brightness > 0.0 {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
        ));
    }
}
