//! Trading with villagers (§5.8, M3.4): each kind of villager has a calling, the piece of a
//! village it works at and what it trades, as a row of the `creatures!` table (INV-6). A villager
//! offers a few of its calling's trades at a time, chosen afresh from its seed each day, each a
//! few times before it's sold out, and the server makes every exchange (`server::creatures`,
//! INV-4). Coins are the villages' money: villagers pay them for what a player brings, and take
//! them for what they make.

use serde::{Deserialize, Serialize};

use crate::block::BlockType::{Bed, Glass, GlassPane, Lantern, Thatch, Torch, WoolBlock};
use crate::chunks::generate::structures::village::Role;
use crate::inventory::Inventory;
use crate::items::Item::{self, Block, Coin};
use crate::items::ItemStack;

/// How many trades a villager offers at a time.
pub const OFFERED: usize = 3;

/// What a villager does: where it works, and the trades it chooses from.
#[derive(Debug, Clone, PartialEq)]
pub struct Calling {
    pub works_at: Role,
    pub trades: &'static [Offer],
}

/// A trade: what the villager takes, how many, and what it gives for them, and how many times it
/// makes it a day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub takes: (Item, u8),
    pub gives: (Item, u8),
    pub stock: u8,
}

impl Offer {
    /// Whether `inventory` holds what it takes.
    pub fn affordable(&self, inventory: &Inventory) -> bool {
        let (item, count) = self.takes;
        inventory.count(item) >= u32::from(count)
    }

    /// Takes what it takes from `inventory` and gives back what it gives, what doesn't fit
    /// handed back to be dropped, or refuses, changing nothing, if `inventory` hasn't enough.
    pub fn make(&self, inventory: &mut Inventory) -> Result<Option<ItemStack>, Unaffordable> {
        let (item, count) = self.takes;
        if !inventory.take_items(item, u32::from(count)) {
            return Err(Unaffordable);
        }
        let (item, count) = self.gives;
        Ok(inventory.add(ItemStack::new(item, count)))
    }
}

/// A trade refused as what it takes isn't carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unaffordable;

/// What a villager has offered today: the day it chose them, the trades it chose by their place
/// in its calling's, and how many of each it's made since.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stock {
    pub day: u64,
    pub chosen: [u8; OFFERED],
    pub sold: [u8; OFFERED],
}

impl Stock {
    /// The trades a villager of `calling` and `seed` offers on `day`, by a hash of the two, with
    /// none sold yet.
    pub fn of(calling: &Calling, seed: u64, day: u64) -> Self {
        let mut chosen = [0; OFFERED];
        let mut left: Vec<u8> = (0..calling.trades.len() as u8).collect();
        let mut draw = seed ^ day.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        for slot in chosen.iter_mut().take(left.len().min(OFFERED)) {
            draw = (draw ^ (draw >> 31)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            *slot = left.remove((draw % left.len() as u64) as usize);
        }
        Self {
            day,
            chosen,
            sold: [0; OFFERED],
        }
    }

    /// Its trades as of `day`, chosen again if they were chosen another day.
    pub fn restocked(self, calling: &Calling, seed: u64, day: u64) -> Self {
        if self.day == day {
            self
        } else {
            Self::of(calling, seed, day)
        }
    }

    /// Each trade offered and how many more times it'll be made.
    pub fn offers(&self, calling: &Calling) -> [(Offer, u8); OFFERED] {
        std::array::from_fn(|i| {
            let offer = calling.trades[self.chosen[i] as usize];
            (offer, offer.stock.saturating_sub(self.sold[i]))
        })
    }
}

// What each calling trades, a coin being worth about what a few minutes' work gets.
const fn offer(takes: (Item, u8), gives: (Item, u8), stock: u8) -> Offer {
    Offer {
        takes,
        gives,
        stock,
    }
}

/// Farmers buy wheat and sell food.
pub const FARMER: &[Offer] = &[
    offer((Item::Wheat, 16), (Coin, 1), 6),
    offer((Item::Seeds, 24), (Coin, 1), 4),
    offer((Coin, 1), (Item::Bread, 4), 6),
    offer((Coin, 1), (Item::Apple, 5), 4),
    offer((Coin, 2), (Block(Thatch), 6), 4),
    offer((Item::Beef, 6), (Coin, 1), 3),
];

/// Smiths buy coal and iron and sell what they make of them.
pub const SMITH: &[Offer] = &[
    offer((Item::Coal, 12), (Coin, 1), 6),
    offer((Item::IronIngot, 3), (Coin, 1), 4),
    offer((Coin, 3), (Item::IronPickaxe, 1), 1),
    offer((Coin, 4), (Item::IronSword, 1), 1),
    offer((Coin, 6), (Item::IronChestplate, 1), 1),
    offer((Coin, 2), (Item::Shears, 1), 2),
];

/// Traders buy wool and leather and sell what comes from far off.
pub const TRADER: &[Offer] = &[
    offer((Item::Wool, 10), (Coin, 1), 6),
    offer((Item::Leather, 6), (Coin, 1), 4),
    offer((Coin, 1), (Block(Glass), 8), 4),
    offer((Coin, 8), (Item::GoldIngot, 1), 2),
    offer((Coin, 8), (Item::FrostCrystal, 1), 2),
    offer((Coin, 8), (Item::Jade, 1), 2),
];

/// Shepherds buy wheat for their flocks and sell their wool and mutton.
pub const SHEPHERD: &[Offer] = &[
    offer((Item::Wheat, 12), (Coin, 1), 4),
    offer((Coin, 1), (Item::Wool, 6), 6),
    offer((Coin, 1), (Block(WoolBlock), 2), 4),
    offer((Coin, 1), (Item::CookedMutton, 3), 4),
    offer((Coin, 2), (Block(Bed), 1), 2),
];

/// Lamplighters buy coal and sell light.
pub const LAMPLIGHTER: &[Offer] = &[
    offer((Item::Coal, 10), (Coin, 1), 6),
    offer((Block(Torch), 32), (Coin, 1), 4),
    offer((Coin, 1), (Block(Torch), 16), 6),
    offer((Coin, 2), (Block(Lantern), 3), 4),
    offer((Coin, 1), (Block(GlassPane), 16), 4),
];

#[cfg(test)]
mod tests {
    use super::*;

    const CALLING: Calling = Calling {
        works_at: Role::Farm,
        trades: FARMER,
    };

    #[test]
    fn test_a_villager_offers_a_few_different_trades_the_same_all_day_and_others_the_next() {
        let today = Stock::of(&CALLING, 7, 3);
        assert_eq!(today, Stock::of(&CALLING, 7, 3));
        let mut chosen = today.chosen.to_vec();
        chosen.sort_unstable();
        chosen.dedup();
        assert_eq!(OFFERED, chosen.len());
        // the days and villagers differ
        let days: Vec<_> = (0..20)
            .map(|day| Stock::of(&CALLING, 7, day).chosen)
            .collect();
        assert!(days.iter().any(|d| *d != today.chosen));
        let sold = Stock {
            sold: [1, 0, 0],
            ..today
        };
        assert_eq!(sold, sold.restocked(&CALLING, 7, 3));
        assert_eq!([0; OFFERED], sold.restocked(&CALLING, 7, 4).sold);
        let (offer, left) = sold.offers(&CALLING)[0];
        assert_eq!(offer.stock - 1, left);
    }

    #[test]
    fn test_a_trade_takes_its_price_and_gives_its_goods_or_changes_nothing() {
        let offer = FARMER[0];
        let mut inventory = Inventory::default();
        inventory.add(ItemStack::new(Item::Wheat, 20));
        assert!(offer.affordable(&inventory));
        assert_eq!(Ok(None), offer.make(&mut inventory));
        assert_eq!(4, inventory.count(Item::Wheat));
        assert_eq!(1, inventory.count(Coin));
        assert!(!offer.affordable(&inventory));
        let before = inventory.clone();
        assert_eq!(Err(Unaffordable), offer.make(&mut inventory));
        assert_eq!(before, inventory);
    }

    #[test]
    fn test_every_trade_is_fair_and_whole() {
        for trades in [FARMER, SMITH, TRADER, SHEPHERD, LAMPLIGHTER] {
            assert!(trades.len() > OFFERED);
            // each either buys for coins or sells for them
            for offer in trades {
                assert!(
                    (offer.takes.0 == Coin) != (offer.gives.0 == Coin),
                    "{offer:?}"
                );
                for (item, count) in [offer.takes, offer.gives] {
                    assert!(count >= 1 && count <= item.max_stack(), "{offer:?}");
                }
                assert!(offer.stock >= 1);
            }
            // and buys something, so coins can be had from every one
            assert!(trades.iter().any(|offer| offer.gives.0 == Coin));
        }
    }
}
