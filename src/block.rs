use crate::chunks::generate::light::MAX_LIGHT;

/// A block's row in the table: how it behaves and looks.
struct Properties {
    /// Its stable name, which saves and the network protocol store instead of its discriminant.
    name: &'static str,
    /// Whether the player is kept out of it.
    solid: bool,
    /// How many levels of sky light it takes from light passing through it, see `light.rs`.
    light_opacity: u8,
    /// How much it darkens the ambient occlusion of the faces beside it.
    occlusion_weight: f32,
    // The tiles its top, side and bottom faces draw.
    top: Tile,
    side: Tile,
    bottom: Tile,
    /// The one tile coarse LODs draw all of its faces with.
    lod: Tile,
}

// Declares `BlockType` and its table of `Properties` together, so each block's properties sit
// beside its variant and the table can't fall out of step with the enum.
macro_rules! blocks {
    ($($(#[$attr:meta])* $variant:ident $properties:tt,)*) => {
        #[derive(Debug, Default, PartialEq, Eq, Copy, Clone)]
        #[repr(u8)]
        pub enum BlockType {
            $($(#[$attr])* $variant,)*
        }

        impl BlockType {
            /// Every block, in declaration order.
            pub const ALL: [BlockType; [$(stringify!($variant)),*].len()] =
                [$(BlockType::$variant),*];
        }

        const PROPERTIES: [Properties; BlockType::ALL.len()] = [$(Properties $properties),*];
    };
}

blocks! {
    #[default]
    Air {
        name: "air", solid: false, light_opacity: 0, occlusion_weight: 0.0,
        // Never drawn.
        top: Tile::Stone, side: Tile::Stone, bottom: Tile::Stone, lod: Tile::Stone,
    },
    Stone {
        name: "stone", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        top: Tile::Stone, side: Tile::Stone, bottom: Tile::Stone, lod: Tile::Stone,
    },
    Grass {
        name: "grass", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        // Far off, grass stays green on hillsides.
        top: Tile::GrassTop, side: Tile::GrassSide, bottom: Tile::Dirt, lod: Tile::GrassTop,
    },
    Sand {
        name: "sand", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        top: Tile::Sand, side: Tile::Sand, bottom: Tile::Sand, lod: Tile::Sand,
    },
    Water {
        name: "water", solid: false, light_opacity: 2, occlusion_weight: 0.4,
        top: Tile::Water, side: Tile::Water, bottom: Tile::Water, lod: Tile::Water,
    },
    Snow {
        name: "snow", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        top: Tile::Snow, side: Tile::SnowSide, bottom: Tile::Snow, lod: Tile::SnowSide,
    },
    Log {
        name: "log", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        top: Tile::LogEnd, side: Tile::LogBark, bottom: Tile::LogEnd, lod: Tile::LogBark,
    },
    Leaves {
        name: "leaves", solid: true, light_opacity: 1, occlusion_weight: 0.3,
        top: Tile::Leaves, side: Tile::Leaves, bottom: Tile::Leaves, lod: Tile::Leaves,
    },
    Dirt {
        name: "dirt", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        top: Tile::Dirt, side: Tile::Dirt, bottom: Tile::Dirt, lod: Tile::Dirt,
    },
    Gravel {
        name: "gravel", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        top: Tile::Gravel, side: Tile::Gravel, bottom: Tile::Gravel, lod: Tile::Gravel,
    },
    Ice {
        name: "ice", solid: true, light_opacity: 3, occlusion_weight: 1.0,
        top: Tile::Ice, side: Tile::Ice, bottom: Tile::Ice, lod: Tile::Ice,
    },
    Sandstone {
        name: "sandstone", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        top: Tile::SandstoneTop, side: Tile::SandstoneSide, bottom: Tile::SandstoneTop,
        lod: Tile::SandstoneSide,
    },
}

impl BlockType {
    fn properties(self) -> &'static Properties {
        &PROPERTIES[self as usize]
    }

    pub fn name(&self) -> &'static str {
        self.properties().name
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|block| block.name() == name)
    }

    pub fn occlusion_weight(&self) -> f32 {
        self.properties().occlusion_weight
    }

    pub fn light_opacity(&self) -> u8 {
        self.properties().light_opacity
    }

    pub fn is_solid(&self) -> bool {
        self.properties().solid
    }

    // Whether the faces of blocks behind this one are hidden by it. Leaves have holes.
    pub fn is_opaque(&self) -> bool {
        self.light_opacity() == MAX_LIGHT
    }

    // The tile a face of this block draws. A flora class would slot in here, one tile for every
    // face, with the mesher branching on its shape before it ever asks for a face.
    pub fn tile(&self, face: Face) -> Tile {
        let properties = self.properties();
        match face {
            Face::Top => properties.top,
            Face::Side => properties.side,
            Face::Bottom => properties.bottom,
        }
    }

    pub fn lod_tile(&self) -> Tile {
        self.properties().lod
    }
}

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum Face {
    Top,
    Side,
    Bottom,
}

impl Face {
    pub fn of_normal_y(y: f32) -> Self {
        if y > 0.5 {
            Face::Top
        } else if y < -0.5 {
            Face::Bottom
        } else {
            Face::Side
        }
    }
}

// The textures in blocks.png, left to right, in `generate.py`'s `TILES` order.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[repr(u8)]
pub enum Tile {
    Stone,
    GrassTop,
    Sand,
    Water,
    Snow,
    LogBark,
    Leaves,
    GrassSide,
    Dirt,
    LogEnd,
    Gravel,
    Ice,
    SandstoneSide,
    SandstoneTop,
    SnowSide,
}

pub const TILE_COUNT: usize = 15;

impl Tile {
    // What meshes carry in vertex alpha for `block_texture.wesl`.
    pub fn atlas_cell(self) -> f32 {
        self as u8 as f32
    }

    // Greyscale tiles that vertex colours tint to the climate's grass.
    pub fn tinted(self) -> bool {
        matches!(self, Tile::GrassTop | Tile::Leaves)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{BlockType, Face, Tile, TILE_COUNT};

    #[test]
    fn test_block_names_are_unique_snake_case_and_round_trip() {
        let mut names = HashSet::new();
        for block in BlockType::ALL {
            let name = block.name();
            assert!(names.insert(name), "{name} is used twice");
            assert!(name.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
            assert_eq!(Some(block), BlockType::from_name(name));
        }
        assert_eq!(None, BlockType::from_name("Stone"));
    }

    #[test]
    fn test_tile_count_matches_the_last_tile() {
        assert_eq!(TILE_COUNT, Tile::SnowSide as usize + 1);
    }

    #[test]
    fn test_blocks_pick_their_tile_by_face() {
        assert_eq!(Tile::GrassTop, BlockType::Grass.tile(Face::Top));
        assert_eq!(Tile::GrassSide, BlockType::Grass.tile(Face::Side));
        assert_eq!(Tile::Dirt, BlockType::Grass.tile(Face::Bottom));
        assert_eq!(Tile::LogEnd, BlockType::Log.tile(Face::Bottom));
        assert_eq!(Tile::LogBark, BlockType::Log.tile(Face::Side));
        assert_eq!(Tile::GrassTop, BlockType::Grass.lod_tile());
        assert_eq!(Tile::LogBark, BlockType::Log.lod_tile());
    }

    #[test]
    fn test_ice_lets_light_through_but_is_solid() {
        assert!(!BlockType::Ice.is_opaque());
        assert!(BlockType::Ice.light_opacity() > BlockType::Water.light_opacity());
        assert_eq!(1.0, BlockType::Ice.occlusion_weight());
    }

    #[test]
    fn test_occlusion_weights() {
        assert_eq!(0.0, BlockType::Air.occlusion_weight());
        assert_eq!(1.0, BlockType::Stone.occlusion_weight());
        let w = BlockType::Water.occlusion_weight();
        assert!(w > 0.0 && w < 1.0);
    }

    #[test]
    fn test_only_solid_blocks_are_opaque() {
        assert!(BlockType::Stone.is_opaque());
        assert!(BlockType::Log.is_opaque());
        assert!(!BlockType::Leaves.is_opaque());
        assert!(!BlockType::Water.is_opaque());
        assert!(!BlockType::Air.is_opaque());
    }
}
