//! Crafting (§5.3, D-4): a recipe turns a list of ingredients into an output, made at a station,
//! by hand or at a crafting table. Every recipe is a row of the `recipes!` table (INV-6). The
//! server crafts (`server::crafting`), as clients ask, and the inventory screen lists the
//! recipes (`inventory::recipes`).

use serde::{Deserialize, Serialize};

use crate::block::BlockType;
use crate::inventory::Inventory;
use crate::items::{Item, ItemStack};

/// Where a recipe is made.
#[derive(Debug, Default, PartialEq, Eq, Copy, Clone, Hash)]
pub enum Station {
    /// Anywhere, in the inventory screen.
    #[default]
    Hand,
    /// At a crafting table, opened by right-clicking it.
    CraftingTable,
}

impl Station {
    /// The station a block is, if it's one.
    pub fn of_block(block: BlockType) -> Option<Self> {
        (block == BlockType::CraftingTable).then_some(Station::CraftingTable)
    }

    /// Whether a recipe made at `station` can be made here: a crafting table makes what hands
    /// can too.
    pub fn makes(self, station: Station) -> bool {
        self == station || self == Station::CraftingTable
    }
}

/// A recipe's row in the table.
struct Properties {
    /// Its stable name, which the protocol sends.
    name: &'static str,
    station: Station,
    ingredients: &'static [(Item, u8)],
    output: (Item, u8),
}

// Declares `Recipe`, a variant for each row, and the rows' properties.
macro_rules! recipes {
    ($($variant:ident {
        name: $name:literal,
        station: $station:ident,
        ingredients: [$(($ingredient:expr, $count:literal)),* $(,)?],
        output: ($output:expr, $made:literal) $(,)?
    },)*) => {
        /// A way to make something.
        #[derive(Debug, PartialEq, Eq, Copy, Clone, Hash)]
        pub enum Recipe {
            $($variant,)*
        }

        impl Recipe {
            /// Every recipe, in the order the inventory screen lists them.
            pub const ALL: &[Recipe] = &[$(Recipe::$variant),*];

            fn properties(self) -> &'static Properties {
                match self {
                    $(Recipe::$variant => &Properties {
                        name: $name,
                        station: Station::$station,
                        ingredients: &[$(($ingredient, $count)),*],
                        output: ($output, $made),
                    },)*
                }
            }
        }
    };
}

const LOG: Item = Item::Block(BlockType::Log);
const PLANKS: Item = Item::Block(BlockType::Planks);
const COBBLESTONE: Item = Item::Block(BlockType::Cobblestone);
const SAND: Item = Item::Block(BlockType::Sand);

// A tool takes three of its head's material and two sticks, or one and two for a shovel. Coal ore
// drops itself for now, so torches are made from it until there's a coal item to make them from.
// Smelting at a furnace comes with M1.6.
recipes! {
    Planks {
        name: "planks", station: Hand,
        ingredients: [(LOG, 1)], output: (PLANKS, 4),
    },
    Sticks {
        name: "sticks", station: Hand,
        ingredients: [(PLANKS, 2)], output: (Item::Stick, 4),
    },
    Torches {
        name: "torches", station: Hand,
        ingredients: [(Item::Block(BlockType::CoalOre), 1), (Item::Stick, 1)],
        output: (Item::Block(BlockType::Torch), 4),
    },
    CraftingTable {
        name: "crafting_table", station: Hand,
        ingredients: [(PLANKS, 4)], output: (Item::Block(BlockType::CraftingTable), 1),
    },
    WoodenPickaxe {
        name: "wooden_pickaxe", station: CraftingTable,
        ingredients: [(PLANKS, 3), (Item::Stick, 2)], output: (Item::WoodenPickaxe, 1),
    },
    WoodenAxe {
        name: "wooden_axe", station: CraftingTable,
        ingredients: [(PLANKS, 3), (Item::Stick, 2)], output: (Item::WoodenAxe, 1),
    },
    WoodenShovel {
        name: "wooden_shovel", station: CraftingTable,
        ingredients: [(PLANKS, 1), (Item::Stick, 2)], output: (Item::WoodenShovel, 1),
    },
    StonePickaxe {
        name: "stone_pickaxe", station: CraftingTable,
        ingredients: [(COBBLESTONE, 3), (Item::Stick, 2)], output: (Item::StonePickaxe, 1),
    },
    StoneAxe {
        name: "stone_axe", station: CraftingTable,
        ingredients: [(COBBLESTONE, 3), (Item::Stick, 2)], output: (Item::StoneAxe, 1),
    },
    StoneShovel {
        name: "stone_shovel", station: CraftingTable,
        ingredients: [(COBBLESTONE, 1), (Item::Stick, 2)], output: (Item::StoneShovel, 1),
    },
    CopperPickaxe {
        name: "copper_pickaxe", station: CraftingTable,
        ingredients: [(Item::CopperIngot, 3), (Item::Stick, 2)], output: (Item::CopperPickaxe, 1),
    },
    CopperAxe {
        name: "copper_axe", station: CraftingTable,
        ingredients: [(Item::CopperIngot, 3), (Item::Stick, 2)], output: (Item::CopperAxe, 1),
    },
    CopperShovel {
        name: "copper_shovel", station: CraftingTable,
        ingredients: [(Item::CopperIngot, 1), (Item::Stick, 2)], output: (Item::CopperShovel, 1),
    },
    IronPickaxe {
        name: "iron_pickaxe", station: CraftingTable,
        ingredients: [(Item::IronIngot, 3), (Item::Stick, 2)], output: (Item::IronPickaxe, 1),
    },
    IronAxe {
        name: "iron_axe", station: CraftingTable,
        ingredients: [(Item::IronIngot, 3), (Item::Stick, 2)], output: (Item::IronAxe, 1),
    },
    IronShovel {
        name: "iron_shovel", station: CraftingTable,
        ingredients: [(Item::IronIngot, 1), (Item::Stick, 2)], output: (Item::IronShovel, 1),
    },
    Furnace {
        name: "furnace", station: CraftingTable,
        ingredients: [(COBBLESTONE, 8)], output: (Item::Block(BlockType::Furnace), 1),
    },
    StoneBricks {
        name: "stone_bricks", station: CraftingTable,
        ingredients: [(COBBLESTONE, 4)], output: (Item::Block(BlockType::StoneBricks), 4),
    },
    Sandstone {
        name: "sandstone", station: CraftingTable,
        ingredients: [(SAND, 4)], output: (Item::Block(BlockType::Sandstone), 1),
    },
}

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

    /// What it takes, as items and how many of each.
    pub fn ingredients(self) -> &'static [(Item, u8)] {
        self.properties().ingredients
    }

    /// What it makes.
    pub fn output(self) -> ItemStack {
        let (item, count) = self.properties().output;
        ItemStack::new(item, count)
    }

    /// How many times it can be made from what's in `inventory`'s slots.
    pub fn times_makeable(self, inventory: &Inventory) -> u32 {
        self.ingredients()
            .iter()
            .map(|&(item, count)| inventory.count(item) / u32::from(count))
            .min()
            .unwrap_or(0)
    }

    /// Takes its ingredients from `inventory` and returns what it makes, for the caller to put
    /// away, or nothing if they aren't all there.
    pub fn make(self, inventory: &mut Inventory) -> Option<ItemStack> {
        if self.times_makeable(inventory) == 0 {
            return None;
        }
        for &(item, count) in self.ingredients() {
            inventory.take_items(item, count.into());
        }
        Some(self.output())
    }
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

    use super::{Recipe, Station};
    use crate::block::BlockType;
    use crate::inventory::{Inventory, HOTBAR_SLOTS};
    use crate::items::{Item, ItemStack};

    const LOG: Item = Item::Block(BlockType::Log);
    const PLANKS: Item = Item::Block(BlockType::Planks);

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
        // and every tool has a recipe
        for item in Item::all().filter(|item| item.tool().is_some()) {
            assert!(
                Recipe::ALL.iter().any(|r| r.output().item == item),
                "{item:?}"
            );
        }
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
        assert_eq!(None, Station::of_block(BlockType::Furnace));
        // a log can be made into a table and the table into a pickaxe
        assert_eq!(Station::Hand, Recipe::Planks.station());
        assert_eq!(Station::Hand, Recipe::CraftingTable.station());
        assert_eq!(Station::CraftingTable, Recipe::WoodenPickaxe.station());
        assert_eq!(Station::Hand, Recipe::Torches.station());
        assert_eq!(
            ItemStack::new(Item::Block(BlockType::Torch), 4),
            Recipe::Torches.output()
        );
    }

    #[test]
    fn test_making_takes_the_ingredients_from_the_main_inventory_first() {
        let mut inventory = Inventory::default();
        inventory.slots[0] = Some(ItemStack::new(PLANKS, 2));
        inventory.slots[HOTBAR_SLOTS] = Some(ItemStack::new(PLANKS, 2));
        inventory.slots[1] = Some(ItemStack::new(Item::Stick, 3));
        assert_eq!(1, Recipe::WoodenPickaxe.times_makeable(&inventory));
        assert_eq!(
            Some(ItemStack::new(Item::WoodenPickaxe, 1)),
            Recipe::WoodenPickaxe.make(&mut inventory)
        );
        assert_eq!(None, inventory.slots[HOTBAR_SLOTS]);
        assert_eq!(Some(ItemStack::new(PLANKS, 1)), inventory.slots[0]);
        assert_eq!(Some(ItemStack::new(Item::Stick, 1)), inventory.slots[1]);
        // and without them, nothing's taken or made
        let before = inventory.clone();
        assert_eq!(None, Recipe::WoodenPickaxe.make(&mut inventory));
        assert_eq!(before, inventory);
        assert_eq!(0, Recipe::Planks.times_makeable(&inventory));
    }

    #[test]
    fn test_times_makeable_is_set_by_the_scarcest_ingredient() {
        let mut inventory = Inventory::default();
        inventory.slots[3] = Some(ItemStack::new(LOG, 5));
        assert_eq!(5, Recipe::Planks.times_makeable(&inventory));
        inventory.slots[4] = Some(ItemStack::full(PLANKS));
        inventory.slots[5] = Some(ItemStack::new(Item::Stick, 5));
        assert_eq!(2, Recipe::WoodenAxe.times_makeable(&inventory));
        assert_eq!(32, Recipe::Sticks.times_makeable(&inventory));
    }
}
