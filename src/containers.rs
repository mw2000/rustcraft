//! Containers (§5.2): blocks that hold items of their own, which stay with the world rather than
//! with a player: the furnace, which smelts what's put in it with the fuel it's given, and the
//! chest, which only holds things. A chest the world generated is filled from its loot table the
//! first time it's opened (`loot`, §5.6), and is like any other from then on. The server keeps them
//! (`Containers`, INV-4) beside the edit layer, as the state of the blocks they're in, and saves
//! them in the region files with their chunks' edits (`save::region`). Players with one open are
//! sent it as it changes (`server::containers`).

use bevy::math::{I64Vec3, U16Vec3};
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

use crate::block::BlockType;
use crate::chunks::chunk::ChunkCoordinate;
use crate::crafting::{burn_time, Recipe, SMELT_TIME};
use crate::inventory::{add_to, click_stack, Click, Inventory};
use crate::items::{take, Item, ItemStack};

/// A furnace's slots: what's smelted, what's burnt, and what's made.
pub const INPUT: usize = 0;
pub const FUEL: usize = 1;
pub const OUTPUT: usize = 2;

/// What a container block holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Container {
    /// The block it's in, which says what kind of container it is.
    pub block: BlockType,
    pub slots: Vec<Option<ItemStack>>,
    /// A furnace's fire and how far it's got with what it's smelting.
    pub fire: Fire,
}

/// A furnace's fire, in seconds.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Fire {
    /// How long the fuel last put on has left to burn.
    pub left: f32,
    /// How long that fuel burns in all, which the flame shows what's left against.
    pub lasts: f32,
    /// How long the item in the input slot has been smelting, of `SMELT_TIME`.
    pub smelted: f32,
}

impl Fire {
    /// How much of the fuel's left, from 1 as it's lit to 0 when it's out, as its flame shows.
    pub fn flame(self) -> f32 {
        if self.lasts > 0.0 {
            self.left / self.lasts
        } else {
            0.0
        }
    }

    /// How far the item being smelted has got, from 0 to 1, as its arrow shows.
    pub fn progress(self) -> f32 {
        self.smelted / SMELT_TIME
    }
}

impl Container {
    /// An empty container for `block`, if it's a block that holds things.
    pub fn new(block: BlockType) -> Option<Self> {
        let slots = match block {
            BlockType::Furnace => 3,
            BlockType::Chest => 27,
            _ => return None,
        };
        Some(Self {
            block,
            slots: vec![None; slots],
            fire: Fire::default(),
        })
    }

    /// Whether it has the slots its block's containers have, as one loaded must.
    pub fn is_whole(&self) -> bool {
        Container::new(self.block).is_some_and(|empty| empty.slots.len() == self.slots.len())
    }

    // Whether `item` can be put in `slot`: only fuel in a furnace's fuel slot, and nothing in its
    // output, which is only taken from. A chest takes anything anywhere.
    fn accepts(&self, slot: usize, item: Item) -> bool {
        if self.block != BlockType::Furnace {
            return true;
        }
        match slot {
            FUEL => burn_time(item).is_some(),
            OUTPUT => false,
            _ => true,
        }
    }

    /// Does what clicking its slot `slot` does, with the stack on `inventory`'s cursor: as an
    /// inventory's slot, but for what it won't accept. A furnace's output is only ever taken,
    /// all of it at once if the cursor has room, and shift-click moves a stack into the
    /// inventory.
    pub fn click(&mut self, slot: usize, click: Click, inventory: &mut Inventory) {
        let accepted = inventory
            .cursor
            .is_none_or(|carried| self.accepts(slot, carried.item));
        let Some(held) = self.slots.get_mut(slot) else {
            return;
        };
        match click {
            Click::Shift => {
                if let Some(stack) = held.take() {
                    *held = inventory.add(stack);
                }
            }
            _ if self.block == BlockType::Furnace && slot == OUTPUT => {
                let Some(made) = *held else {
                    return;
                };
                match &mut inventory.cursor {
                    None => inventory.cursor = held.take(),
                    Some(carried) if carried.stacks_with(made) && carried.room() >= made.count => {
                        carried.merge(made);
                        *held = None;
                    }
                    Some(_) => {}
                }
            }
            _ if accepted => click_stack(held, &mut inventory.cursor, click),
            _ => {}
        }
    }

    /// Moves as much of `stack` in as fits where it goes, as shift-clicking it in the inventory
    /// does: into a chest as into an inventory, and into a furnace what smelts into the input
    /// and fuel onto the fire. Returns what's left of it.
    pub fn add(&mut self, stack: ItemStack) -> Option<ItemStack> {
        if self.block != BlockType::Furnace {
            return add_to(&mut self.slots, stack);
        }
        let slot = if Recipe::smelting(stack.item).is_some() {
            INPUT
        } else if burn_time(stack.item).is_some() {
            FUEL
        } else {
            return Some(stack);
        };
        match &mut self.slots[slot] {
            Some(held) => held.merge(stack),
            empty => {
                *empty = Some(stack);
                None
            }
        }
    }

    // What smelting the input makes, if it smelts and there's room for it in the output.
    fn smelts_into(&self) -> Option<ItemStack> {
        let input = self.slots.get(INPUT).copied().flatten()?;
        let made = Recipe::smelting(input.item)?.output();
        let fits = self.slots[OUTPUT]
            .is_none_or(|output| output.stacks_with(made) && output.room() >= made.count);
        fits.then_some(made)
    }

    /// Runs a furnace for `seconds`: its fire burns down, smelting an item from the input into
    /// the output every `SMELT_TIME` there's room for it, and once it's out it takes the next
    /// fuel, but only if there's something to smelt. An item part smelted as the fire goes out,
    /// or that's taken out, starts again. Returns whether it's still burning or could light,
    /// and so needs running on. However long it's run for, it ends as it would have frame by
    /// frame.
    pub fn run(&mut self, mut seconds: f32) -> bool {
        if self.block != BlockType::Furnace {
            return false;
        }
        while seconds > 0.0 {
            let made = self.smelts_into();
            if self.fire.left <= 0.0 && (made.is_none() || !self.light()) {
                self.fire = Fire::default();
                return false;
            }
            // on to whichever comes first: the time's up, the fuel's burnt, or the item's done
            let mut step = seconds.min(self.fire.left);
            if made.is_some() {
                step = step.min(SMELT_TIME - self.fire.smelted);
            }
            seconds -= step;
            self.fire.left -= step;
            let Some(made) = made else {
                self.fire.smelted = 0.0;
                continue;
            };
            self.fire.smelted += step;
            if self.fire.smelted >= SMELT_TIME {
                self.fire.smelted = 0.0;
                take(&mut self.slots[INPUT], 1);
                match &mut self.slots[OUTPUT] {
                    Some(output) => output.count += made.count,
                    empty => *empty = Some(made),
                }
            }
        }
        self.fire.left > 0.0 || (self.smelts_into().is_some() && self.slots[FUEL].is_some())
    }

    // Puts the next fuel on the fire, if there's any.
    fn light(&mut self) -> bool {
        let Some(fuel) = self.slots[FUEL] else {
            return false;
        };
        let Some(lasts) = burn_time(fuel.item) else {
            return false;
        };
        take(&mut self.slots[FUEL], 1);
        self.fire.left = lasts;
        self.fire.lasts = lasts;
        true
    }
}

/// Every container in the world, by chunk and where in it, as the edit layer keeps its edits.
/// Only the server has them; a client sees the one it has open (`ContainerChanged`).
///
/// A furnace only reads and writes its own slots, so the server runs every one that's burning,
/// wherever it is and whether its chunk's loaded or not: one left behind smelts on as if its
/// player had stayed, and needs no catching up when they come back. Time doesn't pass for them
/// while the world's closed, as it doesn't for anything else.
#[derive(Resource, Default)]
pub struct Containers {
    chunks: HashMap<ChunkCoordinate, HashMap<U16Vec3, Container>>,
    // the chunks whose containers changed since saving last asked
    changed: HashSet<ChunkCoordinate>,
    // the containers that might be smelting: those burning, and any changed since they last
    // went out, which `run` looks at and lets go of again once they're out for good
    lit: HashSet<I64Vec3>,
}

impl Containers {
    /// The container at `at`, if there's one.
    pub fn get(&self, at: I64Vec3) -> Option<&Container> {
        let local = ChunkCoordinate::local_block(at);
        self.chunks
            .get(&ChunkCoordinate::containing(at))?
            .get(&local)
    }

    /// The container at `at` to change, if there's one.
    pub fn get_mut(&mut self, at: I64Vec3) -> Option<&mut Container> {
        let coord = ChunkCoordinate::containing(at);
        let container = self
            .chunks
            .get_mut(&coord)?
            .get_mut(&ChunkCoordinate::local_block(at))?;
        self.changed.insert(coord);
        self.lit.insert(at);
        Some(container)
    }

    /// The container in `block` at `at`, made empty if it's never been used, or none if `block`
    /// doesn't hold things.
    pub fn open(&mut self, at: I64Vec3, block: BlockType) -> Option<&mut Container> {
        if self
            .get(at)
            .is_none_or(|container| container.block != block)
        {
            let container = Container::new(block)?;
            self.put(at, container);
        }
        self.get_mut(at)
    }

    /// Takes the container at `at` away, as its block's broken.
    pub fn remove(&mut self, at: I64Vec3) -> Option<Container> {
        let coord = ChunkCoordinate::containing(at);
        let container = self
            .chunks
            .get_mut(&coord)?
            .remove(&ChunkCoordinate::local_block(at))?;
        self.changed.insert(coord);
        self.lit.remove(&at);
        Some(container)
    }

    /// Puts back a container loaded from a save, which as it's already saved isn't a change,
    /// lighting it again if it was burning.
    pub fn put(&mut self, at: I64Vec3, container: Container) {
        if container.fire.left > 0.0 {
            self.lit.insert(at);
        }
        let chunk = self.chunks.entry(ChunkCoordinate::containing(at));
        chunk
            .or_default()
            .insert(ChunkCoordinate::local_block(at), container);
    }

    /// The chunks whose containers changed since this was last called.
    pub fn take_changed(&mut self) -> HashSet<ChunkCoordinate> {
        std::mem::take(&mut self.changed)
    }

    /// The chunks with containers in them, in no particular order.
    pub fn chunks(&self) -> impl Iterator<Item = ChunkCoordinate> + '_ {
        self.chunks.keys().copied()
    }

    /// The containers in the chunk at `coord`, in no particular order.
    pub fn in_chunk(
        &self,
        coord: ChunkCoordinate,
    ) -> impl Iterator<Item = (U16Vec3, &Container)> + '_ {
        self.chunks
            .get(&coord)
            .into_iter()
            .flatten()
            .map(|(&l, c)| (l, c))
    }

    /// Runs every furnace that might be smelting for `seconds`, see `Container::run`.
    pub fn run(&mut self, seconds: f32) {
        let (chunks, changed) = (&mut self.chunks, &mut self.changed);
        self.lit.retain(|&at| {
            let coord = ChunkCoordinate::containing(at);
            let local = ChunkCoordinate::local_block(at);
            let Some(container) = chunks.get_mut(&coord).and_then(|c| c.get_mut(&local)) else {
                return false;
            };
            changed.insert(coord);
            container.run(seconds)
        });
    }
}

#[cfg(test)]
mod tests {
    use bevy::math::I64Vec3;

    use super::{Container, Containers, Fire, FUEL, INPUT, OUTPUT};
    use crate::block::BlockType;
    use crate::crafting::SMELT_TIME;
    use crate::inventory::{Click, Inventory};
    use crate::items::{Item, ItemStack};

    const IRON_ORE: Item = Item::Block(BlockType::IronOre);
    const PLANKS: Item = Item::Block(BlockType::Planks);

    fn stack(item: Item, count: u8) -> Option<ItemStack> {
        Some(ItemStack::new(item, count))
    }

    fn furnace(input: Option<ItemStack>, fuel: Option<ItemStack>) -> Container {
        let mut furnace = Container::new(BlockType::Furnace).unwrap();
        furnace.slots[INPUT] = input;
        furnace.slots[FUEL] = fuel;
        furnace
    }

    #[test]
    fn test_furnaces_and_chests_are_containers() {
        let furnace = Container::new(BlockType::Furnace).unwrap();
        assert_eq!(3, furnace.slots.len());
        assert!(furnace.is_whole());
        assert_eq!(27, Container::new(BlockType::Chest).unwrap().slots.len());
        assert_eq!(None, Container::new(BlockType::CraftingTable));
        let short = Container {
            slots: vec![None; 2],
            ..furnace
        };
        assert!(!short.is_whole());
    }

    #[test]
    fn test_smelting_burns_fuel_and_turns_the_input_into_the_output_over_time() {
        // a coal smelts 8
        let mut furnace = furnace(stack(IRON_ORE, 10), stack(Item::Coal, 1));
        assert!(furnace.run(SMELT_TIME / 2.0));
        assert_eq!(
            (stack(IRON_ORE, 10), None),
            (furnace.slots[INPUT], furnace.slots[FUEL])
        );
        assert_eq!(0.5, furnace.fire.progress());
        assert_eq!(1.0 - 1.0 / 16.0, furnace.fire.flame());
        assert!(furnace.run(SMELT_TIME / 2.0));
        assert_eq!(stack(Item::IronIngot, 1), furnace.slots[OUTPUT]);
        assert_eq!(stack(IRON_ORE, 9), furnace.slots[INPUT]);
        // and with no more fuel, it stops once the coal's burnt, eight smelted
        assert!(!furnace.run(SMELT_TIME * 20.0));
        assert_eq!(stack(Item::IronIngot, 8), furnace.slots[OUTPUT]);
        assert_eq!(stack(IRON_ORE, 2), furnace.slots[INPUT]);
        assert_eq!(Fire::default(), furnace.fire);
    }

    #[test]
    fn test_a_long_run_ends_as_frame_by_frame_does() {
        let start = furnace(
            stack(Item::Block(BlockType::Cobblestone), 30),
            stack(PLANKS, 9),
        );
        let mut at_once = start.clone();
        at_once.run(1000.0);
        let mut by_frames = start;
        for _ in 0..60_000 {
            by_frames.run(1.0 / 60.0);
        }
        // 9 planks smelt 13 and a half
        assert_eq!(
            stack(Item::Block(BlockType::Stone), 13),
            at_once.slots[OUTPUT]
        );
        assert_eq!(at_once.slots, by_frames.slots);
        assert_eq!(Fire::default(), by_frames.fire);
    }

    #[test]
    fn test_nothing_burns_without_something_to_smelt_or_room_for_it() {
        let mut idle = furnace(stack(Item::Block(BlockType::Dirt), 4), stack(Item::Coal, 2));
        assert!(!idle.run(100.0));
        assert_eq!(stack(Item::Coal, 2), idle.slots[FUEL]);

        let mut full = furnace(stack(IRON_ORE, 4), stack(Item::Coal, 2));
        full.slots[OUTPUT] = Some(ItemStack::full(Item::IronIngot));
        assert!(!full.run(100.0));
        assert_eq!(stack(Item::Coal, 2), full.slots[FUEL]);
        // and one that fills up as it goes stops there, its fire burning down
        full.slots[OUTPUT] = stack(Item::IronIngot, 63);
        assert!(full.run(SMELT_TIME * 2.0));
        assert_eq!(Some(ItemStack::full(Item::IronIngot)), full.slots[OUTPUT]);
        assert_eq!(
            (stack(IRON_ORE, 3), 0.0),
            (full.slots[INPUT], full.fire.smelted)
        );
        assert!(!full.run(SMELT_TIME * 10.0));
        assert_eq!(stack(Item::Coal, 1), full.slots[FUEL]);
    }

    #[test]
    fn test_clicks_put_only_fuel_on_the_fire_and_only_take_from_the_output() {
        let mut furnace = furnace(None, None);
        furnace.slots[OUTPUT] = stack(Item::CopperIngot, 3);
        let mut inventory = Inventory {
            cursor: stack(IRON_ORE, 5),
            ..Inventory::default()
        };
        furnace.click(FUEL, Click::Left, &mut inventory);
        furnace.click(OUTPUT, Click::Left, &mut inventory);
        assert_eq!(
            (None, stack(IRON_ORE, 5)),
            (furnace.slots[FUEL], inventory.cursor)
        );
        furnace.click(INPUT, Click::Right, &mut inventory);
        assert_eq!(stack(IRON_ORE, 1), furnace.slots[INPUT]);
        inventory.cursor = stack(Item::Coal, 2);
        furnace.click(FUEL, Click::Left, &mut inventory);
        assert_eq!(
            (stack(Item::Coal, 2), None),
            (furnace.slots[FUEL], inventory.cursor)
        );
        // the output's taken whole, onto the cursor, and shift-clicked into the inventory
        furnace.click(OUTPUT, Click::Right, &mut inventory);
        assert_eq!(
            (None, stack(Item::CopperIngot, 3)),
            (furnace.slots[OUTPUT], inventory.cursor)
        );
        furnace.click(FUEL, Click::Shift, &mut inventory);
        assert_eq!(
            (None, stack(Item::Coal, 2)),
            (furnace.slots[FUEL], inventory.slots[0])
        );
    }

    #[test]
    fn test_shift_clicked_in_ore_goes_to_the_input_and_fuel_to_the_fire() {
        let mut furnace = furnace(stack(IRON_ORE, 60), None);
        assert_eq!(
            stack(IRON_ORE, 6),
            furnace.add(ItemStack::new(IRON_ORE, 10))
        );
        assert_eq!(None, furnace.add(ItemStack::new(Item::Stick, 3)));
        assert_eq!(stack(Item::Stick, 3), furnace.slots[FUEL]);
        let dirt = ItemStack::new(Item::Block(BlockType::Dirt), 1);
        assert_eq!(Some(dirt), furnace.add(dirt));
    }

    #[test]
    fn test_a_chest_takes_anything_in_any_slot_and_shift_clicks_both_ways() {
        let mut chest = Container::new(BlockType::Chest).unwrap();
        let mut inventory = Inventory {
            cursor: stack(IRON_ORE, 5),
            ..Inventory::default()
        };
        // even in the slots a furnace keeps for fuel and what it's made
        chest.click(OUTPUT, Click::Right, &mut inventory);
        chest.click(FUEL, Click::Left, &mut inventory);
        assert_eq!(
            (stack(IRON_ORE, 4), stack(IRON_ORE, 1), None),
            (chest.slots[FUEL], chest.slots[OUTPUT], inventory.cursor)
        );
        chest.click(OUTPUT, Click::Left, &mut inventory);
        assert_eq!(stack(IRON_ORE, 1), inventory.cursor);
        // shift-clicked in, it tops up the stacks there are and then fills the first empty slot
        assert_eq!(None, chest.add(ItemStack::new(IRON_ORE, 62)));
        assert_eq!(Some(ItemStack::full(IRON_ORE)), chest.slots[FUEL]);
        assert_eq!(stack(IRON_ORE, 2), chest.slots[0]);
        assert_eq!(None, chest.add(ItemStack::new(Item::Coal, 3)));
        assert_eq!(stack(Item::Coal, 3), chest.slots[OUTPUT]);
        // and shift-clicked out, into the inventory
        chest.click(FUEL, Click::Shift, &mut inventory);
        assert_eq!(None, chest.slots[FUEL]);
        assert_eq!(Some(ItemStack::full(IRON_ORE)), inventory.slots[0]);
        // and it never burns
        assert!(!chest.run(SMELT_TIME));
    }

    #[test]
    fn test_containers_burn_on_where_they_are_until_theyre_out() {
        let mut containers = Containers::default();
        let at = I64Vec3::new(-20, 500, 7);
        assert!(containers.open(at, BlockType::CraftingTable).is_none());
        let furnace = containers.open(at, BlockType::Furnace).unwrap();
        furnace.slots[INPUT] = stack(IRON_ORE, 2);
        furnace.slots[FUEL] = stack(Item::Coal, 1);
        containers.take_changed();
        containers.run(SMELT_TIME * 3.0);
        let furnace = containers.get(at).unwrap();
        assert_eq!(stack(Item::IronIngot, 2), furnace.slots[OUTPUT]);
        // it's burning on with nothing to smelt, and saved as it goes
        assert!(furnace.fire.left > 0.0);
        assert_eq!(1, containers.take_changed().len());
        containers.run(SMELT_TIME * 10.0);
        assert!(containers.lit.is_empty());
        assert_eq!(
            Some(stack(Item::IronIngot, 2)),
            containers.remove(at).map(|c| c.slots[OUTPUT])
        );
        assert!(containers.get(at).is_none());
    }
}
