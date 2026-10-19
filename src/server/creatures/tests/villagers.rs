//! Villagers (M3.4) in the village nearest seed 3's spawn, generated as the world has it.

use std::collections::HashSet;
use std::sync::Arc;

use bevy::ecs::message::Messages;
use bevy::math::I64Vec2;
use bevy_replicon::prelude::ToClients;

use super::super::villagers::{Errand, Place, Townsfolk, Trading};
use super::super::{make_trades, open_trades, send_offers, settle, use_doors, Settled};
use super::*;
use crate::block::Block;
use crate::chunks::generate::generator::generate_chunk;
use crate::chunks::generate::structures::village::{Village, BED};
use crate::creatures::Villager;
use crate::protocol::{BlockChanged, CloseInventory, Trade, TradeOffers};
use crate::trading::Stock;

const VILLAGE_SEED: u32 = 3;

// The village nearest seed 3's spawn, and an App running creatures over its chunks as they
// generate, at `time` of day, unsettled.
fn village(time: f64) -> (App, Arc<Village>) {
    let mut world = World::from_save(VILLAGE_SEED, Edits::default(), 16);
    let regions = world.region_store.clone();
    let (x, z, _) = regions.noise().find_spawn();
    let spawn = I64Vec2::new(x, z);
    let mut villages = regions.villages_near(spawn - 2000, spawn + 2000);
    villages.sort_by_key(|v| (v.well().min.xz() - spawn).length_squared());
    let village = villages
        .first()
        .expect("a village near seed 3's spawn")
        .clone();
    let floor = village.well().min.y;
    let size = CHUNK_SIZE as i64;
    let (min, max) = (
        (village.min - size).div_euclid(I64Vec2::splat(size)),
        (village.max + size).div_euclid(I64Vec2::splat(size)),
    );
    for cx in min.x..=max.x {
        for cz in min.y..=max.y {
            for cy in (floor - 32).div_euclid(size)..=(floor + 32).div_euclid(size) {
                let coord = ChunkCoordinate(I64Vec3::new(cx, cy, cz));
                world.insert_chunk(coord, generate_chunk(regions.clone(), coord));
            }
        }
    }
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(world)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            FRAME,
        )))
        .insert_resource(Chance(StdRng::seed_from_u64(5)))
        .insert_resource(TimeOfDay::from_days(time))
        .insert_resource(SpawnRate(None))
        .init_resource::<Dormant>()
        .init_resource::<Spawners>()
        .init_resource::<Settled>()
        .add_message::<FromClient<HitCreature>>()
        .add_message::<FromClient<UseCreature>>()
        .add_message::<FromClient<Trade>>()
        .add_message::<FromClient<CloseInventory>>()
        .add_message::<ToClients<TradeOffers>>()
        .add_message::<ToClients<BlockChanged>>()
        .add_systems(
            Update,
            (
                wake,
                settle,
                take_paths,
                think,
                move_creatures,
                use_doors,
                sleep,
                receive_hits,
                (open_trades, make_trades).chain(),
                send_offers,
            )
                .chain(),
        );
    (app, village)
}

// Settles the village with a player standing at its well, returning the player.
fn settled(app: &mut App, village: &Village) -> Entity {
    let well = village.well();
    let middle = well.min.as_vec3() + (well.footprint().as_vec2() / 2.0).extend(0.0).xzy();
    let player = player(app, middle + Vec3::Y * 3.0, None);
    run(app, 2.5);
    player
}

fn villagers(app: &mut App) -> Vec<(Entity, Townsfolk)> {
    let mut query = app.world_mut().query::<(Entity, &Townsfolk)>();
    query.iter(app.world()).map(|(e, t)| (e, *t)).collect()
}

fn beds(village: &Village) -> usize {
    let markers = village.pieces.iter().flat_map(|p| p.markers());
    markers.filter(|(kind, _)| *kind == BED).count()
}

#[test]
fn test_a_village_is_settled_once_with_a_villager_by_every_bed() {
    let (mut app, village) = village(0.4);
    // nobody near, nobody lives there yet
    run(&mut app, 2.5);
    assert!(villagers(&mut app).is_empty());
    settled(&mut app, &village);
    let living = villagers(&mut app);
    assert_eq!(beds(&village), living.len());
    assert!(living.len() >= 4);
    // each of a calling there's a workplace for, at home by its own bed, with a name
    let mut callings = HashSet::new();
    for (entity, townsfolk) in &living {
        let kind = app.world().get::<Creature>(*entity).unwrap().0;
        assert!(kind.properties().calling.is_some());
        callings.insert(kind);
        let settler = townsfolk.0;
        assert_eq!(village.cell, settler.village);
        assert_eq!(
            settler.villager,
            *app.world().get::<Villager>(*entity).unwrap()
        );
        assert_eq!(village.style as u8, settler.villager.style);
    }
    let homes: HashSet<_> = living.iter().map(|(_, t)| t.0.home).collect();
    assert_eq!(living.len(), homes.len());
    assert!(callings.len() >= 3, "{callings:?}");
    // and its pens' animals
    let animals = kinds(&mut app)
        .iter()
        .filter(|k| k.properties().calling.is_none())
        .count();
    assert!(animals >= 2, "{animals} animals");
    assert_eq!(1, app.world().resource::<Settled>().0.len());
    // one killed stays dead: the village isn't settled again
    let (first, _) = living[0];
    app.world_mut().despawn(first);
    run(&mut app, 4.0);
    assert_eq!(living.len() - 1, villagers(&mut app).len());
}

// How many villagers are within `near` blocks of where they'd be at `place`.
fn at_place(app: &mut App, place: Place, near: f32) -> (usize, usize) {
    let living = villagers(app);
    let there = living
        .iter()
        .filter(|(entity, townsfolk)| {
            let spot = match place {
                Place::Home => townsfolk.0.home,
                Place::Work => townsfolk.0.work,
                Place::Well => townsfolk.0.well,
            };
            let spot = spot.as_vec3() + Vec3::new(0.5, 0.0, 0.5);
            at(app, *entity).distance(spot) <= near
        })
        .count();
    (there, living.len())
}

// Every door in the village's pieces, at its lower half.
fn doors(app: &App, village: &Village) -> Vec<Block> {
    let world = app.world().resource::<World>();
    let mut found = Vec::new();
    for placed in &village.pieces {
        let (low, high) = placed.bounds();
        for x in low.x..high.x {
            for z in low.z..high.z {
                for y in low.y..high.y {
                    let block = world.get(I64Vec3::new(x, y, z));
                    if block.kind == BlockType::Door && !block.state.top() {
                        found.push(block);
                    }
                }
            }
        }
    }
    found
}

#[test]
fn test_villagers_go_to_work_by_day_through_their_doors_and_home_at_night() {
    let (mut app, village) = village(0.36);
    assert!(doors(&app, &village).iter().all(|door| !door.state.open()));
    settled(&mut app, &village);
    // out of their doors and along the streets to work in the morning
    run(&mut app, 70.0);
    // within a few blocks of where they work, as they walk about it
    let (working, all) = at_place(&mut app, Place::Work, 4.5);
    assert!(working * 3 >= all * 2, "{working} of {all} at work");
    // a door opened as they went is an edit, and shut behind them
    let edits = app.world().resource::<World>().edits();
    let edited = edits.chunks().flat_map(|c| edits.in_chunk(c)).count();
    assert!(edited > 0, "no doors used");
    // and home by their beds at night
    app.insert_resource(TimeOfDay::from_days(0.9));
    run(&mut app, 80.0);
    let (home, all) = at_place(&mut app, Place::Home, 2.0);
    assert!(home * 3 >= all * 2, "{home} of {all} home");
    let open = doors(&app, &village)
        .iter()
        .filter(|d| d.state.open())
        .count();
    assert_eq!(0, open, "doors left open");
}

#[test]
fn test_villagers_run_from_monsters_and_go_back_to_their_day() {
    let (mut app, village) = village(0.5);
    settled(&mut app, &village);
    // out in the square by the well
    let (villager, townsfolk) = villagers(&mut app)[0];
    let was = townsfolk.0.well.as_vec3() + Vec3::new(0.5, 0.0, 0.5);
    app.world_mut()
        .get_mut::<Transform>(villager)
        .unwrap()
        .translation = was;
    let gloamer = spawned(&mut app, CreatureKind::Gloamer, was + Vec3::X * 4.0);
    // a gloamer standing still beside it, in a world with no players to hunt
    app.world_mut().get_mut::<Mind>(gloamer).unwrap().doing = Doing::Cling;
    run(&mut app, 0.3);
    let mind = *app.world().get::<Mind>(villager).unwrap();
    assert!(matches!(mind.doing, Doing::Flee { .. }), "{mind:?}");
    run(&mut app, 3.0);
    let monster = at(&app, gloamer);
    assert!(at(&app, villager).distance(monster) > was.distance(monster) + 2.0);
    // and once it's gone, back to its day
    app.world_mut().despawn(gloamer);
    run(&mut app, 6.0);
    let mind = *app.world().get::<Mind>(villager).unwrap();
    assert!(matches!(mind.doing, Doing::Keep(_)), "{mind:?}");
    assert!(app.world().get::<Errand>(villager).is_some());
}

// A villager in the settled village, and the player standing beside it.
fn beside(app: &mut App, village: &Village) -> (Entity, Entity) {
    let player = settled(app, village);
    let (villager, _) = villagers(app)[0];
    let there = at(app, villager);
    app.world_mut()
        .get_mut::<Transform>(player)
        .unwrap()
        .translation = there + Vec3::X * 1.5;
    (villager, player)
}

fn offered(app: &mut App) -> Vec<Option<Vec<(crate::trading::Offer, u8)>>> {
    let sent = app
        .world_mut()
        .resource_mut::<Messages<ToClients<TradeOffers>>>();
    sent.iter_current_update_messages()
        .map(|m| m.message.0.clone())
        .collect()
}

#[test]
fn test_trading_takes_the_price_gives_the_goods_and_refuses_without_payment() {
    let (mut app, village) = village(0.5);
    let (villager, player) = beside(&mut app, &village);
    let kind = app.world().get::<Creature>(villager).unwrap().0;
    use_on(&mut app, villager);
    assert!(app.world().get::<Trading>(player).is_some());
    // it stops to talk, facing them
    let mind = *app.world().get::<Mind>(villager).unwrap();
    assert_eq!(Doing::Heed(player), mind.doing);
    let stock = app.world().get::<Townsfolk>(villager).unwrap().0.stock;
    let calling = kind.properties().calling.as_ref().unwrap();
    let offers = stock.offers(calling);
    let (offer, left) = offers[0];
    // without what it takes, nothing changes
    let trade_as = |app: &mut App, all| {
        app.world_mut().write_message(FromClient {
            client_id: ClientId::Server,
            message: Trade {
                villager,
                offer: 0,
                all,
            },
        });
        app.update();
    };
    let trade = |app: &mut App| trade_as(app, false);
    trade(&mut app);
    let inventory = |app: &App| app.world().get::<Inventory>(player).unwrap().clone();
    assert_eq!(0, inventory(&app).count(offer.gives.0));
    // with it, the price is taken and the goods given, and the offer sent again with one less
    let (price, count) = offer.takes;
    app.world_mut()
        .get_mut::<Inventory>(player)
        .unwrap()
        .add(ItemStack::new(price, count));
    trade(&mut app);
    assert_eq!(0, inventory(&app).count(price));
    assert_eq!(
        u32::from(offer.gives.1),
        inventory(&app).count(offer.gives.0)
    );
    let sold = app.world().get::<Townsfolk>(villager).unwrap().0.stock;
    assert_eq!(left - 1, sold.offers(calling)[0].1);
    assert_eq!(
        Some(Some(sold.offers(calling).to_vec())),
        offered(&mut app).pop()
    );
    // until it's sold out for the day, then refused, and on the next day it's stocked again
    for _ in 0..left {
        app.world_mut()
            .get_mut::<Inventory>(player)
            .unwrap()
            .add(ItemStack::new(price, count));
        trade(&mut app);
    }
    assert!(inventory(&app).count(price) >= u32::from(count));
    app.insert_resource(TimeOfDay::from_days(1.5));
    app.update();
    let restocked = Stock::of(calling, stock_seed(&app, villager), 1);
    assert_eq!(
        Some(Some(restocked.offers(calling).to_vec())),
        offered(&mut app).pop()
    );
    // shift-clicking trades as many times as it can, here all it has left today, of the day's
    // own first offer, which may not be yesterday's
    let (today, left) = restocked.offers(calling)[0];
    let (price, count) = today.takes;
    let had = inventory(&app).count(today.gives.0);
    for _ in 0..left {
        app.world_mut()
            .get_mut::<Inventory>(player)
            .unwrap()
            .add(ItemStack::new(price, count));
    }
    let paid = inventory(&app).count(price) - u32::from(left) * u32::from(count);
    trade_as(&mut app, true);
    let sold = app.world().get::<Townsfolk>(villager).unwrap().0.stock;
    assert_eq!(0, sold.offers(calling)[0].1);
    assert_eq!(paid, inventory(&app).count(price));
    assert!(inventory(&app).count(today.gives.0) > had);
    // and walking off ends it
    app.world_mut()
        .get_mut::<Transform>(player)
        .unwrap()
        .translation += Vec3::X * 20.0;
    app.update();
    assert_eq!(Some(None), offered(&mut app).pop());
    assert!(app.world().get::<Trading>(player).is_none());
}

fn stock_seed(app: &App, villager: Entity) -> u64 {
    app.world().get::<Villager>(villager).unwrap().seed
}

#[test]
fn test_villagers_sleep_and_wake_with_their_homes_and_trades() {
    let (mut app, village) = village(0.5);
    let player = settled(&mut app, &village);
    let living = villagers(&mut app);
    // as the player goes, they're saved as they are
    app.world_mut()
        .get_mut::<Transform>(player)
        .unwrap()
        .translation += Vec3::X * 400.0;
    app.update();
    assert!(villagers(&mut app).is_empty());
    let dormant = app.world().resource::<Dormant>();
    let saved: Vec<_> = dormant
        .0
        .values()
        .flatten()
        .filter_map(|c| c.settler)
        .collect();
    assert_eq!(living.len(), saved.len());
    for (_, townsfolk) in &living {
        assert!(saved.contains(&townsfolk.0));
    }
    // and wake as they were, the village not settled again
    app.world_mut()
        .get_mut::<Transform>(player)
        .unwrap()
        .translation -= Vec3::X * 400.0;
    run(&mut app, 2.5);
    let mut woken: Vec<_> = villagers(&mut app)
        .into_iter()
        .map(|(_, t)| t.0.home)
        .collect();
    let mut was: Vec<_> = living.iter().map(|(_, t)| t.0.home).collect();
    woken.sort_by_key(|h| h.to_array());
    was.sort_by_key(|h| h.to_array());
    assert_eq!(was, woken);
}
