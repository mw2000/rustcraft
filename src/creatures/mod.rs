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

/// A kind's row in the table.
pub struct Properties {
    /// How wide and deep its body is, and how tall, in blocks.
    pub width: f32,
    pub height: f32,
    /// Full health, in the same half hearts as a player's.
    pub health: u8,
    /// How fast it walks about, and how fast it runs when it's frightened, in blocks a second.
    pub walk_speed: f32,
    pub run_speed: f32,
    /// What it looks like, facing -z.
    pub model: &'static [Part],
    /// What it drops when it dies: each item, and how many of it, chosen evenly.
    pub drops: &'static [(Item, RangeInclusive<u8>)],
    /// Where it appears, if it appears by itself.
    pub spawns: Option<Spawns>,
    /// What it does, the first in the list that has something to do winning.
    pub behaviours: &'static [Behaviour],
    /// Whether it keeps out of water when it finds its way, as it would rather not swim.
    pub avoids_water: bool,
}

/// A box of a creature's model: its size and the middle of it, in blocks from the creature's
/// feet as it faces -z, and its colour, in sRGB. A leg swings from its top as the creature
/// walks, `1.0` or `-1.0` for which way it swings first, and anything else doesn't (`0.0`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Part {
    pub size: Vec3,
    pub centre: Vec3,
    pub colour: [f32; 3],
    pub swing: f32,
}

/// Where a kind appears: on which blocks, out under the sky or not, by day or by night, in
/// groups of how many, and how many there can be round a player before no more come.
#[derive(Debug, Clone, PartialEq)]
pub struct Spawns {
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
    /// Follows a player near it holding this, as a sheep follows wheat.
    FollowLure(Item),
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

const WOOL: [f32; 3] = [0.91, 0.9, 0.86];
const SHEEP_FACE: [f32; 3] = [0.78, 0.66, 0.56];
const SHEEP_LEGS: [f32; 3] = [0.72, 0.62, 0.54];
const SHEEP_EYES: [f32; 3] = [0.08, 0.07, 0.07];

// A leg of a four-legged animal, `size` big, under the hip at `x`, `z`.
const fn leg(size: Vec3, x: f32, z: f32, swing: f32) -> Part {
    Part {
        size,
        centre: Vec3::new(x, size.y / 2.0, z),
        colour: SHEEP_LEGS,
        swing,
    }
}

const SHEEP_LEG: Vec3 = Vec3::new(0.22, 0.6, 0.22);

// A body of wool on four legs, the front pair swinging opposite the back and each against the
// one beside it, a face out in front with a cap of wool, and two eyes.
const SHEEP: &[Part] = &[
    leg(SHEEP_LEG, -0.24, -0.38, 1.0),
    leg(SHEEP_LEG, 0.24, -0.38, -1.0),
    leg(SHEEP_LEG, -0.24, 0.38, -1.0),
    leg(SHEEP_LEG, 0.24, 0.38, 1.0),
    Part {
        size: Vec3::new(0.86, 0.66, 1.2),
        centre: Vec3::new(0.0, 0.92, 0.04),
        colour: WOOL,
        swing: 0.0,
    },
    Part {
        size: Vec3::new(0.46, 0.46, 0.5),
        centre: Vec3::new(0.0, 1.08, -0.74),
        colour: SHEEP_FACE,
        swing: 0.0,
    },
    Part {
        size: Vec3::new(0.52, 0.16, 0.36),
        centre: Vec3::new(0.0, 1.32, -0.68),
        colour: WOOL,
        swing: 0.0,
    },
    Part {
        size: Vec3::new(0.08, 0.08, 0.02),
        centre: Vec3::new(-0.13, 1.14, -1.0),
        colour: SHEEP_EYES,
        swing: 0.0,
    },
    Part {
        size: Vec3::new(0.08, 0.08, 0.02),
        centre: Vec3::new(0.13, 1.14, -1.0),
        colour: SHEEP_EYES,
        swing: 0.0,
    },
];

// Sheep graze the grass in small flocks by day, run when they're hit, follow wheat, and give
// mutton (§5.5a), which cooks in a furnace. Wool waits for M3.2.
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
            drops: &[(Item::Mutton, 1..=2)],
            spawns: Some(Spawns {
                on: &[BlockType::Grass],
                under_sky: true,
                by: Daytime::Day,
                group: 2..=4,
                cap: 8,
            }),
            behaviours: &[
                Behaviour::Flee,
                Behaviour::FollowLure(Item::Wheat),
                Behaviour::Wander,
            ],
            avoids_water: true,
        },
    },
}

impl CreatureKind {
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.name() == name)
    }

    /// Its body, at its feet, as collision takes it.
    pub fn body(self) -> Aabb {
        let Properties { width, height, .. } = *self.properties();
        let half = width / 2.0;
        Aabb {
            min: Vec3::new(-half, 0.0, -half),
            max: Vec3::new(half, height, half),
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

/// A creature as it's saved, while nobody's near it (§4.3): its kind, where its feet are and its
/// health.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SavedCreature {
    pub kind: CreatureKind,
    pub position: Vec3,
    pub health: u8,
}

/// How many times a creature's been hurt, which clients flash it red for as it goes up.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flinches(pub u8);

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::CreatureKind;
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
            assert!(!properties.model.is_empty(), "{kind:?}");
            let body = kind.body();
            assert!((body.max.x - body.min.x - properties.width).abs() < 1e-6);
            for (item, counts) in properties.drops {
                assert_eq!(Some(*item), Item::from_name(item.name()));
                assert!(*counts.start() >= 1 && counts.end() <= &item.max_stack());
            }
        }
    }
}
