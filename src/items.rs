//! Items (§5.2, INV-6): what's carried in an inventory and lies about the world when it's
//! dropped. Every block that can be placed is an item of its own, and the `items!` table
//! declares the rest.

use serde::{Deserialize, Serialize};

use crate::block::BlockType;

/// How many of an item make a full stack, unless its row says otherwise.
pub const MAX_STACK: u8 = 64;

/// An item's row in the table: how it behaves. Tools (tier, durability, speed) and food (the
/// hunger it fills) join it as columns when they come (M1.5, M1.8).
struct Properties {
    /// How many make a full stack.
    max_stack: u8,
}

// What every block item has: blocks aren't rows of their own, so the table can't fall out of
// step with `blocks!`.
const BLOCK_ITEM: Properties = Properties {
    max_stack: MAX_STACK,
};

// Declares `Item`, with a variant for each row beside the one for blocks, and each row's stable
// name and properties.
macro_rules! items {
    ($($(#[$attr:meta])* $variant:ident {
        name: $name:literal, $($column:ident: $value:expr),* $(,)?
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
                    Item::Block(_) => &BLOCK_ITEM,
                    $(Item::$variant => &Properties { $($column: $value),* },)*
                }
            }
        }
    };
}

// The first rows come with crafting (sticks, tools) and smelting (coal, ingots), each with a
// tile of its own to draw it with.
items! {}

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

    /// The block it places, if it's a block.
    pub fn block(self) -> Option<BlockType> {
        match self {
            Item::Block(block) => Some(block),
        }
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

/// Some of one item, from one up to its full stack.
#[derive(Debug, PartialEq, Eq, Copy, Clone, Serialize, Deserialize)]
#[serde(try_from = "SavedStack")]
pub struct ItemStack {
    pub item: Item,
    pub count: u8,
}

impl ItemStack {
    pub fn new(item: Item, count: u8) -> Self {
        debug_assert!((1..=item.max_stack()).contains(&count), "{count} {item:?}");
        Self { item, count }
    }

    /// A full stack of `item`.
    pub fn full(item: Item) -> Self {
        Self::new(item, item.max_stack())
    }

    /// How many more it can take.
    pub fn room(self) -> u8 {
        self.item.max_stack() - self.count
    }

    /// Moves as much of `other` into it as fits, if it's the same item, and returns what's left
    /// of `other`.
    pub fn merge(&mut self, other: ItemStack) -> Option<ItemStack> {
        if other.item != self.item {
            return Some(other);
        }
        let moved = other.count.min(self.room());
        self.count += moved;
        (moved < other.count).then(|| ItemStack::new(other.item, other.count - moved))
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
    let item = stack.item;
    if stack.count == 0 {
        *slot = None;
    }
    Some(ItemStack::new(item, taken))
}

// How a stack is read, checked to be one.
#[derive(Deserialize)]
struct SavedStack {
    item: Item,
    count: u8,
}

impl TryFrom<SavedStack> for ItemStack {
    type Error = String;

    fn try_from(saved: SavedStack) -> Result<Self, String> {
        let SavedStack { item, count } = saved;
        if !(1..=item.max_stack()).contains(&count) {
            return Err(format!("a stack of {} can't hold {count}", item.name()));
        }
        Ok(Self { item, count })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{take, Item, ItemStack, MAX_STACK};
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
}
