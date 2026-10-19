//! The slots of a player's inventory screen, their own, those of the container they have open and
//! those of the crafting grid, and what clicks and drags on them do, as Minecraft players expect
//! (M4.1): left and right clicks, shift-clicks moving stacks across, double clicks gathering a
//! stack onto the cursor, the number keys swapping a slot with the hotbar's, Q throwing from a
//! slot, and drags sharing the cursor's stack over the slots they cross. The crafting grid's
//! output takes what the grid makes, using one of each ingredient laid (M4.2), and the recipe
//! book lays a recipe's ingredients into the grid (`Slots::fill`). Only the server runs them
//! (INV-4), as clients ask with `InventoryClick`, `InventoryDrag` and `FillGrid`.

use serde::{Deserialize, Serialize};

use super::{
    add_to, click_stack, fits, Click, Inventory, ARMOUR_SLOTS, HOTBAR, HOTBAR_SLOTS, SLOTS,
};
use crate::containers::Container;
use crate::crafting::{Recipe, Station};
use crate::items::{take, Item, ItemStack};

/// A slot in the inventory screen: one of the player's own, numbered as clicks and saves number
/// them (armour's past the rest), one of the container they have open, a cell of the crafting
/// grid (`crafting::GRID`), or the grid's output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Place {
    Own(u8),
    Container(u8),
    Grid(u8),
    Output,
}

/// How a drag shares the cursor's stack over the slots it crosses: evenly with the left button,
/// what doesn't share evenly staying on the cursor, or one in each with the right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Spread {
    Even,
    One,
}

/// A player's inventory, the container they have open, if they have one, and the station their
/// screen's at, whose crafting grid it shows, for the clicks and drags in their inventory screen
/// to change. With a container open there's no grid.
pub struct Slots<'a> {
    pub inventory: &'a mut Inventory,
    pub container: Option<&'a mut Container>,
    pub station: Station,
}

impl Slots<'_> {
    /// Does what `click` on the slot at `place` does, returning what it throws, which the caller
    /// drops. A slot there isn't, or a container's with none open, changes nothing.
    pub fn click(&mut self, place: Place, click: Click) -> Option<ItemStack> {
        if place == Place::Output {
            return self.click_output(click);
        }
        match click {
            Click::Gather => {
                self.gather();
                None
            }
            Click::Swap(hotbar) => {
                self.swap(place, usize::from(hotbar));
                None
            }
            Click::Throw { all } => take(self.slot_mut(place)?, if all { u8::MAX } else { 1 }),
            Click::Left | Click::Right | Click::Shift => {
                let in_grid = matches!(place, Place::Grid(cell) if self.in_grid(cell));
                match (place, self.container.as_deref_mut()) {
                    (Place::Container(slot), Some(container)) => {
                        container.click(usize::from(slot), click, self.inventory);
                    }
                    (Place::Container(_), None) | (Place::Output, _) => {}
                    (Place::Grid(cell), _) if in_grid => {
                        let inventory = &mut *self.inventory;
                        let held = &mut inventory.grid[usize::from(cell)];
                        if click == Click::Shift {
                            *held = held.and_then(|stack| add_to(&mut inventory.slots, stack));
                        } else {
                            click_stack(held, &mut inventory.cursor, click);
                        }
                    }
                    (Place::Grid(_), _) => {}
                    (Place::Own(slot), Some(container)) if click == Click::Shift => {
                        shift_into(container, self.inventory, usize::from(slot));
                    }
                    (Place::Own(slot), _) => self.inventory.click(usize::from(slot), click),
                }
                None
            }
        }
    }

    /// Shares the cursor's stack over the slots at `places`, as a drag across them does, and
    /// returns whether it did. It's all or nothing: a drag is refused whole if a slot isn't there,
    /// is crossed twice, holds another item or won't take this one, or there are more slots
    /// than items on the cursor. A slot already full takes none, and what's left stays on the
    /// cursor.
    pub fn drag(&mut self, places: &[Place], spread: Spread) -> bool {
        let Some(carried) = self.inventory.cursor else {
            return false;
        };
        let crossed = places.len();
        let twice = (1..crossed).any(|i| places[..i].contains(&places[i]));
        if crossed == 0 || crossed > usize::from(carried.count) || twice {
            return false;
        }
        if !places.iter().all(|&place| self.takes(place, carried)) {
            return false;
        }
        let share = match spread {
            Spread::Even => carried.count / crossed as u8,
            Spread::One => 1,
        };
        let mut left = carried.count;
        for &place in places {
            let held = self.slot_mut(place).expect("checked it's there");
            let room = held.map_or(carried.item.max_stack(), ItemStack::room);
            let put = share.min(room);
            if put == 0 {
                continue;
            }
            match held {
                Some(stack) => stack.count += put,
                None => *held = Some(ItemStack::new(carried.item, put)),
            }
            left -= put;
        }
        self.inventory.cursor = (left > 0).then_some(ItemStack {
            count: left,
            ..carried
        });
        true
    }

    /// Whether the slot at `place` can have some of `carried` dragged into it: it's there, it
    /// takes the item, and it's empty or holds the same item, which stacks.
    pub fn takes(&self, place: Place, carried: ItemStack) -> bool {
        self.accepts(place, carried.item)
            && self
                .get(place)
                .is_some_and(|held| held.is_none_or(|stack| stack.stacks_with(carried)))
    }

    /// The recipe the crafting grid makes, if it's shown and makes one.
    pub fn recipe(&self) -> Option<Recipe> {
        if self.grid().is_empty() {
            return None;
        }
        Recipe::matching(&self.inventory.grid, self.station)
    }

    /// Lays `recipe`'s ingredients into the crafting grid from the inventory, as clicking it in
    /// the recipe book does: one set more than the grid holds of it already, or with `all` as
    /// many as there are enough for, up to a full stack in each cell. What's in the grid goes
    /// back into the inventory first, so it counts, and if there isn't room for it all nothing's
    /// laid. Returns whether it laid any.
    pub fn fill(&mut self, recipe: Recipe, all: bool) -> bool {
        if self.grid().is_empty() {
            return false;
        }
        let Some(layout) = recipe.layout(self.station) else {
            return false;
        };
        let laid = if self.recipe() == Some(recipe) {
            let stacks = self.inventory.grid.iter().flatten();
            stacks.map(|stack| u32::from(stack.count)).min()
        } else {
            None
        };
        for &cell in self.grid() {
            if let Some(stack) = self.inventory.grid[cell].take() {
                self.inventory.grid[cell] = self.inventory.add(stack);
            }
        }
        if self.inventory.grid.iter().any(Option::is_some) {
            return false;
        }
        let most = layout
            .iter()
            .flatten()
            .map(|item| u32::from(item.max_stack()));
        let wanted = if all { u32::MAX } else { laid.unwrap_or(0) + 1 };
        let sets = wanted
            .min(recipe.times_makeable(self.inventory))
            .min(most.min().unwrap_or(0));
        if sets == 0 {
            return false;
        }
        for (cell, item) in layout.into_iter().enumerate() {
            if let Some(item) = item {
                self.inventory.take_items(item, sets);
                self.inventory.grid[cell] = Some(ItemStack::new(item, sets as u8));
            }
        }
        true
    }

    // The cells of the crafting grid shown, none with a container open.
    fn grid(&self) -> &'static [usize] {
        match self.container {
            Some(_) => &[],
            None => self.station.grid(),
        }
    }

    fn in_grid(&self, cell: u8) -> bool {
        self.grid().contains(&usize::from(cell))
    }

    // A click on the crafting grid's output: left or right takes what it makes onto the cursor,
    // if that holds nothing or the same with room for it; shift makes as many as the grid's
    // ingredients allow into the inventory, until one wouldn't fit whole; a number key makes
    // one into that hotbar slot if it's empty; and Q makes one and throws it, or with Ctrl as
    // many as make a stack.
    fn click_output(&mut self, click: Click) -> Option<ItemStack> {
        let recipe = self.recipe()?;
        let made = recipe.output();
        match click {
            Click::Left | Click::Right => {
                let cursor = &mut self.inventory.cursor;
                match cursor {
                    None => *cursor = Some(made),
                    Some(carried) if carried.stacks_with(made) && carried.room() >= made.count => {
                        carried.count += made.count;
                    }
                    Some(_) => return None,
                }
                self.use_ingredients();
            }
            Click::Shift => {
                while self.recipe() == Some(recipe) {
                    let mut slots = self.inventory.slots;
                    if add_to(&mut slots, made).is_some() {
                        break;
                    }
                    self.inventory.slots = slots;
                    self.use_ingredients();
                }
            }
            Click::Swap(hotbar) => {
                let hotbar = usize::from(hotbar);
                if HOTBAR.contains(&hotbar) && self.inventory.slots[hotbar].is_none() {
                    self.inventory.slots[hotbar] = Some(made);
                    self.use_ingredients();
                }
            }
            Click::Throw { all } => {
                let mut thrown = made;
                self.use_ingredients();
                while all && self.recipe() == Some(recipe) && thrown.room() >= made.count {
                    thrown.count += made.count;
                    self.use_ingredients();
                }
                return Some(thrown);
            }
            Click::Gather => {}
        }
        None
    }

    // Takes one from each cell of the crafting grid, as making what it makes does: every cell
    // with something in it is an ingredient, or it wouldn't have matched.
    fn use_ingredients(&mut self) {
        for cell in &mut self.inventory.grid {
            take(cell, 1);
        }
    }

    // Gathers stacks of the cursor's item onto it, up to a full stack, as a double click does:
    // the container's first and then the inventory's, and partial stacks before full ones, so
    // full stacks are only broken into if there's no other way.
    fn gather(&mut self) {
        let Some(mut carried) = self.inventory.cursor.filter(|c| c.item.max_stack() > 1) else {
            return;
        };
        for full in [false, true] {
            let container = self.container.as_deref_mut().into_iter();
            let theirs = container.flat_map(|container| container.slots.iter_mut());
            let (hotbar, main) = self.inventory.slots.split_at_mut(HOTBAR_SLOTS);
            let own = main.iter_mut().chain(hotbar);
            for held in theirs.chain(own) {
                if carried.room() == 0 {
                    break;
                }
                let Some(stack) = held.filter(|stack| stack.stacks_with(carried)) else {
                    continue;
                };
                if (stack.room() == 0) == full {
                    let taken = take(held, carried.room()).expect("a stack to take from");
                    carried.count += taken.count;
                }
            }
        }
        self.inventory.cursor = Some(carried);
    }

    // Swaps the slot at `place` with hotbar slot `hotbar`, as a number key over it does, if each
    // stack would go in the other's slot: armour only into its own, and a furnace's output only
    // ever out, into an empty hotbar slot.
    fn swap(&mut self, place: Place, hotbar: usize) {
        if !HOTBAR.contains(&hotbar) || place == Place::Own(hotbar as u8) {
            return;
        }
        let held = self.inventory.slots[hotbar];
        let Some(theirs) = self.get(place) else {
            return;
        };
        let takes_held = held.is_none_or(|stack| self.accepts(place, stack.item));
        // what's in the slot goes into the hotbar, which takes anything
        if !takes_held || (theirs.is_none() && held.is_none()) {
            return;
        }
        let theirs = std::mem::replace(self.slot_mut(place).expect("it's there"), held);
        self.inventory.slots[hotbar] = theirs;
    }

    // Whether `item` can go in the slot at `place`: anything in the inventory's or the grid's,
    // a piece of armour only in its own, in a container's what it accepts, and nothing in the
    // grid's output.
    fn accepts(&self, place: Place, item: Item) -> bool {
        match place {
            Place::Own(slot) => {
                let slot = usize::from(slot);
                if ARMOUR_SLOTS.contains(&slot) {
                    fits(ItemStack::new(item, 1), slot - SLOTS)
                } else {
                    slot < SLOTS
                }
            }
            Place::Container(slot) => self
                .container
                .as_ref()
                .is_some_and(|container| container.accepts(usize::from(slot), item)),
            Place::Grid(cell) => self.in_grid(cell),
            Place::Output => false,
        }
    }

    // The stack at `place`, or none if there's no such slot.
    fn get(&self, place: Place) -> Option<Option<ItemStack>> {
        match place {
            Place::Own(slot) if usize::from(slot) < ARMOUR_SLOTS.end => {
                Some(self.inventory.get(usize::from(slot)))
            }
            Place::Own(_) => None,
            Place::Container(slot) => {
                let container = self.container.as_ref()?;
                container.slots.get(usize::from(slot)).copied()
            }
            Place::Grid(cell) if self.in_grid(cell) => Some(self.inventory.grid[usize::from(cell)]),
            Place::Grid(_) => None,
            Place::Output => Some(self.recipe().map(Recipe::output)),
        }
    }

    // The slot at `place`, to change, but for the output, which is only ever made.
    fn slot_mut(&mut self, place: Place) -> Option<&mut Option<ItemStack>> {
        match place {
            Place::Own(slot) => {
                let slot = usize::from(slot);
                match slot.checked_sub(SLOTS) {
                    None => self.inventory.slots.get_mut(slot),
                    Some(piece) => self.inventory.armour.get_mut(piece),
                }
            }
            Place::Container(slot) => {
                let container = self.container.as_deref_mut()?;
                container.slots.get_mut(usize::from(slot))
            }
            Place::Grid(cell) if self.in_grid(cell) => {
                self.inventory.grid.get_mut(usize::from(cell))
            }
            Place::Grid(_) | Place::Output => None,
        }
    }
}

// Shift-clicking a stack in the inventory with a container open moves it into the container,
// if it goes there, and otherwise between the hotbar and the main inventory as ever. Armour
// worn comes off into the inventory, as it does with nothing open.
fn shift_into(container: &mut Container, inventory: &mut Inventory, slot: usize) {
    let Some(stack) = inventory.slots.get_mut(slot).and_then(Option::take) else {
        inventory.click(slot, Click::Shift);
        return;
    };
    let left = container.add(stack);
    inventory.slots[slot] = left;
    if left == Some(stack) {
        inventory.click(slot, Click::Shift);
    }
}

#[cfg(test)]
mod tests {
    use super::{Place, Slots, Spread};
    use crate::block::BlockType;
    use crate::containers::{Container, FUEL, INPUT, OUTPUT};
    use crate::crafting::{Recipe, Station};
    use crate::inventory::{Click, Inventory, ARMOUR_SLOTS, SLOTS};
    use crate::items::{Item, ItemStack};

    const DIRT: Item = Item::Block(BlockType::Dirt);
    const SAND: Item = Item::Block(BlockType::Sand);
    const IRON_ORE: Item = Item::Block(BlockType::IronOre);

    fn stack(item: Item, count: u8) -> Option<ItemStack> {
        Some(ItemStack::new(item, count))
    }

    fn own(slot: usize) -> Place {
        Place::Own(slot as u8)
    }

    fn drag(inventory: &mut Inventory, places: &[Place], spread: Spread) -> bool {
        let mut slots = Slots {
            inventory,
            container: None,
            station: Station::Hand,
        };
        slots.drag(places, spread)
    }

    #[test]
    fn test_a_left_drag_shares_the_cursor_evenly_and_keeps_the_remainder() {
        let mut inventory = Inventory {
            cursor: stack(DIRT, 10),
            ..Inventory::default()
        };
        assert!(drag(
            &mut inventory,
            &[own(9), own(10), own(11)],
            Spread::Even
        ));
        let shared = [inventory.slots[9], inventory.slots[10], inventory.slots[11]];
        assert_eq!([stack(DIRT, 3); 3], shared);
        assert_eq!(stack(DIRT, 1), inventory.cursor);
        // and when it shares exactly, the cursor's emptied
        let mut inventory = Inventory {
            cursor: stack(DIRT, 4),
            ..Inventory::default()
        };
        assert!(drag(&mut inventory, &[own(0), own(1)], Spread::Even));
        assert_eq!(
            (stack(DIRT, 2), None),
            (inventory.slots[1], inventory.cursor)
        );
    }

    #[test]
    fn test_a_drag_tops_up_stacks_only_to_a_full_stack() {
        let mut inventory = Inventory {
            cursor: stack(DIRT, 40),
            ..Inventory::default()
        };
        inventory.slots[0] = stack(DIRT, 60);
        inventory.slots[1] = stack(DIRT, 64);
        assert!(drag(
            &mut inventory,
            &[own(0), own(1), own(2)],
            Spread::Even
        ));
        // 13 each would go, but the first has room for 4 and the second for none
        assert_eq!(stack(DIRT, 64), inventory.slots[0]);
        assert_eq!(stack(DIRT, 64), inventory.slots[1]);
        assert_eq!(stack(DIRT, 13), inventory.slots[2]);
        assert_eq!(stack(DIRT, 40 - 4 - 13), inventory.cursor);
    }

    #[test]
    fn test_a_right_drag_drops_one_in_each_slot() {
        let mut inventory = Inventory {
            cursor: stack(DIRT, 5),
            ..Inventory::default()
        };
        inventory.slots[4] = stack(DIRT, 2);
        assert!(drag(&mut inventory, &[own(3), own(4), own(5)], Spread::One));
        assert_eq!(
            [stack(DIRT, 1), stack(DIRT, 3), stack(DIRT, 1)],
            [inventory.slots[3], inventory.slots[4], inventory.slots[5]]
        );
        assert_eq!(stack(DIRT, 2), inventory.cursor);
    }

    #[test]
    fn test_a_drag_over_another_item_or_a_slot_there_isnt_is_refused_whole() {
        let start = {
            let mut inventory = Inventory {
                cursor: stack(DIRT, 10),
                ..Inventory::default()
            };
            inventory.slots[2] = stack(SAND, 1);
            inventory
        };
        let refused = |places: &[Place], spread| {
            let mut inventory = start.clone();
            assert!(!drag(&mut inventory, places, spread), "{places:?}");
            assert_eq!(start, inventory);
        };
        // mixed items
        refused(&[own(1), own(2)], Spread::Even);
        // a slot there isn't, crossed twice, or with no container open
        refused(&[own(1), own(ARMOUR_SLOTS.end)], Spread::Even);
        refused(&[own(1), own(3), own(1)], Spread::One);
        refused(&[own(1), Place::Container(0)], Spread::Even);
        // dirt isn't worn
        refused(&[own(1), own(SLOTS)], Spread::Even);
        // more slots than items, and none at all
        let eleven: Vec<_> = (9..20).map(own).collect();
        refused(&eleven, Spread::One);
        refused(&[], Spread::Even);
        // and with nothing on the cursor
        let mut empty = Inventory::default();
        assert!(!drag(&mut empty, &[own(0), own(1)], Spread::Even));
    }

    #[test]
    fn test_a_drag_into_a_furnace_goes_only_where_it_may() {
        let mut inventory = Inventory {
            cursor: stack(Item::Coal, 8),
            ..Inventory::default()
        };
        let mut furnace = Container::new(BlockType::Furnace).unwrap();
        let mut slots = Slots {
            inventory: &mut inventory,
            container: Some(&mut furnace),
            station: Station::Hand,
        };
        // never into the output
        let output = Place::Container(OUTPUT as u8);
        assert!(!slots.drag(&[Place::Container(FUEL as u8), output], Spread::Even));
        assert!(slots.drag(&[Place::Container(FUEL as u8), own(0)], Spread::Even));
        assert_eq!(stack(Item::Coal, 4), furnace.slots[FUEL]);
        assert_eq!(stack(Item::Coal, 4), inventory.slots[0]);
    }

    #[test]
    fn test_a_double_click_gathers_the_item_onto_the_cursor_partial_stacks_first() {
        let mut inventory = Inventory {
            cursor: stack(DIRT, 10),
            ..Inventory::default()
        };
        inventory.slots[0] = stack(DIRT, 64);
        inventory.slots[3] = stack(DIRT, 40);
        inventory.slots[12] = stack(DIRT, 20);
        inventory.slots[13] = stack(SAND, 20);
        let mut chest = Container::new(BlockType::Chest).unwrap();
        chest.slots[5] = stack(DIRT, 2);
        let mut slots = Slots {
            inventory: &mut inventory,
            container: Some(&mut chest),
            station: Station::Hand,
        };
        slots.click(own(3), Click::Gather);
        // the chest's 2, the main inventory's 20 and 32 of the hotbar's 40; the full stack's left
        assert_eq!(stack(DIRT, 64), inventory.cursor);
        assert_eq!(None, chest.slots[5]);
        assert_eq!(None, inventory.slots[12]);
        assert_eq!(stack(DIRT, 8), inventory.slots[3]);
        assert_eq!(stack(DIRT, 64), inventory.slots[0]);
        assert_eq!(stack(SAND, 20), inventory.slots[13]);
        // with only full stacks left, they're broken into
        inventory.cursor = stack(DIRT, 60);
        inventory.slots[3] = None;
        let mut slots = Slots {
            inventory: &mut inventory,
            container: None,
            station: Station::Hand,
        };
        slots.click(own(3), Click::Gather);
        assert_eq!(stack(DIRT, 64), inventory.cursor);
        assert_eq!(stack(DIRT, 60), inventory.slots[0]);
        // and with nothing on the cursor, nothing happens
        let before = Inventory::default();
        let mut inventory = before.clone();
        let mut slots = Slots {
            inventory: &mut inventory,
            container: None,
            station: Station::Hand,
        };
        slots.click(own(0), Click::Gather);
        assert_eq!(before, inventory);
    }

    #[test]
    fn test_shift_click_moves_stacks_into_and_out_of_containers() {
        let mut inventory = Inventory::default();
        inventory.slots[0] = stack(IRON_ORE, 5);
        inventory.slots[1] = stack(Item::Coal, 3);
        inventory.slots[2] = stack(SAND, 7);
        let mut furnace = Container::new(BlockType::Furnace).unwrap();
        let mut slots = Slots {
            inventory: &mut inventory,
            container: Some(&mut furnace),
            station: Station::Hand,
        };
        // ore to the input, coal to the fire, and sand, which smelts, to the input too once it's free
        slots.click(own(0), Click::Shift);
        slots.click(own(1), Click::Shift);
        assert_eq!(
            stack(IRON_ORE, 5),
            slots.container.as_ref().unwrap().slots[INPUT]
        );
        assert_eq!(
            stack(Item::Coal, 3),
            slots.container.as_ref().unwrap().slots[FUEL]
        );
        // with the input taken, sand goes across to the main inventory instead
        slots.click(own(2), Click::Shift);
        assert_eq!(stack(SAND, 7), slots.inventory.slots[9]);
        // and back out of the furnace into the inventory
        slots.click(Place::Container(INPUT as u8), Click::Shift);
        assert_eq!(None, furnace.slots[INPUT]);
        assert_eq!(stack(IRON_ORE, 5), inventory.slots[0]);

        // a chest takes anything, topping up stacks first
        let mut chest = Container::new(BlockType::Chest).unwrap();
        chest.slots[4] = stack(SAND, 60);
        let mut slots = Slots {
            inventory: &mut inventory,
            container: Some(&mut chest),
            station: Station::Hand,
        };
        slots.click(own(9), Click::Shift);
        assert_eq!(stack(SAND, 64), chest.slots[4]);
        assert_eq!(stack(SAND, 3), chest.slots[0]);
        assert_eq!(None, inventory.slots[9]);
    }

    #[test]
    fn test_a_number_key_swaps_the_slot_with_that_hotbar_slot() {
        let mut inventory = Inventory::default();
        inventory.slots[2] = stack(DIRT, 5);
        inventory.slots[20] = stack(SAND, 9);
        let mut slots = Slots {
            inventory: &mut inventory,
            container: None,
            station: Station::Hand,
        };
        slots.click(own(20), Click::Swap(2));
        assert_eq!(stack(SAND, 9), slots.inventory.slots[2]);
        assert_eq!(stack(DIRT, 5), slots.inventory.slots[20]);
        // into an empty hotbar slot, it moves
        slots.click(own(20), Click::Swap(7));
        assert_eq!(
            (None, stack(DIRT, 5)),
            (inventory.slots[20], inventory.slots[7])
        );

        // armour swaps only with its own piece, or nothing
        inventory.armour[0] = stack(Item::IronHelmet, 1);
        let mut slots = Slots {
            inventory: &mut inventory,
            container: None,
            station: Station::Hand,
        };
        slots.click(own(SLOTS), Click::Swap(7));
        assert_eq!(stack(Item::IronHelmet, 1), slots.inventory.armour[0]);
        slots.click(own(SLOTS), Click::Swap(0));
        assert_eq!(None, slots.inventory.armour[0]);
        assert_eq!(stack(Item::IronHelmet, 1), slots.inventory.slots[0]);

        // a furnace's output only comes out, and its fuel slot takes only fuel
        let mut furnace = Container::new(BlockType::Furnace).unwrap();
        furnace.slots[OUTPUT] = stack(Item::IronIngot, 3);
        let mut slots = Slots {
            inventory: &mut inventory,
            container: Some(&mut furnace),
            station: Station::Hand,
        };
        slots.click(Place::Container(OUTPUT as u8), Click::Swap(7));
        assert_eq!(
            stack(Item::IronIngot, 3),
            slots.container.as_ref().unwrap().slots[OUTPUT]
        );
        slots.click(Place::Container(OUTPUT as u8), Click::Swap(1));
        assert_eq!(stack(Item::IronIngot, 3), slots.inventory.slots[1]);
        slots.click(Place::Container(FUEL as u8), Click::Swap(7));
        assert_eq!(None, furnace.slots[FUEL]);
        assert_eq!(stack(DIRT, 5), inventory.slots[7]);
    }

    #[test]
    fn test_q_over_a_slot_throws_one_or_the_stack() {
        let mut inventory = Inventory::default();
        inventory.slots[12] = stack(DIRT, 5);
        let mut chest = Container::new(BlockType::Chest).unwrap();
        chest.slots[0] = stack(SAND, 3);
        let mut slots = Slots {
            inventory: &mut inventory,
            container: Some(&mut chest),
            station: Station::Hand,
        };
        assert_eq!(
            stack(DIRT, 1),
            slots.click(own(12), Click::Throw { all: false })
        );
        let all = Click::Throw { all: true };
        assert_eq!(stack(SAND, 3), slots.click(Place::Container(0), all));
        assert_eq!(None, slots.click(Place::Container(0), all));
        assert_eq!(None, slots.click(own(99), all));
        assert_eq!(stack(DIRT, 4), inventory.slots[12]);
    }

    #[test]
    fn test_closing_with_a_stack_on_the_cursor_puts_it_back_or_hands_it_over_to_drop() {
        let mut inventory = Inventory {
            cursor: stack(DIRT, 10),
            ..Inventory::default()
        };
        inventory.slots[5] = stack(DIRT, 60);
        assert_eq!(None, inventory.put_back_cursor());
        assert_eq!(stack(DIRT, 64), inventory.slots[5]);
        assert_eq!(stack(DIRT, 6), inventory.slots[0]);
        inventory.slots = [stack(SAND, 64); SLOTS];
        inventory.cursor = stack(DIRT, 10);
        assert_eq!(stack(DIRT, 10), inventory.put_back_cursor());
        assert_eq!(None, inventory.cursor);
    }

    const PLANKS: Item = Item::Block(BlockType::Planks);
    const LOG: Item = Item::Block(BlockType::Log);

    fn at(inventory: &mut Inventory, station: Station) -> Slots<'_> {
        Slots {
            inventory,
            container: None,
            station,
        }
    }

    // A wooden pickaxe's planks and sticks laid in a table's grid, `count` in each cell.
    fn pickaxe_laid(inventory: &mut Inventory, count: u8) {
        for cell in [0, 1, 2] {
            inventory.grid[cell] = stack(PLANKS, count);
        }
        for cell in [4, 7] {
            inventory.grid[cell] = stack(Item::Stick, count);
        }
    }

    #[test]
    fn test_taking_the_output_uses_one_of_each_ingredient_and_makes_a_fresh_tool() {
        let mut inventory = Inventory::default();
        pickaxe_laid(&mut inventory, 2);
        let mut slots = at(&mut inventory, Station::CraftingTable);
        assert_eq!(Some(Recipe::WoodenPickaxe), slots.recipe());
        slots.click(Place::Output, Click::Left);
        assert_eq!(stack(Item::WoodenPickaxe, 1), inventory.cursor);
        assert_eq!(0, inventory.cursor.unwrap().wear);
        assert_eq!([stack(PLANKS, 1); 3], inventory.grid[..3]);
        assert_eq!(stack(Item::Stick, 1), inventory.grid[7]);
        // a pickaxe on the cursor doesn't stack with another, so nothing's made
        let before = inventory.clone();
        at(&mut inventory, Station::CraftingTable).click(Place::Output, Click::Left);
        assert_eq!(before, inventory);
        // by hand the same grid makes nothing
        inventory.cursor = None;
        let mut slots = at(&mut inventory, Station::Hand);
        assert_eq!(None, slots.recipe());
        assert_eq!(None, slots.click(Place::Output, Click::Left));
        assert_eq!(None, inventory.cursor);
    }

    #[test]
    fn test_taking_the_output_onto_the_same_item_tops_the_cursor_up() {
        let mut inventory = Inventory {
            cursor: stack(PLANKS, 58),
            ..Inventory::default()
        };
        inventory.grid[3] = stack(LOG, 3);
        let mut slots = at(&mut inventory, Station::Hand);
        slots.click(Place::Output, Click::Left);
        assert_eq!(stack(PLANKS, 62), slots.inventory.cursor);
        // with room for only two more, the next isn't taken
        slots.click(Place::Output, Click::Right);
        assert_eq!(stack(PLANKS, 62), inventory.cursor);
        assert_eq!(stack(LOG, 2), inventory.grid[3]);
    }

    #[test]
    fn test_shift_taking_makes_as_many_as_the_grid_holds_until_the_inventory_is_full() {
        let mut inventory = Inventory::default();
        inventory.grid[0] = stack(LOG, 20);
        at(&mut inventory, Station::Hand).click(Place::Output, Click::Shift);
        assert_eq!(None, inventory.grid[0]);
        assert_eq!(80, inventory.count(PLANKS));

        // into a full inventory, as many as fit whole
        let mut inventory = Inventory {
            slots: [stack(SAND, 64); SLOTS],
            ..Inventory::default()
        };
        inventory.slots[5] = stack(PLANKS, 55);
        inventory.grid[4] = stack(LOG, 5);
        at(&mut inventory, Station::Hand).click(Place::Output, Click::Shift);
        assert_eq!(stack(PLANKS, 63), inventory.slots[5]);
        assert_eq!(stack(LOG, 3), inventory.grid[4]);
        // and tools one at a time into empty slots
        let mut inventory = Inventory::default();
        pickaxe_laid(&mut inventory, 3);
        at(&mut inventory, Station::CraftingTable).click(Place::Output, Click::Shift);
        assert_eq!(3, inventory.count(Item::WoodenPickaxe));
        assert!(inventory.grid.iter().all(Option::is_none));
    }

    #[test]
    fn test_the_output_goes_to_an_empty_hotbar_slot_or_is_thrown() {
        let mut inventory = Inventory::default();
        inventory.slots[2] = stack(DIRT, 1);
        inventory.grid[0] = stack(LOG, 30);
        let mut slots = at(&mut inventory, Station::Hand);
        slots.click(Place::Output, Click::Swap(2));
        slots.click(Place::Output, Click::Swap(3));
        assert_eq!(stack(PLANKS, 4), slots.inventory.slots[3]);
        assert_eq!(stack(DIRT, 1), slots.inventory.slots[2]);
        let one = slots.click(Place::Output, Click::Throw { all: false });
        assert_eq!(stack(PLANKS, 4), one);
        // a stack's worth with Ctrl
        let all = slots.click(Place::Output, Click::Throw { all: true });
        assert_eq!(stack(PLANKS, 64), all);
        assert_eq!(stack(LOG, 30 - 2 - 16), inventory.grid[0]);
    }

    #[test]
    fn test_the_grid_takes_clicks_drags_and_keys_only_in_its_cells_shown() {
        let mut inventory = Inventory {
            cursor: stack(DIRT, 8),
            ..Inventory::default()
        };
        let mut slots = at(&mut inventory, Station::Hand);
        // the 2×2's cells are 0, 1, 3 and 4 of the 3×3
        assert!(slots.drag(
            &[
                Place::Grid(0),
                Place::Grid(1),
                Place::Grid(3),
                Place::Grid(4)
            ],
            Spread::Even
        ));
        assert_eq!([stack(DIRT, 2); 2], slots.inventory.grid[..2]);
        slots.inventory.cursor = stack(DIRT, 4);
        assert!(!slots.drag(&[Place::Grid(0), Place::Grid(2)], Spread::Even));
        assert!(!slots.drag(&[Place::Grid(0), Place::Output], Spread::Even));
        slots.click(Place::Grid(8), Click::Left);
        assert_eq!(stack(DIRT, 4), slots.inventory.cursor);
        // shift-click out of the grid into the inventory, a number key swaps, and Q throws
        slots.click(Place::Grid(0), Click::Shift);
        assert_eq!(stack(DIRT, 2), slots.inventory.slots[0]);
        slots.click(Place::Grid(1), Click::Swap(5));
        assert_eq!(stack(DIRT, 2), slots.inventory.slots[5]);
        assert_eq!(
            stack(DIRT, 1),
            slots.click(Place::Grid(3), Click::Throw { all: false })
        );
        assert_eq!(None, slots.inventory.grid[1]);
        // and with a chest open there's no grid at all
        let mut chest = Container::new(BlockType::Chest).unwrap();
        let mut slots = Slots {
            inventory: &mut inventory,
            container: Some(&mut chest),
            station: Station::Hand,
        };
        assert_eq!(
            None,
            slots.click(Place::Grid(4), Click::Throw { all: true })
        );
        assert_eq!(stack(DIRT, 2), inventory.grid[4]);
    }

    #[test]
    fn test_the_recipe_book_lays_a_recipe_from_the_inventory_once_more_each_click() {
        let mut inventory = Inventory::default();
        inventory.slots[0] = stack(PLANKS, 10);
        inventory.slots[12] = stack(Item::Stick, 5);
        inventory.grid[8] = stack(DIRT, 3);
        let mut slots = at(&mut inventory, Station::CraftingTable);
        assert!(slots.fill(Recipe::WoodenPickaxe, false));
        // the dirt went back first
        assert_eq!(stack(DIRT, 3), slots.inventory.slots[1]);
        assert_eq!(None, slots.inventory.grid[8]);
        assert_eq!(Some(Recipe::WoodenPickaxe), slots.recipe());
        assert_eq!([stack(PLANKS, 1); 3], slots.inventory.grid[..3]);
        // a second click lays a second set
        assert!(slots.fill(Recipe::WoodenPickaxe, false));
        assert_eq!(stack(PLANKS, 2), slots.inventory.grid[0]);
        assert_eq!(stack(Item::Stick, 2), slots.inventory.grid[7]);
        // shift lays as many as there's enough for, the sticks here
        assert!(slots.fill(Recipe::WoodenPickaxe, true));
        assert_eq!(stack(Item::Stick, 2), slots.inventory.grid[4]);
        assert_eq!(stack(PLANKS, 2), slots.inventory.grid[0]);
        assert_eq!(10 - 6, slots.inventory.count(PLANKS));
        assert_eq!(1, slots.inventory.count(Item::Stick));
        // a recipe there isn't enough for clears the grid and lays nothing
        assert!(!slots.fill(Recipe::Bed, false));
        assert!(slots.inventory.grid.iter().all(Option::is_none));
        assert_eq!(10, slots.inventory.count(PLANKS));
        // and a table's recipe isn't laid by hand
        let mut slots = at(&mut inventory, Station::Hand);
        assert!(!slots.fill(Recipe::WoodenPickaxe, false));
        assert!(slots.fill(Recipe::Sticks, true));
        assert_eq!([stack(PLANKS, 5), None], slots.inventory.grid[..2]);
        assert_eq!(stack(PLANKS, 5), slots.inventory.grid[3]);
    }

    #[test]
    fn test_shift_in_the_recipe_book_lays_up_to_a_full_stack_in_each_cell() {
        let mut inventory = Inventory::default();
        inventory.slots[..4].fill(stack(LOG, 64));
        let mut slots = at(&mut inventory, Station::Hand);
        assert!(slots.fill(Recipe::Planks, true));
        assert_eq!(stack(LOG, 64), slots.inventory.grid[0]);
        assert_eq!(64 * 3, slots.inventory.count(LOG));
        // with no room to put the grid back, nothing's laid and nothing's lost
        let mut inventory = Inventory {
            slots: [stack(SAND, 64); SLOTS],
            ..Inventory::default()
        };
        inventory.grid[0] = stack(DIRT, 1);
        let before = inventory.clone();
        assert!(!at(&mut inventory, Station::Hand).fill(Recipe::Planks, false));
        assert_eq!(before, inventory);
    }
}
