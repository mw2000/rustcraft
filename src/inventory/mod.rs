//! What a player carries (§5.2), and the rules for moving it about. Only the server changes a
//! player's inventory (INV-4), as clients ask it to (`InventoryClick` and the rest in
//! `protocol.rs`); the screen showing it is `screen`.

#[cfg(feature = "client")]
mod chest;
#[cfg(feature = "client")]
pub mod furnace;
#[cfg(feature = "client")]
pub mod grid;
#[cfg(feature = "client")]
pub mod icons;
#[cfg(feature = "client")]
pub mod look;
#[cfg(feature = "client")]
pub mod recipes;
#[cfg(feature = "client")]
pub mod screen;
pub mod slots;
#[cfg(feature = "client")]
mod tooltip;
#[cfg(feature = "client")]
pub mod trade;

use std::ops::Range;

use bevy::prelude::*;
use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

use crate::block::BlockType;
use crate::crafting::GRID;
use crate::game_mode::GameMode;
use crate::items::{take, Item, ItemStack, ARMOUR_POINT};

/// How many slots an inventory has: the hotbar's, then the main inventory's.
pub const SLOTS: usize = 36;
/// The armour slots, after the rest as clicks and saves number them: helmet, chestplate,
/// leggings and boots.
pub const ARMOUR_SLOTS: Range<usize> = SLOTS..SLOTS + 4;
/// How many of them are the hotbar, along the bottom of the screen.
pub const HOTBAR_SLOTS: usize = 9;
pub(crate) const HOTBAR: Range<usize> = 0..HOTBAR_SLOTS;
const MAIN: Range<usize> = HOTBAR_SLOTS..SLOTS;

/// The items a player carries, in slots 0 to 8 for the hotbar and 9 to 35 for the rest, the
/// armour they wear, the stack they've picked up in the inventory screen, which follows the
/// mouse, and what they've laid in its crafting grid (`crafting::GRID`), which goes back into
/// the inventory as the screen closes.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct Inventory {
    pub slots: [Option<ItemStack>; SLOTS],
    pub armour: [Option<ItemStack>; 4],
    pub cursor: Option<ItemStack>,
    pub grid: [Option<ItemStack>; GRID],
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            slots: [None; SLOTS],
            armour: [None; 4],
            cursor: None,
            grid: [None; GRID],
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
    /// Shift and left: moves the slot's stack between the hotbar and the main inventory, or
    /// between the inventory and the container open.
    Shift,
    /// A double click: gathers stacks of the cursor's item onto it, up to a full stack.
    Gather,
    /// A number key over the slot: swaps it with that hotbar slot, 0 to 8.
    Swap(u8),
    /// Q over the slot: throws one of its stack, or with Ctrl all of it.
    Throw { all: bool },
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
        add_to(&mut self.slots, stack)
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

    /// The stack in `slot`, an armour slot's past the rest, if there's one.
    pub fn get(&self, slot: usize) -> Option<ItemStack> {
        match slot.checked_sub(SLOTS) {
            None => self.slots[slot],
            Some(piece) => self.armour.get(piece).copied().flatten(),
        }
    }

    /// Does what a left, right or shift click on `slot` in the inventory screen does, with no
    /// container open; the other clicks are `slots::Slots`'. A slot there isn't is ignored. An
    /// armour slot takes only its own piece, and shift-clicking armour puts it on if that piece
    /// isn't worn.
    pub fn click(&mut self, slot: usize, click: Click) {
        if ARMOUR_SLOTS.contains(&slot) {
            let worn = &mut self.armour[slot - SLOTS];
            if click == Click::Shift {
                *worn = worn.and_then(|stack| add_to(&mut self.slots, stack));
            } else if matches!(click, Click::Left | Click::Right)
                && self.cursor.is_none_or(|stack| fits(stack, slot - SLOTS))
            {
                std::mem::swap(worn, &mut self.cursor);
            }
            return;
        }
        if slot >= SLOTS {
            return;
        }
        if click != Click::Shift {
            click_stack(&mut self.slots[slot], &mut self.cursor, click);
            return;
        }

        let Some(stack) = self.slots[slot].take() else {
            return;
        };
        if let Some(piece) = stack.item.armour().map(|armour| armour.piece.slot()) {
            if self.armour[piece].is_none() {
                self.armour[piece] = Some(stack);
                return;
            }
        }
        let to = if HOTBAR.contains(&slot) { MAIN } else { HOTBAR };
        self.slots[slot] = add_to(&mut self.slots[to], stack);
    }

    /// How much of a blow the armour worn takes, from 0 for none.
    pub fn protection(&self) -> f32 {
        f32::from(self.armour_points()) * ARMOUR_POINT
    }

    /// How much a blow of `damage` hurts through the armour worn, rounded, and at least half a
    /// heart.
    pub fn blunt(&self, damage: u8) -> u8 {
        let through = f32::from(damage) * (1.0 - self.protection());
        (through.round() as u8).clamp(1, damage.max(1))
    }

    /// The points of armour worn, out of the 20 the HUD's bar shows.
    pub fn armour_points(&self) -> u8 {
        let worn = self.armour.iter().flatten();
        worn.filter_map(|stack| stack.item.armour())
            .map(|armour| armour.points)
            .sum()
    }

    /// Wears each piece of armour worn by a blow taken, which breaks any that was its last.
    pub fn wear_armour(&mut self) {
        for worn in &mut self.armour {
            *worn = worn.and_then(ItemStack::worn);
        }
    }

    /// Everything it holds, taking it all away: its slots, its armour, the cursor's and the
    /// crafting grid's.
    pub fn empty(&mut self) -> impl Iterator<Item = ItemStack> {
        let Self {
            slots,
            armour,
            cursor,
            grid,
        } = std::mem::take(self);
        slots
            .into_iter()
            .chain(armour)
            .chain([cursor])
            .chain(grid)
            .flatten()
    }

    /// Puts the stack on the cursor back in the inventory, as closing the screen does, and
    /// returns what didn't fit, to drop.
    pub fn put_back_cursor(&mut self) -> Option<ItemStack> {
        let stack = self.cursor.take()?;
        self.add(stack)
    }

    /// Puts what's in the crafting grid back in the inventory, as closing the screen does, and
    /// returns what didn't fit, to drop.
    pub fn put_back_grid(&mut self) -> Vec<ItemStack> {
        let laid: Vec<_> = self.grid.iter_mut().filter_map(Option::take).collect();
        laid.into_iter()
            .filter_map(|stack| self.add(stack))
            .collect()
    }
}

// Whether `stack` is the piece of armour worn in armour slot `piece`.
pub(crate) fn fits(stack: ItemStack, piece: usize) -> bool {
    stack
        .item
        .armour()
        .is_some_and(|armour| armour.piece.slot() == piece)
}

/// Puts `stack` in `slots`, topping up the stacks of it there are first and then filling the
/// first empty slot, as picking up does in an inventory and shift-clicking in a chest. Returns
/// what didn't fit.
pub fn add_to(slots: &mut [Option<ItemStack>], stack: ItemStack) -> Option<ItemStack> {
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

/// Does what a left or right click does to the stack `held` in a slot, with the stack `cursor` on
/// the cursor, in an inventory or a container. Other clicks move stacks between slots, which is
/// for whoever has them to do (`slots::Slots`).
pub fn click_stack(held: &mut Option<ItemStack>, cursor: &mut Option<ItemStack>, click: Click) {
    match (click, held.as_mut(), *cursor) {
        (Click::Left, Some(stack), Some(carried)) if stack.stacks_with(carried) => {
            *cursor = stack.merge(carried);
        }
        (Click::Right, Some(stack), None) => {
            // half, rounding up, so a single item can be picked up
            let half = stack.count.div_ceil(2);
            *cursor = take(held, half);
        }
        (Click::Right, None, Some(_)) => *held = take(cursor, 1),
        (Click::Right, Some(stack), Some(carried)) if stack.stacks_with(carried) => {
            if stack.room() > 0 {
                stack.count += 1;
                take(cursor, 1);
            }
        }
        // the rest move stacks between slots, which is for whoever has them to do
        (Click::Shift | Click::Gather | Click::Swap(_) | Click::Throw { .. }, ..) => {}
        _ => std::mem::swap(held, cursor),
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
            (&self.slots[..], self.armour, self.cursor, self.grid).serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for Inventory {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let saved = SavedInventory::deserialize(deserializer)?;
            return saved.try_into().map_err(D::Error::custom);
        }
        let (slots, armour, cursor, grid) =
            <(Vec<Option<ItemStack>>, _, _, _)>::deserialize(deserializer)?;
        let slots = slots
            .try_into()
            .map_err(|_| D::Error::custom("an inventory has the wrong number of slots"))?;
        Ok(Self {
            slots,
            armour,
            cursor,
            grid,
        })
    }
}

// How an inventory is saved: the stacks in it, each with its slot, armour's past the rest, by
// item name (§4.1), and those in the crafting grid by their cell, which saves from before M4.2
// have none of.
#[derive(Serialize, Deserialize)]
struct SavedInventory {
    slots: Vec<SavedSlot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cursor: Option<ItemStack>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    grid: Vec<SavedSlot>,
}

#[derive(Serialize, Deserialize)]
struct SavedSlot {
    slot: usize,
    #[serde(flatten)]
    stack: ItemStack,
}

impl From<Inventory> for SavedInventory {
    fn from(inventory: Inventory) -> Self {
        let slots = inventory.slots.iter().chain(&inventory.armour).enumerate();
        Self {
            slots: slots
                .filter_map(|(slot, stack)| stack.map(|stack| SavedSlot { slot, stack }))
                .collect(),
            cursor: inventory.cursor,
            grid: inventory
                .grid
                .iter()
                .enumerate()
                .filter_map(|(slot, stack)| stack.map(|stack| SavedSlot { slot, stack }))
                .collect(),
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
            let held = match slot.checked_sub(SLOTS) {
                None => inventory.slots.get_mut(slot),
                Some(piece) => inventory.armour.get_mut(piece),
            }
            .ok_or_else(|| format!("the inventory has no slot {slot}"))?;
            if ARMOUR_SLOTS.contains(&slot) && !fits(stack, slot - SLOTS) {
                return Err(format!(
                    "{} can't be worn in slot {slot}",
                    stack.item.name()
                ));
            }
            if held.replace(stack).is_some() {
                return Err(format!("slot {slot} is saved twice"));
            }
        }
        for SavedSlot { slot, stack } in saved.grid {
            let held = inventory
                .grid
                .get_mut(slot)
                .ok_or_else(|| format!("the crafting grid has no cell {slot}"))?;
            if held.replace(stack).is_some() {
                return Err(format!("cell {slot} is saved twice"));
            }
        }
        Ok(inventory)
    }
}

/// Which of the inventory's first nine slots, the hotbar along the bottom of the screen, the
/// player holds, to place from (§5.2). Keys 1 to 9 and the mouse wheel select a slot. It's
/// the client's to choose, as it changes nothing but what's asked for.
#[derive(Component, Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SavedHotbar")]
pub struct Hotbar {
    pub selected: usize,
}

impl Hotbar {
    /// What's in the selected slot of `inventory`.
    pub fn held(self, inventory: &Inventory) -> Option<ItemStack> {
        inventory.slots[self.selected]
    }

    /// Moves the selection `by` slots to the right, or left if it's negative, wrapping round.
    pub fn scroll(&mut self, by: i32) {
        self.selected = (self.selected as i32 + by).rem_euclid(HOTBAR_SLOTS as i32) as usize;
    }

    /// Selects the slot in the hotbar holding `block`, as middle click does, returning whether
    /// there's one.
    pub fn pick(&mut self, block: BlockType, inventory: &Inventory) -> bool {
        let holding = |slot: &Option<ItemStack>| slot.is_some_and(|s| s.item == Item::Block(block));
        let slot = inventory.slots[..HOTBAR_SLOTS].iter().position(holding);
        if let Some(slot) = slot {
            self.selected = slot;
        }
        slot.is_some()
    }
}

// How a hotbar is read, checked. Saves from before M1.1 have its blocks too, which are
// ignored: the inventory holds them now.
#[derive(Deserialize)]
struct SavedHotbar {
    selected: usize,
}

impl TryFrom<SavedHotbar> for Hotbar {
    type Error = String;

    fn try_from(saved: SavedHotbar) -> Result<Self, String> {
        if saved.selected >= HOTBAR_SLOTS {
            return Err(format!("the hotbar has no slot {}", saved.selected));
        }
        Ok(Self {
            selected: saved.selected,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Click, Inventory, ARMOUR_SLOTS, HOTBAR_SLOTS, SLOTS};
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
            ..Inventory::default()
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
    fn test_closing_puts_the_grid_back_and_hands_back_what_doesnt_fit() {
        let mut inventory = two_stacks();
        inventory.grid[0] = stack(DIRT, 3);
        inventory.grid[8] = stack(Item::Stick, 2);
        assert!(inventory.put_back_grid().is_empty());
        assert_eq!(stack(DIRT, 13), inventory.slots[0]);
        assert_eq!(stack(Item::Stick, 2), inventory.slots[2]);
        assert!(inventory.grid.iter().all(Option::is_none));
        inventory.slots = [stack(SAND, 64); SLOTS];
        inventory.grid[3] = stack(DIRT, 4);
        assert_eq!(vec![ItemStack::new(DIRT, 4)], inventory.put_back_grid());
        assert!(inventory.grid.iter().all(Option::is_none));
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
        inventory.grid[4] = stack(SAND, 2);
        let text = toml::to_string(&inventory).unwrap();
        assert!(text.contains("item = \"crafting_table\""), "{text}");
        // and one saved before the grid, with none in it, reads as it did
        let mut before = inventory.clone();
        before.grid = Default::default();
        let old = toml::to_string(&before).unwrap();
        assert!(!old.contains("grid"), "{old}");
        assert_eq!(before, toml::from_str(&old).unwrap());
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

    #[test]
    fn test_armour_goes_on_only_in_its_own_slot_and_comes_off_into_the_inventory() {
        let helmet = stack(Item::IronHelmet, 1);
        let boots = stack(Item::LeatherBoots, 1);
        let mut inventory = Inventory::default();
        inventory.slots[3] = helmet;
        inventory.slots[12] = boots;
        // boots on the cursor go nowhere but the boots' slot
        inventory.click(12, Click::Left);
        inventory.click(ARMOUR_SLOTS.start, Click::Left);
        assert_eq!((None, boots), (inventory.armour[0], inventory.cursor));
        inventory.click(ARMOUR_SLOTS.end - 1, Click::Left);
        assert_eq!((boots, None), (inventory.armour[3], inventory.cursor));
        // nor does dirt go on at all
        inventory.cursor = stack(DIRT, 4);
        inventory.click(ARMOUR_SLOTS.start, Click::Left);
        assert_eq!(None, inventory.armour[0]);
        inventory.cursor = None;
        // shift-clicking puts armour on, and takes it off again into the inventory
        inventory.click(3, Click::Shift);
        assert_eq!((None, helmet), (inventory.slots[3], inventory.armour[0]));
        assert_eq!(2 + 1, inventory.armour_points());
        assert!((inventory.protection() - 0.12).abs() < 1e-6);
        // which takes an eighth off a blow, rounded
        assert_eq!(
            (7, 4, 1),
            (inventory.blunt(8), inventory.blunt(4), inventory.blunt(1))
        );
        inventory.click(ARMOUR_SLOTS.start, Click::Shift);
        assert_eq!((helmet, None), (inventory.slots[0], inventory.armour[0]));
        assert_eq!(helmet, inventory.get(0));
        assert_eq!(boots, inventory.get(ARMOUR_SLOTS.end - 1));
        // a second helmet shift-clicked with one on goes across as anything does
        inventory.click(ARMOUR_SLOTS.end - 1, Click::Shift);
        inventory.armour[0] = helmet;
        inventory.click(0, Click::Shift);
        assert_eq!(helmet, inventory.slots[HOTBAR_SLOTS]);
    }

    #[test]
    fn test_armour_wears_with_blows_and_round_trips_in_its_slots() {
        let mut inventory = Inventory::default();
        inventory.armour[1] = stack(Item::LeatherChestplate, 1);
        inventory.wear_armour();
        assert_eq!(1, inventory.armour[1].unwrap().wear);
        let text = toml::to_string(&inventory).unwrap();
        assert!(text.contains("slot = 37"), "{text}");
        assert_eq!(inventory, toml::from_str(&text).unwrap());
        let bytes = postcard::to_stdvec(&inventory).unwrap();
        assert_eq!(inventory, postcard::from_bytes(&bytes).unwrap());
        // and nothing else is worn there
        let wrong = text.replace("slot = 37", "slot = 36");
        assert!(toml::from_str::<Inventory>(&wrong).is_err());
        // emptied, as dying does, it all comes out
        inventory.slots[0] = stack(DIRT, 3);
        assert_eq!(2, inventory.empty().count());
        assert_eq!(Inventory::default(), inventory);
    }
}
