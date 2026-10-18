//! The creatures in a region, saved apart from its edits as `creatures/r.<x>.<z>.creatures`
//! (§4.3, §5.8), since they move about and are written far more often. Older games don't read
//! the directory, so the world's format isn't raised.

use std::io;

use bevy::math::Vec3;
use serde::Deserialize;

use crate::creatures::{CreatureKind, SavedCreature};

pub const DIR: &str = "creatures";
pub const EXTENSION: &str = "creatures";

// The file's format, its first byte: 2 since M3.2, when creatures could be young or shorn.
const FORMAT: u8 = 2;

// A creature as format 1 saved it, before there were young or shorn ones.
#[derive(Deserialize)]
struct Grown {
    kind: CreatureKind,
    position: Vec3,
    health: u8,
}

/// A region's creatures in postcard (D-6), each by its kind's name, where it is, its health, and
/// how long it has to grow up and to grow its fleece back.
pub fn encode(creatures: &[SavedCreature]) -> Vec<u8> {
    postcard::to_extend(creatures, vec![FORMAT]).expect("creatures always encode")
}

/// The creatures a file holds, those of format 1 grown and unshorn. A kind this game doesn't
/// know, or a newer format, is an error.
pub fn decode(bytes: &[u8]) -> io::Result<Vec<SavedCreature>> {
    let invalid = |e: postcard::Error| io::Error::new(io::ErrorKind::InvalidData, e.to_string());
    match bytes {
        [FORMAT, file @ ..] => postcard::from_bytes(file).map_err(invalid),
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

    use super::{decode, encode};
    use crate::creatures::{CreatureKind, SavedCreature};

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
