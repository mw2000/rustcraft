//! Crafting (§5.3, D-11): a recipe is a pattern of ingredients laid in a crafting grid, by hand
//! in the inventory's 2×2 or at a crafting table's 3×3, or an item smelted in a furnace. A
//! shaped recipe's pattern goes anywhere in the grid it fits, mirrored or not, and a shapeless
//! one's ingredients in any cells at all. Every recipe is a row of the `recipes!` table (INV-6),
//! and what burns in a furnace is `FUELS`. The server matches what's in a player's grid
//! (`inventory::slots`) and smelts (`containers`), and the inventory screen's recipe book lays
//! recipes into the grid (`inventory::recipes`).

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

use crate::block::BlockType;
use crate::inventory::Inventory;
use crate::items::{Item, ItemStack};

/// How many cells a crafting grid has, a crafting table's 3×3, numbered along its rows. The
/// inventory's 2×2 is its top left corner.
pub const GRID: usize = 9;
/// How many cells wide the biggest grid is.
pub const GRID_WIDTH: usize = 3;
const HAND_GRID: [usize; 4] = [0, 1, 3, 4];
const TABLE_GRID: [usize; GRID] = [0, 1, 2, 3, 4, 5, 6, 7, 8];

/// Where a recipe is made.
#[derive(Debug, Default, PartialEq, Eq, Copy, Clone, Hash)]
pub enum Station {
    /// Anywhere, in the inventory screen's 2×2 grid.
    #[default]
    Hand,
    /// At a crafting table, opened by right-clicking it, in its 3×3 grid.
    CraftingTable,
    /// In a furnace, which smelts its input over time with the fuel it's given.
    Furnace,
}

impl Station {
    /// The station a block is, if it's one.
    pub fn of_block(block: BlockType) -> Option<Self> {
        match block {
            BlockType::CraftingTable => Some(Station::CraftingTable),
            BlockType::Furnace => Some(Station::Furnace),
            _ => None,
        }
    }

    /// Whether a recipe made at `station` can be crafted here: a crafting table makes what
    /// hands can too, and a furnace crafts nothing, as it smelts on its own.
    pub fn makes(self, station: Station) -> bool {
        match self {
            Station::Hand => station == Station::Hand,
            Station::CraftingTable => station != Station::Furnace,
            Station::Furnace => false,
        }
    }

    /// The cells of the crafting grid used here: the top left 2×2 by hand, all 3×3 at a table,
    /// and none at a furnace.
    pub fn grid(self) -> &'static [usize] {
        match self {
            Station::Hand => &HAND_GRID,
            Station::CraftingTable => &TABLE_GRID,
            Station::Furnace => &[],
        }
    }

    /// How many cells wide its grid is.
    pub fn grid_width(self) -> usize {
        match self {
            Station::Hand => 2,
            Station::CraftingTable => GRID_WIDTH,
            Station::Furnace => 0,
        }
    }
}

/// What a recipe takes, and how it's laid out.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Pattern {
    /// Rows of keys, a space for an empty cell, and the item each key stands for. It's matched
    /// wherever it fits in the grid, and mirrored left to right, but nothing else may be in
    /// the grid.
    Shaped(&'static [&'static str], &'static [(char, Item)]),
    /// One of each item, in any cells, for blends and dyes where the arrangement doesn't
    /// matter. An item named twice takes two.
    Shapeless(&'static [Item]),
    /// One item, smelted in a furnace into the output.
    Smelted(Item),
}

/// A recipe's row in the table.
struct Properties {
    /// Its stable name, which the protocol sends.
    name: &'static str,
    station: Station,
    pattern: Pattern,
    output: (Item, u8),
}

// Declares `Recipe`, a variant for each row, and the rows' properties.
macro_rules! recipes {
    ($($variant:ident {
        name: $name:literal,
        station: $station:ident,
        pattern: $pattern:expr,
        output: ($output:expr, $made:literal) $(,)?
    },)*) => {
        /// A way to make something.
        #[derive(Debug, PartialEq, Eq, Copy, Clone, Hash)]
        pub enum Recipe {
            $($variant,)*
        }

        impl Recipe {
            /// Every recipe, in the order the recipe book lists them.
            pub const ALL: &[Recipe] = &[$(Recipe::$variant),*];

            fn properties(self) -> &'static Properties {
                match self {
                    $(Recipe::$variant => &Properties {
                        name: $name,
                        station: Station::$station,
                        pattern: $pattern,
                        output: ($output, $made),
                    },)*
                }
            }
        }
    };
}

// A shaped pattern: its rows, then what each key stands for.
macro_rules! shaped {
    ([$($row:literal),+ $(,)?], $($key:literal => $item:expr),+ $(,)?) => {
        Pattern::Shaped(&[$($row),+], &[$(($key, $item)),+])
    };
}

// A shapeless pattern: one of each item named.
macro_rules! shapeless {
    ($($item:expr),+ $(,)?) => {
        Pattern::Shapeless(&[$($item),+])
    };
}

const LOG: Item = Item::Block(BlockType::Log);
const PLANKS: Item = Item::Block(BlockType::Planks);
const DARK_PLANKS: Item = Item::Block(BlockType::DarkPlanks);
const COBBLESTONE: Item = Item::Block(BlockType::Cobblestone);
const STONE: Item = Item::Block(BlockType::Stone);
const STONE_BRICKS: Item = Item::Block(BlockType::StoneBricks);
const BRICKS: Item = Item::Block(BlockType::Bricks);
const SAND: Item = Item::Block(BlockType::Sand);
const SANDSTONE: Item = Item::Block(BlockType::Sandstone);
const DIRT: Item = Item::Block(BlockType::Dirt);
const TERRACOTTA: Item = Item::Block(BlockType::Terracotta);
const THATCH: Item = Item::Block(BlockType::Thatch);
const ADOBE: Item = Item::Block(BlockType::Adobe);
const VINES: Item = Item::Block(BlockType::Vines);
const TALL_GRASS: Item = Item::Block(BlockType::TallGrass);
const STICK: Item = Item::Stick;

// Patterns read as what they make, as Minecraft players know them: a tool's head across the top
// or down one side of its handle of sticks, a sword's blade over its hilt, stairs climbing, slabs
// a row, walls and panes two rows, a chest or a furnace a ring, armour its outline. '#' is the
// main material and '|' a stick. Our materials and counts are our own: a hoe's two of its head,
// stairs six into four and slabs three into six, a door's six planks making three, and blocks of
// metal and crystal four into one.
recipes! {
    Planks {
        name: "planks", station: Hand,
        pattern: shapeless!(LOG), output: (PLANKS, 4),
    },
    Sticks {
        name: "sticks", station: Hand,
        pattern: shaped!(["#", "#"], '#' => PLANKS), output: (STICK, 4),
    },
    // vines plaited of tall grass, as there are none to find yet
    Vines {
        name: "vines", station: Hand,
        pattern: shapeless!(TALL_GRASS, TALL_GRASS, TALL_GRASS), output: (VINES, 1),
    },
    Torches {
        name: "torches", station: Hand,
        pattern: shaped!(["c", "|"], 'c' => Item::Coal, '|' => STICK),
        output: (Item::Block(BlockType::Torch), 4),
    },
    CraftingTable {
        name: "crafting_table", station: Hand,
        pattern: shaped!(["##", "##"], '#' => PLANKS),
        output: (Item::Block(BlockType::CraftingTable), 1),
    },
    WoolBlock {
        name: "wool_block", station: Hand,
        pattern: shaped!(["##", "##"], '#' => Item::Wool),
        output: (Item::Block(BlockType::WoolBlock), 1),
    },
    WoodenPickaxe {
        name: "wooden_pickaxe", station: CraftingTable,
        pattern: shaped!(["###", " | ", " | "], '#' => PLANKS, '|' => STICK),
        output: (Item::WoodenPickaxe, 1),
    },
    WoodenAxe {
        name: "wooden_axe", station: CraftingTable,
        pattern: shaped!(["##", "#|", " |"], '#' => PLANKS, '|' => STICK),
        output: (Item::WoodenAxe, 1),
    },
    WoodenShovel {
        name: "wooden_shovel", station: CraftingTable,
        pattern: shaped!(["#", "|", "|"], '#' => PLANKS, '|' => STICK),
        output: (Item::WoodenShovel, 1),
    },
    WoodenHoe {
        name: "wooden_hoe", station: CraftingTable,
        pattern: shaped!(["##", " |", " |"], '#' => PLANKS, '|' => STICK),
        output: (Item::WoodenHoe, 1),
    },
    WoodenSword {
        name: "wooden_sword", station: CraftingTable,
        pattern: shaped!(["#", "#", "|"], '#' => PLANKS, '|' => STICK),
        output: (Item::WoodenSword, 1),
    },
    StonePickaxe {
        name: "stone_pickaxe", station: CraftingTable,
        pattern: shaped!(["###", " | ", " | "], '#' => COBBLESTONE, '|' => STICK),
        output: (Item::StonePickaxe, 1),
    },
    StoneAxe {
        name: "stone_axe", station: CraftingTable,
        pattern: shaped!(["##", "#|", " |"], '#' => COBBLESTONE, '|' => STICK),
        output: (Item::StoneAxe, 1),
    },
    StoneShovel {
        name: "stone_shovel", station: CraftingTable,
        pattern: shaped!(["#", "|", "|"], '#' => COBBLESTONE, '|' => STICK),
        output: (Item::StoneShovel, 1),
    },
    StoneHoe {
        name: "stone_hoe", station: CraftingTable,
        pattern: shaped!(["##", " |", " |"], '#' => COBBLESTONE, '|' => STICK),
        output: (Item::StoneHoe, 1),
    },
    StoneSword {
        name: "stone_sword", station: CraftingTable,
        pattern: shaped!(["#", "#", "|"], '#' => COBBLESTONE, '|' => STICK),
        output: (Item::StoneSword, 1),
    },
    CopperPickaxe {
        name: "copper_pickaxe", station: CraftingTable,
        pattern: shaped!(["###", " | ", " | "], '#' => Item::CopperIngot, '|' => STICK),
        output: (Item::CopperPickaxe, 1),
    },
    CopperAxe {
        name: "copper_axe", station: CraftingTable,
        pattern: shaped!(["##", "#|", " |"], '#' => Item::CopperIngot, '|' => STICK),
        output: (Item::CopperAxe, 1),
    },
    CopperShovel {
        name: "copper_shovel", station: CraftingTable,
        pattern: shaped!(["#", "|", "|"], '#' => Item::CopperIngot, '|' => STICK),
        output: (Item::CopperShovel, 1),
    },
    CopperHoe {
        name: "copper_hoe", station: CraftingTable,
        pattern: shaped!(["##", " |", " |"], '#' => Item::CopperIngot, '|' => STICK),
        output: (Item::CopperHoe, 1),
    },
    CopperSword {
        name: "copper_sword", station: CraftingTable,
        pattern: shaped!(["#", "#", "|"], '#' => Item::CopperIngot, '|' => STICK),
        output: (Item::CopperSword, 1),
    },
    IronPickaxe {
        name: "iron_pickaxe", station: CraftingTable,
        pattern: shaped!(["###", " | ", " | "], '#' => Item::IronIngot, '|' => STICK),
        output: (Item::IronPickaxe, 1),
    },
    IronAxe {
        name: "iron_axe", station: CraftingTable,
        pattern: shaped!(["##", "#|", " |"], '#' => Item::IronIngot, '|' => STICK),
        output: (Item::IronAxe, 1),
    },
    IronShovel {
        name: "iron_shovel", station: CraftingTable,
        pattern: shaped!(["#", "|", "|"], '#' => Item::IronIngot, '|' => STICK),
        output: (Item::IronShovel, 1),
    },
    IronHoe {
        name: "iron_hoe", station: CraftingTable,
        pattern: shaped!(["##", " |", " |"], '#' => Item::IronIngot, '|' => STICK),
        output: (Item::IronHoe, 1),
    },
    IronSword {
        name: "iron_sword", station: CraftingTable,
        pattern: shaped!(["#", "#", "|"], '#' => Item::IronIngot, '|' => STICK),
        output: (Item::IronSword, 1),
    },
    BrightSteelPickaxe {
        name: "bright_steel_pickaxe", station: CraftingTable,
        pattern: shaped!(["###", " | ", " | "], '#' => Item::BrightSteelIngot, '|' => STICK),
        output: (Item::BrightSteelPickaxe, 1),
    },
    BrightSteelAxe {
        name: "bright_steel_axe", station: CraftingTable,
        pattern: shaped!(["##", "#|", " |"], '#' => Item::BrightSteelIngot, '|' => STICK),
        output: (Item::BrightSteelAxe, 1),
    },
    BrightSteelShovel {
        name: "bright_steel_shovel", station: CraftingTable,
        pattern: shaped!(["#", "|", "|"], '#' => Item::BrightSteelIngot, '|' => STICK),
        output: (Item::BrightSteelShovel, 1),
    },
    BrightSteelHoe {
        name: "bright_steel_hoe", station: CraftingTable,
        pattern: shaped!(["##", " |", " |"], '#' => Item::BrightSteelIngot, '|' => STICK),
        output: (Item::BrightSteelHoe, 1),
    },
    BrightSteelSword {
        name: "bright_steel_sword", station: CraftingTable,
        pattern: shaped!(["#", "#", "|"], '#' => Item::BrightSteelIngot, '|' => STICK),
        output: (Item::BrightSteelSword, 1),
    },
    Shears {
        name: "shears", station: CraftingTable,
        pattern: shaped!([" #", "# "], '#' => Item::IronIngot), output: (Item::Shears, 1),
    },
    // a bucket bent from three ingots, to carry water and lava in (M4.3)
    Bucket {
        name: "bucket", station: CraftingTable,
        pattern: shaped!(["# #", " # "], '#' => Item::IronIngot), output: (Item::Bucket, 1),
    },
    Furnace {
        name: "furnace", station: CraftingTable,
        pattern: shaped!(["###", "# #", "###"], '#' => COBBLESTONE),
        output: (Item::Block(BlockType::Furnace), 1),
    },
    Chest {
        name: "chest", station: CraftingTable,
        pattern: shaped!(["###", "# #", "###"], '#' => PLANKS),
        output: (Item::Block(BlockType::Chest), 1),
    },
    // a barrel's staves of planks, its ends of slabs
    Barrel {
        name: "barrel", station: CraftingTable,
        pattern: shaped!(
            ["#-#", "# #", "#-#"],
            '#' => PLANKS, '-' => Item::Block(BlockType::PlanksSlab),
        ),
        output: (Item::Block(BlockType::Barrel), 1),
    },
    // a mattress of wool on a frame of planks
    Bed {
        name: "bed", station: CraftingTable,
        pattern: shaped!(["www", "###"], 'w' => Item::Wool, '#' => PLANKS),
        output: (Item::Block(BlockType::Bed), 1),
    },
    StoneBricks {
        name: "stone_bricks", station: CraftingTable,
        pattern: shaped!(["##", "##"], '#' => COBBLESTONE), output: (STONE_BRICKS, 4),
    },
    Sandstone {
        name: "sandstone", station: CraftingTable,
        pattern: shaped!(["##", "##"], '#' => SAND), output: (SANDSTONE, 1),
    },
    // Building blocks (M2.4a): shapes cut from their blocks, and weathered and coloured
    // blocks, moss grown from vines and terracotta dyed with flowers. Bookshelves' books are of
    // pressed straw, and dark planks are stained with coal and light ones scoured with sand.
    PlanksStairs {
        name: "planks_stairs", station: CraftingTable,
        pattern: shaped!(["#  ", "## ", "###"], '#' => PLANKS),
        output: (Item::Block(BlockType::PlanksStairs), 4),
    },
    DarkPlanksStairs {
        name: "dark_planks_stairs", station: CraftingTable,
        pattern: shaped!(["#  ", "## ", "###"], '#' => DARK_PLANKS),
        output: (Item::Block(BlockType::DarkPlanksStairs), 4),
    },
    CobblestoneStairs {
        name: "cobblestone_stairs", station: CraftingTable,
        pattern: shaped!(["#  ", "## ", "###"], '#' => COBBLESTONE),
        output: (Item::Block(BlockType::CobblestoneStairs), 4),
    },
    StoneBrickStairs {
        name: "stone_brick_stairs", station: CraftingTable,
        pattern: shaped!(["#  ", "## ", "###"], '#' => STONE_BRICKS),
        output: (Item::Block(BlockType::StoneBrickStairs), 4),
    },
    BrickStairs {
        name: "brick_stairs", station: CraftingTable,
        pattern: shaped!(["#  ", "## ", "###"], '#' => BRICKS),
        output: (Item::Block(BlockType::BrickStairs), 4),
    },
    SandstoneStairs {
        name: "sandstone_stairs", station: CraftingTable,
        pattern: shaped!(["#  ", "## ", "###"], '#' => SANDSTONE),
        output: (Item::Block(BlockType::SandstoneStairs), 4),
    },
    PlanksSlab {
        name: "planks_slab", station: CraftingTable,
        pattern: shaped!(["###"], '#' => PLANKS),
        output: (Item::Block(BlockType::PlanksSlab), 6),
    },
    DarkPlanksSlab {
        name: "dark_planks_slab", station: CraftingTable,
        pattern: shaped!(["###"], '#' => DARK_PLANKS),
        output: (Item::Block(BlockType::DarkPlanksSlab), 6),
    },
    CobblestoneSlab {
        name: "cobblestone_slab", station: CraftingTable,
        pattern: shaped!(["###"], '#' => COBBLESTONE),
        output: (Item::Block(BlockType::CobblestoneSlab), 6),
    },
    StoneBrickSlab {
        name: "stone_brick_slab", station: CraftingTable,
        pattern: shaped!(["###"], '#' => STONE_BRICKS),
        output: (Item::Block(BlockType::StoneBrickSlab), 6),
    },
    BrickSlab {
        name: "brick_slab", station: CraftingTable,
        pattern: shaped!(["###"], '#' => BRICKS),
        output: (Item::Block(BlockType::BrickSlab), 6),
    },
    SandstoneSlab {
        name: "sandstone_slab", station: CraftingTable,
        pattern: shaped!(["###"], '#' => SANDSTONE),
        output: (Item::Block(BlockType::SandstoneSlab), 6),
    },
    CobblestoneWall {
        name: "cobblestone_wall", station: CraftingTable,
        pattern: shaped!(["###", "###"], '#' => COBBLESTONE),
        output: (Item::Block(BlockType::CobblestoneWall), 6),
    },
    StoneBrickWall {
        name: "stone_brick_wall", station: CraftingTable,
        pattern: shaped!(["###", "###"], '#' => STONE_BRICKS),
        output: (Item::Block(BlockType::StoneBrickWall), 6),
    },
    SandstoneWall {
        name: "sandstone_wall", station: CraftingTable,
        pattern: shaped!(["###", "###"], '#' => SANDSTONE),
        output: (Item::Block(BlockType::SandstoneWall), 6),
    },
    PlanksFence {
        name: "planks_fence", station: CraftingTable,
        pattern: shaped!(["#|#", "#|#"], '#' => PLANKS, '|' => STICK),
        output: (Item::Block(BlockType::PlanksFence), 3),
    },
    DarkPlanksFence {
        name: "dark_planks_fence", station: CraftingTable,
        pattern: shaped!(["#|#", "#|#"], '#' => DARK_PLANKS, '|' => STICK),
        output: (Item::Block(BlockType::DarkPlanksFence), 3),
    },
    Door {
        name: "door", station: CraftingTable,
        pattern: shaped!(["##", "##", "##"], '#' => PLANKS),
        output: (Item::Block(BlockType::Door), 3),
    },
    Trapdoor {
        name: "trapdoor", station: CraftingTable,
        pattern: shaped!(["###", "###"], '#' => PLANKS),
        output: (Item::Block(BlockType::Trapdoor), 2),
    },
    Ladder {
        name: "ladder", station: CraftingTable,
        pattern: shaped!(["| |", "|||", "| |"], '|' => STICK),
        output: (Item::Block(BlockType::Ladder), 3),
    },
    // a torch under an iron cap
    Lantern {
        name: "lantern", station: CraftingTable,
        pattern: shaped!(
            ["i", "t"],
            'i' => Item::IronIngot, 't' => Item::Block(BlockType::Torch),
        ),
        output: (Item::Block(BlockType::Lantern), 1),
    },
    GlassPane {
        name: "glass_pane", station: CraftingTable,
        pattern: shaped!(["###", "###"], '#' => Item::Block(BlockType::Glass)),
        output: (Item::Block(BlockType::GlassPane), 16),
    },
    DarkPlanks {
        name: "dark_planks", station: CraftingTable,
        pattern: shaped!([" # ", "#c#", " # "], '#' => PLANKS, 'c' => Item::Coal),
        output: (DARK_PLANKS, 4),
    },
    LightPlanks {
        name: "light_planks", station: CraftingTable,
        pattern: shaped!([" # ", "#s#", " # "], '#' => PLANKS, 's' => SAND),
        output: (Item::Block(BlockType::LightPlanks), 4),
    },
    PolishedStone {
        name: "polished_stone", station: CraftingTable,
        pattern: shaped!(["##", "##"], '#' => STONE),
        output: (Item::Block(BlockType::PolishedStone), 4),
    },
    MossyCobblestone {
        name: "mossy_cobblestone", station: CraftingTable,
        pattern: shapeless!(COBBLESTONE, VINES),
        output: (Item::Block(BlockType::MossyCobblestone), 1),
    },
    MossyStoneBricks {
        name: "mossy_stone_bricks", station: CraftingTable,
        pattern: shapeless!(STONE_BRICKS, VINES),
        output: (Item::Block(BlockType::MossyStoneBricks), 1),
    },
    // eight terracotta round the flower that dyes them
    RedTerracotta {
        name: "red_terracotta", station: CraftingTable,
        pattern: shaped!(
            ["###", "#f#", "###"],
            '#' => TERRACOTTA, 'f' => Item::Block(BlockType::Poppy),
        ),
        output: (Item::Block(BlockType::RedTerracotta), 8),
    },
    YellowTerracotta {
        name: "yellow_terracotta", station: CraftingTable,
        pattern: shaped!(
            ["###", "#f#", "###"],
            '#' => TERRACOTTA, 'f' => Item::Block(BlockType::Dandelion),
        ),
        output: (Item::Block(BlockType::YellowTerracotta), 8),
    },
    BlueTerracotta {
        name: "blue_terracotta", station: CraftingTable,
        pattern: shaped!(
            ["###", "#f#", "###"],
            '#' => TERRACOTTA, 'f' => Item::Block(BlockType::Cornflower),
        ),
        output: (Item::Block(BlockType::BlueTerracotta), 8),
    },
    GreenTerracotta {
        name: "green_terracotta", station: CraftingTable,
        pattern: shaped!(["###", "#f#", "###"], '#' => TERRACOTTA, 'f' => TALL_GRASS),
        output: (Item::Block(BlockType::GreenTerracotta), 8),
    },
    Bookshelf {
        name: "bookshelf", station: CraftingTable,
        pattern: shaped!(["###", "www", "###"], '#' => PLANKS, 'w' => Item::Wheat),
        output: (Item::Block(BlockType::Bookshelf), 1),
    },
    // Villages' blocks (M3.4): plaster of sand and lime burnt from stone, and adobe of earth
    // bound with straw, blended in any order, and thatch of straw.
    Plaster {
        name: "plaster", station: CraftingTable,
        pattern: shapeless!(SAND, SAND, SAND, STONE),
        output: (Item::Block(BlockType::Plaster), 4),
    },
    Adobe {
        name: "adobe", station: CraftingTable,
        pattern: shapeless!(DIRT, DIRT, DIRT, Item::Wheat), output: (ADOBE, 4),
    },
    Thatch {
        name: "thatch", station: CraftingTable,
        pattern: shaped!(["##", "##"], '#' => Item::Wheat), output: (THATCH, 1),
    },
    ThatchStairs {
        name: "thatch_stairs", station: CraftingTable,
        pattern: shaped!(["#  ", "## ", "###"], '#' => THATCH),
        output: (Item::Block(BlockType::ThatchStairs), 4),
    },
    ThatchSlab {
        name: "thatch_slab", station: CraftingTable,
        pattern: shaped!(["###"], '#' => THATCH),
        output: (Item::Block(BlockType::ThatchSlab), 6),
    },
    AdobeStairs {
        name: "adobe_stairs", station: CraftingTable,
        pattern: shaped!(["#  ", "## ", "###"], '#' => ADOBE),
        output: (Item::Block(BlockType::AdobeStairs), 4),
    },
    AdobeSlab {
        name: "adobe_slab", station: CraftingTable,
        pattern: shaped!(["###"], '#' => ADOBE),
        output: (Item::Block(BlockType::AdobeSlab), 6),
    },
    Bread {
        name: "bread", station: CraftingTable,
        pattern: shaped!(["###"], '#' => Item::Wheat), output: (Item::Bread, 1),
    },
    // Bright steel is iron blended with something from a far climate, whichever's nearest: gold
    // from under the deserts, frost crystal from under the cold or jade from the jungle (§5.3).
    BrightSteelBlendOfGold {
        name: "bright_steel_blend_of_gold", station: CraftingTable,
        pattern: shapeless!(Item::IronIngot, Item::GoldIngot),
        output: (Item::BrightSteelBlend, 1),
    },
    BrightSteelBlendOfFrostCrystal {
        name: "bright_steel_blend_of_frost_crystal", station: CraftingTable,
        pattern: shapeless!(Item::IronIngot, Item::FrostCrystal),
        output: (Item::BrightSteelBlend, 1),
    },
    BrightSteelBlendOfJade {
        name: "bright_steel_blend_of_jade", station: CraftingTable,
        pattern: shapeless!(Item::IronIngot, Item::Jade),
        output: (Item::BrightSteelBlend, 1),
    },
    GoldBlock {
        name: "gold_block", station: CraftingTable,
        pattern: shaped!(["##", "##"], '#' => Item::GoldIngot),
        output: (Item::Block(BlockType::GoldBlock), 1),
    },
    FrostCrystalBlock {
        name: "frost_crystal_block", station: CraftingTable,
        pattern: shaped!(["##", "##"], '#' => Item::FrostCrystal),
        output: (Item::Block(BlockType::FrostCrystalBlock), 1),
    },
    PolishedJade {
        name: "polished_jade", station: CraftingTable,
        pattern: shaped!(["##", "##"], '#' => Item::Jade),
        output: (Item::Block(BlockType::PolishedJade), 1),
    },
    BrightSteelBlock {
        name: "bright_steel_block", station: CraftingTable,
        pattern: shaped!(["##", "##"], '#' => Item::BrightSteelIngot),
        output: (Item::Block(BlockType::BrightSteelBlock), 1),
    },
    // an ember held in four frost crystals
    EmberLamp {
        name: "ember_lamp", station: CraftingTable,
        pattern: shaped!([" # ", "#e#", " # "], '#' => Item::FrostCrystal, 'e' => Item::Ember),
        output: (Item::Block(BlockType::EmberLamp), 1),
    },
    // Armour is its outline: a helmet's crown and cheeks, a chestplate's shoulders and body,
    // leggings' waist and legs, and a pair of boots.
    LeatherHelmet {
        name: "leather_helmet", station: CraftingTable,
        pattern: shaped!(["###", "# #"], '#' => Item::Leather),
        output: (Item::LeatherHelmet, 1),
    },
    LeatherChestplate {
        name: "leather_chestplate", station: CraftingTable,
        pattern: shaped!(["# #", "###", "###"], '#' => Item::Leather),
        output: (Item::LeatherChestplate, 1),
    },
    LeatherLeggings {
        name: "leather_leggings", station: CraftingTable,
        pattern: shaped!(["###", "# #", "# #"], '#' => Item::Leather),
        output: (Item::LeatherLeggings, 1),
    },
    LeatherBoots {
        name: "leather_boots", station: CraftingTable,
        pattern: shaped!(["# #", "# #"], '#' => Item::Leather),
        output: (Item::LeatherBoots, 1),
    },
    IronHelmet {
        name: "iron_helmet", station: CraftingTable,
        pattern: shaped!(["###", "# #"], '#' => Item::IronIngot),
        output: (Item::IronHelmet, 1),
    },
    IronChestplate {
        name: "iron_chestplate", station: CraftingTable,
        pattern: shaped!(["# #", "###", "###"], '#' => Item::IronIngot),
        output: (Item::IronChestplate, 1),
    },
    IronLeggings {
        name: "iron_leggings", station: CraftingTable,
        pattern: shaped!(["###", "# #", "# #"], '#' => Item::IronIngot),
        output: (Item::IronLeggings, 1),
    },
    IronBoots {
        name: "iron_boots", station: CraftingTable,
        pattern: shaped!(["# #", "# #"], '#' => Item::IronIngot),
        output: (Item::IronBoots, 1),
    },
    BrightSteelHelmet {
        name: "bright_steel_helmet", station: CraftingTable,
        pattern: shaped!(["###", "# #"], '#' => Item::BrightSteelIngot),
        output: (Item::BrightSteelHelmet, 1),
    },
    BrightSteelChestplate {
        name: "bright_steel_chestplate", station: CraftingTable,
        pattern: shaped!(["# #", "###", "###"], '#' => Item::BrightSteelIngot),
        output: (Item::BrightSteelChestplate, 1),
    },
    BrightSteelLeggings {
        name: "bright_steel_leggings", station: CraftingTable,
        pattern: shaped!(["###", "# #", "# #"], '#' => Item::BrightSteelIngot),
        output: (Item::BrightSteelLeggings, 1),
    },
    BrightSteelBoots {
        name: "bright_steel_boots", station: CraftingTable,
        pattern: shaped!(["# #", "# #"], '#' => Item::BrightSteelIngot),
        output: (Item::BrightSteelBoots, 1),
    },
    // A furnace smelts one item into one.
    CopperIngot {
        name: "copper_ingot", station: Furnace,
        pattern: Pattern::Smelted(Item::Block(BlockType::CopperOre)),
        output: (Item::CopperIngot, 1),
    },
    IronIngot {
        name: "iron_ingot", station: Furnace,
        pattern: Pattern::Smelted(Item::Block(BlockType::IronOre)),
        output: (Item::IronIngot, 1),
    },
    Stone {
        name: "stone", station: Furnace,
        pattern: Pattern::Smelted(COBBLESTONE), output: (STONE, 1),
    },
    GoldIngot {
        name: "gold_ingot", station: Furnace,
        pattern: Pattern::Smelted(Item::Block(BlockType::GoldOre)),
        output: (Item::GoldIngot, 1),
    },
    BrightSteelIngot {
        name: "bright_steel_ingot", station: Furnace,
        pattern: Pattern::Smelted(Item::BrightSteelBlend), output: (Item::BrightSteelIngot, 1),
    },
    // sand into glass, stone bricks cracked by the heat, and earth baked into terracotta
    Glass {
        name: "glass", station: Furnace,
        pattern: Pattern::Smelted(SAND), output: (Item::Block(BlockType::Glass), 1),
    },
    CrackedStoneBricks {
        name: "cracked_stone_bricks", station: Furnace,
        pattern: Pattern::Smelted(STONE_BRICKS),
        output: (Item::Block(BlockType::CrackedStoneBricks), 1),
    },
    Terracotta {
        name: "terracotta", station: Furnace,
        pattern: Pattern::Smelted(DIRT), output: (TERRACOTTA, 1),
    },
    CookedMutton {
        name: "cooked_mutton", station: Furnace,
        pattern: Pattern::Smelted(Item::Mutton), output: (Item::CookedMutton, 1),
    },
    CookedBeef {
        name: "cooked_beef", station: Furnace,
        pattern: Pattern::Smelted(Item::Beef), output: (Item::CookedBeef, 1),
    },
    CookedPork {
        name: "cooked_pork", station: Furnace,
        pattern: Pattern::Smelted(Item::Pork), output: (Item::CookedPork, 1),
    },
    CookedChicken {
        name: "cooked_chicken", station: Furnace,
        pattern: Pattern::Smelted(Item::Chicken), output: (Item::CookedChicken, 1),
    },
}

/// Seconds a furnace takes to smelt one item.
pub const SMELT_TIME: f32 = 10.0;

// What burns in a furnace, and how many items each smelts before it's burnt up.
const FUELS: &[(Item, f32)] = &[
    (Item::Coal, 8.0),
    (LOG, 1.5),
    (PLANKS, 1.5),
    (Item::Stick, 0.5),
];

/// How many seconds `item` burns for in a furnace, if it burns.
pub fn burn_time(item: Item) -> Option<f32> {
    let (_, items) = FUELS.iter().find(|&&(fuel, _)| fuel == item)?;
    Some(items * SMELT_TIME)
}

// What each recipe takes, as items and how many of each, in the order its pattern first names
// them, counted once from the patterns.
static INGREDIENTS: LazyLock<Vec<Vec<(Item, u8)>>> = LazyLock::new(|| {
    let count = |recipe: &Recipe| {
        let mut counts: Vec<(Item, u8)> = Vec::new();
        for item in recipe.cells() {
            match counts.iter_mut().find(|(counted, _)| *counted == item) {
                Some((_, count)) => *count += 1,
                None => counts.push((item, 1)),
            }
        }
        counts
    };
    Recipe::ALL.iter().map(count).collect()
});

impl Recipe {
    /// Its stable name, which the protocol sends instead of its discriminant.
    pub fn name(self) -> &'static str {
        self.properties().name
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|recipe| recipe.name() == name)
    }

    /// Where it's made.
    pub fn station(self) -> Station {
        self.properties().station
    }

    /// What it takes and how it's laid out.
    pub fn pattern(self) -> Pattern {
        self.properties().pattern
    }

    /// What it takes, as items and how many of each, in the order its pattern first names them.
    pub fn ingredients(self) -> &'static [(Item, u8)] {
        &INGREDIENTS[self as usize]
    }

    /// What it makes, new, so a tool made is unworn.
    pub fn output(self) -> ItemStack {
        let (item, count) = self.properties().output;
        ItemStack::new(item, count)
    }

    /// The furnace's recipe that smelts `item`, if it smelts.
    pub fn smelting(item: Item) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|recipe| recipe.pattern() == Pattern::Smelted(item))
    }

    // The item in each of its ingredients' cells, along its pattern's rows.
    fn cells(self) -> Vec<Item> {
        match self.pattern() {
            Pattern::Shaped(rows, keys) => rows
                .iter()
                .flat_map(|row| row.chars())
                .filter_map(|key| key_item(keys, key))
                .collect(),
            Pattern::Shapeless(items) => items.to_vec(),
            Pattern::Smelted(item) => vec![item],
        }
    }

    /// How it's laid into the crafting grid of `station`, as the recipe book lays it and shows
    /// it: a shaped pattern in the grid's top left corner, a shapeless one's items along the
    /// rows of the cells used. None if it isn't made there or doesn't fit.
    pub fn layout(self, station: Station) -> Option<[Option<Item>; GRID]> {
        if !station.makes(self.station()) {
            return None;
        }
        let mut cells = [None; GRID];
        match self.pattern() {
            Pattern::Shaped(rows, keys) => {
                let width = station.grid_width();
                if rows.len() > width || rows.iter().any(|row| row.len() > width) {
                    return None;
                }
                for (r, row) in rows.iter().enumerate() {
                    for (c, key) in row.chars().enumerate() {
                        cells[r * GRID_WIDTH + c] = key_item(keys, key);
                    }
                }
            }
            Pattern::Shapeless(items) => {
                let grid = station.grid();
                if items.len() > grid.len() {
                    return None;
                }
                for (&cell, &item) in grid.iter().zip(items) {
                    cells[cell] = Some(item);
                }
            }
            Pattern::Smelted(_) => return None,
        }
        Some(cells)
    }

    /// The recipe the stacks in a crafting grid's cells make at `station`, if they make one: a
    /// shaped one whose pattern, as it is or mirrored, is all that's there, wherever it is in
    /// the grid, or a shapeless one whose items are exactly those there. Anything in a cell the
    /// station doesn't use spoils it.
    pub fn matching(cells: &[Option<ItemStack>; GRID], station: Station) -> Option<Self> {
        let used = station.grid();
        let items: [Option<Item>; GRID] = std::array::from_fn(|i| cells[i].map(|s| s.item));
        if (0..GRID).any(|cell| items[cell].is_some() && !used.contains(&cell)) {
            return None;
        }
        let bounds = bounds(&items)?;
        Self::ALL
            .iter()
            .copied()
            .filter(|recipe| station.makes(recipe.station()))
            .find(|recipe| recipe.matches(&items, bounds))
    }

    // Whether the items in a grid's cells, spanning `bounds`, make it.
    fn matches(self, items: &[Option<Item>; GRID], bounds: Bounds) -> bool {
        match self.pattern() {
            Pattern::Shaped(rows, keys) => {
                let (height, width) = (bounds.bottom - bounds.top, bounds.right - bounds.left);
                if rows.len() != height || rows.iter().any(|row| row.len() != width) {
                    return false;
                }
                let laid =
                    |r: usize, c: usize| items[(bounds.top + r) * GRID_WIDTH + bounds.left + c];
                let wanted = |r: usize, c: usize| key_item(keys, char::from(rows[r].as_bytes()[c]));
                [false, true].into_iter().any(|mirrored| {
                    (0..height).all(|r| {
                        (0..width).all(|c| {
                            let from = if mirrored { width - 1 - c } else { c };
                            laid(r, c) == wanted(r, from)
                        })
                    })
                })
            }
            Pattern::Shapeless(wanted) => {
                let laid: Vec<Item> = items.iter().flatten().copied().collect();
                let count = |of: &[Item], item: Item| of.iter().filter(|&&i| i == item).count();
                laid.len() == wanted.len()
                    && wanted
                        .iter()
                        .all(|&item| count(&laid, item) == count(wanted, item))
            }
            Pattern::Smelted(_) => false,
        }
    }

    /// How many times it can be made from what's in `inventory`, its slots and its crafting
    /// grid together, as the recipe book lays it from both.
    pub fn times_makeable(self, inventory: &Inventory) -> u32 {
        let in_grid = |item: Item| -> u32 {
            let stacks = inventory.grid.iter().flatten();
            stacks
                .filter(|stack| stack.item == item)
                .map(|stack| u32::from(stack.count))
                .sum()
        };
        self.ingredients()
            .iter()
            .map(|&(item, count)| (inventory.count(item) + in_grid(item)) / u32::from(count))
            .min()
            .unwrap_or(0)
    }
}

// The item a pattern's key stands for, or none for a space.
fn key_item(keys: &[(char, Item)], key: char) -> Option<Item> {
    keys.iter()
        .find(|&&(named, _)| named == key)
        .map(|&(_, item)| item)
}

// The rows and columns a grid's items span, the bottom and right past their ends.
#[derive(Clone, Copy)]
struct Bounds {
    top: usize,
    left: usize,
    bottom: usize,
    right: usize,
}

fn bounds(items: &[Option<Item>; GRID]) -> Option<Bounds> {
    let filled = (0..GRID).filter(|&cell| items[cell].is_some());
    let (rows, columns): (Vec<_>, Vec<_>) = filled
        .map(|cell| (cell / GRID_WIDTH, cell % GRID_WIDTH))
        .unzip();
    Some(Bounds {
        top: *rows.iter().min()?,
        left: *columns.iter().min()?,
        bottom: rows.iter().max()? + 1,
        right: columns.iter().max()? + 1,
    })
}

// By name, as blocks and items are.
impl Serialize for Recipe {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for Recipe {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::from_name(&name)
            .ok_or_else(|| serde::de::Error::custom(format!("there's no recipe called {name}")))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{bounds, burn_time, Pattern, Recipe, Station, GRID, GRID_WIDTH, SMELT_TIME};
    use crate::block::BlockType;
    use crate::inventory::Inventory;
    use crate::items::{Item, ItemStack};

    const LOG: Item = Item::Block(BlockType::Log);
    const PLANKS: Item = Item::Block(BlockType::Planks);
    const COBBLESTONE: Item = Item::Block(BlockType::Cobblestone);
    const DIRT: Item = Item::Block(BlockType::Dirt);

    // A grid with one of each item where the rows of keys say, '.' for an empty cell, from the
    // top left.
    fn grid(rows: &[&str], keys: &[(char, Item)]) -> [Option<ItemStack>; GRID] {
        let mut cells = [None; GRID];
        for (r, row) in rows.iter().enumerate() {
            for (c, key) in row.chars().enumerate() {
                let item = keys.iter().find(|(k, _)| *k == key).map(|&(_, item)| item);
                cells[r * GRID_WIDTH + c] = item.map(|item| ItemStack::new(item, 1));
            }
        }
        cells
    }

    // The grid with `recipe` laid in it at `station` as the recipe book lays it, moved `down`
    // and `right` and mirrored if asked, or none if it doesn't fit so.
    fn laid(
        recipe: Recipe,
        station: Station,
        down: usize,
        right: usize,
        mirrored: bool,
    ) -> Option<[Option<ItemStack>; GRID]> {
        let layout = recipe.layout(station)?;
        let width = station.grid_width();
        let mut cells = [None; GRID];
        for (cell, item) in layout.iter().enumerate() {
            let Some(item) = item else { continue };
            let (r, c) = (cell / GRID_WIDTH, cell % GRID_WIDTH);
            let span = super::bounds(&layout).unwrap().right;
            let c = if mirrored { span - 1 - c } else { c };
            let (r, c) = (r + down, c + right);
            if r >= width || c >= width {
                return None;
            }
            cells[r * GRID_WIDTH + c] = Some(ItemStack::new(*item, 1));
        }
        Some(cells)
    }

    #[test]
    fn test_recipes_have_unique_names_real_items_and_sensible_counts() {
        let mut names = HashSet::new();
        for &recipe in Recipe::ALL {
            let name = recipe.name();
            assert!(names.insert(name), "{name} is used twice");
            assert!(name.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
            assert_eq!(Some(recipe), Recipe::from_name(name));
            let output = recipe.output();
            assert_eq!(Some(output.item), Item::from_name(output.item.name()));
            assert!(
                (1..=output.item.max_stack()).contains(&output.count),
                "{name}"
            );
            assert!(!recipe.ingredients().is_empty(), "{name}");
            let mut seen = HashSet::new();
            for &(item, count) in recipe.ingredients() {
                assert!(seen.insert(item), "{name} names {item:?} twice");
                assert!(count > 0 && item != output.item, "{name}");
                assert_eq!(Some(item), Item::from_name(item.name()));
            }
        }
        // and every tool and piece of armour has a recipe
        for item in Item::all().filter(|item| item.durability().is_some()) {
            assert!(
                Recipe::ALL.iter().any(|r| r.output().item == item),
                "{item:?}"
            );
        }
    }

    #[test]
    fn test_patterns_are_tight_use_their_keys_and_fit_their_station() {
        for &recipe in Recipe::ALL {
            let name = recipe.name();
            match (recipe.pattern(), recipe.station()) {
                (Pattern::Smelted(_), station) => assert_eq!(Station::Furnace, station, "{name}"),
                (_, Station::Furnace) => panic!("{name} is crafted but made in a furnace"),
                (Pattern::Shapeless(items), _) => assert!(!items.is_empty(), "{name}"),
                (Pattern::Shaped(rows, keys), _) => {
                    let width = rows[0].len();
                    assert!((1..=3).contains(&rows.len()), "{name}");
                    assert!((1..=3).contains(&width), "{name}");
                    assert!(rows.iter().all(|row| row.len() == width), "{name}");
                    // every key's used, and stands for something
                    for &(key, _) in keys {
                        assert!(key != ' ' && rows.iter().any(|r| r.contains(key)), "{name}");
                    }
                    for key in rows.iter().flat_map(|row| row.chars()) {
                        assert!(key == ' ' || keys.iter().any(|&(k, _)| k == key), "{name}");
                    }
                    // and there's no empty row or column round its edge
                    let cells = grid(rows, keys);
                    let span = bounds(&cells.map(|c| c.map(|s| s.item))).unwrap();
                    assert_eq!((0, 0), (span.top, span.left), "{name}");
                    assert_eq!((rows.len(), width), (span.bottom, span.right), "{name}");
                }
            }
            // hand recipes fit the inventory's 2×2, and a table lays everything crafted
            let station = recipe.station();
            if station != Station::Furnace {
                assert!(recipe.layout(station).is_some(), "{name}");
                assert!(recipe.layout(Station::CraftingTable).is_some(), "{name}");
            }
        }
    }

    #[test]
    fn test_every_recipe_is_made_by_its_pattern_anywhere_and_by_nothing_else() {
        for &recipe in Recipe::ALL {
            if recipe.station() == Station::Furnace {
                continue;
            }
            let stations = match recipe.station() {
                Station::Hand => vec![Station::Hand, Station::CraftingTable],
                _ => vec![Station::CraftingTable],
            };
            for station in stations {
                let mut placed = 0;
                for down in 0..3 {
                    for right in 0..3 {
                        for mirrored in [false, true] {
                            let Some(cells) = laid(recipe, station, down, right, mirrored) else {
                                continue;
                            };
                            placed += 1;
                            let name = recipe.name();
                            assert_eq!(
                                Some(recipe),
                                Recipe::matching(&cells, station),
                                "{name} at {station:?}, {down} down and {right} right"
                            );
                            // and no other recipe's pattern clashes with it
                            let items = cells.map(|c| c.map(|s| s.item));
                            let span = bounds(&items).unwrap();
                            let clashes: Vec<_> = Recipe::ALL
                                .iter()
                                .filter(|&&other| other != recipe && other.matches(&items, span))
                                .collect();
                            assert!(clashes.is_empty(), "{name} clashes with {clashes:?}");
                        }
                    }
                }
                assert!(placed > 0, "{recipe:?}");
            }
        }
    }

    #[test]
    fn test_shapes_match_anywhere_mirrored_but_not_with_anything_else_in_the_grid() {
        let keys = [('#', PLANKS), ('|', Item::Stick), ('d', DIRT)];
        let table = Station::CraftingTable;
        // an axe in the bottom right corner, and mirrored
        let axe = grid(&[".##", ".#|", "..|"], &keys);
        assert_eq!(Some(Recipe::WoodenAxe), Recipe::matching(&axe, table));
        let mirrored = grid(&["##.", "|#.", "|.."], &keys);
        assert_eq!(Some(Recipe::WoodenAxe), Recipe::matching(&mirrored, table));
        // but not upside down, nor with dirt beside it
        let upside_down = grid(&["..|", ".#|", ".##"], &keys);
        assert_eq!(None, Recipe::matching(&upside_down, table));
        let with_dirt = grid(&["d##", ".#|", "..|"], &keys);
        assert_eq!(None, Recipe::matching(&with_dirt, table));
        // a pickaxe doesn't fit the inventory's 2×2, nor is it made by hand
        let pickaxe = grid(&["###", ".|.", ".|."], &keys);
        assert_eq!(
            Some(Recipe::WoodenPickaxe),
            Recipe::matching(&pickaxe, table)
        );
        assert_eq!(None, Recipe::matching(&pickaxe, Station::Hand));
        // sticks are planks one over another, anywhere, but not side by side
        let sticks = grid(&["...", "...", "..#"], &keys);
        assert_eq!(None, Recipe::matching(&sticks, table));
        let sticks = grid(&["", ".#", ".#"], &keys);
        assert_eq!(Some(Recipe::Sticks), Recipe::matching(&sticks, table));
        let sticks = grid(&[".#", ".#"], &keys);
        assert_eq!(
            Some(Recipe::Sticks),
            Recipe::matching(&sticks, Station::Hand)
        );
        assert_eq!(None, Recipe::matching(&grid(&["##"], &keys), Station::Hand));
        // nothing outside the 2×2 counts by hand
        let outside = grid(&["#.d", "#.."], &keys);
        assert_eq!(None, Recipe::matching(&outside, Station::Hand));
        // and an empty grid makes nothing
        assert_eq!(None, Recipe::matching(&[None; GRID], table));
    }

    #[test]
    fn test_shapeless_recipes_take_their_items_in_any_cells_and_only_those() {
        let keys = [
            ('c', COBBLESTONE),
            ('v', Item::Block(BlockType::Vines)),
            ('s', Item::Block(BlockType::Sand)),
            ('t', Item::Block(BlockType::Stone)),
        ];
        let table = Station::CraftingTable;
        for rows in [&["cv"][..], &["v..", "...", "..c"], &[".", "c", "v"]] {
            let cells = grid(rows, &keys);
            assert_eq!(
                Some(Recipe::MossyCobblestone),
                Recipe::matching(&cells, table),
                "{rows:?}"
            );
        }
        // not with two cobblestones, nor without the vines
        assert_eq!(None, Recipe::matching(&grid(&["cvc"], &keys), table));
        assert_eq!(None, Recipe::matching(&grid(&["c"], &keys), table));
        // three sand and a stone, however they're laid, make plaster
        let plaster = grid(&["s.s", ".t.", "s.."], &keys);
        assert_eq!(Some(Recipe::Plaster), Recipe::matching(&plaster, table));
        let short = grid(&["s.s", ".t."], &keys);
        assert_eq!(None, Recipe::matching(&short, table));
        // and a log anywhere makes planks
        let log = grid(&["..", ".l"], &[('l', LOG)]);
        assert_eq!(Some(Recipe::Planks), Recipe::matching(&log, Station::Hand));
    }

    #[test]
    fn test_recipes_are_sent_by_name() {
        let bytes = postcard::to_stdvec(&Recipe::StonePickaxe).unwrap();
        assert_eq!(b"\x0dstone_pickaxe", bytes.as_slice());
        assert_eq!(Recipe::StonePickaxe, postcard::from_bytes(&bytes).unwrap());
        let unknown = postcard::to_stdvec("diamond_pickaxe").unwrap();
        assert!(postcard::from_bytes::<Recipe>(&unknown).is_err());
    }

    #[test]
    fn test_a_crafting_table_makes_what_hands_do_and_more() {
        assert!(Station::Hand.makes(Station::Hand));
        assert!(!Station::Hand.makes(Station::CraftingTable));
        assert!(Station::CraftingTable.makes(Station::Hand));
        assert!(Station::CraftingTable.makes(Station::CraftingTable));
        assert_eq!(
            Some(Station::CraftingTable),
            Station::of_block(BlockType::CraftingTable)
        );
        // smelting is the furnace's, over time, and never crafted
        for station in [Station::Hand, Station::CraftingTable, Station::Furnace] {
            assert!(!station.makes(Station::Furnace), "{station:?}");
        }
        assert!(!Station::Furnace.makes(Station::Hand));
        assert_eq!(&[0, 1, 3, 4], Station::Hand.grid());
        assert_eq!(GRID, Station::CraftingTable.grid().len());
        assert!(Station::Furnace.grid().is_empty());
        assert_eq!(
            Some(Station::Furnace),
            Station::of_block(BlockType::Furnace)
        );
        assert_eq!(None, Station::of_block(BlockType::Stone));
        // a log can be made into a table and the table into a pickaxe
        assert_eq!(Station::Hand, Recipe::Planks.station());
        assert_eq!(Station::Hand, Recipe::CraftingTable.station());
        assert_eq!(Station::CraftingTable, Recipe::WoodenPickaxe.station());
        assert_eq!(Station::Hand, Recipe::Torches.station());
        assert_eq!(
            ItemStack::new(Item::Block(BlockType::Torch), 4),
            Recipe::Torches.output()
        );
        // from the coal coal ore drops
        assert_eq!(
            &[(Item::Coal, 1), (Item::Stick, 1)],
            Recipe::Torches.ingredients()
        );
        assert_eq!(
            vec![ItemStack::new(Item::Coal, 1)],
            BlockType::CoalOre.drops(&mut rand::rng())
        );
    }

    #[test]
    fn test_every_shape_is_made_at_a_table_stairs_six_into_four_and_slabs_three_into_six() {
        use crate::block::RenderStyle;
        let made = |block: BlockType| {
            Recipe::ALL
                .iter()
                .copied()
                .find(|recipe| recipe.output().item == Item::Block(block))
                .unwrap_or_else(|| panic!("nothing makes {}", block.name()))
        };
        for block in BlockType::ALL {
            let (made_of, makes) = match block.style() {
                RenderStyle::Stairs => (6, 4),
                RenderStyle::Slab => (3, 6),
                RenderStyle::Wall => (6, 6),
                _ => continue,
            };
            let recipe = made(block);
            assert_eq!(
                Station::CraftingTable,
                recipe.station(),
                "{}",
                recipe.name()
            );
            assert_eq!(made_of, recipe.ingredients()[0].1, "{}", recipe.name());
            assert_eq!(makes, recipe.output().count, "{}", recipe.name());
        }
        // and every new building block is made somehow, but for what's only for authoring,
        // spawners, found only in dungeons, paths, trodden in villages, and lava and the
        // obsidian it hardens into
        for block in BlockType::ALL
            .iter()
            .skip(BlockType::LootMarker as usize + 1)
            .filter(|&&block| {
                !matches!(
                    block,
                    BlockType::Spawner | BlockType::Path | BlockType::Lava | BlockType::Obsidian
                )
            })
        {
            made(*block);
        }
        assert_eq!(16, made(BlockType::GlassPane).output().count);
        assert_eq!(&[(Item::Stick, 7)], made(BlockType::Ladder).ingredients());
        assert_eq!(
            &[(Item::IronIngot, 1), (Item::Block(BlockType::Torch), 1)],
            made(BlockType::Lantern).ingredients()
        );
        assert_eq!(3, made(BlockType::Door).output().count);
    }

    #[test]
    fn test_furnaces_smelt_ore_into_ingots_and_burn_coal_and_wood() {
        let smelted = |block| Recipe::smelting(Item::Block(block)).map(|r| r.output().item);
        assert_eq!(Some(Item::CopperIngot), smelted(BlockType::CopperOre));
        assert_eq!(Some(Item::IronIngot), smelted(BlockType::IronOre));
        assert_eq!(
            Some(Item::Block(BlockType::Stone)),
            smelted(BlockType::Cobblestone)
        );
        assert_eq!(None, smelted(BlockType::Planks));
        // and sand into glass, stone bricks into cracked ones, and earth into terracotta
        assert_eq!(
            Some(Item::Block(BlockType::Glass)),
            smelted(BlockType::Sand)
        );
        assert_eq!(
            Some(Item::Block(BlockType::CrackedStoneBricks)),
            smelted(BlockType::StoneBricks)
        );
        assert_eq!(
            Some(Item::Block(BlockType::Terracotta)),
            smelted(BlockType::Dirt)
        );
        // a furnace's rows smelt one item into one, and are never laid in a grid
        for &recipe in Recipe::ALL {
            if recipe.station() == Station::Furnace {
                let [(input, 1)] = recipe.ingredients() else {
                    panic!("{recipe:?} smelts more than one item");
                };
                assert_eq!(Some(recipe), Recipe::smelting(*input));
                assert_eq!(1, recipe.output().count);
                assert_eq!(None, recipe.layout(Station::CraftingTable));
            }
        }
        assert_eq!(Some(8.0 * SMELT_TIME), burn_time(Item::Coal));
        assert_eq!(Some(1.5 * SMELT_TIME), burn_time(LOG));
        assert_eq!(Some(0.5 * SMELT_TIME), burn_time(Item::Stick));
        assert_eq!(None, burn_time(Item::Block(BlockType::CoalOre)));
    }

    #[test]
    fn test_times_makeable_counts_the_slots_and_the_grid_and_is_set_by_the_scarcest() {
        let mut inventory = Inventory::default();
        inventory.slots[3] = Some(ItemStack::new(LOG, 5));
        assert_eq!(5, Recipe::Planks.times_makeable(&inventory));
        inventory.slots[4] = Some(ItemStack::full(PLANKS));
        inventory.slots[5] = Some(ItemStack::new(Item::Stick, 5));
        assert_eq!(2, Recipe::WoodenAxe.times_makeable(&inventory));
        assert_eq!(32, Recipe::Sticks.times_makeable(&inventory));
        // what's in the grid counts too
        inventory.grid[8] = Some(ItemStack::new(Item::Stick, 1));
        assert_eq!(3, Recipe::WoodenAxe.times_makeable(&inventory));
        assert_eq!(0, Recipe::Chest.times_makeable(&Inventory::default()));
    }

    #[test]
    fn test_layouts_put_shapes_top_left_and_shapeless_items_along_the_grid() {
        let pickaxe = Recipe::StonePickaxe.layout(Station::CraftingTable).unwrap();
        let (c, s) = (Some(COBBLESTONE), Some(Item::Stick));
        assert_eq!([c, c, c, None, s, None, None, s, None], pickaxe);
        assert_eq!(None, Recipe::StonePickaxe.layout(Station::Hand));
        let vines = Recipe::Vines.layout(Station::Hand).unwrap();
        let grass = Some(Item::Block(BlockType::TallGrass));
        assert_eq!(
            [grass, grass, None, grass],
            [vines[0], vines[1], vines[2], vines[3]]
        );
        let table = Recipe::CraftingTable.layout(Station::Hand).unwrap();
        assert_eq!(4, table.iter().flatten().count());
        assert_eq!(None, table[2]);
    }

    #[test]
    fn test_the_ingredients_are_counted_from_the_patterns() {
        assert_eq!(
            &[(Item::Wheat, 3)],
            Recipe::Bread.ingredients(),
            "bread's a row of wheat"
        );
        assert_eq!(
            &[(COBBLESTONE, 2), (Item::Stick, 2)],
            Recipe::StoneHoe.ingredients()
        );
        assert_eq!(&[(PLANKS, 8)], Recipe::Chest.ingredients());
        assert_eq!(&[(Item::Wool, 3), (PLANKS, 3)], Recipe::Bed.ingredients());
        assert_eq!(&[(Item::IronIngot, 2)], Recipe::Shears.ingredients());
        assert_eq!(&[(Item::IronIngot, 3)], Recipe::Bucket.ingredients());
        assert_eq!(&[(DIRT, 3), (Item::Wheat, 1)], Recipe::Adobe.ingredients());
        assert_eq!(Station::Hand, Recipe::WoolBlock.station());
    }

    #[test]
    fn test_furnaces_cook_every_meat_into_something_more_filling() {
        for (raw, cooked) in [
            (Item::Mutton, Item::CookedMutton),
            (Item::Beef, Item::CookedBeef),
            (Item::Pork, Item::CookedPork),
            (Item::Chicken, Item::CookedChicken),
        ] {
            let recipe = Recipe::smelting(raw).unwrap();
            assert_eq!(ItemStack::new(cooked, 1), recipe.output());
            assert!(
                cooked.fills() > raw.fills() && raw.fills().is_some(),
                "{raw:?}"
            );
        }
        // and leather and feathers are only materials, for now
        for material in [Item::Leather, Item::Feather, Item::Wool] {
            assert_eq!((None, None), (Recipe::smelting(material), material.fills()));
        }
    }

    #[test]
    fn test_swords_and_armour_are_made_at_a_table_from_their_material() {
        assert_eq!(
            &[(PLANKS, 2), (Item::Stick, 1)],
            Recipe::WoodenSword.ingredients()
        );
        // armour is its outline, and takes as many as that has cells
        for (recipe, count) in [
            (Recipe::LeatherHelmet, 5),
            (Recipe::LeatherChestplate, 8),
            (Recipe::LeatherLeggings, 7),
            (Recipe::IronBoots, 4),
        ] {
            assert_eq!(count, recipe.ingredients()[0].1, "{recipe:?}");
        }
        for recipe in [Recipe::BrightSteelSword, Recipe::BrightSteelHelmet] {
            assert_eq!(Station::CraftingTable, recipe.station());
        }
    }
}
