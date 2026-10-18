//! How creatures and other players move beyond going from place to place (M-A2): breathing,
//! blinking, looking about, grazing, tails swaying, a flinch when hurt, an arm's swing and a
//! death that tips over and fades. Each is a function of the time and what's being drawn, and
//! of a seed so that no two creatures breathe or blink together, so they're deterministic and
//! bounded, and drawing (`client::creatures`, `client::models`) only reads them.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bevy::prelude::*;

// How much a body swells as it breathes in, and how long a breath takes, in seconds.
const BREATH: f32 = 0.025;
const BREATH_SECONDS: f32 = 3.2;
// Eyes blink once in every so many seconds, at a time in it the seed picks, for so long.
const BLINK_EVERY: f32 = 4.5;
const BLINK: f32 = 0.14;
// Grazing: in each spell of so many seconds, it may lower its head for so long, easing down and
// up over the last part of that, and it does so one spell in so many.
const GRAZE_EVERY: f32 = 12.0;
const GRAZE: f32 = 3.5;
const GRAZE_EASE: f32 = 0.6;
const GRAZE_CHANCE: f32 = 0.6;
/// How far a grazing head's lowered, in radians.
pub const GRAZE_PITCH: f32 = -1.0;
// A tail sways a little all the time, in radians, and now and then, one spell in so many
// seconds, flicks further, quickly.
const TAIL_SWAY: f32 = 0.12;
const FLICK_EVERY: f32 = 5.0;
const FLICK: f32 = 0.45;
const FLICK_ANGLE: f32 = 0.6;
/// How far a head turns from straight ahead to look at someone, across and up or down, in
/// radians.
pub const LOOK_YAW: f32 = 1.2;
pub const LOOK_PITCH: f32 = 0.7;
// Someone further round behind than this, past where the head turns to, isn't looked at.
const LOOK_BEHIND: f32 = 0.4;
// A flinch rears the body back this far, in radians, and is over in so long.
const FLINCH_ANGLE: f32 = 0.25;
const FLINCH: f32 = 0.3;
// Dying, a body tips onto its side over the first of this, then fades out over the rest, in
// seconds.
const TIP: f32 = 0.45;
/// How long dying takes, in seconds, after which what's drawn goes.
pub const DEATH: f32 = 1.0;
// An arm's swing, as a player breaks or places, raises it this far, in radians, and takes so
// long.
const ARM_SWING: f32 = 1.3;
/// How long a player's arm takes to swing, in seconds.
pub const ARM_SWING_SECONDS: f32 = 0.3;
// How fast a wing flaps, in radians a second, and how far, in radians, at the most.
const FLAP_RATE: f32 = 22.0;
const FLAP: f32 = 0.9;

/// A number from 0 up to 1 for `seed` and `n`, the same every time for the same two.
pub fn hash(seed: u32, n: u32) -> f32 {
    // a round of PCG's output mix, enough to scatter neighbouring numbers
    let mut x = seed.wrapping_mul(0x9e37_79b9) ^ n.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    (x >> 8) as f32 / (1 << 24) as f32
}

/// 0 at or below 0, rising smoothly to 1 at or above 1.
pub fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// `from` moved towards `to` over `dt` seconds, as far as `rate` takes it, a fraction of the
/// way that doesn't depend on how the time's cut up into frames.
pub fn ease(from: f32, to: f32, rate: f32, dt: f32) -> f32 {
    from + (to - from) * (1.0 - (-rate * dt).exp())
}

/// An angle eased towards another the short way round, as `ease` does.
pub fn ease_angle(from: f32, to: f32, rate: f32, dt: f32) -> f32 {
    from + wrap(to - from) * (1.0 - (-rate * dt).exp())
}

/// An angle as the same one between -π and π.
pub fn wrap(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}

/// How much bigger a body is drawn at time `t` than it is, breathing in and out.
pub fn breath(t: f32, seed: u32) -> f32 {
    1.0 + BREATH * (t / BREATH_SECONDS * TAU + hash(seed, 0) * TAU).sin()
}

/// Whether eyes are shut at time `t`.
pub fn blinking(t: f32, seed: u32) -> bool {
    let spell = (t / BLINK_EVERY).floor();
    let start = hash(seed, spell as i32 as u32) * (BLINK_EVERY - BLINK);
    (start..start + BLINK).contains(&(t - spell * BLINK_EVERY))
}

/// How far a head's lowered to graze at time `t`, from 0 up to 1.
pub fn grazing(t: f32, seed: u32) -> f32 {
    let spell = (t / GRAZE_EVERY).floor();
    let n = spell as i32 as u32;
    if hash(seed, n.wrapping_mul(2)) > GRAZE_CHANCE {
        return 0.0;
    }
    let start = hash(seed, n.wrapping_mul(2) + 1) * (GRAZE_EVERY - GRAZE);
    let into = t - spell * GRAZE_EVERY - start;
    smoothstep(into / GRAZE_EASE).min(smoothstep((GRAZE - into) / GRAZE_EASE))
}

/// How far a tail's turned aside at time `t`, in radians.
pub fn tail(t: f32, seed: u32) -> f32 {
    let sway = TAIL_SWAY * (t * 1.7 + hash(seed, 1) * TAU).sin();
    let spell = (t / FLICK_EVERY).floor();
    let start = hash(seed, spell as i32 as u32 ^ 0x5bd1) * (FLICK_EVERY - FLICK);
    let into = (t - spell * FLICK_EVERY - start) / FLICK;
    let flick = if (0.0..1.0).contains(&into) {
        FLICK_ANGLE * (into * TAU).sin()
    } else {
        0.0
    };
    sway + flick
}

/// Which way a head turns, across and up, from a body facing `yaw` to look from `eye` at
/// `target`, within how far it turns, or none if the target's too far round behind.
pub fn head_look(yaw: f32, eye: Vec3, target: Vec3) -> Option<Vec2> {
    let to = target - eye;
    let across = wrap((-to.x).atan2(-to.z) - yaw);
    if across.abs() > LOOK_YAW + LOOK_BEHIND {
        return None;
    }
    let up = to.y.atan2(to.xz().length());
    Some(Vec2::new(
        across.clamp(-LOOK_YAW, LOOK_YAW),
        up.clamp(-LOOK_PITCH, LOOK_PITCH),
    ))
}

/// How far a body rears back `since` seconds after it's hurt, in radians.
pub fn flinch(since: f32) -> f32 {
    if (0.0..FLINCH).contains(&since) {
        FLINCH_ANGLE * (since / FLINCH * PI).sin()
    } else {
        0.0
    }
}

/// How far a body's tipped onto its side `age` seconds into dying, in radians, and how much of
/// it's left to see, from 1 down to 0, or none once it's over.
pub fn death(age: f32) -> Option<(f32, f32)> {
    if age >= DEATH {
        return None;
    }
    let tipped = smoothstep(age / TIP) * FRAC_PI_2;
    Some((tipped, 1.0 - smoothstep((age - TIP) / (DEATH - TIP))))
}

/// How far a player's arm is raised `since` seconds into a swing, in radians, forward.
pub fn arm_swing(since: f32) -> f32 {
    if (0.0..ARM_SWING_SECONDS).contains(&since) {
        ARM_SWING * (since / ARM_SWING_SECONDS * PI).sin()
    } else {
        0.0
    }
}

/// How far a wing's raised at time `t`, flapping `amount` of the most it does, in radians.
pub fn flap(t: f32, amount: f32) -> f32 {
    amount * FLAP * (0.4 + 0.6 * (t * FLAP_RATE).sin().abs())
}

/// How fully legs swing at a speed, from 0 standing to 1 at `full` and over, easing in and out
/// so a step's worth of pose doesn't jerk them.
pub fn stride_swing(speed: f32, full: f32) -> f32 {
    smoothstep(speed / full)
}

#[cfg(test)]
mod tests {
    use std::f32::consts::{FRAC_PI_2, PI};

    use bevy::math::{Vec2, Vec3};

    use super::*;

    // Times over a few minutes, in uneven steps.
    fn times() -> impl Iterator<Item = f32> {
        (0..20_000).map(|i| i as f32 * 0.0137)
    }

    #[test]
    fn test_idle_life_is_the_same_every_time_and_within_its_bounds() {
        for seed in [0, 1, 77, u32::MAX] {
            for t in times() {
                let (b, g, tail_angle) = (breath(t, seed), grazing(t, seed), tail(t, seed));
                assert_eq!(b, breath(t, seed));
                assert_eq!(g, grazing(t, seed));
                assert_eq!(blinking(t, seed), blinking(t, seed));
                assert!((1.0 - BREATH..=1.0 + BREATH).contains(&b), "{b}");
                assert!((0.0..=1.0).contains(&g), "{g}");
                assert!(tail_angle.abs() <= TAIL_SWAY + FLICK_ANGLE, "{tail_angle}");
                assert!((0.0..1.0).contains(&hash(seed, t as u32)));
            }
        }
    }

    #[test]
    fn test_eyes_blink_briefly_once_in_every_few_seconds_and_out_of_step() {
        let shut = |seed| times().filter(|&t| blinking(t, seed)).count() as f32;
        let all = times().count() as f32;
        // about a blink's worth of each spell
        let share = shut(3) / all;
        assert!((share - BLINK / BLINK_EVERY).abs() < 0.01, "{share}");
        // and two creatures don't blink together
        let together = times()
            .filter(|&t| blinking(t, 3) && blinking(t, 4))
            .count();
        assert!((together as f32) < shut(3) * 0.2, "{together}");
    }

    #[test]
    fn test_grazing_eases_down_and_up_and_happens_now_and_then() {
        let grazes: Vec<_> = times().map(|t| grazing(t, 9)).collect();
        assert!(grazes.contains(&1.0) && grazes.contains(&0.0));
        // never jumping, at a step of 14 ms
        let steepest = grazes
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0, f32::max);
        assert!(steepest < 0.05, "{steepest}");
    }

    #[test]
    fn test_heads_look_within_their_limits_and_not_behind() {
        let eye = Vec3::new(0.0, 1.0, 0.0);
        // facing -z, someone ahead and a little to the right (+x) and up
        let ahead = head_look(0.0, eye, Vec3::new(1.0, 2.0, -4.0)).unwrap();
        assert!(ahead.x < 0.0 && ahead.x > -0.5 && ahead.y > 0.0, "{ahead}");
        // someone far off to the side is turned to as far as a head turns
        let aside = head_look(0.0, eye, Vec3::new(-4.0, 1.0, -0.5)).unwrap();
        assert!((aside.x - LOOK_YAW).abs() < 1e-6, "{aside}");
        // straight up, only as far as it tilts
        let up = head_look(0.0, eye, Vec3::new(0.0, 10.0, -0.1)).unwrap();
        assert!((up.y - LOOK_PITCH).abs() < 1e-6, "{up}");
        // and behind, not at all
        assert_eq!(None, head_look(0.0, eye, Vec3::new(0.0, 1.0, 4.0)));
        // facing the other way, behind is ahead
        let turned = head_look(PI, eye, Vec3::new(0.0, 1.0, 4.0)).unwrap();
        assert!(turned.length() < 1e-5, "{turned}");
        for yaw in [-3.0, -1.0, 0.5, 2.9] {
            for target in [Vec3::new(3.0, -5.0, 1.0), Vec3::new(-2.0, 4.0, -7.0)] {
                if let Some(look) = head_look(yaw, eye, target) {
                    assert!(look.x.abs() <= LOOK_YAW && look.y.abs() <= LOOK_PITCH);
                }
            }
        }
        assert_eq!(Vec2::ZERO, head_look(0.0, eye, eye + Vec3::NEG_Z).unwrap());
    }

    #[test]
    fn test_easing_goes_the_short_way_and_doesnt_depend_on_the_frame_rate() {
        let once = ease(0.0, 1.0, 5.0, 0.1);
        let twice = ease(ease(0.0, 1.0, 5.0, 0.05), 1.0, 5.0, 0.05);
        assert!((once - twice).abs() < 1e-6);
        assert!(once > 0.0 && once < 1.0);
        // from just short of a turn round to just past it, through 0 rather than back round
        let turned = ease_angle(6.0, 0.2, 100.0, 1.0);
        assert!((wrap(turned) - 0.2).abs() < 1e-4, "{turned}");
        assert!((stride_swing(0.0, 1.4)).abs() < 1e-6 && stride_swing(5.0, 1.4) == 1.0);
    }

    #[test]
    fn test_a_death_tips_over_then_fades_and_ends() {
        let (tipped, left) = death(0.0).unwrap();
        assert_eq!((0.0, 1.0), (tipped, left));
        let (tipped, left) = death(TIP).unwrap();
        assert!((tipped - FRAC_PI_2).abs() < 1e-6 && left == 1.0);
        let (_, left) = death((TIP + DEATH) / 2.0).unwrap();
        assert!(left > 0.0 && left < 1.0);
        assert_eq!(None, death(DEATH));
        // and a flinch and an arm's swing are each over in a moment
        assert_eq!(0.0, flinch(0.0));
        assert!(flinch(FLINCH / 2.0) > 0.2);
        assert_eq!(0.0, flinch(FLINCH));
        assert!(arm_swing(ARM_SWING_SECONDS / 2.0) > 1.0);
        assert_eq!(0.0, arm_swing(ARM_SWING_SECONDS));
        for t in times() {
            assert!(flap(t, 1.0) <= FLAP && flap(t, 0.0) == 0.0);
        }
    }
}
