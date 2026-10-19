//! Items (§5.2, INV-6): what's carried in an inventory and lies about the world when it's
//! dropped. Every block that can be placed is an item of its own, and the `items!` table
//! declares the rest.

use serde::{Deserialize, Serialize};

use crate::block::BlockType;

/// How many of an item make a full stack, unless its row says otherwise.
pub const MAX_STACK: u8 = 64;
/// How long a bare hand, or anything that isn't a tool, takes to hit again, in seconds.
pub const HAND_COOLDOWN: f32 = 0.4;

/// An item's row in the table: how it behaves.
struct Properties {
    /// How many make a full stack.
    max_stack: u8,
    /// What it does as a tool, if it's one.
    tool: Option<Tool>,
    /// How much of the food bar eating one fills (§5.5), or 0 if it isn't food.
    fills: u8,
    /// The block it's planted as, if it's planted, as seeds are wheat's first stage.
    plants: Option<BlockType>,
    /// What it does as armour, if it's worn.
    armour: Option<Armour>,
}

// What every block item has, and every row of the table unless it says otherwise: blocks
// aren't rows of their own, so the table can't fall out of step with `blocks!`.
const PLAIN: Properties = Properties {
    max_stack: MAX_STACK,
    tool: None,
    fills: 0,
    plants: None,
    armour: None,
};

/// What a tool is for: each is quick at breaking the blocks whose row names it (M1.2).
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum ToolKind {
    Pickaxe,
    Axe,
    Shovel,
    /// Breaks nothing quicker, but tills dirt and grass into farmland (`farming`).
    Hoe,
    /// Shears sheep (§5.8), and cuts wool quicker.
    Shears,
    /// Hits hardest and quickest (M3.3), and breaks nothing quicker.
    Sword,
}

impl ToolKind {
    // How much harder than a hand it hits, before its tier's added, and how long it takes to
    // swing again, in seconds: a sword hits hard and often, an axe as hard but slower, and the
    // rest are tools first.
    fn blow(self) -> (u8, f32) {
        match self {
            ToolKind::Sword => (3, 0.6),
            ToolKind::Axe => (2, 0.9),
            ToolKind::Pickaxe => (1, 0.7),
            ToolKind::Shovel | ToolKind::Hoe | ToolKind::Shears => (0, 0.7),
        }
    }
}

/// Where a piece of armour is worn, in the order of the inventory's armour slots.
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum Piece {
    Helmet,
    Chestplate,
    Leggings,
    Boots,
}

impl Piece {
    pub const ALL: [Piece; 4] = [
        Piece::Helmet,
        Piece::Chestplate,
        Piece::Leggings,
        Piece::Boots,
    ];

    /// Its slot among the four armour slots.
    pub fn slot(self) -> usize {
        self as usize
    }
}

/// An armour row's columns in the table.
#[derive(Debug, PartialEq, Copy, Clone)]
pub struct Armour {
    pub piece: Piece,
    /// How much of a blow it takes, in points of `ARMOUR_POINT` each.
    pub points: u8,
    /// How many blows it takes before it's worn out.
    pub durability: u16,
    /// What colour it's drawn on a player, in sRGB.
    pub colour: [f32; 3],
}

/// How much of a blow a point of armour takes: 4%, so a full set of bright steel takes 72%.
pub const ARMOUR_POINT: f32 = 0.04;

/// A tool's columns in the table.
#[derive(Debug, PartialEq, Copy, Clone)]
pub struct Tool {
    pub kind: ToolKind,
    /// Its tier (§5.3), from wood (1) to bright steel (5): a block needing a higher one drops
    /// nothing when it's broken with it.
    pub tier: u8,
    /// How many blocks it breaks before it breaks itself.
    pub durability: u16,
    /// How many times quicker than a bare hand it breaks the blocks it's for.
    pub speed: f32,
}

// Each tier's durability and speed, from wood to bright steel. A tier lasts longer and digs
// quicker than the one before, so each is worth the trip to the ore it needs, and bright steel,
// the furthest, by the most.
const TIERS: [(u16, f32); 5] = [(64, 4.0), (128, 6.0), (192, 8.0), (256, 10.0), (384, 13.0)];

// A row's properties as a tool of `kind` in `tier`, from 1.
const fn tool(kind: ToolKind, tier: u8) -> Properties {
    let (durability, speed) = TIERS[tier as usize - 1];
    Properties {
        max_stack: 1,
        tool: Some(Tool {
            kind,
            tier,
            durability,
            speed,
        }),
        ..PLAIN
    }
}

// Shears are of no tier, so hit as a hand does, and wear a use for each sheep shorn or block
// cut.
const SHEARS: Properties = Properties {
    max_stack: 1,
    tool: Some(Tool {
        kind: ToolKind::Shears,
        tier: 0,
        durability: 200,
        speed: 5.0,
    }),
    ..PLAIN
};

// Each material's armour: the points of its helmet, chestplate, leggings and boots, how many
// blows each piece lasts, and its colour. Leather is a first night's, iron halves a blow, and
// bright steel, the furthest, takes nearly three quarters of one.
const LEATHER: ([u8; 4], u16, [f32; 3]) = ([1, 3, 2, 1], 80, [0.52, 0.32, 0.18]);
const IRON: ([u8; 4], u16, [f32; 3]) = ([2, 5, 4, 2], 200, [0.74, 0.74, 0.78]);
const BRIGHT_STEEL: ([u8; 4], u16, [f32; 3]) = ([3, 7, 5, 3], 320, [0.66, 0.76, 0.88]);

// A row's properties as the `piece` of armour of `material`.
const fn armour(piece: Piece, material: ([u8; 4], u16, [f32; 3])) -> Properties {
    let (points, durability, colour) = material;
    Properties {
        max_stack: 1,
        armour: Some(Armour {
            piece,
            points: points[piece as usize],
            durability,
            colour,
        }),
        ..PLAIN
    }
}

// A row's properties as food that fills `fills` of the food bar.
const fn food(fills: u8) -> Properties {
    Properties { fills, ..PLAIN }
}

// A row's properties as what's planted as `crop`.
const fn plants(crop: BlockType) -> Properties {
    Properties {
        plants: Some(crop),
        ..PLAIN
    }
}

// Declares `Item`, with a variant for each row beside the one for blocks, and each row's stable
// name and properties. A row draws the tile in `items.png` at its place in the table.
macro_rules! items {
    ($($(#[$attr:meta])* $variant:ident {
        name: $name:literal $(, $properties:expr)? $(,)?
    },)*) => {
        /// Something to carry, by its kind: a block to place, or one of the items in the table.
        #[derive(Debug, PartialEq, Eq, Copy, Clone, Hash)]
        pub enum Item {
            /// A block, which only tangible blocks are: not air or water.
            Block(BlockType),
            $($(#[$attr])* $variant,)*
        }

        impl Item {
            // The items that aren't blocks, in declaration order.
            const OTHERS: &[Item] = &[$(Item::$variant),*];

            /// Its stable name, which saves store instead of its discriminant (§4.1). A block's
            /// item has the block's name.
            pub fn name(self) -> &'static str {
                match self {
                    Item::Block(block) => block.name(),
                    $(Item::$variant => $name,)*
                }
            }

            fn properties(self) -> &'static Properties {
                match self {
                    Item::Block(_) => &PLAIN,
                    $(Item::$variant => {
                        const PROPERTIES: Properties = items!(@properties $($properties)?);
                        &PROPERTIES
                    })*
                }
            }
        }
    };
    (@properties) => { PLAIN };
    (@properties $properties:expr) => { $properties };
}

use Piece::{Boots, Chestplate, Helmet, Leggings};
use ToolKind::{Axe, Hoe, Pickaxe, Shovel, Sword};

// Coal comes from coal ore, ingots from smelting ore in a furnace, and the tools are made from
// planks, cobblestone and ingots at a crafting table (`crafting`), swords and armour too (M3.3).
// Apples come from leaves, now and then, seeds from tall grass, and wheat from the seeds sown
// (`farming`), which bread is baked from. Meat comes from animals (§5.8) and cooks in a furnace;
// wool from sheep, shorn with shears or killed, and leather and feathers from cows and chickens,
// leather for armour, and feathers, which wait for books and arrows to be of use. Gold, frost crystal and jade come from
// ores under far climates, and any of them blended with iron smelts into bright steel, the best
// tools; embers come from the deepest ore.
items! {
    Stick { name: "stick" },
    Coal { name: "coal" },
    CopperIngot { name: "copper_ingot" },
    IronIngot { name: "iron_ingot" },
    WoodenPickaxe { name: "wooden_pickaxe", tool(Pickaxe, 1) },
    WoodenAxe { name: "wooden_axe", tool(Axe, 1) },
    WoodenShovel { name: "wooden_shovel", tool(Shovel, 1) },
    StonePickaxe { name: "stone_pickaxe", tool(Pickaxe, 2) },
    StoneAxe { name: "stone_axe", tool(Axe, 2) },
    StoneShovel { name: "stone_shovel", tool(Shovel, 2) },
    CopperPickaxe { name: "copper_pickaxe", tool(Pickaxe, 3) },
    CopperAxe { name: "copper_axe", tool(Axe, 3) },
    CopperShovel { name: "copper_shovel", tool(Shovel, 3) },
    IronPickaxe { name: "iron_pickaxe", tool(Pickaxe, 4) },
    IronAxe { name: "iron_axe", tool(Axe, 4) },
    IronShovel { name: "iron_shovel", tool(Shovel, 4) },
    Apple { name: "apple", food(4) },
    WoodenHoe { name: "wooden_hoe", tool(Hoe, 1) },
    StoneHoe { name: "stone_hoe", tool(Hoe, 2) },
    CopperHoe { name: "copper_hoe", tool(Hoe, 3) },
    IronHoe { name: "iron_hoe", tool(Hoe, 4) },
    Seeds { name: "seeds", plants(BlockType::Wheat0) },
    Wheat { name: "wheat" },
    Bread { name: "bread", food(5) },
    Mutton { name: "mutton", food(2) },
    CookedMutton { name: "cooked_mutton", food(6) },
    GoldIngot { name: "gold_ingot" },
    FrostCrystal { name: "frost_crystal" },
    Jade { name: "jade" },
    BrightSteelBlend { name: "bright_steel_blend" },
    BrightSteelIngot { name: "bright_steel_ingot" },
    Ember { name: "ember" },
    BrightSteelPickaxe { name: "bright_steel_pickaxe", tool(Pickaxe, 5) },
    BrightSteelAxe { name: "bright_steel_axe", tool(Axe, 5) },
    BrightSteelShovel { name: "bright_steel_shovel", tool(Shovel, 5) },
    BrightSteelHoe { name: "bright_steel_hoe", tool(Hoe, 5) },
    Wool { name: "wool" },
    Shears { name: "shears", SHEARS },
    Leather { name: "leather" },
    Feather { name: "feather" },
    Beef { name: "beef", food(3) },
    CookedBeef { name: "cooked_beef", food(8) },
    Pork { name: "pork", food(3) },
    CookedPork { name: "cooked_pork", food(8) },
    Chicken { name: "chicken", food(2) },
    CookedChicken { name: "cooked_chicken", food(6) },
    WoodenSword { name: "wooden_sword", tool(Sword, 1) },
    StoneSword { name: "stone_sword", tool(Sword, 2) },
    CopperSword { name: "copper_sword", tool(Sword, 3) },
    IronSword { name: "iron_sword", tool(Sword, 4) },
    BrightSteelSword { name: "bright_steel_sword", tool(Sword, 5) },
    LeatherHelmet { name: "leather_helmet", armour(Helmet, LEATHER) },
    LeatherChestplate { name: "leather_chestplate", armour(Chestplate, LEATHER) },
    LeatherLeggings { name: "leather_leggings", armour(Leggings, LEATHER) },
    LeatherBoots { name: "leather_boots", armour(Boots, LEATHER) },
    IronHelmet { name: "iron_helmet", armour(Helmet, IRON) },
    IronChestplate { name: "iron_chestplate", armour(Chestplate, IRON) },
    IronLeggings { name: "iron_leggings", armour(Leggings, IRON) },
    IronBoots { name: "iron_boots", armour(Boots, IRON) },
    BrightSteelHelmet { name: "bright_steel_helmet", armour(Helmet, BRIGHT_STEEL) },
    BrightSteelChestplate { name: "bright_steel_chestplate", armour(Chestplate, BRIGHT_STEEL) },
    BrightSteelLeggings { name: "bright_steel_leggings", armour(Leggings, BRIGHT_STEEL) },
    BrightSteelBoots { name: "bright_steel_boots", armour(Boots, BRIGHT_STEEL) },
    // Villages' money (M3.4), which only trading with villagers gives and takes (`trading`).
    Coin { name: "coin" },
}

impl Item {
    /// Every item: the blocks' first, in their order, then the table's.
    pub fn all() -> impl Iterator<Item = Item> {
        let blocks = BlockType::ALL.into_iter().filter(BlockType::is_tangible);
        blocks.map(Item::Block).chain(Self::OTHERS.iter().copied())
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::all().find(|item| item.name() == name)
    }

    /// Its name for people: "stone_bricks" as "Stone bricks".
    pub fn display_name(self) -> String {
        let mut name = self.name().replace('_', " ");
        name[..1].make_ascii_uppercase();
        name
    }

    pub fn max_stack(self) -> u8 {
        self.properties().max_stack
    }

    /// What it does as a tool, if it's one.
    pub fn tool(self) -> Option<Tool> {
        self.properties().tool
    }

    /// How much hitting a creature with it hurts, in half hearts: a bare hand's or anything
    /// else's 1, and a tool's more the better its tier, a sword's most.
    pub fn damage(self) -> u8 {
        self.tool()
            .map_or(1, |tool| (tool.kind.blow().0 + tool.tier).max(1))
    }

    /// How long a hit with it takes to swing again, in seconds.
    pub fn cooldown(self) -> f32 {
        self.tool().map_or(HAND_COOLDOWN, |tool| tool.kind.blow().1)
    }

    /// What it does as armour, if it's worn.
    pub fn armour(self) -> Option<Armour> {
        self.properties().armour
    }

    /// How many uses it has before it's worn out, if it wears: a tool's blocks or blows, and
    /// armour's blows taken.
    pub fn durability(self) -> Option<u16> {
        let tool = self.tool().map(|tool| tool.durability);
        tool.or(self.armour().map(|armour| armour.durability))
    }

    /// How much of the food bar eating one fills, if it's food.
    pub fn fills(self) -> Option<u8> {
        Some(self.properties().fills).filter(|&fills| fills > 0)
    }

    /// The block it places, if it's a block.
    pub fn block(self) -> Option<BlockType> {
        match self {
            Item::Block(block) => Some(block),
            _ => None,
        }
    }

    /// The block it puts in the world, if any: a block item's block, and what seeds are planted
    /// as.
    pub fn places(self) -> Option<BlockType> {
        self.block().or(self.properties().plants)
    }

    /// Which tile of `items.png` draws it, if it isn't a block, which its block's tiles draw.
    pub fn sprite(self) -> Option<usize> {
        Self::OTHERS.iter().position(|&other| other == self)
    }

    /// How many of the table's items there are, and so tiles in `items.png`.
    pub fn sprites() -> usize {
        Self::OTHERS.len()
    }
}

// By name, as blocks are.
impl Serialize for Item {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for Item {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::from_name(&name)
            .ok_or_else(|| serde::de::Error::custom(format!("there's no item called {name}")))
    }
}

/// Some of one item, from one up to its full stack. A tool comes one to a stack, and the stack
/// keeps how worn it is, so a tool put down or dropped is as worn when it's picked up again.
#[derive(Debug, PartialEq, Eq, Copy, Clone, Deserialize)]
#[serde(try_from = "SavedStack")]
pub struct ItemStack {
    pub item: Item,
    pub count: u8,
    /// How many uses a tool or armour has had, of its durability. Nothing else wears.
    pub wear: u16,
}

// Saved with its wear left out when there's none, as most stacks have, and sent with it always,
// as postcard reads each field in turn and can't tell one's missing.
impl Serialize for ItemStack {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let worn = self.wear > 0 || !serializer.is_human_readable();
        let mut stack = serializer.serialize_struct("ItemStack", 2 + usize::from(worn))?;
        stack.serialize_field("item", &self.item)?;
        stack.serialize_field("count", &self.count)?;
        if worn {
            stack.serialize_field("wear", &self.wear)?;
        }
        stack.end()
    }
}

impl ItemStack {
    pub fn new(item: Item, count: u8) -> Self {
        debug_assert!((1..=item.max_stack()).contains(&count), "{count} {item:?}");
        Self {
            item,
            count,
            wear: 0,
        }
    }

    /// A full stack of `item`.
    pub fn full(item: Item) -> Self {
        Self::new(item, item.max_stack())
    }

    /// How many more it can take.
    pub fn room(self) -> u8 {
        self.item.max_stack() - self.count
    }

    /// Whether `other` can go on top of it: it's the same item, and that item stacks. Two
    /// tools never do, however alike.
    pub fn stacks_with(self, other: ItemStack) -> bool {
        self.item == other.item && self.item.max_stack() > 1
    }

    /// Moves as much of `other` into it as fits, if they stack, and returns what's left of
    /// `other`.
    pub fn merge(&mut self, other: ItemStack) -> Option<ItemStack> {
        if !self.stacks_with(other) {
            return Some(other);
        }
        let moved = other.count.min(self.room());
        self.count += moved;
        (moved < other.count).then(|| ItemStack::new(other.item, other.count - moved))
    }

    /// A tool after breaking a block or hitting with it, or armour after a blow, or nothing if
    /// that wore it out. Anything else doesn't wear.
    pub fn worn(self) -> Option<Self> {
        let Some(durability) = self.item.durability() else {
            return Some(self);
        };
        let wear = self.wear + 1;
        (wear < durability).then_some(Self { wear, ..self })
    }

    /// How much of a worn tool's or armour's durability is left, from 1 when new down towards
    /// 0, or none if it doesn't wear or is new, as only a worn one shows it.
    pub fn durability_left(self) -> Option<f32> {
        let durability = self.item.durability().filter(|_| self.wear > 0)?;
        Some(1.0 - f32::from(self.wear) / f32::from(durability))
    }
}

/// Takes up to `count` off the stack in `slot`, leaving it empty if that's all of it.
pub fn take(slot: &mut Option<ItemStack>, count: u8) -> Option<ItemStack> {
    let stack = slot.as_mut()?;
    let taken = count.min(stack.count);
    if taken == 0 {
        return None;
    }
    stack.count -= taken;
    let taken = ItemStack {
        count: taken,
        ..*stack
    };
    if stack.count == 0 {
        *slot = None;
    }
    Some(taken)
}

// How a stack is read, checked to be one.
#[derive(Deserialize)]
struct SavedStack {
    item: Item,
    count: u8,
    #[serde(default)]
    wear: u16,
}

impl TryFrom<SavedStack> for ItemStack {
    type Error = String;

    fn try_from(saved: SavedStack) -> Result<Self, String> {
        let SavedStack { item, count, wear } = saved;
        if !(1..=item.max_stack()).contains(&count) {
            return Err(format!("a stack of {} can't hold {count}", item.name()));
        }
        let durability = item.durability().unwrap_or(0);
        if wear > 0 && wear >= durability {
            return Err(format!("{} can't be worn {wear}", item.name()));
        }
        Ok(Self { item, count, wear })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use rand::rngs::StdRng;
    use rand::SeedableRng;

    use super::{take, Item, ItemStack, ToolKind, MAX_STACK};
    use crate::block::BlockType;

    #[test]
    fn test_item_names_are_unique_snake_case_and_round_trip() {
        let mut names = HashSet::new();
        for item in Item::all() {
            let name = item.name();
            assert!(names.insert(name), "{name} is used twice");
            assert!(name.starts_with(|c: char| c.is_ascii_lowercase()));
            assert!(name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'));
            assert_eq!(Some(item), Item::from_name(name));
        }
    }

    #[test]
    fn test_display_names() {
        assert_eq!("Stone", Item::Block(BlockType::Stone).display_name());
        assert_eq!(
            "Crafting table",
            Item::Block(BlockType::CraftingTable).display_name()
        );
    }

    #[test]
    fn test_every_tangible_block_and_nothing_else_is_an_item() {
        for block in BlockType::ALL {
            let item = Item::from_name(block.name());
            assert_eq!(block.is_tangible(), item.is_some(), "{block:?}");
            if let Some(item) = item {
                assert_eq!(Some(block), item.block());
                assert_eq!(MAX_STACK, item.max_stack());
            }
        }
    }

    #[test]
    fn test_blocks_drop_themselves_or_what_their_row_says() {
        let mut rng = StdRng::seed_from_u64(1);
        let mut drop = |block: BlockType| block.drops(&mut rng);
        let block = |block| vec![ItemStack::new(Item::Block(block), 1)];
        assert_eq!(block(BlockType::Dirt), drop(BlockType::Grass));
        assert_eq!(block(BlockType::Dirt), drop(BlockType::Dirt));
        assert_eq!(block(BlockType::Dirt), drop(BlockType::Farmland));
        assert_eq!(block(BlockType::Cobblestone), drop(BlockType::Stone));
        assert_eq!(block(BlockType::IronOre), drop(BlockType::IronOre));
        assert_eq!(block(BlockType::Log), drop(BlockType::Log));
        assert_eq!(
            vec![ItemStack::new(Item::Coal, 1)],
            drop(BlockType::CoalOre)
        );
        for nothing in [BlockType::Air, BlockType::Water] {
            assert_eq!(Vec::<ItemStack>::new(), drop(nothing), "{nothing:?}");
        }
        // flowers and ferns are picked
        for plant in [
            BlockType::Poppy,
            BlockType::Dandelion,
            BlockType::Cornflower,
            BlockType::Fern,
        ] {
            assert_eq!(block(plant), drop(plant), "{plant:?}");
        }
        // and every drop is an item there is
        for block in BlockType::ALL {
            for stack in (0..100).flat_map(|_| drop(block)) {
                assert_eq!(Some(stack.item), Item::from_name(stack.item.name()));
            }
        }
    }

    // How many of each item breaking `block` a thousand times leaves.
    fn drops_of(block: BlockType) -> Vec<(Item, u32)> {
        let mut rng = StdRng::seed_from_u64(3);
        let mut counts: Vec<(Item, u32)> = Vec::new();
        for stack in (0..1000).flat_map(|_| block.drops(&mut rng)) {
            match counts.iter_mut().find(|(item, _)| *item == stack.item) {
                Some((_, count)) => *count += u32::from(stack.count),
                None => counts.push((stack.item, stack.count.into())),
            }
        }
        counts
    }

    #[test]
    fn test_tall_grass_drops_itself_and_a_seed_one_time_in_eight() {
        let [(grass, grasses), (seeds, sown)] = drops_of(BlockType::TallGrass)[..] else {
            panic!("tall grass drops more than grass and seeds");
        };
        assert_eq!((Item::Block(BlockType::TallGrass), 1000), (grass, grasses));
        assert_eq!(Item::Seeds, seeds);
        assert!((90..160).contains(&sown), "{sown} seeds");
    }

    #[test]
    fn test_short_and_wild_grass_drop_themselves_and_seeds_less_and_more_often() {
        let seeds = |grass| {
            let drops = drops_of(grass);
            assert_eq!((Item::Block(grass), 1000), drops[0], "{grass:?}");
            assert_eq!(Item::Seeds, drops[1].0, "{grass:?}");
            drops[1].1
        };
        let (short, tall, wild) = (
            seeds(BlockType::ShortGrass),
            seeds(BlockType::TallGrass),
            seeds(BlockType::WildGrass),
        );
        assert!((50..120).contains(&short), "{short} seeds from short grass");
        assert!((200..300).contains(&wild), "{wild} seeds from wild grass");
        assert!(short < tall && tall < wild);
    }

    #[test]
    fn test_wheat_drops_its_seed_until_its_ripe_and_then_wheat_and_seeds() {
        for unripe in &crate::farming::WHEAT[..crate::farming::WHEAT.len() - 1] {
            assert_eq!(vec![(Item::Seeds, 1000)], drops_of(*unripe), "{unripe:?}");
        }
        let [(wheat, reaped), (seeds, sown)] = drops_of(BlockType::RipeWheat)[..] else {
            panic!("ripe wheat drops more than wheat and seeds");
        };
        assert_eq!((Item::Wheat, 1000), (wheat, reaped));
        // one to three a time, two on average
        assert_eq!(Item::Seeds, seeds);
        assert!((1900..2100).contains(&sown), "{sown} seeds");
        let mut rng = StdRng::seed_from_u64(4);
        for _ in 0..100 {
            let seeds = BlockType::RipeWheat.drops(&mut rng)[1].count;
            assert!((1..=3).contains(&seeds));
        }
    }

    #[test]
    fn test_leaves_drop_an_apple_one_time_in_twenty() {
        let mut rng = StdRng::seed_from_u64(7);
        let drops: Vec<_> = (0..2000)
            .map(|_| BlockType::Leaves.drops(&mut rng))
            .collect();
        let apple = vec![ItemStack::new(Item::Apple, 1)];
        let apples = drops.iter().filter(|&drop| *drop == apple).count();
        assert!((70..130).contains(&apples), "{apples} apples");
        assert!(drops.iter().all(|drop| drop.is_empty() || *drop == apple));
        assert_eq!(Some(4), Item::Apple.fills());
        assert_eq!(Some(5), Item::Bread.fills());
        assert_eq!(None, Item::Wheat.fills());
        assert_eq!(None, Item::Stick.fills());
        assert_eq!(None, Item::Block(BlockType::Leaves).fills());
    }

    #[test]
    fn test_stacks_merge_up_to_a_full_stack() {
        let dirt = Item::Block(BlockType::Dirt);
        let mut stack = ItemStack::new(dirt, 60);
        assert_eq!(
            Some(ItemStack::new(dirt, 6)),
            stack.merge(ItemStack::new(dirt, 10))
        );
        assert_eq!(ItemStack::full(dirt), stack);
        let sand = ItemStack::new(Item::Block(BlockType::Sand), 1);
        let mut small = ItemStack::new(dirt, 1);
        assert_eq!(Some(sand), small.merge(sand));
        assert_eq!(None, small.merge(ItemStack::new(dirt, 2)));
        assert_eq!(3, small.count);
    }

    #[test]
    fn test_taking_from_a_slot_empties_it_at_the_last() {
        let dirt = Item::Block(BlockType::Dirt);
        let mut slot = Some(ItemStack::new(dirt, 3));
        assert_eq!(Some(ItemStack::new(dirt, 2)), take(&mut slot, 2));
        assert_eq!(Some(ItemStack::new(dirt, 1)), take(&mut slot, 5));
        assert_eq!(None, slot);
        assert_eq!(None, take(&mut slot, 1));
    }

    #[test]
    fn test_stacks_are_saved_by_name_and_checked() {
        let stack = ItemStack::new(Item::Block(BlockType::CoalOre), 12);
        let text = toml::to_string(&stack).unwrap();
        assert_eq!("item = \"coal_ore\"\ncount = 12\n", text);
        assert_eq!(stack, toml::from_str(&text).unwrap());
        for bad in [
            "item = \"coal_ore\"\ncount = 65",
            "item = \"coal_ore\"\ncount = 0",
        ] {
            assert!(toml::from_str::<ItemStack>(bad).is_err(), "{bad}");
        }
        assert!(toml::from_str::<ItemStack>("item = \"water\"\ncount = 1").is_err());
    }

    #[test]
    fn test_tools_come_one_to_a_stack_and_get_better_by_tier() {
        let pickaxes = [
            Item::WoodenPickaxe,
            Item::StonePickaxe,
            Item::CopperPickaxe,
            Item::IronPickaxe,
            Item::BrightSteelPickaxe,
        ];
        for (tier, pair) in (1..).zip(pickaxes.windows(2)) {
            let (worse, better) = (pair[0].tool().unwrap(), pair[1].tool().unwrap());
            assert_eq!((tier, tier + 1), (worse.tier, better.tier));
            assert!(better.durability > worse.durability && better.speed > worse.speed);
        }
        for item in Item::all() {
            match item.durability() {
                Some(_) => assert_eq!(1, item.max_stack(), "{item:?}"),
                None => assert_eq!(MAX_STACK, item.max_stack(), "{item:?}"),
            }
        }
        assert_eq!(ToolKind::Shovel, Item::IronShovel.tool().unwrap().kind);
        assert_eq!(None, Item::Stick.tool());
        assert_eq!(None, Item::Stick.block());
    }

    #[test]
    fn test_only_the_tables_items_have_sprites_in_order() {
        assert_eq!(None, Item::Block(BlockType::Stone).sprite());
        assert_eq!(Some(0), Item::Stick.sprite());
        assert_eq!(Some(Item::sprites() - 1), Item::Coin.sprite());
    }

    #[test]
    fn test_seeds_plant_wheat_and_blocks_place_themselves() {
        assert_eq!(Some(BlockType::Wheat0), Item::Seeds.places());
        assert_eq!(None, Item::Seeds.block());
        assert_eq!(
            Some(BlockType::Farmland),
            Item::Block(BlockType::Farmland).places()
        );
        for nothing in [Item::Wheat, Item::Bread, Item::WoodenHoe] {
            assert_eq!(None, nothing.places(), "{nothing:?}");
        }
        assert_eq!(ToolKind::Hoe, Item::CopperHoe.tool().unwrap().kind);
    }

    #[test]
    fn test_tools_never_stack_and_keep_their_wear() {
        let mut worn = ItemStack::new(Item::StoneAxe, 1);
        worn.wear = 5;
        let new = ItemStack::new(Item::StoneAxe, 1);
        assert!(!worn.stacks_with(new));
        assert_eq!(Some(new), worn.merge(new));
        assert_eq!(Some(1.0 - 5.0 / 128.0), worn.durability_left());
        assert_eq!(None, new.durability_left());
        // taking a tool from its slot takes its wear along with it
        let mut slot = Some(worn);
        assert_eq!(Some(worn), take(&mut slot, 1));
    }

    #[test]
    fn test_a_tools_wear_is_saved_and_checked() {
        let mut stack = ItemStack::new(Item::IronPickaxe, 1);
        stack.wear = 17;
        let text = toml::to_string(&stack).unwrap();
        assert_eq!("item = \"iron_pickaxe\"\ncount = 1\nwear = 17\n", text);
        assert_eq!(stack, toml::from_str(&text).unwrap());
        // and sent, worn or not
        for sent in [stack, ItemStack::new(Item::Stick, 3)] {
            let bytes = postcard::to_stdvec(&sent).unwrap();
            assert_eq!(sent, postcard::from_bytes(&bytes).unwrap());
        }
        for bad in [
            "item = \"iron_pickaxe\"\ncount = 1\nwear = 256",
            "item = \"iron_pickaxe\"\ncount = 2",
            "item = \"stick\"\ncount = 1\nwear = 1",
        ] {
            assert!(toml::from_str::<ItemStack>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn test_swords_hit_hardest_and_quickest_and_get_better_by_tier() {
        let swords = [
            Item::WoodenSword,
            Item::StoneSword,
            Item::CopperSword,
            Item::IronSword,
            Item::BrightSteelSword,
        ];
        assert_eq!([4, 5, 6, 7, 8], swords.map(Item::damage));
        // an axe as hard as a sword a tier below, and slower, and a hand or a stick hits for 1
        assert_eq!(6, Item::IronAxe.damage());
        assert!(Item::IronAxe.cooldown() > Item::IronSword.cooldown());
        assert_eq!((1, 1), (Item::Stick.damage(), Item::Shears.damage()));
        assert_eq!(1, Item::Block(BlockType::Dirt).damage());
        assert_eq!(ToolKind::Sword, Item::StoneSword.tool().unwrap().kind);
    }

    #[test]
    fn test_armour_is_worn_in_its_piece_better_by_material_and_wears() {
        use super::{Piece, ARMOUR_POINT};
        let sets = [
            [
                Item::LeatherHelmet,
                Item::LeatherChestplate,
                Item::LeatherLeggings,
                Item::LeatherBoots,
            ],
            [
                Item::IronHelmet,
                Item::IronChestplate,
                Item::IronLeggings,
                Item::IronBoots,
            ],
            [
                Item::BrightSteelHelmet,
                Item::BrightSteelChestplate,
                Item::BrightSteelLeggings,
                Item::BrightSteelBoots,
            ],
        ];
        let mut last = 0;
        for set in sets {
            for (item, piece) in set.into_iter().zip(Piece::ALL) {
                assert_eq!(piece, item.armour().unwrap().piece, "{item:?}");
                assert_eq!(1, item.max_stack());
            }
            let points: u8 = set.iter().map(|item| item.armour().unwrap().points).sum();
            assert!(
                points > last && f32::from(points) * ARMOUR_POINT < 0.75,
                "{points}"
            );
            last = points;
        }
        assert_eq!(None, Item::IronSword.armour());
        // a piece wears out after as many blows as it lasts, as a tool does
        let mut boots = Some(ItemStack::new(Item::LeatherBoots, 1));
        for _ in 0..80 {
            boots = boots.and_then(ItemStack::worn);
        }
        assert_eq!(None, boots);
    }
}
