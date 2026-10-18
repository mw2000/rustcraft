use crate::block::{BlockType, RenderStyle};
use crate::items::ToolKind;

/// What a block sounds like underfoot, struck and broken: each has its own footsteps and
/// breaking sounds in `assets/sounds/`, made by the sounds skill's `synth.py`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Surface {
    Grass,
    Dirt,
    Sand,
    Gravel,
    Stone,
    Wood,
    Snow,
    Leaves,
    Ice,
}

impl Surface {
    /// The surface `block` is, or none for what has no sound of its own, air and water. Blocks
    /// not named go by the tool that breaks them, so a new block sounds right from the start: a
    /// pickaxe's stone, an axe's wood, a shovel's dirt, and plants rustle like leaves.
    pub fn of(block: BlockType) -> Option<Self> {
        use BlockType::*;
        Some(match block {
            Air | Water | StructureVoid | LootMarker => return None,
            Grass => Self::Grass,
            Sand => Self::Sand,
            Gravel => Self::Gravel,
            Snow => Self::Snow,
            Leaves => Self::Leaves,
            Ice | Glass | GlassPane => Self::Ice,
            Vines => Self::Leaves,
            _ if block.style() == RenderStyle::Cross => Self::Leaves,
            _ => match block.tool() {
                Some(ToolKind::Pickaxe) => Self::Stone,
                Some(ToolKind::Shovel | ToolKind::Hoe) => Self::Dirt,
                Some(ToolKind::Axe) | None => Self::Wood,
            },
        })
    }

    /// Its name in the sounds' file names.
    pub fn name(self) -> &'static str {
        match self {
            Self::Grass => "grass",
            Self::Dirt => "dirt",
            Self::Sand => "sand",
            Self::Gravel => "gravel",
            Self::Stone => "stone",
            Self::Wood => "wood",
            Self::Snow => "snow",
            Self::Leaves => "leaves",
            Self::Ice => "ice",
        }
    }

    pub const ALL: [Self; 9] = [
        Self::Grass,
        Self::Dirt,
        Self::Sand,
        Self::Gravel,
        Self::Stone,
        Self::Wood,
        Self::Snow,
        Self::Leaves,
        Self::Ice,
    ];
}

#[cfg(test)]
mod tests {
    use super::Surface;
    use crate::block::BlockType::*;

    #[test]
    fn test_blocks_sound_like_what_theyre_made_of() {
        let cases = [
            (Grass, Some(Surface::Grass)),
            (Dirt, Some(Surface::Dirt)),
            (Farmland, Some(Surface::Dirt)),
            (Sand, Some(Surface::Sand)),
            (Gravel, Some(Surface::Gravel)),
            (Stone, Some(Surface::Stone)),
            (Cobblestone, Some(Surface::Stone)),
            (IronOre, Some(Surface::Stone)),
            (Furnace, Some(Surface::Stone)),
            (Sandstone, Some(Surface::Stone)),
            (Log, Some(Surface::Wood)),
            (Planks, Some(Surface::Wood)),
            (Chest, Some(Surface::Wood)),
            (Torch, Some(Surface::Wood)),
            (Snow, Some(Surface::Snow)),
            (Leaves, Some(Surface::Leaves)),
            (TallGrass, Some(Surface::Leaves)),
            (Poppy, Some(Surface::Leaves)),
            (RipeWheat, Some(Surface::Leaves)),
            (Ice, Some(Surface::Ice)),
            (GlassPane, Some(Surface::Ice)),
            (Vines, Some(Surface::Leaves)),
            (StoneBrickStairs, Some(Surface::Stone)),
            (Door, Some(Surface::Wood)),
            (Air, None),
            (Water, None),
        ];
        for (block, surface) in cases {
            assert_eq!(surface, Surface::of(block), "{}", block.name());
        }
    }
}
