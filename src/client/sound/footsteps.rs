//! Footsteps, jumping, landing and swimming, for the local player, and the other players and
//! creatures seen, whose are heard where they are.

use bevy::prelude::*;

use super::{PlaySound, SoundKind, Surface};
use crate::block::BlockType;
use crate::client::creatures::SeenCreature;
use crate::client::others::SeenPlayer;
use crate::client::player::LocalPlayer;
use crate::player::{Movement, Player, BODY};
use crate::world::World;

// How far a body walks between footsteps, in blocks: two and a half steps a second walking,
// three and a bit sprinting.
const STRIDE: f32 = 1.7;
// How far into a stride the first step comes, setting off from standing, so it's heard soon.
const FIRST_STEP: f32 = 0.6;
// How far through the water a body goes between strokes.
const STROKE: f32 = 2.2;
// How far a body must fall to land with a thump, or to splash into water, in blocks.
const LANDING: f32 = 0.6;
const SPLASHING: f32 = 1.0;
// More than this between frames is being put somewhere, not moving.
const TELEPORT: f32 = 4.0;
// How near the top of a block, in blocks, feet are standing on it.
const STANDING: f32 = 0.06;

pub(super) struct FootstepsPlugin;

impl Plugin for FootstepsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(walk_heard::<LocalPlayer>)
            .add_observer(walk_heard::<SeenPlayer>)
            .add_observer(walk_heard::<SeenCreature>)
            .add_systems(Update, step.run_if(resource_exists::<World>));
    }
}

/// How a body's been moving, to time its footsteps by.
#[derive(Component, Default, Debug)]
struct Gait {
    last: Option<Vec3>,
    // walked since the last step, across
    walked: f32,
    // the ground last stood on, while it's still being stood on
    standing: Option<Surface>,
    // the highest the feet have been since last standing or swimming, while off the ground
    fell_from: Option<f32>,
    swimming: bool,
    swum: f32,
}

impl Gait {
    /// Moves the body's feet on to `feet`, standing on `ground` if it's on any, or swimming,
    /// and returns the sound that makes, if any.
    fn advance(
        &mut self,
        feet: Vec3,
        ground: Option<Surface>,
        swimming: bool,
    ) -> Option<SoundKind> {
        let was_swimming = std::mem::replace(&mut self.swimming, swimming);
        let was_standing = std::mem::replace(&mut self.standing, ground);
        let moved = feet - self.last.replace(feet)?;
        if moved.length() > TELEPORT {
            self.fell_from = None;
            return None;
        }
        if swimming {
            let fell = self.fell_from.take().map_or(0.0, |top| top - feet.y);
            if !was_swimming && fell > SPLASHING {
                return Some(SoundKind::Splash);
            }
            self.swum += moved.length();
            return (self.swum >= STROKE).then(|| {
                self.swum = 0.0;
                SoundKind::Swim
            });
        }
        let Some(surface) = ground else {
            let top = self.fell_from.get_or_insert(feet.y);
            *top = top.max(feet.y);
            // pushing off the ground to jump is a step
            return was_standing.filter(|_| moved.y > 0.0).map(SoundKind::Step);
        };
        if let Some(top) = self.fell_from.take() {
            if top - feet.y > LANDING {
                self.walked = 0.0;
                return Some(SoundKind::Land(surface));
            }
        }
        let across = moved.xz().length();
        if across == 0.0 {
            self.walked = STRIDE * FIRST_STEP;
            return None;
        }
        self.walked += across;
        (self.walked >= STRIDE).then(|| {
            self.walked -= STRIDE;
            SoundKind::Step(surface)
        })
    }

    /// Moves the body on without a sound, as flying does.
    fn hover(&mut self, feet: Vec3) {
        *self = Self {
            last: Some(feet),
            ..default()
        };
    }
}

// The local player, and the other players and creatures seen, each have a gait.
fn walk_heard<T: Component>(add: On<Add<T>>, mut commands: Commands) {
    commands.entity(add.entity).insert(Gait::default());
}

/// What a body with its feet at `feet` stands on, if anything, and whether it's swimming, its
/// middle in water as movement has it.
fn footing(world: &World, feet: Vec3) -> (Option<Surface>, bool) {
    let middle = feet + Vec3::Y * BODY.max.y / 2.0;
    let swimming = world.get_block_at(middle.floor().as_i64vec3()) == BlockType::Water;
    let under = (feet.y - STANDING).floor();
    if feet.y - (under + 1.0) > STANDING {
        return (None, swimming);
    }
    // under the middle, else under a corner, as a body can stand at the edge of a block
    let corners = [
        Vec2::ZERO,
        Vec2::new(BODY.min.x, BODY.min.z),
        Vec2::new(BODY.min.x, BODY.max.z),
        Vec2::new(BODY.max.x, BODY.min.z),
        Vec2::new(BODY.max.x, BODY.max.z),
    ];
    let ground = corners.into_iter().find_map(|corner| {
        let at = Vec3::new(feet.x + corner.x, under, feet.z + corner.y);
        let block = world.get_block_at(at.floor().as_i64vec3());
        block.is_solid().then(|| Surface::of(block)).flatten()
    });
    (ground, swimming)
}

fn step(
    world: Res<World>,
    mut bodies: Query<(&Transform, &mut Gait, Option<&Player>, Has<LocalPlayer>)>,
    mut sounds: MessageWriter<PlaySound>,
) {
    for (body, mut gait, player, local) in &mut bodies {
        let feet = body.translation;
        // only the local player's known to be flying, and only they have a `Player` here
        if player.is_some_and(|player| player.movement == Movement::Fly) {
            gait.hover(feet);
            continue;
        }
        let (ground, swimming) = footing(&world, feet);
        if let Some(kind) = gait.advance(feet, ground, swimming) {
            sounds.write(PlaySound {
                kind,
                at: (!local).then_some(feet),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec3;

    use super::*;

    const DT: f32 = 1.0 / 60.0;

    // Walks a gait across stone at `speed` blocks a second for `seconds`, returning its sounds.
    fn walk(gait: &mut Gait, speed: f32, seconds: f32) -> Vec<SoundKind> {
        let start = gait.last.unwrap_or_default();
        (0..=(seconds / DT) as usize)
            .filter_map(|frame| {
                let feet = start + Vec3::X * speed * frame as f32 * DT;
                gait.advance(feet, Some(Surface::Stone), false)
            })
            .collect()
    }

    #[test]
    fn test_footsteps_come_faster_the_faster_the_walk_and_not_at_all_standing_still() {
        let walking = walk(&mut Gait::default(), 4.3, 10.0);
        let sprinting = walk(&mut Gait::default(), 5.6, 10.0);
        assert!(walking
            .iter()
            .all(|&kind| kind == SoundKind::Step(Surface::Stone)));
        // about two and a half steps a second walking, and more sprinting
        assert!((24..=27).contains(&walking.len()), "{}", walking.len());
        assert!(sprinting.len() > walking.len() + 5, "{}", sprinting.len());
        assert!(walk(&mut Gait::default(), 0.0, 10.0).is_empty());
    }

    #[test]
    fn test_setting_off_steps_soon() {
        let mut gait = Gait::default();
        walk(&mut gait, 0.0, 1.0);
        let first = (1..)
            .find(|&frame| {
                let feet = Vec3::X * 4.3 * frame as f32 * DT;
                gait.advance(feet, Some(Surface::Grass), false).is_some()
            })
            .unwrap();
        assert!((first as f32) * DT < 0.25, "{first}");
    }

    #[test]
    fn test_jumping_pushes_off_and_falling_lands_with_a_thump() {
        let mut gait = Gait::default();
        let ground = Some(Surface::Dirt);
        gait.advance(Vec3::ZERO, ground, false);
        assert_eq!(
            Some(SoundKind::Step(Surface::Dirt)),
            gait.advance(Vec3::Y * 0.1, None, false)
        );
        // up a block, and down onto one, isn't much of a fall
        gait.advance(Vec3::Y * 1.2, None, false);
        assert_eq!(None, gait.advance(Vec3::Y, ground, false));
        // but off a ledge is
        gait.advance(Vec3::new(0.0, 0.5, 0.0), None, false);
        gait.advance(Vec3::new(0.0, -1.0, 0.0), None, false);
        assert_eq!(
            Some(SoundKind::Land(Surface::Dirt)),
            gait.advance(Vec3::new(0.0, -2.0, 0.0), ground, false)
        );
    }

    #[test]
    fn test_falling_into_water_splashes_and_swimming_strokes() {
        let mut gait = Gait::default();
        gait.advance(Vec3::Y * 6.0, None, false);
        gait.advance(Vec3::Y * 5.0, None, false);
        assert_eq!(
            Some(SoundKind::Splash),
            gait.advance(Vec3::Y * 3.5, None, true)
        );
        let strokes = (1..=120)
            .filter_map(|frame| {
                let feet = Vec3::new(2.6 * frame as f32 * DT, 3.5, 0.0);
                gait.advance(feet, None, true)
            })
            .collect::<Vec<_>>();
        assert!(strokes.iter().all(|&kind| kind == SoundKind::Swim));
        assert_eq!(2, strokes.len());
    }

    #[test]
    fn test_being_put_somewhere_or_flying_makes_no_sound() {
        let mut gait = Gait::default();
        gait.advance(Vec3::new(0.0, 50.0, 0.0), None, false);
        assert_eq!(None, gait.advance(Vec3::ZERO, Some(Surface::Stone), false));
        gait.hover(Vec3::new(0.0, 3.0, 0.0));
        assert_eq!(None, gait.advance(Vec3::ZERO, Some(Surface::Stone), false));
    }
}
