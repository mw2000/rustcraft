//! Creatures (§5.8, INV-6): the animals and monsters living in the world. Every kind is a row of
//! the `creatures!` table, which says how big it is, how it moves, what it looks like, what it
//! drops, where it spawns, how it behaves and how it fights. The server simulates them
//! (`server::creatures`) and clients draw them where the server says they are
//! (`client::creatures`).

pub mod path;

use std::ops::{Range, RangeInclusive};

use bevy::math::I64Vec3;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::block::BlockType;
use crate::collision::Aabb;
use crate::items::Item;

/// How big a young creature is beside a grown one.
pub const YOUNG_SCALE: f32 = 0.5;

/// A kind's row in the table.
pub struct Properties {
    /// How wide and deep its body is, and how tall, in blocks, grown.
    pub width: f32,
    pub height: f32,
    /// Full health, in the same half hearts as a player's.
    pub health: u8,
    /// How fast it walks about, and how fast it runs when it's frightened, in blocks a second.
    pub walk_speed: f32,
    pub run_speed: f32,
    /// What it looks like, facing -z, and where its head turns from, in blocks from its feet.
    pub model: &'static [Part],
    pub neck: Vec3,
    /// Whether it lowers its head to graze now and then as it stands about.
    pub grazes: bool,
    /// What it drops when it dies grown: each item, and how many of it, chosen evenly, so a
    /// range from 0 sometimes drops none.
    pub drops: &'static [(Item, RangeInclusive<u8>)],
    /// What shearing it gives, and how many, if it can be shorn. Killed unshorn, it drops one
    /// of it too.
    pub fleece: Option<(Item, RangeInclusive<u8>)>,
    /// What it follows a player holding, and is fed to breed.
    pub food: &'static [Item],
    /// Where it appears, if it appears by itself.
    pub spawns: Option<Spawns>,
    /// What it does, the first in the list that has something to do winning.
    pub behaviours: &'static [Behaviour],
    /// Whether it keeps out of water when it finds its way, as it would rather not swim.
    pub avoids_water: bool,
    /// Whether it flutters down rather than falling, so no fall hurts it, as a chicken does.
    pub flutters: bool,
    /// How it fights players, if it does.
    pub attack: Option<Attack>,
    /// Whether it goes once nobody's near, unsaved, as monsters do, rather than sleeping.
    pub despawns: bool,
    /// Whether it fades away once the sun's up and it's out under the sky.
    pub fades_by_day: bool,
    /// Whether it hangs from the ceiling it appears under until it drops on someone, and no fall
    /// hurts it.
    pub clings: bool,
    /// How often a dungeon's spawner is of its kind, by weight against the others', 0 for never.
    pub guards: u8,
}

/// How a kind fights: how hard and how far it hits, how long it winds up a blow, which is how
/// long a player has to get away, and how long it takes to strike again, how hard it knocks
/// them back, how far off it notices them, and what's said when it kills someone.
#[derive(Debug, Clone, PartialEq)]
pub struct Attack {
    pub damage: u8,
    /// Blocks from its body to theirs.
    pub reach: f32,
    pub wind_up: f32,
    pub cooldown: f32,
    /// In blocks a second.
    pub knockback: f32,
    pub sight: f32,
    /// How near it leaps at its prey from, in blocks, or 0 if it doesn't.
    pub lunge: f32,
    /// What it throws from afar, rather than hitting.
    pub throws: Option<Throw>,
    /// What's said of someone it kills, after their name.
    pub slays: &'static str,
}

/// How a kind that throws things keeps its distance: further than the range's start and nearer
/// than its end, in blocks across, and how fast it throws, in blocks a second.
#[derive(Debug, Clone, PartialEq)]
pub struct Throw {
    pub keep: Range<f32>,
    pub speed: f32,
}

/// A box of a creature's model: its size and the middle of it, in blocks from the creature's
/// feet as it faces -z, its colour, in sRGB, what it moves with, and what it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Part {
    pub size: Vec3,
    pub centre: Vec3,
    pub colour: [f32; 3],
    pub joint: Joint,
    pub look: Look,
}

/// What a part of a model moves with (`client::creatures`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Joint {
    /// The body, which breathes.
    Body,
    /// A leg, swinging from its top as the creature walks, `1.0` or `-1.0` for which way first.
    Leg(f32),
    /// An arm on the side `1.0` (+x) or `-1.0`, swinging from its top against the legs as the
    /// creature walks, and raised as it winds up a blow.
    Arm(f32),
    /// The head, turning from the kind's neck to look about and graze.
    Head,
    /// A tail, swaying and flicking from where it meets the body.
    Tail,
    /// A wing on the side `1.0` (+x) or `-1.0`, flapping from its top as the creature flutters.
    Wing(f32),
}

/// What a part of a model is, beyond a box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    Plain,
    /// Fleece, shown only while the creature isn't shorn.
    Fleece,
    /// An eye, which blinks.
    Eye,
    /// An eye that glows in the dark, as a monster's do, and blinks.
    Glow,
}

/// Where a kind appears: in which biomes, by name, the base terrain's being `base`, or all of
/// them if none are named, on which blocks, or under them if it hangs from ceilings, out under
/// the sky or not, by day or by night, only in the dark or not, only below a height or not, in
/// groups of how many, and how many there can be round a player before no more come.
#[derive(Debug, Clone, PartialEq)]
pub struct Spawns {
    pub biomes: &'static [&'static str],
    pub on: &'static [BlockType],
    pub ceiling: bool,
    pub under_sky: bool,
    pub by: Daytime,
    /// Only where no block gives light, and where it's under the sky, only at night.
    pub dark: bool,
    pub below: Option<i64>,
    pub group: RangeInclusive<u8>,
    pub cap: usize,
}

/// When a kind appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Daytime {
    Day,
    Night,
    Always,
}

/// Something a creature does, as the server's brains run it (`server::creatures`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Behaviour {
    /// Runs from whatever hurt it, for a few seconds.
    Flee,
    /// Goes to another of its kind that's been fed too, and makes a young one with it.
    Breed,
    /// Follows a player near it holding its food, as a sheep follows wheat.
    FollowLure,
    /// Stands about, and now and then walks somewhere near.
    Wander,
    /// Goes after a player it sees, and attacks them as its kind's `Attack` says: up to them to
    /// hit them, or keeping its distance to throw at them.
    Hunt,
    /// Hangs from the ceiling, if it's still there, until a player comes under it.
    Ambush,
    /// Runs from bright light, and from a player holding something that gives it.
    ShunLight,
}

// Declares `CreatureKind`, with a variant for each row, and each row's stable name and
// properties.
macro_rules! creatures {
    ($($(#[$attr:meta])* $variant:ident { name: $name:literal, $properties:expr $(,)? },)*) => {
        /// A kind of creature.
        #[derive(Debug, PartialEq, Eq, Copy, Clone, Hash)]
        pub enum CreatureKind {
            $($(#[$attr])* $variant,)*
        }

        impl CreatureKind {
            /// Every kind, in the table's order.
            pub const ALL: &[CreatureKind] = &[$(CreatureKind::$variant),*];

            /// Its stable name, which saves and the network go by (§4.1).
            pub fn name(self) -> &'static str {
                match self {
                    $(CreatureKind::$variant => $name,)*
                }
            }

            pub fn properties(self) -> &'static Properties {
                match self {
                    $(CreatureKind::$variant => {
                        const PROPERTIES: Properties = $properties;
                        &PROPERTIES
                    })*
                }
            }
        }
    };
}

// A box of the body.
const fn part(size: [f32; 3], centre: [f32; 3], colour: [f32; 3]) -> Part {
    Part {
        size: Vec3::from_array(size),
        centre: Vec3::from_array(centre),
        colour,
        joint: Joint::Body,
        look: Look::Plain,
    }
}

// The same box moving with `joint`.
const fn on(joint: Joint, part: Part) -> Part {
    Part { joint, ..part }
}

// The same box as fleece, which shearing takes away.
const fn fleece(part: Part) -> Part {
    Part {
        look: Look::Fleece,
        ..part
    }
}

// A box of the head.
const fn head(size: [f32; 3], centre: [f32; 3], colour: [f32; 3]) -> Part {
    on(Joint::Head, part(size, centre, colour))
}

// A leg, `size` big, under the hip at `x`, `z`, swinging first the way `swing` says.
const fn leg(size: [f32; 3], x: f32, z: f32, colour: [f32; 3], swing: f32) -> Part {
    on(Joint::Leg(swing), part(size, [x, size[1] / 2.0, z], colour))
}

// Four legs under a body, `x` either side of the middle and `z` before and behind it, the front
// pair swinging opposite the back and each against the one beside it.
const fn legs(size: [f32; 3], x: f32, z: f32, colour: [f32; 3]) -> [Part; 4] {
    [
        leg(size, -x, -z, colour, 1.0),
        leg(size, x, -z, colour, -1.0),
        leg(size, -x, z, colour, -1.0),
        leg(size, x, z, colour, 1.0),
    ]
}

// Two eyes `size` across, `x` either side of the middle at `y`, on a face at `z`.
const fn eyes(x: f32, y: f32, z: f32, size: f32) -> [Part; 2] {
    [
        eye([size, size, 0.02], [-x, y, z]),
        eye([size, size, 0.02], [x, y, z]),
    ]
}

// An eye, `size` big, at `centre` on the face.
const fn eye(size: [f32; 3], centre: [f32; 3]) -> Part {
    Part {
        look: Look::Eye,
        ..head(size, centre, EYES)
    }
}

const EYES: [f32; 3] = [0.08, 0.07, 0.07];

const WOOL: [f32; 3] = [0.91, 0.9, 0.86];
const SHEEP_SKIN: [f32; 3] = [0.78, 0.66, 0.56];
const SHEEP_LEGS: [f32; 3] = [0.72, 0.62, 0.54];

// A body of wool over a shorn one of skin, on four legs, with a stub of a tail, and a face out
// in front under a cap of wool, with ears.
const SHEEP: &[Part] = &{
    let [a, b, c, d] = legs([0.22, 0.6, 0.22], 0.24, 0.38, SHEEP_LEGS);
    let [left, right] = eyes(0.13, 1.14, -1.0, 0.08);
    [
        a,
        b,
        c,
        d,
        part([0.66, 0.5, 1.04], [0.0, 0.92, 0.04], SHEEP_SKIN),
        fleece(part([0.86, 0.66, 1.2], [0.0, 0.92, 0.04], WOOL)),
        on(
            Joint::Tail,
            part([0.18, 0.24, 0.16], [0.0, 0.94, 0.64], WOOL),
        ),
        head([0.46, 0.46, 0.5], [0.0, 1.08, -0.74], SHEEP_SKIN),
        fleece(head([0.52, 0.16, 0.36], [0.0, 1.32, -0.68], WOOL)),
        head([0.08, 0.16, 0.12], [-0.27, 1.18, -0.62], SHEEP_SKIN),
        head([0.08, 0.16, 0.12], [0.27, 1.18, -0.62], SHEEP_SKIN),
        left,
        right,
    ]
};

const HIDE: [f32; 3] = [0.34, 0.23, 0.16];
const PATCH: [f32; 3] = [0.88, 0.86, 0.8];
const HOOVES: [f32; 3] = [0.2, 0.15, 0.12];
const MUZZLE: [f32; 3] = [0.8, 0.6, 0.55];
const HORN: [f32; 3] = [0.86, 0.82, 0.7];

// A deep brown body with pale patches on its flanks, on four dark legs, an udder and a long
// thin tail, and a broad head with a pink muzzle and two short horns.
const COW: &[Part] = &{
    let [a, b, c, d] = legs([0.24, 0.7, 0.24], 0.26, 0.44, HOOVES);
    let [left, right] = eyes(0.15, 1.3, -1.05, 0.08);
    [
        a,
        b,
        c,
        d,
        part([0.9, 0.7, 1.36], [0.0, 1.05, 0.04], HIDE),
        part([0.92, 0.36, 0.42], [0.0, 1.12, 0.24], PATCH),
        part([0.92, 0.26, 0.3], [0.0, 0.96, -0.3], PATCH),
        part([0.3, 0.12, 0.3], [0.0, 0.66, 0.3], MUZZLE),
        on(
            Joint::Tail,
            part([0.08, 0.56, 0.08], [0.0, 1.08, 0.76], HIDE),
        ),
        head([0.54, 0.5, 0.4], [0.0, 1.22, -0.84], HIDE),
        head([0.4, 0.22, 0.12], [0.0, 1.07, -1.08], MUZZLE),
        head([0.1, 0.16, 0.1], [-0.22, 1.54, -0.8], HORN),
        head([0.1, 0.16, 0.1], [0.22, 1.54, -0.8], HORN),
        left,
        right,
    ]
};

const PIG_SKIN: [f32; 3] = [0.93, 0.66, 0.64];
const SNOUT: [f32; 3] = [0.84, 0.5, 0.5];

// A round pink body low on four short legs with a little tail, and a big head with a flat snout
// and ears.
const PIG: &[Part] = &{
    let [a, b, c, d] = legs([0.22, 0.34, 0.22], 0.2, 0.32, PIG_SKIN);
    let [left, right] = eyes(0.13, 0.76, -0.91, 0.07);
    [
        a,
        b,
        c,
        d,
        part([0.7, 0.54, 1.0], [0.0, 0.6, 0.04], PIG_SKIN),
        on(
            Joint::Tail,
            part([0.08, 0.14, 0.08], [0.0, 0.76, 0.56], SNOUT),
        ),
        head([0.5, 0.46, 0.44], [0.0, 0.68, -0.68], PIG_SKIN),
        head([0.26, 0.18, 0.08], [0.0, 0.6, -0.94], SNOUT),
        head([0.14, 0.12, 0.04], [-0.17, 0.94, -0.74], SNOUT),
        head([0.14, 0.12, 0.04], [0.17, 0.94, -0.74], SNOUT),
        left,
        right,
    ]
};

const FEATHERS: [f32; 3] = [0.95, 0.94, 0.9];
const BEAK: [f32; 3] = [0.92, 0.62, 0.2];
const COMB: [f32; 3] = [0.82, 0.16, 0.14];

// A small white body on two thin legs, with folded wings, a tail, and a head with a beak, a
// wattle and a comb.
const CHICKEN: &[Part] = &{
    let [left, right] = eyes(0.07, 0.76, -0.33, 0.05);
    [
        leg([0.06, 0.26, 0.06], -0.08, 0.02, BEAK, 1.0),
        leg([0.06, 0.26, 0.06], 0.08, 0.02, BEAK, -1.0),
        part([0.36, 0.32, 0.46], [0.0, 0.42, 0.02], FEATHERS),
        on(
            Joint::Wing(-1.0),
            part([0.06, 0.22, 0.34], [-0.21, 0.44, 0.04], FEATHERS),
        ),
        on(
            Joint::Wing(1.0),
            part([0.06, 0.22, 0.34], [0.21, 0.44, 0.04], FEATHERS),
        ),
        on(
            Joint::Tail,
            part([0.24, 0.2, 0.08], [0.0, 0.58, 0.28], FEATHERS),
        ),
        head([0.22, 0.28, 0.2], [0.0, 0.7, -0.22], FEATHERS),
        head([0.12, 0.07, 0.1], [0.0, 0.7, -0.37], BEAK),
        head([0.06, 0.1, 0.04], [0.0, 0.6, -0.33], COMB),
        head([0.06, 0.08, 0.14], [0.0, 0.88, -0.22], COMB),
        left,
        right,
    ]
};

// Two eyes that glow, as `eyes` are placed.
const fn glowing(x: f32, y: f32, z: f32, size: f32, colour: [f32; 3]) -> [Part; 2] {
    let [left, right] = eyes(x, y, z, size);
    [
        Part {
            look: Look::Glow,
            colour,
            ..left
        },
        Part {
            look: Look::Glow,
            colour,
            ..right
        },
    ]
}

// An arm, `size` big, hanging from the shoulder at `x`, its top at `top`, `z` along the body.
const fn arm(size: [f32; 3], x: f32, top: f32, z: f32, colour: [f32; 3]) -> Part {
    let side = if x < 0.0 { -1.0 } else { 1.0 };
    on(
        Joint::Arm(side),
        part(size, [x, top - size[1] / 2.0, z], colour),
    )
}

const DUSK_HIDE: [f32; 3] = [0.15, 0.16, 0.23];
const DUSK_BELLY: [f32; 3] = [0.25, 0.26, 0.34];
const DUSK_LEGS: [f32; 3] = [0.11, 0.11, 0.17];
const BONE: [f32; 3] = [0.74, 0.75, 0.78];
const PALE_GLOW: [f32; 3] = [0.78, 1.0, 0.86];

// Lean and long in the leg, slate dark to be hard to see at night: a hunched back with a ridge
// of pale spines, a long thin tail, and a narrow head held low and forward, with eyes that glow.
const GLOAMER: &[Part] = &{
    let [a, b, c, d] = legs([0.14, 0.62, 0.14], 0.19, 0.32, DUSK_LEGS);
    let [left, right] = glowing(0.09, 1.02, -0.98, 0.07, PALE_GLOW);
    [
        a,
        b,
        c,
        d,
        part([0.5, 0.42, 1.0], [0.0, 0.82, 0.04], DUSK_HIDE),
        part([0.56, 0.24, 0.42], [0.0, 1.07, -0.24], DUSK_HIDE),
        part([0.4, 0.1, 0.7], [0.0, 0.6, 0.04], DUSK_BELLY),
        part([0.06, 0.16, 0.1], [0.0, 1.26, -0.3], BONE),
        part([0.06, 0.13, 0.1], [0.0, 1.13, 0.02], BONE),
        part([0.06, 0.1, 0.1], [0.0, 1.07, 0.3], BONE),
        on(
            Joint::Tail,
            part([0.07, 0.07, 0.6], [0.0, 0.92, 0.84], DUSK_HIDE),
        ),
        head([0.34, 0.3, 0.46], [0.0, 0.98, -0.74], DUSK_HIDE),
        head([0.22, 0.17, 0.24], [0.0, 0.91, -1.04], DUSK_BELLY),
        head([0.08, 0.2, 0.08], [-0.12, 1.2, -0.62], DUSK_HIDE),
        head([0.08, 0.2, 0.08], [0.12, 1.2, -0.62], DUSK_HIDE),
        left,
        right,
    ]
};

const CARAPACE: [f32; 3] = [0.66, 0.64, 0.7];
const UNDERSIDE: [f32; 3] = [0.44, 0.41, 0.5];
const FEELERS: [f32; 3] = [0.34, 0.31, 0.4];
const CAVE_GLOW: [f32; 3] = [0.6, 0.86, 1.0];

// Flat and pale, as things that never see the sun are: a broad plated body on six short legs,
// a low head with mandibles and four small eyes that glow. It hangs upside down from ceilings.
const LURKER: &[Part] = &{
    let [a, b] = glowing(0.07, 0.42, -0.625, 0.05, CAVE_GLOW);
    let [c, d] = glowing(0.15, 0.38, -0.625, 0.04, CAVE_GLOW);
    let leg = [0.1, 0.3, 0.1];
    [
        leg_at(leg, -0.44, -0.3, 1.0),
        leg_at(leg, 0.44, -0.3, -1.0),
        leg_at(leg, -0.44, 0.05, -1.0),
        leg_at(leg, 0.44, 0.05, 1.0),
        leg_at(leg, -0.44, 0.4, 1.0),
        leg_at(leg, 0.44, 0.4, -1.0),
        part([0.8, 0.24, 0.9], [0.0, 0.36, 0.05], CARAPACE),
        part([0.84, 0.08, 0.94], [0.0, 0.22, 0.05], UNDERSIDE),
        part([0.5, 0.08, 0.6], [0.0, 0.52, 0.1], UNDERSIDE),
        head([0.4, 0.2, 0.24], [0.0, 0.36, -0.5], CARAPACE),
        head([0.06, 0.06, 0.16], [-0.12, 0.27, -0.66], FEELERS),
        head([0.06, 0.06, 0.16], [0.12, 0.27, -0.66], FEELERS),
        a,
        b,
        c,
        d,
    ]
};

// One of a lurker's legs, in its colour.
const fn leg_at(size: [f32; 3], x: f32, z: f32, swing: f32) -> Part {
    leg(size, x, z, FEELERS, swing)
}

const MOSSY_SKIN: [f32; 3] = [0.45, 0.52, 0.37];
const DARK_SKIN: [f32; 3] = [0.34, 0.4, 0.28];
const CLOAK: [f32; 3] = [0.36, 0.26, 0.16];
const HOOD: [f32; 3] = [0.27, 0.19, 0.12];
const AMBER_GLOW: [f32; 3] = [1.0, 0.76, 0.3];

// Small and hunched in a ragged brown cloak and hood, on bandy legs, with long arms for throwing,
// big ears out of the hood and amber eyes that glow.
const SLINGER: &[Part] = &{
    let [left, right] = glowing(0.08, 1.08, -0.215, 0.06, AMBER_GLOW);
    [
        leg([0.14, 0.42, 0.16], -0.1, 0.0, DARK_SKIN, 1.0),
        leg([0.14, 0.42, 0.16], 0.1, 0.0, DARK_SKIN, -1.0),
        part([0.4, 0.48, 0.3], [0.0, 0.66, 0.02], CLOAK),
        part([0.46, 0.52, 0.1], [0.0, 0.72, 0.2], HOOD),
        part([0.12, 0.12, 0.08], [0.12, 0.46, -0.16], HOOD),
        arm([0.1, 0.5, 0.1], -0.26, 0.88, 0.0, MOSSY_SKIN),
        arm([0.1, 0.5, 0.1], 0.26, 0.88, 0.0, MOSSY_SKIN),
        head([0.36, 0.32, 0.34], [0.0, 1.05, -0.04], MOSSY_SKIN),
        head([0.42, 0.18, 0.4], [0.0, 1.2, 0.0], HOOD),
        head([0.2, 0.08, 0.06], [-0.27, 1.08, 0.0], MOSSY_SKIN),
        head([0.2, 0.08, 0.06], [0.27, 1.08, 0.0], MOSSY_SKIN),
        head([0.08, 0.1, 0.08], [0.0, 1.0, -0.24], DARK_SKIN),
        left,
        right,
    ]
};

const BARROW_STONE: [f32; 3] = [0.43, 0.43, 0.45];
const BARROW_DARK: [f32; 3] = [0.29, 0.29, 0.31];
const BARROW_MOSS: [f32; 3] = [0.3, 0.41, 0.22];
const EMBER_GLOW: [f32; 3] = [1.0, 0.46, 0.14];

// A hulk of grey stone with moss on its shoulders: huge shoulders over a thick body on short
// legs, arms hanging nearly to the ground, and a small head sunk between them under a heavy
// brow, with eyes like embers.
const BARROW_BRUTE: &[Part] = &{
    let [left, right] = glowing(0.12, 2.04, -0.465, 0.08, EMBER_GLOW);
    [
        leg([0.34, 0.7, 0.36], -0.28, 0.05, BARROW_DARK, 1.0),
        leg([0.34, 0.7, 0.36], 0.28, 0.05, BARROW_DARK, -1.0),
        part([0.96, 0.9, 0.7], [0.0, 1.15, 0.0], BARROW_STONE),
        part([0.7, 0.5, 0.06], [0.0, 1.05, -0.36], BARROW_DARK),
        part([1.3, 0.42, 0.8], [0.0, 1.72, -0.02], BARROW_STONE),
        part([0.6, 0.12, 0.5], [0.18, 1.96, 0.06], BARROW_MOSS),
        arm([0.34, 1.3, 0.36], -0.82, 1.86, -0.05, BARROW_STONE),
        arm([0.34, 1.3, 0.36], 0.82, 1.86, -0.05, BARROW_STONE),
        head([0.5, 0.44, 0.48], [0.0, 2.05, -0.22], BARROW_STONE),
        head([0.56, 0.1, 0.14], [0.0, 2.17, -0.44], BARROW_DARK),
        left,
        right,
    ]
};

// What a farm animal does: runs when it's hit, breeds once it's fed, follows its food, and
// otherwise wanders.
const ANIMAL: &[Behaviour] = &[
    Behaviour::Flee,
    Behaviour::Breed,
    Behaviour::FollowLure,
    Behaviour::Wander,
];

// What a monster does: goes after a player it sees, and otherwise wanders. A lurker runs from
// light first, and hangs from the ceiling till someone's under it.
const MONSTER: &[Behaviour] = &[Behaviour::Hunt, Behaviour::Wander];
const LURKING: &[Behaviour] = &[
    Behaviour::ShunLight,
    Behaviour::Ambush,
    Behaviour::Hunt,
    Behaviour::Wander,
];

// The ground monsters come out on, under the sky and in the caves.
const OPEN_GROUND: &[BlockType] = &[
    BlockType::Grass,
    BlockType::Dirt,
    BlockType::Sand,
    BlockType::Snow,
    BlockType::Gravel,
];
const CAVE_ROCK: &[BlockType] = &[
    BlockType::Stone,
    BlockType::Dirt,
    BlockType::Gravel,
    BlockType::Sandstone,
];
const ANY_GROUND: &[BlockType] = &[
    BlockType::Grass,
    BlockType::Dirt,
    BlockType::Sand,
    BlockType::Snow,
    BlockType::Gravel,
    BlockType::Stone,
    BlockType::Sandstone,
];

// Animals living on the grass, out by day, which give meat (§5.5a) that cooks in a furnace:
// sheep and cows in open country, pigs in the woods, and chickens nearly everywhere, but none
// in deserts or on glaciers.
creatures! {
    Sheep {
        name: "sheep",
        Properties {
            width: 0.9,
            height: 1.3,
            health: 8,
            walk_speed: 1.4,
            run_speed: 3.4,
            model: SHEEP,
            neck: Vec3::from_array([0.0, 1.1, -0.5]),
            grazes: true,
            drops: &[(Item::Mutton, 1..=2)],
            fleece: Some((Item::Wool, 1..=3)),
            food: &[Item::Wheat],
            spawns: Some(Spawns {
                biomes: &["base", "foothills", "savanna", "taiga"],
                on: &[BlockType::Grass],
                ceiling: false,
                under_sky: true,
                by: Daytime::Day,
                dark: false,
                below: None,
                group: 2..=4,
                cap: 8,
            }),
            behaviours: ANIMAL,
            avoids_water: true,
            flutters: false,
            attack: None,
            despawns: false,
            fades_by_day: false,
            clings: false,
            guards: 0,
        },
    },
    // Leather and feathers are for later: armour, books, arrows.
    Cow {
        name: "cow",
        Properties {
            width: 0.9,
            height: 1.4,
            health: 10,
            walk_speed: 1.2,
            run_speed: 3.0,
            model: COW,
            neck: Vec3::from_array([0.0, 1.2, -0.62]),
            grazes: true,
            drops: &[(Item::Beef, 1..=3), (Item::Leather, 0..=2)],
            fleece: None,
            food: &[Item::Wheat],
            spawns: Some(Spawns {
                biomes: &["base", "foothills", "savanna", "forest"],
                on: &[BlockType::Grass],
                ceiling: false,
                under_sky: true,
                by: Daytime::Day,
                dark: false,
                below: None,
                group: 2..=4,
                cap: 6,
            }),
            behaviours: ANIMAL,
            avoids_water: true,
            flutters: false,
            attack: None,
            despawns: false,
            fades_by_day: false,
            clings: false,
            guards: 0,
        },
    },
    // There are no carrots yet, so pigs root for apples in the woods, and take wheat as cows do.
    Pig {
        name: "pig",
        Properties {
            width: 0.8,
            height: 0.9,
            health: 10,
            walk_speed: 1.3,
            run_speed: 3.2,
            model: PIG,
            neck: Vec3::from_array([0.0, 0.66, -0.46]),
            grazes: false,
            drops: &[(Item::Pork, 1..=3)],
            fleece: None,
            food: &[Item::Apple, Item::Wheat],
            spawns: Some(Spawns {
                biomes: &["forest", "jungle", "taiga"],
                on: &[BlockType::Grass],
                ceiling: false,
                under_sky: true,
                by: Daytime::Day,
                dark: false,
                below: None,
                group: 1..=3,
                cap: 6,
            }),
            behaviours: ANIMAL,
            avoids_water: true,
            flutters: false,
            attack: None,
            despawns: false,
            fades_by_day: false,
            clings: false,
            guards: 0,
        },
    },
    Chicken {
        name: "chicken",
        Properties {
            width: 0.45,
            height: 0.75,
            health: 4,
            walk_speed: 1.2,
            run_speed: 3.2,
            model: CHICKEN,
            neck: Vec3::from_array([0.0, 0.6, -0.16]),
            grazes: false,
            drops: &[(Item::Chicken, 1..=1), (Item::Feather, 0..=2)],
            fleece: None,
            food: &[Item::Seeds],
            spawns: Some(Spawns {
                biomes: &["base", "forest", "foothills", "jungle", "savanna", "taiga"],
                on: &[BlockType::Grass],
                ceiling: false,
                under_sky: true,
                by: Daytime::Day,
                dark: false,
                below: None,
                group: 2..=4,
                cap: 6,
            }),
            behaviours: ANIMAL,
            avoids_water: true,
            flutters: true,
            attack: None,
            despawns: false,
            fades_by_day: false,
            clings: false,
            guards: 0,
        },
    },
    // Monsters (M3.3), out in the dark: none of them drops meat, they despawn once nobody's
    // near, and they're never saved. A gloamer hunts the open at night, stalking slowly, then
    // running and leaping the last few blocks, and fades away at sunrise.
    Gloamer {
        name: "gloamer",
        Properties {
            width: 0.7,
            height: 1.25,
            health: 16,
            walk_speed: 2.0,
            run_speed: 5.0,
            model: GLOAMER,
            neck: Vec3::from_array([0.0, 0.98, -0.5]),
            grazes: false,
            drops: &[(Item::Leather, 0..=1)],
            fleece: None,
            food: &[],
            spawns: Some(Spawns {
                biomes: &[],
                on: OPEN_GROUND,
                ceiling: false,
                under_sky: true,
                by: Daytime::Night,
                dark: true,
                below: None,
                group: 1..=2,
                cap: 3,
            }),
            behaviours: MONSTER,
            avoids_water: true,
            flutters: false,
            attack: Some(Attack {
                damage: 3,
                reach: 1.2,
                wind_up: 0.3,
                cooldown: 1.3,
                knockback: 5.0,
                sight: 24.0,
                lunge: 4.5,
                throws: None,
                slays: "was run down by a gloamer",
            }),
            despawns: true,
            fades_by_day: true,
            clings: false,
            guards: 0,
        },
    },
    // Hangs from cave ceilings in the dark and drops on whoever walks under it, but runs from
    // torchlight, and from a torch held near it.
    Lurker {
        name: "lurker",
        Properties {
            width: 0.9,
            height: 0.6,
            health: 12,
            walk_speed: 1.6,
            run_speed: 3.8,
            model: LURKER,
            neck: Vec3::from_array([0.0, 0.36, -0.4]),
            grazes: false,
            drops: &[],
            fleece: None,
            food: &[],
            spawns: Some(Spawns {
                biomes: &[],
                on: CAVE_ROCK,
                ceiling: true,
                under_sky: false,
                by: Daytime::Always,
                dark: true,
                below: None,
                group: 1..=1,
                cap: 3,
            }),
            behaviours: LURKING,
            avoids_water: true,
            flutters: false,
            attack: Some(Attack {
                damage: 4,
                reach: 1.0,
                wind_up: 0.25,
                cooldown: 1.1,
                knockback: 3.0,
                sight: 12.0,
                lunge: 0.0,
                throws: None,
                slays: "was ambushed by a lurker",
            }),
            despawns: true,
            fades_by_day: false,
            clings: true,
            guards: 2,
        },
    },
    // Keeps its distance, out in the open at night and in the caves, and throws stones in an
    // arc, so it's best rushed, or ducked behind something.
    Slinger {
        name: "slinger",
        Properties {
            width: 0.6,
            height: 1.3,
            health: 10,
            walk_speed: 1.8,
            run_speed: 3.6,
            model: SLINGER,
            neck: Vec3::from_array([0.0, 0.9, 0.0]),
            grazes: false,
            drops: &[(Item::Block(BlockType::Cobblestone), 0..=2), (Item::Coal, 0..=1)],
            fleece: None,
            food: &[],
            spawns: Some(Spawns {
                biomes: &[],
                on: ANY_GROUND,
                ceiling: false,
                under_sky: false,
                by: Daytime::Always,
                dark: true,
                below: None,
                group: 1..=2,
                cap: 2,
            }),
            behaviours: MONSTER,
            avoids_water: true,
            flutters: false,
            attack: Some(Attack {
                damage: 3,
                reach: 0.0,
                wind_up: 0.6,
                cooldown: 2.4,
                knockback: 3.0,
                sight: 20.0,
                lunge: 0.0,
                throws: Some(Throw {
                    keep: 5.0..13.0,
                    speed: 16.0,
                }),
                slays: "was brought down by a slinger's stone",
            }),
            despawns: true,
            fades_by_day: false,
            clings: false,
            guards: 2,
        },
    },
    // Guards dungeons and the deep caves: slow and tough, and hits very hard, but raises its
    // arms a full second before it does, which is time to step back.
    BarrowBrute {
        name: "barrow_brute",
        Properties {
            width: 0.98,
            height: 2.3,
            health: 40,
            walk_speed: 1.3,
            run_speed: 2.4,
            model: BARROW_BRUTE,
            neck: Vec3::from_array([0.0, 1.9, -0.1]),
            grazes: false,
            drops: &[(Item::IronIngot, 0..=2), (Item::Coal, 1..=3)],
            fleece: None,
            food: &[],
            spawns: Some(Spawns {
                biomes: &[],
                on: CAVE_ROCK,
                ceiling: false,
                under_sky: false,
                by: Daytime::Always,
                dark: true,
                below: Some(400),
                group: 1..=1,
                cap: 1,
            }),
            behaviours: MONSTER,
            avoids_water: true,
            flutters: false,
            attack: Some(Attack {
                damage: 8,
                reach: 1.8,
                wind_up: 1.0,
                cooldown: 2.2,
                knockback: 9.0,
                sight: 14.0,
                lunge: 0.0,
                throws: None,
                slays: "was crushed by a barrow brute",
            }),
            despawns: true,
            fades_by_day: false,
            clings: false,
            guards: 1,
        },
    },
}

impl CreatureKind {
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.name() == name)
    }

    /// The kind a spawner at `at` in the world of `seed` brings out: chosen by the kinds'
    /// `guards`, from a hash of the two, so it's always the same one.
    pub fn guarding(seed: u32, at: I64Vec3) -> Self {
        let mut hash = u64::from(seed) ^ 0x9e37_79b9_7f4a_7c15;
        for n in at.to_array() {
            hash = (hash ^ n as u64).wrapping_mul(0xff51_afd7_ed55_8ccd);
            hash ^= hash >> 33;
        }
        let weight = |kind: &Self| u64::from(kind.properties().guards);
        let mut pick = hash % Self::ALL.iter().map(weight).sum::<u64>();
        for kind in Self::ALL {
            if pick < weight(kind) {
                return *kind;
            }
            pick -= weight(kind);
        }
        unreachable!("the pick is under the weights' sum")
    }

    /// Its body, at its feet, as collision takes it, smaller while it's young.
    pub fn body(self, young: bool) -> Aabb {
        let Properties { width, height, .. } = *self.properties();
        let scale = if young { YOUNG_SCALE } else { 1.0 };
        let half = width * scale / 2.0;
        Aabb {
            min: Vec3::new(-half, 0.0, -half),
            max: Vec3::new(half, height * scale, half),
        }
    }
}

// By name, as blocks and items are.
impl Serialize for CreatureKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for CreatureKind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::from_name(&name)
            .ok_or_else(|| serde::de::Error::custom(format!("there's no creature called {name}")))
    }
}

/// A creature, of its kind. Replicated to clients, which draw it.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Creature(pub CreatureKind);

/// Where a creature is, by its feet, and which way it faces, at `at` milliseconds by the
/// server's clock, which clients draw it from as they do other players' poses. The server sets
/// it as it moves, every `POSE_INTERVAL` at most.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CreaturePose {
    pub at: u32,
    pub position: Vec3,
    pub yaw: f32,
}

/// How a creature is beyond its kind, as clients draw it: young, shorn of its fleece, or in
/// love, having been fed. The server keeps it in step with how long each lasts.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Condition {
    pub young: bool,
    pub shorn: bool,
    pub in_love: bool,
}

/// A creature as it's saved, while nobody's near it (§4.3): its kind, where its feet are, its
/// health, and how many seconds it has left to grow up and for its fleece to grow back, 0 for
/// one grown or unshorn.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SavedCreature {
    pub kind: CreatureKind,
    pub position: Vec3,
    pub health: u8,
    pub grows_up_in: f32,
    pub regrows_in: f32,
}

impl SavedCreature {
    /// A grown creature of `kind` with its feet at `position`, at full health and unshorn.
    pub fn grown(kind: CreatureKind, position: Vec3) -> Self {
        Self {
            kind,
            position,
            health: kind.properties().health,
            grows_up_in: 0.0,
            regrows_in: 0.0,
        }
    }
}

/// How a creature stands, as clients draw it: winding up a blow, which it's about to land, and
/// hanging from a ceiling, upside down.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stance {
    pub winding: bool,
    pub clinging: bool,
}

/// On a creature about to be taken away for something other than dying, as a monster fading by
/// day is, so clients don't show it dying (M3.3). The server despawns it the frame after.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vanishing;

/// How fast missiles fall, in blocks a second a second: slower than bodies do, so a throw's arc
/// reads from afar.
pub const MISSILE_GRAVITY: f32 = 20.0;

/// Something thrown, a slinger's stone: where from and how fast. It flies under
/// `MISSILE_GRAVITY` from where it's thrown, so clients draw it from that alone, and the server
/// takes it away when it hits something.
#[derive(Component, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Missile {
    pub from: Vec3,
    pub velocity: Vec3,
}

impl Missile {
    /// Where it is `t` seconds after it's thrown.
    pub fn at(&self, t: f32) -> Vec3 {
        self.from + self.velocity * t + Vec3::NEG_Y * MISSILE_GRAVITY * t * t / 2.0
    }
}

/// How many times a creature's been hurt, which clients flash it red for as it goes up.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flinches(pub u8);

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use bevy::math::I64Vec3;

    use super::{Behaviour, CreatureKind, Joint, Look, Part, YOUNG_SCALE};
    use crate::items::Item;

    #[test]
    fn test_kinds_have_unique_names_that_round_trip() {
        let mut names = HashSet::new();
        for &kind in CreatureKind::ALL {
            assert!(names.insert(kind.name()));
            assert_eq!(Some(kind), CreatureKind::from_name(kind.name()));
            let sent = postcard::to_stdvec(&kind).unwrap();
            assert_eq!(kind, postcard::from_bytes(&sent).unwrap());
        }
        assert_eq!(None, CreatureKind::from_name("dragon"));
    }

    #[test]
    fn test_spawners_guard_by_weight_the_same_kind_every_time() {
        let mut seen = HashSet::new();
        for x in 0..200 {
            let at = I64Vec3::new(x * 37, 60 + x % 7, -x * 11);
            let kind = CreatureKind::guarding(3, at);
            assert_eq!(kind, CreatureKind::guarding(3, at));
            assert!(kind.properties().guards > 0, "{kind:?}");
            seen.insert(kind);
        }
        let guarding = CreatureKind::ALL
            .iter()
            .filter(|k| k.properties().guards > 0);
        assert_eq!(guarding.count(), seen.len());
    }

    #[test]
    fn test_every_kind_is_whole() {
        for &kind in CreatureKind::ALL {
            let properties = kind.properties();
            assert!(properties.health > 0 && properties.run_speed >= properties.walk_speed);
            assert!(!properties.model.is_empty());
            // a kind follows and breeds by its food, so one that does has some
            let tended = [Behaviour::FollowLure, Behaviour::Breed];
            let tends = tended.iter().any(|b| properties.behaviours.contains(b));
            assert_eq!(tends, !properties.food.is_empty(), "{kind:?}");
            // monsters fight, despawn and never breed, and animals none of those
            assert_eq!(properties.attack.is_some(), properties.despawns, "{kind:?}");
            let hunts = properties.behaviours.contains(&Behaviour::Hunt);
            assert_eq!(properties.attack.is_some(), hunts, "{kind:?}");
            assert!(!(tends && hunts), "{kind:?}");
            let body = kind.body(false);
            assert!((body.max.x - body.min.x - properties.width).abs() < 1e-6);
            let young = kind.body(true);
            assert!((young.max.y - properties.height * YOUNG_SCALE).abs() < 1e-6);
            let drops = properties.drops.iter().chain(&properties.fleece);
            for (item, counts) in drops {
                assert_eq!(Some(*item), Item::from_name(item.name()));
                assert!(*counts.end() >= 1 && counts.end() <= &item.max_stack());
            }
            // its model stands on the ground, not far over its body's top
            for part in properties.model {
                assert!(part.centre.y - part.size.y / 2.0 >= -1e-6, "{kind:?}");
                assert!(part.centre.y + part.size.y / 2.0 <= properties.height + 0.3);
            }
            // it lives in biomes there are
            let biomes = crate::chunks::generate::biome::Biomes::new(3);
            for name in properties.spawns.iter().flat_map(|spawns| spawns.biomes) {
                assert!(
                    *name == "base" || biomes.names().any(|b| b == *name),
                    "{name}"
                );
            }
            // only a kind with a fleece has parts of one, and every kind has a head with eyes
            let has = |look| properties.model.iter().any(|part| part.look == look);
            assert_eq!(properties.fleece.is_some(), has(Look::Fleece), "{kind:?}");
            assert!(has(Look::Eye) || has(Look::Glow), "{kind:?}");
            let eye = |part: &&Part| matches!(part.look, Look::Eye | Look::Glow);
            for part in properties.model.iter().filter(eye) {
                assert_eq!(Joint::Head, part.joint);
            }
        }
    }
}
