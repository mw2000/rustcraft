//! What a player carries (§5.2), and the rules for moving it about. Only the server changes a
//! player's inventory (INV-4), as clients ask it to (`InventoryClick` and the rest in
//! `protocol.rs`); the screen showing it is `screen`.

pub mod icons;
pub mod recipes;
pub mod screen;

use std::ops::Range;

use bevy::prelude::*;
use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

use crate::block::BlockType;
use crate::game_mode::GameMode;
use crate::items::{take, Item, ItemStack};

/// How many slots an inventory has: the hotbar's, then the main inventory's.
pub const SLOTS: usize = 36;
/// How many of them are the hotbar, along the bottom of the screen.
pub const HOTBAR_SLOTS: usize = 9;
const HOTBAR: Range<usize> = 0..HOTBAR_SLOTS;
const MAIN: Range<usize> = HOTBAR_SLOTS..SLOTS;

/// The items a player carries, in slots 0 to 8 for the hotbar and 9 to 35 for the rest, and
/// the stack they've picked up in the inventory screen, which follows the mouse.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct Inventory {
    pub slots: [Option<ItemStack>; SLOTS],
    pub cursor: Option<ItemStack>,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            slots: [None; SLOTS],
            cursor: None,
        }
    }
}

/// A click on a slot in the inventory screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Click {
    /// Left: picks up the slot's stack, puts the cursor's down, tops the slot's up from it, or
    /// swaps them.
    Left,
    /// Right: picks up half the slot's stack, or puts one of the cursor's down.
    Right,
    /// Shift and left: moves the slot's stack between the hotbar and the main inventory.
    Shift,
}

impl Inventory {
    /// What a new player starts with: nothing in survival, and blocks to build with in creative.
    pub fn starting(mode: GameMode) -> Self {
        let mut inventory = Self::default();
        if mode.has_every_block() {
            use BlockType::*;
            let blocks = [
                Stone,
                Cobblestone,
                StoneBricks,
                Bricks,
                Planks,
                Log,
                Dirt,
                Grass,
                Sandstone,
            ];
            for (slot, block) in inventory.slots.iter_mut().zip(blocks) {
                *slot = Some(ItemStack::full(Item::Block(block)));
            }
        }
        inventory
    }

    /// Whether any of `item` would fit.
    pub fn has_room_for(&self, item: Item) -> bool {
        self.slots
            .iter()
            .any(|slot| slot.is_none_or(|stack| stack.item == item && stack.room() > 0))
    }

    /// Puts `stack` away, topping up the stacks of it there are first and then filling the
    /// first empty slot, the hotbar's before the rest. Returns what didn't fit.
    pub fn add(&mut self, stack: ItemStack) -> Option<ItemStack> {
        self.add_within(stack, 0..SLOTS)
    }

    fn add_within(&mut self, stack: ItemStack, slots: Range<usize>) -> Option<ItemStack> {
        let slots = &mut self.slots[slots];
        let mut left = Some(stack);
        for held in slots.iter_mut().flatten() {
            // and once it's all in, there's nothing left to put away
            left = held.merge(left?);
        }
        if let Some(empty) = slots.iter_mut().find(|slot| slot.is_none()) {
            *empty = left.take();
        }
        left
    }

    /// Takes one of `item` from `slot`, as placing a block does, if it's there.
    pub fn use_one(&mut self, slot: usize, item: Item) -> bool {
        let Some(held) = self.slots.get_mut(slot) else {
            return false;
        };
        held.is_some_and(|stack| stack.item == item) && take(held, 1).is_some()
    }

    /// How many of `item` there are in its slots.
    pub fn count(&self, item: Item) -> u32 {
        let stacks = self.slots.iter().flatten();
        stacks
            .filter(|stack| stack.item == item)
            .map(|s| u32::from(s.count))
            .sum()
    }

    /// Takes `count` of `item` from its slots, if there are that many, from the main
    /// inventory's before the hotbar's, so what's held stays in hand as long as it can.
    pub fn take_items(&mut self, item: Item, count: u32) -> bool {
        if self.count(item) < count {
            return false;
        }
        let mut left = count;
        let (hotbar, main) = self.slots.split_at_mut(HOTBAR_SLOTS);
        for slot in main.iter_mut().chain(hotbar) {
            if slot.is_some_and(|stack| stack.item == item) {
                let taken = take(slot, u8::try_from(left).unwrap_or(u8::MAX));
                left -= taken.map_or(0, |stack| u32::from(stack.count));
            }
        }
        true
    }

    /// Wears the tool in `slot` by a block broken with it, which breaks it if that was the last
    /// its durability had in it.
    pub fn wear(&mut self, slot: usize) {
        if let Some(held) = self.slots.get_mut(slot) {
            *held = held.and_then(ItemStack::worn);
        }
    }

    /// Takes the stack in `slot`, or one of it, to drop.
    pub fn drop_from(&mut self, slot: usize, all: bool) -> Option<ItemStack> {
        let held = self.slots.get_mut(slot)?;
        take(held, if all { u8::MAX } else { 1 })
    }

    /// Does what clicking `slot` in the inventory screen does. A slot there isn't is ignored.
    pub fn click(&mut self, slot: usize, click: Click) {
        if slot >= SLOTS {
            return;
        }
        let held = &mut self.slots[slot];
        match click {
            Click::Left => match (held.as_mut(), self.cursor) {
                (Some(stack), Some(carried)) if stack.stacks_with(carried) => {
                    self.cursor = stack.merge(carried);
                }
                _ => std::mem::swap(held, &mut self.cursor),
            },
            Click::Right => match (held.as_mut(), self.cursor) {
                (Some(stack), None) => {
                    // half, rounding up, so a single item can be picked up
                    let half = stack.count.div_ceil(2);
                    self.cursor = take(held, half);
                }
                (None, Some(_)) => *held = take(&mut self.cursor, 1),
                (Some(stack), Some(carried)) if stack.stacks_with(carried) => {
                    if stack.room() > 0 {
                        stack.count += 1;
                        take(&mut self.cursor, 1);
                    }
                }
                _ => std::mem::swap(held, &mut self.cursor),
            },
            Click::Shift => {
                let Some(stack) = held.take() else {
                    return;
                };
                let to = if HOTBAR.contains(&slot) { MAIN } else { HOTBAR };
                self.slots[slot] = self.add_within(stack, to);
            }
        }
    }

    /// Puts the stack on the cursor back in the inventory, as closing the screen does, and
    /// returns what didn't fit, to drop.
    pub fn put_back_cursor(&mut self) -> Option<ItemStack> {
        let stack = self.cursor.take()?;
        self.add(stack)
    }
}

// Saved as the stacks in it with their slots, to be read in `world.toml`, and sent to clients
// (`InventoryChanged`, `Welcome`) as every slot in order, as postcard has no way to write the
// saved form's flattened stacks.
impl Serialize for Inventory {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            SavedInventory::from(self.clone()).serialize(serializer)
        } else {
            (&self.slots[..], self.cursor).serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Inventory {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let saved = SavedInventory::deserialize(deserializer)?;
            return saved.try_into().map_err(D::Error::custom);
        }
        let (slots, cursor) = <(Vec<Option<ItemStack>>, _)>::deserialize(deserializer)?;
        let slots = slots
            .try_into()
            .map_err(|_| D::Error::custom("an inventory has the wrong number of slots"))?;
        Ok(Self { slots, cursor })
    }
}

// How an inventory is saved: the stacks in it, each with its slot, by item name (§4.1).
#[derive(Serialize, Deserialize)]
struct SavedInventory {
    slots: Vec<SavedSlot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cursor: Option<ItemStack>,
}

#[derive(Serialize, Deserialize)]
struct SavedSlot {
    slot: usize,
    #[serde(flatten)]
    stack: ItemStack,
}

impl From<Inventory> for SavedInventory {
    fn from(inventory: Inventory) -> Self {
        let slots = inventory.slots.iter().enumerate();
        Self {
            slots: slots
                .filter_map(|(slot, stack)| stack.map(|stack| SavedSlot { slot, stack }))
                .collect(),
            cursor: inventory.cursor,
        }
    }
}

impl TryFrom<SavedInventory> for Inventory {
    type Error = String;

    fn try_from(saved: SavedInventory) -> Result<Self, String> {
        let mut inventory = Inventory {
            cursor: saved.cursor,
            ..default()
        };
        for SavedSlot { slot, stack } in saved.slots {
            let held = inventory
                .slots
                .get_mut(slot)
                .ok_or_else(|| format!("the inventory has no slot {slot}"))?;
            if held.replace(stack).is_some() {
                return Err(format!("slot {slot} is saved twice"));
            }
        }
        Ok(inventory)
    }
}

#[cfg(test)]
mod tests {
    use super::{Click, Inventory, HOTBAR_SLOTS, SLOTS};
    use crate::block::BlockType;
    use crate::game_mode::GameMode;
    use crate::items::{Item, ItemStack};

    const DIRT: Item = Item::Block(BlockType::Dirt);
    const SAND: Item = Item::Block(BlockType::Sand);

    fn stack(item: Item, count: u8) -> Option<ItemStack> {
        Some(ItemStack::new(item, count))
    }

    #[test]
    fn test_survival_starts_with_nothing_and_creative_with_blocks() {
        assert_eq!(
            Inventory::default(),
            Inventory::starting(GameMode::Survival)
        );
        let creative = Inventory::starting(GameMode::Creative);
        assert!(creative.slots[..HOTBAR_SLOTS].iter().all(Option::is_some));
        assert!(creative.slots[HOTBAR_SLOTS..].iter().all(Option::is_none));
    }

    #[test]
    fn test_adding_tops_up_stacks_first_then_fills_the_first_empty_slot() {
        let mut inventory = Inventory::default();
        inventory.slots[3] = stack(DIRT, 60);
        inventory.slots[20] = stack(DIRT, 63);
        inventory.slots[0] = stack(SAND, 1);
        assert_eq!(None, inventory.add(ItemStack::new(DIRT, 10)));
        assert_eq!(stack(DIRT, 64), inventory.slots[3]);
        assert_eq!(stack(DIRT, 64), inventory.slots[20]);
        // what's left goes in the first empty slot, in the hotbar
        assert_eq!(stack(DIRT, 5), inventory.slots[1]);
        assert_eq!(stack(SAND, 1), inventory.slots[0]);
    }

    #[test]
    fn test_a_full_inventory_hands_back_what_doesnt_fit() {
        let mut inventory = Inventory {
            slots: [stack(SAND, 64); SLOTS],
            cursor: None,
        };
        inventory.slots[30] = stack(DIRT, 62);
        assert!(inventory.has_room_for(DIRT));
        assert!(!inventory.has_room_for(SAND));
        assert_eq!(stack(DIRT, 3), inventory.add(ItemStack::new(DIRT, 5)));
        assert!(!inventory.has_room_for(DIRT));
    }

    #[test]
    fn test_using_one_takes_it_from_the_slot_only_if_its_there() {
        let mut inventory = Inventory::default();
        inventory.slots[2] = stack(DIRT, 2);
        assert!(!inventory.use_one(2, SAND));
        assert!(!inventory.use_one(1, DIRT));
        assert!(!inventory.use_one(SLOTS, DIRT));
        assert!(inventory.use_one(2, DIRT));
        assert!(inventory.use_one(2, DIRT));
        assert_eq!(None, inventory.slots[2]);
        assert!(!inventory.use_one(2, DIRT));
    }

    #[test]
    fn test_dropping_takes_one_or_the_stack() {
        let mut inventory = Inventory::default();
        inventory.slots[0] = stack(DIRT, 5);
        assert_eq!(stack(DIRT, 1), inventory.drop_from(0, false));
        assert_eq!(stack(DIRT, 4), inventory.drop_from(0, true));
        assert_eq!(None, inventory.drop_from(0, true));
    }

    // An inventory with dirt in slot 0 and sand in slot 1.
    fn two_stacks() -> Inventory {
        let mut inventory = Inventory::default();
        inventory.slots[0] = stack(DIRT, 10);
        inventory.slots[1] = stack(SAND, 5);
        inventory
    }

    #[test]
    fn test_left_click_picks_up_puts_down_and_swaps() {
        let mut inventory = two_stacks();
        inventory.click(0, Click::Left);
        assert_eq!(
            (None, stack(DIRT, 10)),
            (inventory.slots[0], inventory.cursor)
        );
        inventory.click(5, Click::Left);
        assert_eq!(
            (stack(DIRT, 10), None),
            (inventory.slots[5], inventory.cursor)
        );
        // onto another item, they swap
        inventory.click(1, Click::Left);
        inventory.click(5, Click::Left);
        assert_eq!(stack(SAND, 5), inventory.slots[5]);
        assert_eq!(stack(DIRT, 10), inventory.cursor);
    }

    #[test]
    fn test_left_click_onto_the_same_item_tops_the_slot_up() {
        let mut inventory = Inventory::default();
        inventory.slots[0] = stack(DIRT, 60);
        inventory.cursor = stack(DIRT, 10);
        inventory.click(0, Click::Left);
        assert_eq!(
            (stack(DIRT, 64), stack(DIRT, 6)),
            (inventory.slots[0], inventory.cursor)
        );
        // and onto a full stack, nothing moves
        inventory.click(0, Click::Left);
        assert_eq!(
            (stack(DIRT, 64), stack(DIRT, 6)),
            (inventory.slots[0], inventory.cursor)
        );
    }

    #[test]
    fn test_right_click_picks_up_half_and_puts_down_one() {
        let mut inventory = two_stacks();
        inventory.click(1, Click::Right);
        assert_eq!(
            (stack(SAND, 2), stack(SAND, 3)),
            (inventory.slots[1], inventory.cursor)
        );
        inventory.click(7, Click::Right);
        inventory.click(1, Click::Right);
        assert_eq!(stack(SAND, 1), inventory.slots[7]);
        assert_eq!(stack(SAND, 3), inventory.slots[1]);
        assert_eq!(stack(SAND, 1), inventory.cursor);
        // the last one off the cursor empties it
        inventory.click(7, Click::Right);
        assert_eq!(
            (stack(SAND, 2), None),
            (inventory.slots[7], inventory.cursor)
        );
        // a single item is picked up whole
        inventory.slots[8] = stack(DIRT, 1);
        inventory.click(8, Click::Right);
        assert_eq!(
            (None, stack(DIRT, 1)),
            (inventory.slots[8], inventory.cursor)
        );
        // and onto another item, right click swaps as left does
        inventory.click(7, Click::Right);
        assert_eq!(
            (stack(DIRT, 1), stack(SAND, 2)),
            (inventory.slots[7], inventory.cursor)
        );
    }

    #[test]
    fn test_shift_click_moves_a_stack_between_the_hotbar_and_the_rest() {
        let mut inventory = two_stacks();
        inventory.slots[12] = stack(DIRT, 60);
        inventory.click(0, Click::Shift);
        // topping up the stack there first
        assert_eq!(None, inventory.slots[0]);
        assert_eq!(stack(DIRT, 64), inventory.slots[12]);
        assert_eq!(stack(DIRT, 6), inventory.slots[HOTBAR_SLOTS]);
        inventory.click(12, Click::Shift);
        assert_eq!(stack(DIRT, 64), inventory.slots[0]);
        // with no room, it stays where it is
        inventory.slots[..HOTBAR_SLOTS].fill(stack(SAND, 64));
        inventory.click(HOTBAR_SLOTS, Click::Shift);
        assert_eq!(stack(DIRT, 6), inventory.slots[HOTBAR_SLOTS]);
        assert_eq!(None, inventory.cursor);
    }

    #[test]
    fn test_clicks_on_slots_there_arent_are_ignored() {
        let mut inventory = two_stacks();
        inventory.click(SLOTS, Click::Left);
        assert_eq!(two_stacks(), inventory);
    }

    #[test]
    fn test_the_cursors_stack_goes_back_or_is_handed_back_if_theres_no_room() {
        let mut inventory = two_stacks();
        inventory.cursor = stack(DIRT, 4);
        assert_eq!(None, inventory.put_back_cursor());
        assert_eq!(
            (stack(DIRT, 14), None),
            (inventory.slots[0], inventory.cursor)
        );
        inventory.slots = [stack(SAND, 64); SLOTS];
        inventory.cursor = stack(DIRT, 4);
        assert_eq!(stack(DIRT, 4), inventory.put_back_cursor());
        assert_eq!(None, inventory.cursor);
    }

    #[test]
    fn test_inventories_round_trip_by_item_name() {
        let mut inventory = two_stacks();
        inventory.slots[35] = stack(Item::Block(BlockType::CraftingTable), 1);
        inventory.slots[20] = Some(ItemStack {
            wear: 40,
            ..ItemStack::new(Item::CopperAxe, 1)
        });
        inventory.cursor = stack(DIRT, 3);
        let text = toml::to_string(&inventory).unwrap();
        assert!(text.contains("item = \"crafting_table\""), "{text}");
        assert!(text.contains("wear = 40"), "{text}");
        assert_eq!(inventory, toml::from_str(&text).unwrap());

        // a slot there isn't, or one saved twice, is refused
        let off_the_end = text.replace("slot = 35", "slot = 36");
        assert!(toml::from_str::<Inventory>(&off_the_end).is_err());
        let twice = text.replace("slot = 35", "slot = 1");
        assert!(toml::from_str::<Inventory>(&twice).is_err());

        // and as they're sent to clients
        let bytes = postcard::to_stdvec(&inventory).unwrap();
        assert_eq!(inventory, postcard::from_bytes(&bytes).unwrap());
    }
}
