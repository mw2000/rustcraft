use crate::chunks::generate::light::MAX_LIGHT;
use crate::items::Item;

/// A block's row in the table: how it behaves and looks.
struct Properties {
    /// Its stable name, which saves and the network protocol store instead of its discriminant.
    name: &'static str,
    /// Whether the player is kept out of it.
    solid: bool,
    /// Whether a block placed where it is takes its place.
    replaceable: bool,
    /// How it's drawn.
    style: RenderStyle,
    /// What it leaves when it's broken in survival.
    drops: Drops,
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

/// How a block is drawn.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum RenderStyle {
    /// Not at all.
    None,
    /// As a cube's faces, those not hidden by an opaque neighbour, merged where they match.
    Cube,
    /// As water's surface, in a mesh of its own.
    Liquid,
    /// As two quads crossing diagonally through the cell, seen from either side, at full detail
    /// only. Plants are drawn this way.
    Cross,
}

/// What a block leaves behind when it's broken in survival, to be picked up.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum Drops {
    Nothing,
    /// Its own block item.
    Itself,
    /// Another block's item, as stone leaves cobblestone.
    Block(BlockType),
}

// Declares `BlockType` and its table of `Properties` together, so each block's properties sit
// beside its variant and the table can't fall out of step with the enum.
macro_rules! blocks {
    ($($(#[$attr:meta])* $variant:ident $properties:tt,)*) => {
        #[derive(Debug, Default, PartialEq, Eq, Copy, Clone, Hash)]
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
        style: RenderStyle::None, replaceable: true, drops: Drops::Nothing,
        // Never drawn.
        top: Tile::Stone, side: Tile::Stone, bottom: Tile::Stone, lod: Tile::Stone,
    },
    Stone {
        name: "stone", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Block(BlockType::Cobblestone),
        top: Tile::Stone, side: Tile::Stone, bottom: Tile::Stone, lod: Tile::Stone,
    },
    Grass {
        name: "grass", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Block(BlockType::Dirt),
        // Far off, grass stays green on hillsides.
        top: Tile::GrassTop, side: Tile::GrassSide, bottom: Tile::Dirt, lod: Tile::GrassTop,
    },
    Sand {
        name: "sand", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        top: Tile::Sand, side: Tile::Sand, bottom: Tile::Sand, lod: Tile::Sand,
    },
    Water {
        name: "water", solid: false, light_opacity: 2, occlusion_weight: 0.4,
        style: RenderStyle::Liquid, replaceable: true, drops: Drops::Nothing,
        top: Tile::Water, side: Tile::Water, bottom: Tile::Water, lod: Tile::Water,
    },
    Snow {
        name: "snow", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        top: Tile::Snow, side: Tile::SnowSide, bottom: Tile::Snow, lod: Tile::SnowSide,
    },
    Log {
        name: "log", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        top: Tile::LogEnd, side: Tile::LogBark, bottom: Tile::LogEnd, lod: Tile::LogBark,
    },
    Leaves {
        name: "leaves", solid: true, light_opacity: 1, occlusion_weight: 0.3,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Nothing,
        // Saplings and apples come with farming (M1.9b).
        top: Tile::Leaves, side: Tile::Leaves, bottom: Tile::Leaves, lod: Tile::Leaves,
    },
    Dirt {
        name: "dirt", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        top: Tile::Dirt, side: Tile::Dirt, bottom: Tile::Dirt, lod: Tile::Dirt,
    },
    Gravel {
        name: "gravel", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        top: Tile::Gravel, side: Tile::Gravel, bottom: Tile::Gravel, lod: Tile::Gravel,
    },
    Ice {
        name: "ice", solid: true, light_opacity: 3, occlusion_weight: 1.0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        top: Tile::Ice, side: Tile::Ice, bottom: Tile::Ice, lod: Tile::Ice,
    },
    Sandstone {
        name: "sandstone", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        top: Tile::SandstoneTop, side: Tile::SandstoneSide, bottom: Tile::SandstoneTop,
        lod: Tile::SandstoneSide,
    },
    Planks {
        name: "planks", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        top: Tile::Planks, side: Tile::Planks, bottom: Tile::Planks, lod: Tile::Planks,
    },
    Cobblestone {
        name: "cobblestone", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        top: Tile::Cobblestone, side: Tile::Cobblestone, bottom: Tile::Cobblestone,
        lod: Tile::Cobblestone,
    },
    StoneBricks {
        name: "stone_bricks", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        top: Tile::StoneBricks, side: Tile::StoneBricks, bottom: Tile::StoneBricks,
        lod: Tile::StoneBricks,
    },
    Bricks {
        name: "bricks", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        top: Tile::Bricks, side: Tile::Bricks, bottom: Tile::Bricks, lod: Tile::Bricks,
    },
    CoalOre {
        name: "coal_ore", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        top: Tile::CoalOre, side: Tile::CoalOre, bottom: Tile::CoalOre, lod: Tile::CoalOre,
    },
    CopperOre {
        name: "copper_ore", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        top: Tile::CopperOre, side: Tile::CopperOre, bottom: Tile::CopperOre, lod: Tile::CopperOre,
    },
    IronOre {
        name: "iron_ore", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        top: Tile::IronOre, side: Tile::IronOre, bottom: Tile::IronOre, lod: Tile::IronOre,
    },
    CraftingTable {
        name: "crafting_table", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        top: Tile::CraftingTableTop, side: Tile::CraftingTableSide, bottom: Tile::Planks,
        lod: Tile::CraftingTableSide,
    },
    Furnace {
        name: "furnace", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        // Blocks have no facing yet, so every side has the furnace's mouth.
        top: Tile::Stone, side: Tile::FurnaceSide, bottom: Tile::Stone, lod: Tile::FurnaceSide,
    },
    // Plants, standing on the block below and gone with it. They're broken at a touch, let light
    // and the player through and are built over, so their faces hide nothing. They drop nothing
    // until farming, which has tall grass drop seeds.
    TallGrass {
        name: "tall_grass", solid: false, light_opacity: 0, occlusion_weight: 0.0,
        style: RenderStyle::Cross, replaceable: true, drops: Drops::Nothing,
        top: Tile::TallGrass, side: Tile::TallGrass, bottom: Tile::TallGrass, lod: Tile::TallGrass,
    },
    Poppy {
        name: "poppy", solid: false, light_opacity: 0, occlusion_weight: 0.0,
        style: RenderStyle::Cross, replaceable: true, drops: Drops::Nothing,
        top: Tile::Poppy, side: Tile::Poppy, bottom: Tile::Poppy, lod: Tile::Poppy,
    },
    Dandelion {
        name: "dandelion", solid: false, light_opacity: 0, occlusion_weight: 0.0,
        style: RenderStyle::Cross, replaceable: true, drops: Drops::Nothing,
        top: Tile::Dandelion, side: Tile::Dandelion, bottom: Tile::Dandelion, lod: Tile::Dandelion,
    },
    Cornflower {
        name: "cornflower", solid: false, light_opacity: 0, occlusion_weight: 0.0,
        style: RenderStyle::Cross, replaceable: true, drops: Drops::Nothing,
        top: Tile::Cornflower, side: Tile::Cornflower, bottom: Tile::Cornflower,
        lod: Tile::Cornflower,
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

    pub fn is_replaceable(&self) -> bool {
        self.properties().replaceable
    }

    pub fn style(&self) -> RenderStyle {
        self.properties().style
    }

    /// The item it leaves when it's broken in survival, if any.
    pub fn drops(self) -> Option<Item> {
        match self.properties().drops {
            Drops::Nothing => None,
            Drops::Itself => Some(Item::Block(self)),
            Drops::Block(block) => Some(Item::Block(block)),
        }
    }

    /// Whether it's a plant, which stands on the block under it and goes when that does.
    pub fn is_plant(&self) -> bool {
        self.style() == RenderStyle::Cross
    }

    /// Whether it's something there, rather than air or water: what's looked at, broken and
    /// placed.
    pub fn is_tangible(&self) -> bool {
        self.is_solid() || self.is_plant()
    }

    // Whether the faces of blocks behind this one are hidden by it. Leaves have holes.
    pub fn is_opaque(&self) -> bool {
        self.light_opacity() == MAX_LIGHT
    }

    // The tile a face of this block draws. Plants draw the same tile on every face of their cross.
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

// By name, so a block means the same to every save and every game version that knows it.
impl serde::Serialize for BlockType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> serde::Deserialize<'de> for BlockType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::from_name(&name)
            .ok_or_else(|| serde::de::Error::custom(format!("there's no block called {name}")))
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
    Planks,
    Cobblestone,
    StoneBricks,
    Bricks,
    CoalOre,
    CopperOre,
    IronOre,
    CraftingTableTop,
    CraftingTableSide,
    FurnaceSide,
    TallGrass,
    Poppy,
    Dandelion,
    Cornflower,
}

pub const TILE_COUNT: usize = 29;

impl Tile {
    // What meshes carry in vertex alpha for `block_texture.wesl`.
    pub fn atlas_cell(self) -> f32 {
        self as u8 as f32
    }

    // Greyscale tiles that vertex colours tint to the climate's grass.
    pub fn tinted(self) -> bool {
        matches!(self, Tile::GrassTop | Tile::Leaves | Tile::TallGrass)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{BlockType, Face, RenderStyle, Tile, TILE_COUNT};

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
    fn test_blocks_are_sent_and_saved_by_name() {
        let bytes = postcard::to_stdvec(&BlockType::IronOre).unwrap();
        assert_eq!(b"\x08iron_ore", bytes.as_slice());
        assert_eq!(BlockType::IronOre, postcard::from_bytes(&bytes).unwrap());
        let unknown = postcard::to_stdvec("mithril").unwrap();
        assert!(postcard::from_bytes::<BlockType>(&unknown).is_err());
    }

    #[test]
    fn test_tile_count_matches_the_last_tile() {
        assert_eq!(TILE_COUNT, Tile::Cornflower as usize + 1);
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

    #[test]
    fn test_plants_are_crosses_walked_through_seen_through_and_built_over() {
        let plants: Vec<_> = BlockType::ALL
            .into_iter()
            .filter(BlockType::is_plant)
            .collect();
        assert_eq!(
            vec![
                BlockType::TallGrass,
                BlockType::Poppy,
                BlockType::Dandelion,
                BlockType::Cornflower
            ],
            plants
        );
        for plant in plants {
            assert_eq!(RenderStyle::Cross, plant.style());
            assert!(!plant.is_solid() && !plant.is_opaque());
            assert_eq!(0, plant.light_opacity());
            assert_eq!(0.0, plant.occlusion_weight());
            assert!(plant.is_replaceable() && plant.is_tangible());
        }
    }

    #[test]
    fn test_only_air_water_and_plants_are_replaceable() {
        for block in BlockType::ALL {
            let expected = matches!(block, BlockType::Air | BlockType::Water) || block.is_plant();
            assert_eq!(expected, block.is_replaceable(), "{block:?}");
            assert_eq!(
                !expected || block.is_plant(),
                block.is_tangible(),
                "{block:?}"
            );
        }
    }
}
