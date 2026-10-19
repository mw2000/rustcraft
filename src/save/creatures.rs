//! The creatures in a region, saved apart from its edits as `creatures/r.<x>.<z>.creatures`
//! (§4.3, §5.8), since they move about and are written far more often. Older games don't read
//! the directory, so the world's format isn't raised.

use std::io;

use bevy::math::Vec3;
use serde::Deserialize;

use crate::creatures::{CreatureKind, SavedCreature};

pub const DIR: &str = "creatures";
pub const EXTENSION: &str = "creatures";

// The file's format, its first byte: 2 since M3.2, when creatures could be young or shorn, and
// 3 since M3.4, when they could be villagers.
const FORMAT: u8 = 3;

// A creature as format 1 saved it, before there were young or shorn ones.
#[derive(Deserialize)]
struct Grown {
    kind: CreatureKind,
    position: Vec3,
    health: u8,
}

// And as format 2 did, before there were villagers.
#[derive(Deserialize)]
struct Unsettled {
    kind: CreatureKind,
    position: Vec3,
    health: u8,
    grows_up_in: f32,
    regrows_in: f32,
}

/// A region's creatures in postcard (D-6), each by its kind's name, where it is, its health, how
/// long it has to grow up and to grow its fleece back, and a villager's home, work and trades.
pub fn encode(creatures: &[SavedCreature]) -> Vec<u8> {
    postcard::to_extend(creatures, vec![FORMAT]).expect("creatures always encode")
}

/// The creatures a file holds, those of format 1 grown and unshorn. A kind this game doesn't
/// know, or a newer format, is an error.
pub fn decode(bytes: &[u8]) -> io::Result<Vec<SavedCreature>> {
    let invalid = |e: postcard::Error| io::Error::new(io::ErrorKind::InvalidData, e.to_string());
    match bytes {
        [FORMAT, file @ ..] => postcard::from_bytes(file).map_err(invalid),
        [2, file @ ..] => {
            let old: Vec<Unsettled> = postcard::from_bytes(file).map_err(invalid)?;
            let old = old.into_iter().map(|old| SavedCreature {
                health: old.health,
                grows_up_in: old.grows_up_in,
                regrows_in: old.regrows_in,
                ..SavedCreature::grown(old.kind, old.position)
            });
            Ok(old.collect())
        }
        [1, file @ ..] => {
            let grown: Vec<Grown> = postcard::from_bytes(file).map_err(invalid)?;
            let grown = grown.into_iter().map(|old| SavedCreature {
                health: old.health,
                ..SavedCreature::grown(old.kind, old.position)
            });
            Ok(grown.collect())
        }
        [format, ..] => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("creatures format {format}, perhaps saved by a newer version"),
        )),
        [] => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "an empty creatures file",
        )),
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec3;

    use bevy::math::{I64Vec2, I64Vec3};

    use super::{decode, encode};
    use crate::creatures::{CreatureKind, SavedCreature, Settler, Villager};
    use crate::trading::Stock;

    #[test]
    fn test_creatures_round_trip_by_name() {
        let sheep = SavedCreature {
            health: 5,
            grows_up_in: 12.5,
            regrows_in: 30.0,
            ..SavedCreature::grown(CreatureKind::Sheep, Vec3::new(-3.5, 470.0, 1e6))
        };
        let bytes = encode(&[sheep, sheep]);
        assert_eq!(vec![sheep, sheep], decode(&bytes).unwrap());
        assert!(bytes.windows(5).any(|w| w == b"sheep"));
        assert!(decode(&[9, 0]).is_err());
        let mut unknown = bytes.clone();
        let at = unknown.windows(5).position(|w| w == b"sheep").unwrap();
        unknown[at..at + 5].copy_from_slice(b"shark");
        assert!(decode(&unknown).is_err());
    }

    #[test]
    fn test_villagers_keep_their_homes_work_and_trades() {
        let smith = SavedCreature {
            settler: Some(Settler {
                villager: Villager { style: 2, seed: 99 },
                village: I64Vec2::new(-3, 4),
                home: I64Vec3::new(1, 470, -2),
                work: I64Vec3::new(10, 471, 5),
                well: I64Vec3::new(4, 470, 0),
                stock: Stock {
                    day: 12,
                    chosen: [4, 0, 2],
                    sold: [1, 0, 3],
                },
            }),
            ..SavedCreature::grown(CreatureKind::Smith, Vec3::new(1.5, 470.0, -1.5))
        };
        assert_eq!(vec![smith], decode(&encode(&[smith])).unwrap());
    }

    #[test]
    fn test_creatures_saved_before_m3_4_are_read_unsettled() {
        // format 2: a list of one lamb, by its kind, position, health and times
        let mut old = vec![2, 1, 5];
        old.extend(b"sheep");
        for x in [1.5f32, 2.0, 3.5] {
            old.extend(x.to_le_bytes());
        }
        old.push(7);
        for t in [10.0f32, 0.0] {
            old.extend(t.to_le_bytes());
        }
        let lamb = SavedCreature {
            health: 7,
            grows_up_in: 10.0,
            ..SavedCreature::grown(CreatureKind::Sheep, Vec3::new(1.5, 2.0, 3.5))
        };
        assert_eq!(vec![lamb], decode(&old).unwrap());
    }

    #[test]
    fn test_creatures_saved_before_m3_2_are_read_grown_and_unshorn() {
        // format 1: a list of one sheep, by its kind, position and health
        let mut old = vec![1, 1, 5];
        old.extend(b"sheep");
        for x in [1.5f32, 2.0, 3.5] {
            old.extend(x.to_le_bytes());
        }
        old.push(7);
        let sheep = SavedCreature {
            health: 7,
            ..SavedCreature::grown(CreatureKind::Sheep, Vec3::new(1.5, 2.0, 3.5))
        };
        assert_eq!(vec![sheep], decode(&old).unwrap());
    }
}
