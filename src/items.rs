//! Items (§5.2, INV-6): what's carried in an inventory and lies about the world when it's
//! dropped. Every block that can be placed is an item of its own, and the `items!` table
//! declares the rest.

use serde::{Deserialize, Serialize};

use crate::block::BlockType;

/// How many of an item make a full stack, unless its row says otherwise.
pub const MAX_STACK: u8 = 64;

/// An item's row in the table: how it behaves. Food (the hunger it fills) joins it as a column
/// when it comes (M1.8).
struct Properties {
    /// How many make a full stack.
    max_stack: u8,
    /// What it does as a tool, if it's one.
    tool: Option<Tool>,
}

// What every block item has, and every row of the table unless it says otherwise: blocks
// aren't rows of their own, so the table can't fall out of step with `blocks!`.
const PLAIN: Properties = Properties {
    max_stack: MAX_STACK,
    tool: None,
};

/// What a tool is for: each is quick at breaking the blocks whose row names it (M1.2).
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub enum ToolKind {
    Pickaxe,
    Axe,
    Shovel,
}

/// A tool's columns in the table.
#[derive(Debug, PartialEq, Copy, Clone)]
pub struct Tool {
    pub kind: ToolKind,
    /// Its tier (§5.3), from wood (1) to iron (4): a block needing a higher one drops nothing
    /// when it's broken with it.
    pub tier: u8,
    /// How many blocks it breaks before it breaks itself.
    pub durability: u16,
    /// How many times quicker than a bare hand it breaks the blocks it's for.
    pub speed: f32,
}

// Each tier's durability and speed, from wood to iron. A tier lasts longer and digs quicker than
// the one before, so each is worth the trip to the ore it needs.
const TIERS: [(u16, f32); 4] = [(64, 4.0), (128, 6.0), (192, 8.0), (256, 10.0)];

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

use ToolKind::{Axe, Pickaxe, Shovel};

// Ingots come from smelting (M1.6), and the tools are made from planks, cobblestone and ingots
// at a crafting table (`crafting`). Swords wait for combat (M3.3).
items! {
    Stick { name: "stick" },
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

    /// The block it places, if it's a block.
    pub fn block(self) -> Option<BlockType> {
        match self {
            Item::Block(block) => Some(block),
            _ => None,
        }
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
    /// How many blocks a tool has broken, of its durability. Nothing else wears.
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

    /// A tool after breaking a block with it, or nothing if that wore it out. Anything else
    /// doesn't wear.
    pub fn worn(self) -> Option<Self> {
        let Some(tool) = self.item.tool() else {
            return Some(self);
        };
        let wear = self.wear + 1;
        (wear < tool.durability).then_some(Self { wear, ..self })
    }

    /// How much of a worn tool's durability is left, from 1 when new down towards 0, or none
    /// if it isn't a tool or is new, as only a worn one shows it.
    pub fn durability_left(self) -> Option<f32> {
        let tool = self.item.tool().filter(|_| self.wear > 0)?;
        Some(1.0 - f32::from(self.wear) / f32::from(tool.durability))
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
        let durability = item.tool().map_or(0, |tool| tool.durability);
        if wear > 0 && wear >= durability {
            return Err(format!("{} can't be worn {wear}", item.name()));
        }
        Ok(Self { item, count, wear })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{take, Item, ItemStack, ToolKind, MAX_STACK};
    use crate::block::BlockType;

    #[test]
    fn test_item_names_are_unique_snake_case_and_round_trip() {
        let mut names = HashSet::new();
        for item in Item::all() {
            let name = item.name();
            assert!(names.insert(name), "{name} is used twice");
            assert!(name.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
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
        let drop = |block: BlockType| block.drops().and_then(Item::block);
        assert_eq!(Some(BlockType::Dirt), drop(BlockType::Grass));
        assert_eq!(Some(BlockType::Dirt), drop(BlockType::Dirt));
        assert_eq!(Some(BlockType::Cobblestone), drop(BlockType::Stone));
        assert_eq!(Some(BlockType::IronOre), drop(BlockType::IronOre));
        assert_eq!(Some(BlockType::Log), drop(BlockType::Log));
        for nothing in [
            BlockType::Air,
            BlockType::Water,
            BlockType::Leaves,
            BlockType::TallGrass,
            BlockType::Poppy,
        ] {
            assert_eq!(None, drop(nothing), "{nothing:?}");
        }
        // and every drop is an item there is
        for block in BlockType::ALL {
            if let Some(item) = block.drops() {
                assert_eq!(Some(item), Item::from_name(item.name()));
            }
        }
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
        ];
        for (tier, pair) in (1..).zip(pickaxes.windows(2)) {
            let (worse, better) = (pair[0].tool().unwrap(), pair[1].tool().unwrap());
            assert_eq!((tier, tier + 1), (worse.tier, better.tier));
            assert!(better.durability > worse.durability && better.speed > worse.speed);
        }
        for item in Item::all() {
            match item.tool() {
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
        assert_eq!(Some(Item::sprites() - 1), Item::IronShovel.sprite());
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
}
