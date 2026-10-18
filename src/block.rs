use rand::{Rng, RngExt};

use crate::chunks::generate::light::MAX_LIGHT;
use crate::farming::WHEAT;
use crate::items::ToolKind::{self, Axe, Pickaxe, Shovel};
use crate::items::{Item, ItemStack};

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
    /// How long it takes to break in survival by hand, in seconds, or with a tool it isn't for.
    hardness: f32,
    /// The tool that breaks it quicker, if any.
    tool: Option<ToolKind>,
    /// The tier of that tool it needs to drop anything, or 0 if a hand will do (§5.3).
    tier: u8,
    /// How many levels of light it takes from light passing through it, see `light.rs`.
    light_opacity: u8,
    /// The level of block light it gives off, 0 for none, see `BlockLight`.
    emits: u8,
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
    /// As a thin stick standing on the floor of the cell, its flame unshaded, at full detail
    /// only.
    Torch,
}

/// What a block leaves behind when it's broken in survival, to be picked up.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum Drops {
    Nothing,
    /// Its own block item.
    Itself,
    /// Another block's item, as stone leaves cobblestone.
    Block(BlockType),
    /// An item that isn't a block, as coal ore leaves coal.
    Item(Item),
    /// An item one time in so many, and otherwise nothing, as leaves now and then leave an
    /// apple.
    Chance(Item, u32),
    /// Some of an item, from the first count to the second, as ripe wheat leaves its seeds.
    Several(Item, u8, u8),
    /// What both leave, as tall grass leaves itself and now and then seeds.
    Both(&'static Drops, &'static Drops),
}

impl Drops {
    // Adds what `block` leaves to `stacks`, with `rng` deciding what's left to chance.
    fn roll(self, block: BlockType, rng: &mut impl Rng, stacks: &mut Vec<ItemStack>) {
        let one = |item| ItemStack::new(item, 1);
        match self {
            Drops::Nothing => {}
            Drops::Itself => stacks.push(one(Item::Block(block))),
            Drops::Block(block) => stacks.push(one(Item::Block(block))),
            Drops::Item(item) => stacks.push(one(item)),
            Drops::Chance(item, one_in) => {
                if rng.random_ratio(1, one_in) {
                    stacks.push(one(item));
                }
            }
            Drops::Several(item, least, most) => {
                stacks.push(ItemStack::new(item, rng.random_range(least..=most)));
            }
            Drops::Both(first, second) => {
                first.roll(block, rng, stacks);
                second.roll(block, rng, stacks);
            }
        }
    }
}

// A crop's row: a plant (see `TallGrass`) that isn't built over, so a field isn't lost to a
// block placed carelessly, and stands only on farmland (`BlockType::stands_on`).
const fn crop(name: &'static str, tile: Tile, drops: Drops) -> Properties {
    Properties {
        name,
        solid: false,
        replaceable: false,
        style: RenderStyle::Cross,
        drops,
        hardness: 0.0,
        tool: None,
        tier: 0,
        light_opacity: 0,
        emits: 0,
        occlusion_weight: 0.0,
        top: tile,
        side: tile,
        bottom: tile,
        lod: tile,
    }
}

// Unripe wheat gives back the seed it grew from, and ripe wheat gives wheat and a few more.
const UNRIPE: Drops = Drops::Item(Item::Seeds);
const RIPE: Drops = Drops::Both(
    &Drops::Item(Item::Wheat),
    &Drops::Several(Item::Seeds, 1, 3),
);

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
        name: "air", solid: false, light_opacity: 0, occlusion_weight: 0.0, emits: 0,
        style: RenderStyle::None, replaceable: true, drops: Drops::Nothing,
        hardness: 0.0, tool: None, tier: 0,
        // Never drawn.
        top: Tile::Stone, side: Tile::Stone, bottom: Tile::Stone, lod: Tile::Stone,
    },
    Stone {
        name: "stone", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Block(BlockType::Cobblestone),
        hardness: 7.5, tool: Some(Pickaxe), tier: 1,
        top: Tile::Stone, side: Tile::Stone, bottom: Tile::Stone, lod: Tile::Stone,
    },
    Grass {
        name: "grass", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Block(BlockType::Dirt),
        hardness: 0.9, tool: Some(Shovel), tier: 0,
        // Far off, grass stays green on hillsides.
        top: Tile::GrassTop, side: Tile::GrassSide, bottom: Tile::Dirt, lod: Tile::GrassTop,
    },
    Sand {
        name: "sand", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 0.75, tool: Some(Shovel), tier: 0,
        top: Tile::Sand, side: Tile::Sand, bottom: Tile::Sand, lod: Tile::Sand,
    },
    Water {
        name: "water", solid: false, light_opacity: 2, occlusion_weight: 0.4, emits: 0,
        style: RenderStyle::Liquid, replaceable: true, drops: Drops::Nothing,
        hardness: 0.0, tool: None, tier: 0,
        top: Tile::Water, side: Tile::Water, bottom: Tile::Water, lod: Tile::Water,
    },
    Snow {
        name: "snow", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 0.3, tool: Some(Shovel), tier: 0,
        top: Tile::Snow, side: Tile::SnowSide, bottom: Tile::Snow, lod: Tile::SnowSide,
    },
    Log {
        name: "log", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 3.0, tool: Some(Axe), tier: 0,
        top: Tile::LogEnd, side: Tile::LogBark, bottom: Tile::LogEnd, lod: Tile::LogBark,
    },
    Leaves {
        name: "leaves", solid: true, light_opacity: 1, occlusion_weight: 0.3, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Chance(Item::Apple, 20),
        hardness: 0.3, tool: None, tier: 0,
        // Saplings come with farming (M1.9b).
        top: Tile::Leaves, side: Tile::Leaves, bottom: Tile::Leaves, lod: Tile::Leaves,
    },
    Dirt {
        name: "dirt", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 0.75, tool: Some(Shovel), tier: 0,
        top: Tile::Dirt, side: Tile::Dirt, bottom: Tile::Dirt, lod: Tile::Dirt,
    },
    Gravel {
        name: "gravel", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 0.9, tool: Some(Shovel), tier: 0,
        top: Tile::Gravel, side: Tile::Gravel, bottom: Tile::Gravel, lod: Tile::Gravel,
    },
    Ice {
        name: "ice", solid: true, light_opacity: 3, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 0.75, tool: Some(Pickaxe), tier: 0,
        top: Tile::Ice, side: Tile::Ice, bottom: Tile::Ice, lod: Tile::Ice,
    },
    Sandstone {
        name: "sandstone", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 4.0, tool: Some(Pickaxe), tier: 1,
        top: Tile::SandstoneTop, side: Tile::SandstoneSide, bottom: Tile::SandstoneTop,
        lod: Tile::SandstoneSide,
    },
    Planks {
        name: "planks", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 3.0, tool: Some(Axe), tier: 0,
        top: Tile::Planks, side: Tile::Planks, bottom: Tile::Planks, lod: Tile::Planks,
    },
    Cobblestone {
        name: "cobblestone", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 7.5, tool: Some(Pickaxe), tier: 1,
        top: Tile::Cobblestone, side: Tile::Cobblestone, bottom: Tile::Cobblestone,
        lod: Tile::Cobblestone,
    },
    StoneBricks {
        name: "stone_bricks", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 7.5, tool: Some(Pickaxe), tier: 1,
        top: Tile::StoneBricks, side: Tile::StoneBricks, bottom: Tile::StoneBricks,
        lod: Tile::StoneBricks,
    },
    Bricks {
        name: "bricks", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 7.5, tool: Some(Pickaxe), tier: 1,
        top: Tile::Bricks, side: Tile::Bricks, bottom: Tile::Bricks, lod: Tile::Bricks,
    },
    CoalOre {
        name: "coal_ore", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Item(Item::Coal),
        hardness: 9.0, tool: Some(Pickaxe), tier: 1,
        top: Tile::CoalOre, side: Tile::CoalOre, bottom: Tile::CoalOre, lod: Tile::CoalOre,
    },
    CopperOre {
        name: "copper_ore", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 9.0, tool: Some(Pickaxe), tier: 2,
        top: Tile::CopperOre, side: Tile::CopperOre, bottom: Tile::CopperOre, lod: Tile::CopperOre,
    },
    IronOre {
        name: "iron_ore", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 9.0, tool: Some(Pickaxe), tier: 3,
        top: Tile::IronOre, side: Tile::IronOre, bottom: Tile::IronOre, lod: Tile::IronOre,
    },
    // Ores found only under far climates (§5.3), which need an iron pickaxe, and the deepest,
    // which needs bright steel and glows a little in the dark of the caverns it's found in.
    GoldOre {
        name: "gold_ore", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 10.0, tool: Some(Pickaxe), tier: 4,
        top: Tile::GoldOre, side: Tile::GoldOre, bottom: Tile::GoldOre, lod: Tile::GoldOre,
    },
    FrostCrystalOre {
        name: "frost_crystal_ore", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        emits: 0,
        style: RenderStyle::Cube, replaceable: false,
        drops: Drops::Item(Item::FrostCrystal),
        hardness: 10.0, tool: Some(Pickaxe), tier: 4,
        top: Tile::FrostCrystalOre, side: Tile::FrostCrystalOre, bottom: Tile::FrostCrystalOre,
        lod: Tile::FrostCrystalOre,
    },
    JadeOre {
        name: "jade_ore", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Item(Item::Jade),
        hardness: 10.0, tool: Some(Pickaxe), tier: 4,
        top: Tile::JadeOre, side: Tile::JadeOre, bottom: Tile::JadeOre, lod: Tile::JadeOre,
    },
    EmberOre {
        name: "ember_ore", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 5,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Item(Item::Ember),
        hardness: 12.0, tool: Some(Pickaxe), tier: 5,
        top: Tile::EmberOre, side: Tile::EmberOre, bottom: Tile::EmberOre, lod: Tile::EmberOre,
    },
    // What the far ores are worked into, to build with.
    GoldBlock {
        name: "gold_block", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 9.0, tool: Some(Pickaxe), tier: 1,
        top: Tile::GoldBlock, side: Tile::GoldBlock, bottom: Tile::GoldBlock, lod: Tile::GoldBlock,
    },
    FrostCrystalBlock {
        name: "frost_crystal_block", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 9.0, tool: Some(Pickaxe), tier: 1,
        top: Tile::FrostCrystalBlock, side: Tile::FrostCrystalBlock,
        bottom: Tile::FrostCrystalBlock, lod: Tile::FrostCrystalBlock,
    },
    PolishedJade {
        name: "polished_jade", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 9.0, tool: Some(Pickaxe), tier: 1,
        top: Tile::PolishedJade, side: Tile::PolishedJade, bottom: Tile::PolishedJade,
        lod: Tile::PolishedJade,
    },
    BrightSteelBlock {
        name: "bright_steel_block", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 12.0, tool: Some(Pickaxe), tier: 1,
        top: Tile::BrightSteelBlock, side: Tile::BrightSteelBlock,
        bottom: Tile::BrightSteelBlock, lod: Tile::BrightSteelBlock,
    },
    // Embers behind frost crystal panes, as bright as a torch.
    EmberLamp {
        name: "ember_lamp", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0,
        emits: 14,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 3.0, tool: Some(Pickaxe), tier: 0,
        top: Tile::EmberLamp, side: Tile::EmberLamp, bottom: Tile::EmberLamp,
        lod: Tile::EmberLamp,
    },
    CraftingTable {
        name: "crafting_table", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 3.75, tool: Some(Axe), tier: 0,
        top: Tile::CraftingTableTop, side: Tile::CraftingTableSide, bottom: Tile::Planks,
        lod: Tile::CraftingTableSide,
    },
    Furnace {
        name: "furnace", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 8.0, tool: Some(Pickaxe), tier: 1,
        // Blocks have no facing yet, so every side has the furnace's mouth.
        top: Tile::Stone, side: Tile::FurnaceSide, bottom: Tile::Stone, lod: Tile::FurnaceSide,
    },
    // Plants, standing on the block below and gone with it. They're broken at a touch, let light
    // and the player through and are built over, so their faces hide nothing. Flowers and ferns
    // drop nothing, and tall grass itself, for beds' mattresses, and a seed one time in eight.
    TallGrass {
        name: "tall_grass", solid: false, light_opacity: 0, occlusion_weight: 0.0, emits: 0,
        style: RenderStyle::Cross, replaceable: true,
        drops: Drops::Both(&Drops::Itself, &Drops::Chance(Item::Seeds, 8)),
        hardness: 0.0, tool: None, tier: 0,
        top: Tile::TallGrass, side: Tile::TallGrass, bottom: Tile::TallGrass, lod: Tile::TallGrass,
    },
    Poppy {
        name: "poppy", solid: false, light_opacity: 0, occlusion_weight: 0.0, emits: 0,
        style: RenderStyle::Cross, replaceable: true, drops: Drops::Nothing,
        hardness: 0.0, tool: None, tier: 0,
        top: Tile::Poppy, side: Tile::Poppy, bottom: Tile::Poppy, lod: Tile::Poppy,
    },
    Dandelion {
        name: "dandelion", solid: false, light_opacity: 0, occlusion_weight: 0.0, emits: 0,
        style: RenderStyle::Cross, replaceable: true, drops: Drops::Nothing,
        hardness: 0.0, tool: None, tier: 0,
        top: Tile::Dandelion, side: Tile::Dandelion, bottom: Tile::Dandelion, lod: Tile::Dandelion,
    },
    Cornflower {
        name: "cornflower", solid: false, light_opacity: 0, occlusion_weight: 0.0, emits: 0,
        style: RenderStyle::Cross, replaceable: true, drops: Drops::Nothing,
        hardness: 0.0, tool: None, tier: 0,
        top: Tile::Cornflower, side: Tile::Cornflower, bottom: Tile::Cornflower,
        lod: Tile::Cornflower,
    },
    // In the shade of jungles and taiga, tinted like tall grass.
    Fern {
        name: "fern", solid: false, light_opacity: 0, occlusion_weight: 0.0, emits: 0,
        style: RenderStyle::Cross, replaceable: true, drops: Drops::Nothing,
        hardness: 0.0, tool: None, tier: 0,
        top: Tile::Fern, side: Tile::Fern, bottom: Tile::Fern, lod: Tile::Fern,
    },
    // Stands on the floor of its cell; torches on walls need blocks to face a way.
    Torch {
        name: "torch", solid: false, light_opacity: 0, occlusion_weight: 0.0, emits: 14,
        style: RenderStyle::Torch, replaceable: false, drops: Drops::Itself,
        hardness: 0.0, tool: None, tier: 0,
        top: Tile::Torch, side: Tile::Torch, bottom: Tile::Torch, lod: Tile::Torch,
    },
    // A cube for now, rather than two blocks long. Right-clicking one sets where its sleeper
    // respawns, and at night sleeps in it (§5.5).
    Bed {
        name: "bed", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 1.0, tool: Some(Axe), tier: 0,
        top: Tile::BedTop, side: Tile::BedSide, bottom: Tile::Planks, lod: Tile::BedTop,
    },
    // Dirt or grass a hoe's tilled, for crops to stand on (`farming`). It's dirt again once
    // something solid's put on it, and breaking it leaves dirt.
    Farmland {
        name: "farmland", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Block(BlockType::Dirt),
        hardness: 0.75, tool: Some(Shovel), tier: 0,
        top: Tile::FarmlandTop, side: Tile::Dirt, bottom: Tile::Dirt, lod: Tile::Dirt,
    },
    // Wheat's stages, from the shoots seeds come up as to ripe ears (`farming::WHEAT`).
    Wheat0 { ..crop("wheat_0", Tile::Wheat0, UNRIPE) },
    Wheat1 { ..crop("wheat_1", Tile::Wheat1, UNRIPE) },
    Wheat2 { ..crop("wheat_2", Tile::Wheat2, UNRIPE) },
    Wheat3 { ..crop("wheat_3", Tile::Wheat3, UNRIPE) },
    Wheat4 { ..crop("wheat_4", Tile::Wheat4, UNRIPE) },
    Wheat5 { ..crop("wheat_5", Tile::Wheat5, UNRIPE) },
    Wheat6 { ..crop("wheat_6", Tile::Wheat6, UNRIPE) },
    RipeWheat { ..crop("ripe_wheat", Tile::RipeWheat, RIPE) },
    // A container of 27 slots (`containers`), which leaves what's in it as well when it's broken.
    // Every side has the latch, as blocks have no facing yet.
    Chest {
        name: "chest", solid: true, light_opacity: MAX_LIGHT, occlusion_weight: 1.0, emits: 0,
        style: RenderStyle::Cube, replaceable: false, drops: Drops::Itself,
        hardness: 3.0, tool: Some(Axe), tier: 0,
        top: Tile::ChestTop, side: Tile::ChestSide, bottom: Tile::Planks, lod: Tile::ChestSide,
    },
    // Blocks for authoring structures in creative (§5.6, `authoring`), never generated nor
    // dropped. The world draws neither: creative shows them as small boxes, so a structure looks
    // as it will while it's built, and survival not at all. A structure void is a cell a
    // template leaves as the world has it, and is built over as air is. A loot marker is where
    // a template's `loot` marker goes.
    StructureVoid {
        name: "structure_void", solid: false, light_opacity: 0, occlusion_weight: 0.0, emits: 0,
        style: RenderStyle::None, replaceable: true, drops: Drops::Nothing,
        hardness: 0.0, tool: None, tier: 0,
        top: Tile::StructureVoid, side: Tile::StructureVoid, bottom: Tile::StructureVoid,
        lod: Tile::StructureVoid,
    },
    LootMarker {
        name: "loot_marker", solid: false, light_opacity: 0, occlusion_weight: 0.0, emits: 0,
        style: RenderStyle::None, replaceable: false, drops: Drops::Nothing,
        hardness: 0.0, tool: None, tier: 0,
        top: Tile::LootMarker, side: Tile::LootMarker, bottom: Tile::LootMarker,
        lod: Tile::LootMarker,
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

    pub fn emits(&self) -> u8 {
        self.properties().emits
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

    /// What it leaves when it's broken in survival, with `rng` deciding what's left to chance.
    pub fn drops(self, rng: &mut impl Rng) -> Vec<ItemStack> {
        let mut stacks = Vec::new();
        self.properties().drops.roll(self, rng, &mut stacks);
        stacks
    }

    /// How long it takes to break by hand in survival, in seconds, see `breaking`.
    pub fn hardness(self) -> f32 {
        self.properties().hardness
    }

    /// The tool it's broken quicker with, if there's one.
    pub fn tool(self) -> Option<ToolKind> {
        self.properties().tool
    }

    /// The tier of tool it needs to drop anything, or 0 if a hand will do.
    pub fn tier(self) -> u8 {
        self.properties().tier
    }

    /// Whether it's a plant.
    pub fn is_plant(&self) -> bool {
        self.style() == RenderStyle::Cross
    }

    /// Whether it stands on the block under it, and goes when that does: plants and torches.
    pub fn needs_ground(&self) -> bool {
        matches!(self.style(), RenderStyle::Cross | RenderStyle::Torch)
    }

    /// Whether it's one of a crop's stages, which grow on farmland.
    pub fn is_crop(&self) -> bool {
        WHEAT.contains(self)
    }

    /// Whether it can stand on `ground`, if it `needs_ground`: a crop on farmland, and the
    /// rest on anything solid.
    pub fn stands_on(self, ground: BlockType) -> bool {
        if self.is_crop() {
            ground == BlockType::Farmland
        } else {
            ground.is_solid()
        }
    }

    /// Whether it's something there, rather than air or water: what's looked at, broken and
    /// placed.
    pub fn is_tangible(&self) -> bool {
        self.is_solid() || self.needs_ground() || self.is_for_authoring()
    }

    /// Whether it's only for authoring structures: a structure void or a marker.
    pub fn is_for_authoring(&self) -> bool {
        matches!(self, BlockType::StructureVoid | BlockType::LootMarker)
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
    Torch,
    BedTop,
    BedSide,
    FarmlandTop,
    Wheat0,
    Wheat1,
    Wheat2,
    Wheat3,
    Wheat4,
    Wheat5,
    Wheat6,
    RipeWheat,
    ChestTop,
    ChestSide,
    StructureVoid,
    LootMarker,
    Fern,
    GoldOre,
    FrostCrystalOre,
    JadeOre,
    EmberOre,
    GoldBlock,
    FrostCrystalBlock,
    PolishedJade,
    BrightSteelBlock,
    EmberLamp,
}

pub const TILE_COUNT: usize = 55;

impl Tile {
    // What meshes carry in vertex alpha for `block_texture.wesl`.
    pub fn atlas_cell(self) -> f32 {
        self as u8 as f32
    }

    // Greyscale tiles that vertex colours tint to the climate's grass.
    pub fn tinted(self) -> bool {
        matches!(
            self,
            Tile::GrassTop | Tile::Leaves | Tile::TallGrass | Tile::Fern
        )
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
            assert!(name.starts_with(|c: char| c.is_ascii_lowercase()));
            assert!(name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'));
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
        assert_eq!(TILE_COUNT, Tile::EmberLamp as usize + 1);
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
            .filter(|plant| plant.is_plant() && !plant.is_crop())
            .collect();
        assert_eq!(
            vec![
                BlockType::TallGrass,
                BlockType::Poppy,
                BlockType::Dandelion,
                BlockType::Cornflower,
                BlockType::Fern
            ],
            plants
        );
        for plant in BlockType::ALL.into_iter().filter(BlockType::is_plant) {
            assert_eq!(RenderStyle::Cross, plant.style());
            assert!(!plant.is_solid() && !plant.is_opaque());
            assert_eq!(0, plant.light_opacity());
            assert_eq!(0.0, plant.occlusion_weight());
            assert!(plant.is_tangible() && plant.needs_ground());
            // but crops aren't built over
            assert_eq!(!plant.is_crop(), plant.is_replaceable(), "{plant:?}");
        }
    }

    #[test]
    fn test_crops_stand_on_farmland_and_everything_else_on_solid_ground() {
        for crop in crate::farming::WHEAT {
            assert!(crop.is_crop() && crop.is_plant());
            assert!(crop.stands_on(BlockType::Farmland));
            assert!(!crop.stands_on(BlockType::Dirt) && !crop.stands_on(BlockType::Grass));
        }
        for plant in [BlockType::TallGrass, BlockType::Torch] {
            assert!(!plant.is_crop());
            assert!(plant.stands_on(BlockType::Farmland) && plant.stands_on(BlockType::Stone));
            assert!(!plant.stands_on(BlockType::Water) && !plant.stands_on(BlockType::Poppy));
        }
        assert_eq!(Tile::FarmlandTop, BlockType::Farmland.tile(Face::Top));
        assert_eq!(Tile::Dirt, BlockType::Farmland.tile(Face::Side));
    }

    #[test]
    fn test_torches_glow_let_light_and_players_through_and_stand_on_the_ground() {
        let torch = BlockType::Torch;
        assert_eq!(14, torch.emits());
        assert_eq!(RenderStyle::Torch, torch.style());
        assert!(!torch.is_solid() && !torch.is_opaque() && !torch.is_replaceable());
        assert_eq!(0, torch.light_opacity());
        assert!(torch.needs_ground() && torch.is_tangible() && !torch.is_plant());
        let one = crate::items::ItemStack::new(crate::items::Item::Block(torch), 1);
        assert_eq!(vec![one], torch.drops(&mut rand::rng()));
        assert_eq!(0, BlockType::Stone.emits());
    }

    #[test]
    fn test_authoring_blocks_are_reached_for_but_walked_and_seen_through_and_never_drawn() {
        for block in [BlockType::StructureVoid, BlockType::LootMarker] {
            assert!(block.is_for_authoring() && block.is_tangible());
            assert!(!block.is_solid() && !block.needs_ground());
            assert_eq!(0, block.light_opacity());
            assert_eq!(RenderStyle::None, block.style());
            assert!(block.drops(&mut rand::rng()).is_empty());
        }
        assert!(!BlockType::Air.is_for_authoring() && !BlockType::Stone.is_for_authoring());
    }

    #[test]
    fn test_only_air_water_plants_other_than_crops_and_structure_void_are_replaceable() {
        for block in BlockType::ALL {
            let expected = matches!(
                block,
                BlockType::Air | BlockType::Water | BlockType::StructureVoid
            ) || (block.is_plant() && !block.is_crop());
            assert_eq!(expected, block.is_replaceable(), "{block:?}");
            assert_eq!(
                !expected || block.is_plant() || block.is_for_authoring(),
                block.is_tangible(),
                "{block:?}"
            );
        }
    }
}
