//! Creatures (§5.8, INV-6): the animals and, later, monsters living in the world. Every kind is a
//! row of the `creatures!` table, which says how big it is, how it moves, what it looks like,
//! what it drops, where it spawns and how it behaves. The server simulates them
//! (`server::creatures`) and clients draw them where the server says they are
//! (`client::creatures`).

pub mod path;

use std::ops::RangeInclusive;

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
}

/// Where a kind appears: in which biomes, by name, the base terrain's being `base`, on which
/// blocks, out under the sky or not, by day or by night, in groups of how many, and how many
/// there can be round a player before no more come.
#[derive(Debug, Clone, PartialEq)]
pub struct Spawns {
    pub biomes: &'static [&'static str],
    pub on: &'static [BlockType],
    pub under_sky: bool,
    pub by: Daytime,
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

// What a farm animal does: runs when it's hit, breeds once it's fed, follows its food, and
// otherwise wanders.
const ANIMAL: &[Behaviour] = &[
    Behaviour::Flee,
    Behaviour::Breed,
    Behaviour::FollowLure,
    Behaviour::Wander,
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
                under_sky: true,
                by: Daytime::Day,
                group: 2..=4,
                cap: 8,
            }),
            behaviours: ANIMAL,
            avoids_water: true,
            flutters: false,
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
                under_sky: true,
                by: Daytime::Day,
                group: 2..=4,
                cap: 6,
            }),
            behaviours: ANIMAL,
            avoids_water: true,
            flutters: false,
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
                under_sky: true,
                by: Daytime::Day,
                group: 1..=3,
                cap: 6,
            }),
            behaviours: ANIMAL,
            avoids_water: true,
            flutters: false,
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
                under_sky: true,
                by: Daytime::Day,
                group: 2..=4,
                cap: 6,
            }),
            behaviours: ANIMAL,
            avoids_water: true,
            flutters: true,
        },
    },
}

impl CreatureKind {
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.name() == name)
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

/// How many times a creature's been hurt, which clients flash it red for as it goes up.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flinches(pub u8);

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{CreatureKind, Joint, Look, YOUNG_SCALE};
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
    fn test_every_kind_is_whole() {
        for &kind in CreatureKind::ALL {
            let properties = kind.properties();
            assert!(properties.health > 0 && properties.run_speed >= properties.walk_speed);
            assert!(!properties.model.is_empty() && !properties.food.is_empty());
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
            assert!(has(Look::Eye), "{kind:?}");
            for part in properties
                .model
                .iter()
                .filter(|part| part.look == Look::Eye)
            {
                assert_eq!(Joint::Head, part.joint);
            }
        }
    }
}
