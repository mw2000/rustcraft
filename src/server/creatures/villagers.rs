//! Villagers on the server (§5.8, M3.4). A village is settled the first time a player comes near
//! it with its ground loaded: a villager for every bed in its houses, each with a calling by the
//! workplaces there in turn, and animals in its pens; it's remembered as settled (`Settled`,
//! saved with the world), so those who die stay dead. Each keeps its day (`place_now`): at work,
//! meeting the others at the well at noon and in the evening, and home by its bed at night,
//! finding its way along the village's streets (`Village::way`) and opening the doors it passes,
//! shutting them behind it. It runs from monsters near it, and trades with players.

use std::collections::{BTreeSet, VecDeque};

use bevy::math::{I64Vec2, I64Vec3};
use bevy::prelude::*;
use bevy_replicon::prelude::*;
use rand::RngExt;

use super::{creature, Mind, Route};
use crate::block::BlockType;
use crate::building::{other_half, toggled};
use crate::chunks::chunk::ChunkCoordinate;
use crate::chunks::generate::structures::village::{Role, Village, ANIMAL, BED, GATHER, WORK};
use crate::chunks::generate::trees::hash;
use crate::clock::{day_fraction, TimeOfDay};
use crate::creatures::path::Walker;
use crate::creatures::{CreatureKind, SavedCreature, Settler, Villager};
use crate::player::{Player, PlayerOwner};
use crate::protocol::{BlockChanged, CloseInventory, Trade, TradeOffers, UseCreature};
use crate::server::drops::{Chance, DroppedItem};
use crate::server::sync::BlockChanges;
use crate::trading::{Offer, Stock, OFFERED};
use crate::world::World;

// How near a village, in blocks, a player comes before it's settled, how often that's looked
// at, in seconds, and how far away a villager runs from a monster.
const SETTLE_DISTANCE: i64 = 64;
const SETTLE_EVERY: f32 = 2.0;
pub(super) const WARY_DISTANCE: f32 = 10.0;
// How long a villager tries for the next cell of its way before going on to the one after, in
// seconds, how far round where it is it walks about, and how far across from a door's middle
// it's through it, to shut it behind it.
const GIVE_UP: f32 = 15.0;
const MILL: i64 = 2;
const THROUGH: f32 = 1.1;

/// The villages settled, by their cells on the villages' grid, saved with the world.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct Settled(pub BTreeSet<(i64, i64)>);

/// A villager's life in its village, which only the server knows.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Townsfolk(pub Settler);

/// Where a villager's going: the place, and the cells of its way there still to come, and since
/// when, by the server's clock in seconds, it's been trying for the next.
#[derive(Component, Debug, Default)]
pub struct Errand {
    going: Option<Place>,
    way: VecDeque<I64Vec3>,
    since: f32,
}

/// The doors a villager has opened and not yet shut behind it, by their lower halves.
#[derive(Component, Debug, Default)]
pub struct Doorways(Vec<I64Vec3>);

/// A player trading with a villager, and the offers they were last sent.
#[derive(Component, Debug)]
pub struct Trading {
    pub villager: Entity,
    sent: Option<[(Offer, u8); OFFERED]>,
}

/// The places in a villager's day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Home,
    Work,
    Well,
}

impl Townsfolk {
    fn spot(&self, place: Place) -> I64Vec3 {
        match place {
            Place::Home => self.0.home,
            Place::Work => self.0.work,
            Place::Well => self.0.well,
        }
    }
}

/// What a villager is besides a creature, as it's spawned from `settler`.
pub fn townsperson(settler: Settler) -> impl Bundle {
    (
        settler.villager,
        Townsfolk(settler),
        Errand::default(),
        Doorways::default(),
    )
}

/// Where a villager with `seed` is at `time`: at home from dusk to just after dawn, at the well
/// at noon and in the evening, and otherwise at work, each a little earlier or later than the
/// others, so they don't all set off at once.
pub fn place_now(seed: u64, time: f64) -> Place {
    let early = (seed >> 20) % 1000;
    let t = f64::from(day_fraction(time)) + early as f64 / 1000.0 * 0.04 - 0.02;
    match t {
        t if !(0.27..0.75).contains(&t) => Place::Home,
        t if (0.48..0.54).contains(&t) || t >= 0.69 => Place::Well,
        _ => Place::Work,
    }
}

/// Settles the villages players come near, whose beds and pens are on ground the server holds:
/// a villager by each bed, of the calling of each of its workplaces in turn, living there, and an
/// animal by each pen's markers, once for each village.
pub(in crate::server) fn settle(
    time: Res<Time>,
    world: Res<World>,
    clock: Res<TimeOfDay>,
    mut settled: ResMut<Settled>,
    players: Query<&Transform, With<Player>>,
    mut since: Local<f32>,
    mut commands: Commands,
) {
    *since += time.delta_secs();
    if *since < SETTLE_EVERY {
        return;
    }
    *since = 0.0;
    for body in &players {
        let column = body.translation.floor().as_i64vec3().xz();
        let near = world
            .region_store
            .villages_near(column - SETTLE_DISTANCE, column + SETTLE_DISTANCE);
        for village in near {
            let cell = (village.cell.x, village.cell.y);
            let markers = || village.pieces.iter().flat_map(|piece| piece.markers());
            let loaded = markers()
                .filter(|(kind, _)| [BED, ANIMAL].contains(kind))
                .all(|(_, at)| world.is_chunk_generated(ChunkCoordinate::containing(at)));
            if settled.0.contains(&cell) || !loaded {
                continue;
            }
            settled.0.insert(cell);
            for saved in villagers(&village, world.seed(), clock.0) {
                let settler = saved.settler.expect("villagers are settled");
                commands.spawn((creature(saved, 0.0), townsperson(settler)));
            }
            for saved in animals(&village, world.seed()) {
                commands.spawn(creature(saved, 0.0));
            }
        }
    }
}

// The villagers of `village` in the world of `seed` as it's settled at `time`, each at home.
fn villagers(village: &Village, seed: u32, time: f64) -> Vec<SavedCreature> {
    let of = |kind: &'static str| {
        let markers = village.pieces.iter().map(|piece| piece.markers());
        markers.flatten().filter(move |(k, _)| *k == kind)
    };
    // every place to work at with a calling, a place at each workplace in turn, the nearest the
    // well first, so the first villagers take one at each and the rest fill them up
    let workplaces: Vec<(CreatureKind, Vec<I64Vec3>)> = (0..village.pieces.len())
        .filter_map(|piece| {
            let role = village.role(piece);
            let kind = CreatureKind::ALL.iter().copied().find(|kind| {
                let calling = kind.properties().calling.as_ref();
                calling.is_some_and(|calling| calling.works_at == role)
            })?;
            let markers = village.pieces[piece].markers();
            let works: Vec<I64Vec3> = markers
                .filter(|(k, _)| *k == WORK)
                .map(|(_, at)| at)
                .collect();
            (!works.is_empty()).then_some((kind, works))
        })
        .collect();
    let most = workplaces
        .iter()
        .map(|(_, works)| works.len())
        .max()
        .unwrap_or(0);
    let callings: Vec<(CreatureKind, I64Vec3)> = (0..most)
        .flat_map(|round| {
            let at = |&(kind, ref works): &(CreatureKind, Vec<I64Vec3>)| {
                works.get(round).map(|&work| (kind, work))
            };
            workplaces.iter().filter_map(at).collect::<Vec<_>>()
        })
        .collect();
    let wells: Vec<I64Vec3> = of(GATHER).map(|(_, at)| at).collect();
    if callings.is_empty() || wells.is_empty() {
        return Vec::new();
    }
    of(BED)
        .enumerate()
        .map(|(i, (_, home))| {
            let (kind, work) = callings[i % callings.len()];
            let calling = kind.properties().calling.as_ref().unwrap();
            let seed = hash(
                u64::from(seed) ^ 0x7111_a9e5,
                home.x,
                hash(0, home.y, home.z) as i64,
            );
            let villager = Villager {
                style: village.style as u8,
                seed,
            };
            let settler = Settler {
                villager,
                village: village.cell,
                home,
                work,
                well: wells[i % wells.len()],
                stock: Stock::of(calling, seed, time.floor() as u64),
            };
            SavedCreature {
                settler: Some(settler),
                ..SavedCreature::grown(kind, home.as_vec3() + Vec3::new(0.5, 0.0, 0.5))
            }
        })
        .collect()
}

// The animals in `village`'s pens, a kind to each pen by a hash of where it is.
fn animals(village: &Village, seed: u32) -> Vec<SavedCreature> {
    const PENNED: [CreatureKind; 4] = [
        CreatureKind::Sheep,
        CreatureKind::Cow,
        CreatureKind::Pig,
        CreatureKind::Chicken,
    ];
    let pens = (0..village.pieces.len()).filter(|&piece| village.role(piece) == Role::Pen);
    pens.flat_map(|pen| {
        let placed = &village.pieces[pen];
        let draw = hash(u64::from(seed), placed.min.x, placed.min.z);
        let kind = PENNED[(draw % PENNED.len() as u64) as usize];
        let animals = placed.markers().filter(|(k, _)| *k == ANIMAL);
        animals
            .map(move |(_, at)| SavedCreature::grown(kind, at.as_vec3() + Vec3::new(0.5, 0.0, 0.5)))
    })
    .collect()
}

/// Keeps a villager's day, going to `place`: finding its way there along the village's streets
/// as it sets off, then walking it a cell at a time, and once there walking about a little now
/// and then, but for at home, where it stands by its bed.
#[allow(clippy::too_many_arguments)]
pub(super) fn keep(
    world: &World,
    townsfolk: &Townsfolk,
    errand: &mut Errand,
    place: Place,
    from: I64Vec3,
    walker: Walker,
    now: f32,
    ready: bool,
    route: &mut Route,
    mind: &mut Mind,
    chance: &mut Chance,
) {
    let spot = townsfolk.spot(place);
    if errand.going != Some(place) {
        errand.going = Some(place);
        errand.way = way(world, &townsfolk.0, from, spot).into();
        errand.since = now;
        route.stop();
    }
    if let Some(&next) = errand.way.front() {
        let there = (next - from).xz().abs().max_element() <= 1 && (next.y - from.y).abs() <= 1;
        if there || now - errand.since > GIVE_UP {
            errand.way.pop_front();
            errand.since = now;
            route.stop();
        } else if ready && !route.is_going() {
            route.plan(world, from, next, 0, walker, now);
        }
        return;
    }
    if !ready || route.is_going() || now < mind.until {
        return;
    }
    // there: by its bed at home, and walking about a little at work and at the well
    let rng = &mut chance.0;
    mind.until = now + rng.random_range(super::IDLE);
    if place == Place::Home {
        // in the very cell, out of the way of the door
        if (spot - from).xz() != I64Vec2::ZERO || (spot.y - from.y).abs() > 1 {
            route.plan(world, from, spot, 0, walker, now);
        }
        return;
    }
    let offset = I64Vec3::new(
        rng.random_range(-MILL..=MILL),
        0,
        rng.random_range(-MILL..=MILL),
    );
    // only somewhere it can get to, not out through a wall and round by the door
    route.plan(world, from, spot + offset, 0, walker, now);
    route.exact = true;
}

/// Forgets where a villager was going, so it finds its way afresh from wherever it is once it
/// goes back to keeping its day, as after running from something.
pub(super) fn set_off_again(errand: &mut Errand) {
    errand.going = None;
}

// The way from `from` to `spot` in the village `settler` lives in.
fn way(world: &World, settler: &Settler, from: I64Vec3, spot: I64Vec3) -> Vec<I64Vec3> {
    let Some(village) = world.region_store.village(settler.village) else {
        return vec![spot];
    };
    let piece = (0..village.pieces.len()).find(|&p| village.pieces[p].covers(spot.x, spot.z));
    village.way(from, piece.unwrap_or(0), spot)
}

/// Opens the doors in the way of each villager, both halves, as it comes to them, and shuts
/// those it's opened once it's gone through, telling the clients near them.
pub(in crate::server) fn use_doors(
    mut world: ResMut<World>,
    mut villagers: Query<(&Transform, &Route, &mut Doorways), With<Townsfolk>>,
    mut changed: BlockChanges,
) {
    for (body, route, mut doorways) in &mut villagers {
        let at = body.translation;
        let here = super::cell_of(at);
        let ahead: Vec<I64Vec3> = std::iter::once(here)
            .chain(route.cells.iter().take(2).copied())
            .flat_map(|cell| [cell, cell + I64Vec3::Y])
            .collect();
        for &cell in &ahead {
            let block = world.get(cell);
            if block.kind != BlockType::Door || block.state.open() || block.state.top() {
                continue;
            }
            swing(&mut world, cell, &mut changed);
            doorways.0.push(cell);
        }
        // shut once it's through, out of the doorway and not going back through it
        doorways.0.retain(|&door| {
            let middle = door.as_vec3() + Vec3::new(0.5, 0.0, 0.5);
            if middle.xz().distance(at.xz()) <= THROUGH || ahead.contains(&door) {
                return true;
            }
            if world.get(door).state.open() {
                swing(&mut world, door, &mut changed);
            }
            false
        });
    }
}

// Opens or shuts the door at `at`, both its halves.
fn swing(world: &mut World, at: I64Vec3, changed: &mut BlockChanges) {
    let was = world.get(at);
    let Some(to) = toggled(was) else {
        return;
    };
    let other = other_half(at, was).map(|(at, half)| (at, toggled(half).unwrap()));
    for (at, to) in std::iter::once((at, to)).chain(other) {
        if world.edit(at, to) {
            changed.write(BlockChanged {
                block: at,
                to,
                by: None,
            });
        }
    }
}

/// Opens trading with the villagers players right-click in survival, if they're alive and in
/// reach.
pub(in crate::server) fn open_trades(
    mut uses: MessageReader<FromClient<UseCreature>>,
    players: Query<(
        Entity,
        &PlayerOwner,
        &Transform,
        &crate::survival::Vitals,
        &crate::game_mode::GameMode,
    )>,
    villagers: Query<(&super::Creature, &Transform), With<Townsfolk>>,
    mut commands: Commands,
) {
    for FromClient { client_id, message } in uses.read() {
        let Some((player, _, body, vitals, mode)) =
            players.iter().find(|(_, o, ..)| o.0 == *client_id)
        else {
            continue;
        };
        let Ok((creature, villager)) = villagers.get(message.creature) else {
            continue;
        };
        // in survival, where there's an inventory to trade from
        let reaches = super::in_reach(body, villager.translation, creature.0.body(false));
        if !vitals.is_dead() && !mode.has_every_block() && reaches {
            commands.entity(player).insert(Trading {
                villager: message.creature,
                sent: None,
            });
        }
    }
}

/// Makes the trades players ask for, if they're still trading with that villager and have what
/// it takes, and it has the trade left to make: what they get that doesn't fit is dropped at
/// their feet. Refused, nothing changes.
pub(in crate::server) fn make_trades(
    mut trades: MessageReader<FromClient<Trade>>,
    mut closes: MessageReader<FromClient<CloseInventory>>,
    clock: Res<TimeOfDay>,
    mut players: Query<(
        Entity,
        &PlayerOwner,
        &Transform,
        &mut crate::inventory::Inventory,
        Option<&Trading>,
    )>,
    mut villagers: Query<(&super::Creature, &mut Townsfolk)>,
    mut chance: ResMut<Chance>,
    mut commands: Commands,
) {
    let day = clock.0.floor() as u64;
    for FromClient { client_id, message } in trades.read() {
        let Some((_, _, body, mut inventory, Some(trading))) =
            players.iter_mut().find(|(_, o, ..)| o.0 == *client_id)
        else {
            continue;
        };
        if trading.villager != message.villager {
            continue;
        }
        let Ok((creature, mut townsfolk)) = villagers.get_mut(message.villager) else {
            continue;
        };
        let calling = creature.0.properties().calling.as_ref().unwrap();
        let settler = &mut townsfolk.0;
        let stock = settler.stock.restocked(calling, settler.villager.seed, day);
        let index = message.offer as usize;
        let Some(&(offer, left)) = stock.offers(calling).get(index) else {
            continue;
        };
        if left == 0 {
            continue;
        }
        let Ok(spare) = offer.make(&mut inventory) else {
            continue;
        };
        settler.stock = Stock {
            sold: std::array::from_fn(|i| stock.sold[i] + u8::from(i == index)),
            ..stock
        };
        if let Some(spare) = spare {
            let (dropped, at) = DroppedItem::scattered(spare, body.translation, &mut chance.0);
            commands.spawn((dropped, at, Replicated));
        }
    }
    for FromClient { client_id, .. } in closes.read() {
        if let Some((player, ..)) = players.iter().find(|(_, o, ..)| o.0 == *client_id) {
            commands.entity(player).remove::<Trading>();
        }
    }
}

/// Sends each player trading the offers of the villager they're trading with as they change, or
/// that trading's over once it's gone or out of their reach, which closes their screen.
pub(in crate::server) fn send_offers(
    clock: Res<TimeOfDay>,
    mut players: Query<(Entity, &PlayerOwner, &Transform, &mut Trading)>,
    villagers: Query<(&super::Creature, &Transform, &Townsfolk)>,
    mut offers: MessageWriter<ToClients<TradeOffers>>,
    mut commands: Commands,
) {
    let day = clock.0.floor() as u64;
    for (player, owner, body, mut trading) in &mut players {
        let send = |offers: Option<Vec<(Offer, u8)>>| ToClients {
            targets: SendTargets::Single(owner.0),
            message: TradeOffers(offers),
        };
        let open = villagers
            .get(trading.villager)
            .ok()
            .filter(|(creature, at, _)| {
                super::in_reach(body, at.translation, creature.0.body(false))
            });
        let Some((creature, _, townsfolk)) = open else {
            offers.write(send(None));
            commands.entity(player).remove::<Trading>();
            continue;
        };
        let calling = creature.0.properties().calling.as_ref().unwrap();
        let settler = &townsfolk.0;
        let now = settler
            .stock
            .restocked(calling, settler.villager.seed, day)
            .offers(calling);
        if trading.sent != Some(now) {
            offers.write(send(Some(now.to_vec())));
            trading.sent = Some(now);
        }
    }
}
