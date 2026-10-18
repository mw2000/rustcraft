//! Authoring structures in creative (§5.6): an operator selects a box by its corners with `/pos1`
//! and `/pos2`, and the server saves it as a template or places one (`server::structures`). This
//! is the selection both sides know; `client::authoring` draws it.

use bevy::math::I64Vec3;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::block::BlockType;
use crate::chunks::generate::structures::{LOOT, SPAWNER, STRUCTURE_DEPTH, STRUCTURE_HEIGHT};

/// The biggest box a structure can be saved from: as tall as a template can be, sunk layers and
/// all, and four chunks across. That's room for any one building, as bigger structures are
/// assembled from pieces, while placing one stays a burst of at most 127,000 edits, which the
/// edit layer and the clients near it take in their stride.
pub const MAX_SIZE: I64Vec3 = I64Vec3::new(64, STRUCTURE_HEIGHT + STRUCTURE_DEPTH, 64);

/// The blocks standing for a template's markers, and the kind of marker each is. A spawner is
/// its own marker, being what the server builds there.
pub const MARKERS: [(BlockType, &str); 2] =
    [(BlockType::LootMarker, LOOT), (BlockType::Spawner, SPAWNER)];

/// The box a player has selected to save as a structure, by its corners, each set or not yet,
/// and the height of the floor of the structure last placed into it, while it's that box still.
#[derive(Component, Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub corners: [Option<I64Vec3>; 2],
    pub floor: Option<i64>,
}

impl Selection {
    /// Moves corner `corner`, 0 or 1, to `at`, which makes it another box, with no floor.
    pub fn set(&mut self, corner: usize, at: I64Vec3) {
        self.corners[corner] = Some(at);
        self.floor = None;
    }

    /// The box's lowest block and its size, once both corners are set.
    pub fn bounds(&self) -> Option<(I64Vec3, I64Vec3)> {
        let [Some(a), Some(b)] = self.corners else {
            return None;
        };
        Some((a.min(b), (a - b).abs() + 1))
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::I64Vec3;

    use super::Selection;

    #[test]
    fn test_a_selection_is_the_box_between_its_corners_either_way_round() {
        let mut selection = Selection::default();
        assert_eq!(None, selection.bounds());
        selection.set(0, I64Vec3::new(3, 70, -2));
        assert_eq!(None, selection.bounds());
        selection.set(1, I64Vec3::new(-3, 64, 4));
        let bounds = Some((I64Vec3::new(-3, 64, -2), I64Vec3::new(7, 7, 7)));
        assert_eq!(bounds, selection.bounds());
        // one block, as both corners
        selection.set(1, I64Vec3::new(3, 70, -2));
        assert_eq!(
            Some((I64Vec3::new(3, 70, -2), I64Vec3::ONE)),
            selection.bounds()
        );
        // and moving a corner forgets the floor placed into the box
        selection.floor = Some(71);
        selection.set(0, I64Vec3::ZERO);
        assert_eq!(None, selection.floor);
    }
}
