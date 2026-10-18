//! The creatures in a region, saved apart from its edits as `creatures/r.<x>.<z>.creatures`
//! (§4.3, §5.8), since they move about and are written far more often. Older games don't read
//! the directory, so the world's format isn't raised.

use std::io;

use crate::creatures::SavedCreature;

pub const DIR: &str = "creatures";
pub const EXTENSION: &str = "creatures";

// The file's format, its first byte.
const FORMAT: u8 = 1;

/// A region's creatures in postcard (D-6), each by its kind's name, where it is and its health.
pub fn encode(creatures: &[SavedCreature]) -> Vec<u8> {
    postcard::to_extend(creatures, vec![FORMAT]).expect("creatures always encode")
}

/// The creatures a file holds. A kind this game doesn't know, or a newer format, is an error.
pub fn decode(bytes: &[u8]) -> io::Result<Vec<SavedCreature>> {
    let invalid = |message: String| io::Error::new(io::ErrorKind::InvalidData, message);
    match bytes {
        [FORMAT, file @ ..] => postcard::from_bytes(file).map_err(|e| invalid(e.to_string())),
        [format, ..] => Err(invalid(format!(
            "creatures format {format}, perhaps saved by a newer version"
        ))),
        [] => Err(invalid("an empty creatures file".into())),
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
            kind: CreatureKind::Sheep,
            position: Vec3::new(-3.5, 470.0, 1e6),
            health: 5,
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
}
