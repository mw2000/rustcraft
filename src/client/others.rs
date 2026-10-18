//! Other players as this client sees them: each where their poses put them, drawn a little
//! behind time so there's always a pose either side to move smoothly between, though they come
//! twenty times a second and unevenly. On the host they're the server's own players, which move
//! in steps as each pose arrives; elsewhere they're replicated, with no place of their own.

use std::collections::VecDeque;

use bevy::prelude::*;

use super::player::LocalPlayer;
use crate::player::name::PlayerName;
use crate::player::PlayerLook;
use crate::protocol::PlayerMoved;

// How far behind their newest pose, in milliseconds, players are shown: two poses' worth, so a
// late or lost one doesn't leave nothing to move towards.
const DELAY: f64 = 100.0;
// How long past their newest pose a player keeps going the way they were before stopping
// to wait for the next.
const EXTRAPOLATION: f64 = 100.0;
// Enough to span the delay with some lost.
const KEPT: usize = 8;

/// Another player as this client sees them, drawn where their poses put them, on an entity of
/// its own, which goes with them.
#[derive(Component)]
#[relationship(relationship_target = Seen)]
#[require(Poses, PlayerLook, Transform, Visibility = Visibility::Hidden)]
pub struct SeenPlayer(pub Entity);

/// On another player, what this client sees of them.
#[derive(Component)]
#[relationship_target(relationship = SeenPlayer, linked_spawn)]
pub struct Seen(Entity);

/// The poses of the player seen, oldest first, and the moment in them shown, by the player's own
/// clock, in milliseconds.
#[derive(Component, Default)]
pub struct Poses {
    poses: VecDeque<Pose>,
    shown: Option<f64>,
}

/// Where something seen was, which way it faced and looked, at `at` milliseconds by its clock.
#[derive(Clone, Copy, Debug)]
pub(super) struct Pose {
    pub at: f64,
    pub position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
}

impl Poses {
    /// Keeps `pose`, unless a newer one's come already.
    pub(super) fn add(&mut self, pose: Pose) {
        // what's overtaken a pose sent after it on the way is out of date
        if self.poses.back().is_some_and(|newest| newest.at >= pose.at) {
            return;
        }
        self.poses.push_back(pose);
        if self.poses.len() > KEPT {
            self.poses.pop_front();
        }
    }

    // Moves what's shown on by `dt` milliseconds and returns the pose there, if there's been
    // one. It runs at this client's pace, drifting back towards `DELAY` behind the newest, and
    // jumps there if it's fallen far behind or the newest is long past.
    fn advance(&mut self, dt: f64) -> Option<Pose> {
        let newest = *self.poses.back()?;
        let wanted = newest.at - DELAY;
        let shown = self.shown.map_or(wanted, |shown| {
            let shown = shown + dt;
            let shown = shown + (wanted - shown) * (dt / 1000.0).min(1.0);
            if shown < wanted - DELAY {
                wanted
            } else {
                shown.min(newest.at + EXTRAPOLATION)
            }
        });
        self.shown = Some(shown);
        Some(self.at(shown))
    }

    // The pose at `at`: between the two either side, or beyond the newest, the way the last two
    // were going.
    fn at(&self, at: f64) -> Pose {
        let after = self.poses.iter().position(|pose| pose.at >= at);
        let (from, to) = match after {
            Some(0) => return self.poses[0],
            Some(after) => (self.poses[after - 1], self.poses[after]),
            None if self.poses.len() == 1 => return self.poses[0],
            None => (
                self.poses[self.poses.len() - 2],
                self.poses[self.poses.len() - 1],
            ),
        };
        let t = ((at - from.at) / (to.at - from.at)) as f32;
        let turned = (to.yaw - from.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        Pose {
            at,
            position: from.position.lerp(to.position, t),
            yaw: from.yaw + turned * t,
            pitch: from.pitch.lerp(to.pitch, t),
        }
    }
}

/// Sees each other player that arrives: the host's and those that join it, but not this
/// client's own.
pub(super) fn see_players(
    arrived: Query<Entity, (Added<PlayerName>, Without<LocalPlayer>)>,
    mut commands: Commands,
) {
    for player in &arrived {
        commands.spawn(SeenPlayer(player));
    }
}

/// Keeps the poses of the players seen as they come.
pub(super) fn receive_poses(
    mut moved: MessageReader<PlayerMoved>,
    seen: Query<&Seen>,
    mut poses: Query<&mut Poses>,
) {
    for moved in moved.read() {
        let Ok(mut poses) = seen
            .get(moved.player)
            .and_then(|seen| poses.get_mut(seen.0))
        else {
            continue;
        };
        poses.add(Pose {
            at: moved.at.into(),
            position: moved.position,
            yaw: moved.yaw,
            pitch: moved.pitch,
        });
    }
}

/// Puts each player seen, and each creature (`client::creatures`), where their poses say they
/// are now, facing the way they look, and shows them once they've had one.
pub(super) fn move_seen(
    time: Res<Time>,
    mut seen: Query<(&mut Poses, &mut Transform, &mut PlayerLook, &mut Visibility)>,
) {
    let dt = time.delta_secs_f64() * 1000.0;
    for (mut poses, mut body, mut look, mut visibility) in &mut seen {
        let Some(pose) = poses.advance(dt) else {
            continue;
        };
        body.translation = pose.position;
        body.rotation = Quat::from_rotation_y(pose.yaw);
        *look = PlayerLook {
            yaw: pose.yaw,
            pitch: pose.pitch,
        };
        visibility.set_if_neq(Visibility::Inherited);
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec3;

    use super::{Pose, Poses, DELAY, EXTRAPOLATION};

    fn pose(at: f64, x: f32, yaw: f32) -> Pose {
        Pose {
            at,
            position: Vec3::X * x,
            yaw,
            pitch: 0.0,
        }
    }

    #[test]
    fn test_players_are_shown_between_their_poses_a_little_behind() {
        let mut poses = Poses::default();
        assert!(poses.advance(16.0).is_none());
        for (at, x) in [(1000.0, 0.0), (1050.0, 1.0), (1100.0, 2.0)] {
            poses.add(pose(at, x, 0.0));
        }
        // the newest less the delay, halfway between the first two
        let shown = poses.advance(16.0).unwrap();
        assert_eq!(1100.0 - DELAY, shown.at);
        assert!((shown.position.x - 0.0).abs() < 1e-5);
        let shown = poses.advance(25.0).unwrap();
        assert!(
            shown.position.x > 0.4 && shown.position.x < 0.6,
            "{shown:?}"
        );
        // a pose overtaken by a newer one is passed over
        poses.add(pose(1075.0, 50.0, 0.0));
        assert!(poses.advance(1.0).unwrap().position.x < 1.0);
    }

    #[test]
    fn test_players_go_on_a_little_past_their_newest_pose_then_wait() {
        let mut poses = Poses::default();
        poses.add(pose(0.0, 0.0, 0.0));
        poses.add(pose(50.0, 1.0, 0.0));
        poses.advance(0.0);
        // as though the next poses were lost
        let mut shown = pose(0.0, 0.0, 0.0);
        for _ in 0..30 {
            shown = poses.advance(10.0).unwrap();
        }
        let furthest = 1.0 + (EXTRAPOLATION / 50.0) as f32;
        assert!((shown.position.x - furthest).abs() < 1e-4, "{shown:?}");
    }

    #[test]
    fn test_players_turn_the_short_way_round() {
        let mut poses = Poses::default();
        poses.add(pose(0.0, 0.0, 6.0));
        poses.add(pose(100.0, 0.0, 0.2));
        let halfway = poses.at(50.0).yaw;
        let expected = 6.0 + (0.2 + std::f32::consts::TAU - 6.0) / 2.0;
        assert!((halfway - expected).abs() < 1e-4, "{halfway}");
    }
}
