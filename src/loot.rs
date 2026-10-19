//! Loot (§5.6, INV-6): what chests found in the world hold. Every loot table is a row of
//! `TABLES`, and a structure's chests roll from the one with its name. A chest the world generated
//! holds nothing in the generated world (INV-1): the first time it's opened or broken, the server
//! rolls its table from the world's seed and where it stands (INV-2), so every player and every
//! reload agrees on what was in it, and keeps what came up as an ordinary container from then on
//! (`server::containers`).

use std::ops::RangeInclusive;

use bevy::math::{I64Vec3, Vec3Swizzles};
use rand::distr::weighted::WeightedIndex;
use rand::distr::Distribution;
use rand::rngs::Xoshiro256PlusPlus;
use rand::seq::IndexedRandom;
use rand::{RngExt, SeedableRng};

use crate::block::BlockType;
use crate::chunks::generate::region_store::RegionStore;
use crate::chunks::generate::structures::LOOT;
use crate::chunks::generate::trees::hash;
use crate::items::{Item, ItemStack};

/// A named list of what a chest might hold, and how many things it holds.
#[derive(Debug)]
pub struct LootTable {
    /// Its stable name: the name of the structure whose chests roll from it.
    pub name: &'static str,
    /// How many entries are drawn, each a stack in a slot of its own.
    rolls: RangeInclusive<u8>,
    entries: &'static [Entry],
}

/// Something a loot table can draw: how many of it, and how often it comes up, against the
/// weights of the table's other entries.
#[derive(Debug)]
struct Entry {
    item: Item,
    count: RangeInclusive<u8>,
    weight: u32,
}

const fn entry(item: Item, count: RangeInclusive<u8>, weight: u32) -> Entry {
    Entry {
        item,
        count,
        weight,
    }
}

const fn block(block: BlockType) -> Item {
    Item::Block(block)
}

use Item::*;

// One for each structure (§5.6), better the deeper or further out it's found: those near
// the surface and the start give food, coal and torches with a little metal, and only now and
// then a tool, and those deep down or far away iron and iron tools, and what the far climates'
// ores give (§5.3): each far structure what's found under its own climate, and the deepest
// embers and now and then bright steel.
pub const TABLES: &[LootTable] = &[
    // a forest ruin, found early
    LootTable {
        name: "forest_ruin",
        rolls: 3..=6,
        entries: &[
            entry(Coal, 2..=6, 10),
            entry(Bread, 1..=3, 8),
            entry(Apple, 1..=4, 8),
            entry(Seeds, 2..=8, 6),
            entry(block(BlockType::Torch), 2..=6, 6),
            entry(Stick, 2..=8, 5),
            entry(CopperIngot, 1..=3, 4),
            entry(IronIngot, 1..=2, 2),
            entry(StonePickaxe, 1..=1, 2),
            entry(IronAxe, 1..=1, 1),
        ],
    },
    // a lookout on a hilltop, kept in light and firewood
    LootTable {
        name: "watchtower",
        rolls: 3..=6,
        entries: &[
            entry(block(BlockType::Torch), 4..=10, 10),
            entry(Bread, 1..=4, 8),
            entry(Coal, 3..=8, 8),
            entry(block(BlockType::Log), 2..=6, 6),
            entry(CopperIngot, 1..=4, 6),
            entry(IronIngot, 1..=3, 4),
            entry(StoneAxe, 1..=1, 3),
            entry(CopperPickaxe, 1..=1, 2),
            entry(IronShovel, 1..=1, 1),
        ],
    },
    // a lodge on a glacier, far out and stocked for the cold
    LootTable {
        name: "glacier_outpost",
        rolls: 4..=7,
        entries: &[
            entry(Bread, 2..=5, 10),
            entry(Coal, 4..=10, 8),
            entry(block(BlockType::Log), 4..=10, 6),
            entry(block(BlockType::Torch), 4..=12, 6),
            entry(IronIngot, 2..=5, 6),
            entry(CopperIngot, 2..=6, 4),
            entry(block(BlockType::Bed), 1..=1, 2),
            entry(IronPickaxe, 1..=1, 2),
            entry(IronAxe, 1..=1, 2),
            entry(FrostCrystal, 1..=4, 4),
            entry(BrightSteelIngot, 1..=2, 1),
        ],
    },
    // old workings underground, left with what was dug
    LootTable {
        name: "mineshaft",
        rolls: 4..=7,
        entries: &[
            entry(Coal, 4..=12, 10),
            entry(block(BlockType::Torch), 4..=12, 8),
            entry(block(BlockType::CopperOre), 2..=6, 6),
            entry(Bread, 1..=3, 6),
            entry(block(BlockType::IronOre), 1..=4, 5),
            entry(block(BlockType::Planks), 4..=12, 5),
            entry(IronIngot, 1..=4, 4),
            entry(CopperPickaxe, 1..=1, 3),
            entry(IronPickaxe, 1..=1, 1),
        ],
    },
    // a vault deep underground
    LootTable {
        name: "dungeon",
        rolls: 4..=8,
        entries: &[
            entry(IronIngot, 2..=6, 8),
            entry(Coal, 4..=10, 6),
            entry(CopperIngot, 2..=6, 6),
            entry(Bread, 2..=4, 6),
            entry(Apple, 2..=5, 4),
            entry(IronPickaxe, 1..=1, 2),
            entry(IronAxe, 1..=1, 2),
            entry(IronShovel, 1..=1, 2),
            entry(IronHoe, 1..=1, 1),
            entry(Ember, 1..=3, 3),
            entry(GoldIngot, 1..=3, 2),
            entry(BrightSteelIngot, 1..=2, 2),
            entry(BrightSteelPickaxe, 1..=1, 1),
        ],
    },
    // a temple overgrown by the jungle, holding what's mined under it
    LootTable {
        name: "jungle_temple",
        rolls: 5..=8,
        entries: &[
            entry(Jade, 2..=6, 10),
            entry(IronIngot, 2..=6, 8),
            entry(Apple, 2..=5, 6),
            entry(CopperIngot, 2..=6, 5),
            entry(block(BlockType::Torch), 4..=8, 4),
            entry(IronAxe, 1..=1, 3),
            entry(IronPickaxe, 1..=1, 2),
            entry(GoldIngot, 1..=4, 3),
            entry(block(BlockType::PolishedJade), 1..=2, 2),
            entry(BrightSteelIngot, 1..=2, 1),
        ],
    },
    // a desert temple, the furthest out and the richest
    LootTable {
        name: "desert_temple",
        rolls: 5..=9,
        entries: &[
            entry(IronIngot, 3..=8, 10),
            entry(CopperIngot, 3..=8, 6),
            entry(Bread, 2..=5, 6),
            entry(block(BlockType::Torch), 4..=8, 4),
            entry(IronPickaxe, 1..=1, 3),
            entry(IronAxe, 1..=1, 3),
            entry(IronShovel, 1..=1, 2),
            entry(IronHoe, 1..=1, 1),
            entry(GoldIngot, 2..=6, 6),
            entry(Jade, 1..=4, 3),
            entry(BrightSteelIngot, 1..=3, 2),
            entry(BrightSteelAxe, 1..=1, 1),
        ],
    },
    // a village's houses, inn and smithy (M3.4), kept in food and a little money, which is
    // worth taking only once, as the chests aren't filled again
    LootTable {
        name: "village",
        rolls: 2..=5,
        entries: &[
            entry(Bread, 1..=4, 10),
            entry(Wheat, 2..=8, 8),
            entry(Apple, 1..=4, 6),
            entry(Seeds, 2..=8, 5),
            entry(block(BlockType::Torch), 2..=6, 5),
            entry(Coin, 1..=3, 4),
            entry(Coal, 2..=5, 4),
            entry(Wool, 1..=3, 3),
            entry(IronIngot, 1..=2, 2),
            entry(Shears, 1..=1, 1),
        ],
    },
];

impl LootTable {
    pub fn named(name: &str) -> Option<&'static LootTable> {
        TABLES.iter().find(|table| table.name == name)
    }

    /// What a chest of `slots` slots at `at` holds, in a world of seed `seed`: the table rolled
    /// from those alone, so it comes up the same every time. Each stack drawn lands in an empty
    /// slot picked at random, as if thrown in.
    pub fn roll(&self, seed: u32, at: I64Vec3, slots: usize) -> Vec<Option<ItemStack>> {
        let mut rng =
            Xoshiro256PlusPlus::seed_from_u64(hash(hash(seed.into(), at.x, at.y), at.z, 0));
        let weights = WeightedIndex::new(self.entries.iter().map(|entry| entry.weight))
            .expect("a loot table has entries, all weighted");
        let mut chest = vec![None; slots];
        for _ in 0..rng.random_range(self.rolls.clone()) {
            let entry = &self.entries[weights.sample(&mut rng)];
            let stack = ItemStack::new(entry.item, rng.random_range(entry.count.clone()));
            let empty: Vec<_> = (0..slots).filter(|&slot| chest[slot].is_none()).collect();
            let Some(&slot) = empty.choose(&mut rng) else {
                break;
            };
            chest[slot] = Some(stack);
        }
        chest
    }
}

/// The table the chest the world generated at `at` rolls from: the one named after the
/// structure whose loot marker put it there (§5.6), found from the seed as chunks find the
/// structures reaching into them. None for a chest no structure accounts for, which stays empty.
pub fn generated_table(regions: &RegionStore, at: I64Vec3) -> Option<&'static LootTable> {
    let column = at.xz();
    let structures = regions.structures_near(column, column);
    let placed = structures
        .iter()
        .find(|placed| placed.markers().any(|marker| marker == (LOOT, at)))?;
    LootTable::named(placed.loot_table())
}

/// Where the chest is in seed 3's watchtower nearest its spawn, for tests.
#[cfg(test)]
pub(crate) fn watchtower_chest(regions: &RegionStore) -> I64Vec3 {
    let spawn = regions.noise().find_spawn();
    let spawn = bevy::math::I64Vec2::new(spawn.0, spawn.1);
    let structures = regions.structures_near(spawn - 4096, spawn + 4096);
    let tower = structures
        .iter()
        .filter(|placed| placed.name() == "watchtower")
        .min_by_key(|placed| (placed.min.xz() - spawn).length_squared())
        .expect("seed 3 has a watchtower near its spawn");
    let mut chests = tower.markers().filter(|&(kind, _)| kind == LOOT);
    chests.next().expect("a watchtower has a chest").1
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use bevy::math::I64Vec3;

    use std::sync::Arc;

    use super::{generated_table, watchtower_chest, LootTable, TABLES};
    use crate::block::BlockType;
    use crate::chunks::chunk::ChunkCoordinate;
    use crate::chunks::generate::generator::generate_chunk;
    use crate::chunks::generate::noise::NoiseGenerator;
    use crate::chunks::generate::region_store::RegionStore;
    use crate::chunks::generate::structures::village::VILLAGES;
    use crate::chunks::generate::structures::{LOOT, STRUCTURES};
    use crate::containers::Container;

    #[test]
    fn test_loot_tables_have_unique_names_and_sensible_entries() {
        let chest = Container::new(BlockType::Chest).unwrap().slots.len();
        let mut names = HashSet::new();
        for table in TABLES {
            assert!(names.insert(table.name), "{} is used twice", table.name);
            assert_eq!(
                Some(table.name),
                LootTable::named(table.name).map(|t| t.name)
            );
            assert!(*table.rolls.start() >= 1 && !table.rolls.is_empty());
            assert!(usize::from(*table.rolls.end()) <= chest, "{}", table.name);
            assert!(!table.entries.is_empty());
            for entry in table.entries {
                let item = entry.item;
                // a real item, which a save can name, in stacks it comes in
                assert_eq!(Some(item), crate::items::Item::from_name(item.name()));
                assert!(entry.weight > 0, "{item:?} in {}", table.name);
                assert!(*entry.count.start() >= 1 && !entry.count.is_empty());
                assert!(*entry.count.end() <= item.max_stack(), "{item:?}");
            }
        }
        // each one a structure's, or villages', so none is left unrolled
        for table in TABLES {
            assert!(
                STRUCTURES.iter().any(|s| s.name == table.name) || table.name == VILLAGES.name,
                "{} is no structure's",
                table.name
            );
        }
        assert!(LootTable::named("treasure").is_none());
    }

    #[test]
    fn test_rolling_is_the_same_for_a_seed_and_place_and_differs_between_them() {
        let at = I64Vec3::new(120, 431, -77);
        for table in TABLES {
            let rolled = table.roll(7, at, 27);
            assert_eq!(rolled, table.roll(7, at, 27));
            let stacks = rolled.iter().flatten().count();
            assert!(table.rolls.contains(&(stacks as u8)), "{}", table.name);
        }
        let dungeon = LootTable::named("dungeon").unwrap();
        let elsewhere = [
            dungeon.roll(7, at + I64Vec3::X, 27),
            dungeon.roll(7, at + I64Vec3::Y, 27),
            dungeon.roll(7, at + I64Vec3::Z, 27),
            dungeon.roll(8, at, 27),
        ];
        let here = dungeon.roll(7, at, 27);
        for there in elsewhere {
            assert_ne!(here, there);
        }
    }

    #[test]
    fn test_every_structure_with_chests_has_a_table() {
        assert!(LootTable::named(&VILLAGES.name).is_some());
        for structure in STRUCTURES.iter() {
            let markers = &structure.template.markers;
            if markers.iter().any(|(kind, _)| kind == LOOT) {
                assert!(
                    LootTable::named(&structure.name).is_some(),
                    "{}",
                    structure.name
                );
            }
        }
    }

    #[test]
    fn test_a_watchtower_generates_a_chest_that_rolls_the_same_loot_every_time() {
        let regions = Arc::new(RegionStore::new(Arc::new(NoiseGenerator::new(3))));
        let at = watchtower_chest(&regions);
        let coord = ChunkCoordinate::containing(at);
        let chunk = generate_chunk(regions.clone(), coord);
        let local = ChunkCoordinate::local_block(at);
        assert_eq!(BlockType::Chest, chunk.get_block_at(local));
        let table = generated_table(&regions, at).unwrap();
        assert_eq!("watchtower", table.name);
        let rolled = table.roll(3, at, 27);
        assert!(rolled.iter().any(Option::is_some));
        // and from a world that's never placed it before, as a reloaded one hasn't
        let fresh = RegionStore::new(Arc::new(NoiseGenerator::new(3)));
        assert_eq!(rolled, generated_table(&fresh, at).unwrap().roll(3, at, 27));
        // but nothing else there has a table
        assert!(generated_table(&regions, at + I64Vec3::Y).is_none());
    }
}
