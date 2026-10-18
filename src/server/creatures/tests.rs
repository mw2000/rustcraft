mod monsters;

use std::time::{Duration, Instant};

use bevy::math::I64Vec3;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy_replicon::prelude::{ClientId, FromClient};
use rand::rngs::StdRng;
use rand::SeedableRng;

use super::{
    breed, creature, despawn, fight, fly_missiles, grow, move_creatures, receive_hits,
    receive_uses, sleep, spawn, take_paths, think, vanish, wake, Doing, Dormant, Health, Life,
    Mind, Route, SpawnRate, Spawners,
};
use crate::block::BlockType;
use crate::chunks::chunk::{ChunkCoordinate, ChunkData, CELLS, CHUNK_SIZE};
use crate::chunks::generate::noise::NoiseGenerator;
use crate::clock::TimeOfDay;
use crate::creatures::{Condition, Creature, CreatureKind, Flinches, SavedCreature};
use crate::edits::Edits;
use crate::inventory::Inventory;
use crate::items::{Item, ItemStack};
use crate::player::{Player, PlayerOwner};
use crate::protocol::{HitCreature, UseCreature};
use crate::server::drops::{Chance, DroppedItem};
use crate::server::survival::{Combat, Footing};
use crate::server::{Blow, HeldItem};
use crate::world::World;

const FRAME: f32 = 0.05;
// The top of the ground, the field's floor, and where feet stand on it.
const FLOOR: i64 = 16;
const FEET: f32 = 17.0;

// The world the tests' fields are in, with a seed of its own.
const SEED: u32 = 1;

// A field of `surface` 9 chunks across round the origin, its top at `FLOOR`, over stone, with
// the creatures' systems running on it by day, and none appearing by themselves.
fn app(surface: BlockType) -> App {
    field(surface, I64Vec3::ZERO)
}

// The same, round the chunk `centre`, at the origin's height.
fn field(surface: BlockType, centre: I64Vec3) -> App {
    let mut world = World::from_save(SEED, Edits::default(), 16);
    let size = CHUNK_SIZE as usize;
    let ground = ChunkData::from_cells((0..CELLS).map(|i| {
        if i % size == 0 {
            surface
        } else {
            BlockType::Air
        }
    }));
    for x in -4..=4 {
        for z in -4..=4 {
            let coord = |y| ChunkCoordinate(centre + I64Vec3::new(x, y, z));
            world.insert_chunk(coord(0), ChunkData::from_cells([BlockType::Stone; CELLS]));
            world.insert_chunk(coord(1), ground.clone());
            world.insert_chunk(coord(2), ChunkData::default());
        }
    }
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(world)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            FRAME,
        )))
        .insert_resource(Chance(StdRng::seed_from_u64(5)))
        .insert_resource(TimeOfDay(0.5))
        .insert_resource(SpawnRate(None))
        .init_resource::<Dormant>()
        .init_resource::<Spawners>()
        .init_resource::<Blows>()
        .add_message::<FromClient<HitCreature>>()
        .add_message::<FromClient<UseCreature>>()
        .add_message::<Blow>()
        .add_systems(
            Update,
            (
                wake,
                spawn,
                take_paths,
                grow,
                think,
                move_creatures,
                fight,
                fly_missiles,
                breed,
                despawn,
                vanish,
                sleep,
                receive_hits,
                receive_uses,
                keep_blows,
            )
                .chain(),
        );
    app
}

// The blows creatures have struck at players, kept as they come, as the players' survival,
// which lands them, isn't part of these tests.
#[derive(Resource, Default)]
struct Blows(Vec<Blow>);

fn keep_blows(mut blows: MessageReader<Blow>, mut kept: ResMut<Blows>) {
    kept.0.extend(blows.read().copied());
}

// The nearest chunk to the origin, of those on a coarse grid, whose land within 60 blocks is
// all `biome`, as far as creatures appear round a player standing in it.
fn chunk_in(biome: &str) -> I64Vec3 {
    let noise = NoiseGenerator::new(SEED);
    let all_biome = |at: I64Vec3| {
        let middle = at.as_dvec3() * CHUNK_SIZE as f64;
        (-6..=6).all(|x| {
            (-6..=6).all(|z| {
                let sample = noise.sample(
                    middle.x + f64::from(x) * 10.0,
                    middle.z + f64::from(z) * 10.0,
                );
                noise.biomes().name(sample.biome).unwrap_or("base") == biome
            })
        })
    };
    let grid = (-60..=60).flat_map(|x| (-60..=60).map(move |z| I64Vec3::new(x * 16, 0, z * 16)));
    let mut found: Vec<_> = grid.filter(|&at| all_biome(at)).collect();
    found.sort_by_key(|at| at.x * at.x + at.z * at.z);
    *found.first().unwrap_or_else(|| panic!("no {biome} near"))
}

fn spawned(app: &mut App, kind: CreatureKind, at: Vec3) -> Entity {
    let grown = SavedCreature::grown(kind, at);
    app.world_mut().spawn(creature(grown, 0.0)).id()
}

fn sheep(app: &mut App, at: Vec3) -> Entity {
    spawned(app, CreatureKind::Sheep, at)
}

fn player(app: &mut App, feet: Vec3, held: Option<ItemStack>) -> Entity {
    let mut inventory = Inventory::default();
    inventory.slots[0] = held;
    app.world_mut()
        .spawn((
            Player::default(),
            PlayerOwner(ClientId::Server),
            HeldItem(held.map(|stack| stack.item)),
            inventory,
            Transform::from_translation(feet),
            Combat::default(),
            Footing::default(),
        ))
        .id()
}

// Runs `seconds` of frames, each once the ways being found on the compute pool are, so they
// arrive the frame after they're asked for however busy the machine running every test is.
fn run(app: &mut App, seconds: f32) {
    for _ in 0..(seconds / FRAME).round() as usize {
        let started = Instant::now();
        let mut routes = app.world_mut().query::<&Route>();
        while routes.iter(app.world()).any(|route| {
            route
                .planning
                .as_ref()
                .is_some_and(|task| !task.is_finished())
        }) {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "a way took too long"
            );
            std::thread::yield_now();
        }
        app.update();
    }
}

fn at(app: &App, entity: Entity) -> Vec3 {
    app.world().get::<Transform>(entity).unwrap().translation
}

fn health(app: &App, entity: Entity) -> u8 {
    app.world().get::<Health>(entity).unwrap().points
}

fn hit(app: &mut App, creature: Entity) {
    app.world_mut().write_message(FromClient {
        client_id: ClientId::Server,
        message: HitCreature { creature },
    });
}

// Uses what the player holds on `creature`, and runs the frame after, which shows its
// condition.
fn use_on(app: &mut App, creature: Entity) {
    app.world_mut().write_message(FromClient {
        client_id: ClientId::Server,
        message: UseCreature { creature },
    });
    app.update();
    app.update();
}

fn life(app: &mut App, creature: Entity) -> Mut<'_, Life> {
    app.world_mut().get_mut::<Life>(creature).unwrap()
}

fn condition(app: &App, creature: Entity) -> Condition {
    *app.world().get::<Condition>(creature).unwrap()
}

fn held(app: &App, player: Entity) -> Option<ItemStack> {
    app.world().get::<Inventory>(player).unwrap().slots[0]
}

// The stacks dropped, by item, each with how many there are.
fn dropped(app: &mut App) -> Vec<(Item, u8)> {
    let mut drops = app.world_mut().query::<&DroppedItem>();
    let mut stacks: Vec<_> = drops
        .iter(app.world())
        .map(|drop| (drop.stack.item, drop.stack.count))
        .collect();
    stacks.sort_by_key(|(item, _)| item.name());
    stacks
}

fn clear_drops(app: &mut App) {
    let mut drops = app
        .world_mut()
        .query_filtered::<Entity, With<DroppedItem>>();
    let drops: Vec<_> = drops.iter(app.world()).collect();
    for drop in drops {
        app.world_mut().despawn(drop);
    }
}

fn every_creature(app: &mut App) -> Vec<Entity> {
    let mut query = app.world_mut().query_filtered::<Entity, With<Creature>>();
    query.iter(app.world()).collect()
}

// Kills a creature with a hit, dropping what it drops, however soon after the last.
fn kill(app: &mut App, creature: Entity) {
    app.world_mut().get_mut::<Health>(creature).unwrap().points = 1;
    let mut players = app.world_mut().query::<&mut Combat>();
    for mut combat in players.iter_mut(app.world_mut()) {
        combat.ready_at = 0.0;
    }
    hit(app, creature);
    app.update();
    assert!(app.world().get_entity(creature).is_err());
}

fn is_monster(kind: CreatureKind) -> bool {
    kind.properties().attack.is_some()
}

fn kinds(app: &mut App) -> Vec<CreatureKind> {
    let mut query = app.world_mut().query::<&Creature>();
    query.iter(app.world()).map(|creature| creature.0).collect()
}

fn creatures(app: &mut App) -> Vec<Vec3> {
    let mut query = app
        .world_mut()
        .query_filtered::<&Transform, With<Creature>>();
    query
        .iter(app.world())
        .map(|body| body.translation)
        .collect()
}

#[test]
fn test_a_sheep_falls_lands_and_is_hurt_only_by_a_long_fall() {
    let mut app = app(BlockType::Grass);
    player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
    let low = sheep(&mut app, Vec3::new(10.5, FEET + 3.0, 0.5));
    let high = sheep(&mut app, Vec3::new(-10.5, FEET + 10.0, 0.5));
    run(&mut app, 2.0);
    for sheep in [low, high] {
        assert!(
            (at(&app, sheep).y - FEET).abs() < 1e-4,
            "{}",
            at(&app, sheep)
        );
    }
    assert_eq!(8, health(&app, low));
    // 10 blocks is 7 beyond the 3 that hurt nobody
    assert_eq!(1, health(&app, high));
}

#[test]
fn test_a_sheep_follows_wheat_finding_its_way_up_a_step() {
    let mut app = app(BlockType::Grass);
    // a step a block up from x 4 on, across the whole way
    {
        let mut world = app.world_mut().resource_mut::<World>();
        for x in 4..12 {
            for z in -6..=6 {
                world.edit(I64Vec3::new(x, FLOOR + 1, z), BlockType::Dirt);
            }
        }
    }
    let wheat = ItemStack::new(Item::Wheat, 1);
    let feet = Vec3::new(8.5, FEET + 1.0, 0.5);
    player(&mut app, feet, Some(wheat));
    let sheep = sheep(&mut app, Vec3::new(1.5, FEET, 0.5));
    run(&mut app, 8.0);
    let end = at(&app, sheep);
    assert!((end.y - (FEET + 1.0)).abs() < 1e-3, "{end}");
    assert!((end - feet).xz().length() < 3.5, "{end}");
    assert!(matches!(
        app.world().get::<Mind>(sheep).unwrap().doing,
        Doing::Follow(_)
    ));
}

#[test]
fn test_a_sheep_floats_in_water() {
    let mut app = app(BlockType::Grass);
    {
        let mut world = app.world_mut().resource_mut::<World>();
        for x in 10..15 {
            for z in 10..15 {
                for y in FLOOR - 3..=FLOOR {
                    world.edit(I64Vec3::new(x, y, z), BlockType::Water);
                }
            }
        }
    }
    player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
    let sheep = sheep(&mut app, Vec3::new(12.5, FEET + 1.0, 12.5));
    let mut lowest = f32::INFINITY;
    for _ in 0..100 {
        run(&mut app, FRAME);
        lowest = lowest.min(at(&app, sheep).y);
    }
    // under it the pool's 4 deep, but it's kept up near the top, its middle at the surface
    assert!(lowest > FEET - 1.5, "{lowest}");
    assert!(at(&app, sheep).y < FEET, "{}", at(&app, sheep));
    // and falling in hurts nothing
    assert_eq!(8, health(&app, sheep));
}

#[test]
fn test_a_hit_hurts_a_sheep_once_knocks_it_back_and_sends_it_running() {
    let mut app = app(BlockType::Grass);
    let feet = Vec3::new(0.5, FEET, 0.5);
    player(&mut app, feet, None);
    let sheep = sheep(&mut app, Vec3::new(2.5, FEET, 0.5));
    app.update();
    // a flurry of clicks in one go counts once
    hit(&mut app, sheep);
    hit(&mut app, sheep);
    app.update();
    assert_eq!(7, health(&app, sheep));
    assert_eq!(Flinches(1), *app.world().get::<Flinches>(sheep).unwrap());
    run(&mut app, 0.3);
    assert!(
        at(&app, sheep).x > 3.0,
        "knocked back to {}",
        at(&app, sheep)
    );
    assert!(matches!(
        app.world().get::<Mind>(sheep).unwrap().doing,
        Doing::Flee { .. }
    ));
    run(&mut app, 2.5);
    assert!(at(&app, sheep).distance(feet) > 6.0, "{}", at(&app, sheep));
}

#[test]
fn test_a_sheep_out_of_reach_isnt_hurt() {
    let mut app = app(BlockType::Grass);
    player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
    let sheep = sheep(&mut app, Vec3::new(12.5, FEET, 0.5));
    hit(&mut app, sheep);
    app.update();
    assert_eq!(8, health(&app, sheep));
}

#[test]
fn test_a_sheep_killed_drops_mutton_and_wool_and_wears_the_tool() {
    let mut app = app(BlockType::Grass);
    let axe = ItemStack::new(Item::IronAxe, 1);
    let player = player(&mut app, Vec3::new(0.5, FEET, 0.5), Some(axe));
    let sheep = sheep(&mut app, Vec3::new(2.5, FEET, 0.5));
    app.update();
    // an iron axe hits for 6, and swings again after 0.9 s
    hit(&mut app, sheep);
    app.update();
    assert_eq!(2, health(&app, sheep));
    run(&mut app, 0.6);
    hit(&mut app, sheep);
    app.update();
    assert_eq!(2, health(&app, sheep));
    run(&mut app, 0.3);
    hit(&mut app, sheep);
    app.update();
    assert!(app.world().get_entity(sheep).is_err());
    // and a wool, as it wasn't shorn
    let [(Item::Mutton, mutton), (Item::Wool, 1)] = dropped(&mut app)[..] else {
        panic!("the sheep dropped {:?}", dropped(&mut app));
    };
    assert!((1..=2).contains(&mutton));
    let worn = app.world().get::<Inventory>(player).unwrap().slots[0];
    assert_eq!(2, worn.unwrap().wear);
}

#[test]
fn test_sheep_sleep_far_from_everyone_and_wake_as_someone_comes_back() {
    let mut app = app(BlockType::Grass);
    let player = player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
    let at = Vec3::new(10.5, FEET, 0.5);
    let sheep = sheep(&mut app, at);
    app.world_mut().get_mut::<Health>(sheep).unwrap().points = 5;
    app.update();
    app.world_mut()
        .get_mut::<Transform>(player)
        .unwrap()
        .translation
        .x = 200.0;
    app.update();
    assert!(creatures(&mut app).is_empty());
    let asleep: Vec<_> = app
        .world()
        .resource::<Dormant>()
        .0
        .values()
        .flatten()
        .copied()
        .collect();
    assert_eq!(1, asleep.len());
    assert_eq!((CreatureKind::Sheep, 5), (asleep[0].kind, asleep[0].health));
    // not woken from just past where it slept
    app.world_mut()
        .get_mut::<Transform>(player)
        .unwrap()
        .translation
        .x = 120.0;
    app.update();
    assert!(creatures(&mut app).is_empty());
    app.world_mut()
        .get_mut::<Transform>(player)
        .unwrap()
        .translation
        .x = 20.0;
    app.update();
    let mut woken = app.world_mut().query::<(&Creature, &Health)>();
    let woken: Vec<_> = woken
        .iter(app.world())
        .map(|(c, h)| (c.0, h.points))
        .collect();
    assert_eq!(vec![(CreatureKind::Sheep, 5)], woken);
    assert!(app.world().resource::<Dormant>().0.is_empty());
}

#[test]
fn test_animals_appear_on_grass_in_their_biomes_out_of_sight_by_day_up_to_their_own_caps() {
    for biome in ["base", "forest"] {
        animals_appear_in(biome);
    }
}

fn animals_appear_in(biome: &str) {
    let chunk = chunk_in(biome);
    let mut app = field(BlockType::Grass, chunk);
    app.insert_resource(SpawnRate(Some(0.1)));
    let feet = (chunk * CHUNK_SIZE as i64).as_vec3() + Vec3::new(0.5, FEET, 0.5);
    player(&mut app, feet, None);
    while creatures(&mut app).is_empty() {
        app.update();
    }
    for creature in creatures(&mut app) {
        let off = (creature - feet).xz();
        assert!((19.0..53.0).contains(&off.length()), "{creature}");
        // looking along -z, which they're never put near
        assert!(off.angle_to(Vec2::NEG_Y).abs() > 0.75, "{creature}");
        assert_eq!(FEET, creature.y);
    }
    run(&mut app, 20.0);
    let kinds = kinds(&mut app);
    for &kind in CreatureKind::ALL {
        let spawns = kind.properties().spawns.as_ref().unwrap();
        let count = kinds.iter().filter(|&&k| k == kind).count();
        // and no monsters, by day
        let expected = if spawns.biomes.contains(&biome) {
            spawns.cap
        } else {
            0
        };
        assert_eq!(expected, count, "{kind:?} in {biome}");
    }
}

#[test]
fn test_animals_dont_appear_at_night_away_from_grass_or_in_deserts() {
    for (surface, time) in [(BlockType::Grass, 0.0), (BlockType::Sand, 0.5)] {
        let mut app = app(surface);
        app.insert_resource(SpawnRate(Some(0.1)))
            .insert_resource(TimeOfDay(time));
        player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
        run(&mut app, 3.0);
        let animals = kinds(&mut app).into_iter().filter(|k| !is_monster(*k));
        assert_eq!(0, animals.count(), "{surface:?} at {time}");
    }
    // nor in a desert, even on grass
    let chunk = chunk_in("desert");
    let mut app = field(BlockType::Grass, chunk);
    app.insert_resource(SpawnRate(Some(0.1)));
    let feet = (chunk * CHUNK_SIZE as i64).as_vec3() + Vec3::new(0.5, FEET, 0.5);
    player(&mut app, feet, None);
    run(&mut app, 3.0);
    assert!(creatures(&mut app).is_empty());
}

#[test]
fn test_animals_drop_their_meat_and_more_when_killed_grown() {
    let mut app = app(BlockType::Grass);
    player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
    let mut leather = 0;
    for _ in 0..20 {
        let cow = spawned(&mut app, CreatureKind::Cow, Vec3::new(2.5, FEET, 0.5));
        app.update();
        kill(&mut app, cow);
        let drops = dropped(&mut app);
        let beef = drops
            .iter()
            .find(|(item, _)| *item == Item::Beef)
            .unwrap()
            .1;
        assert!((1..=3).contains(&beef), "{drops:?}");
        if let Some(&(_, hides)) = drops.iter().find(|(item, _)| *item == Item::Leather) {
            assert!((1..=2).contains(&hides));
            leather += 1;
        }
        assert!(drops.len() <= 2, "{drops:?}");
        clear_drops(&mut app);
    }
    // a cow sometimes leaves no leather
    assert!((5..20).contains(&leather), "{leather}");

    // a sheep leaves a wool unless it's shorn, and a young one nothing
    let sheep = sheep(&mut app, Vec3::new(2.5, FEET, 0.5));
    app.update();
    kill(&mut app, sheep);
    assert!(dropped(&mut app).contains(&(Item::Wool, 1)));
    clear_drops(&mut app);
    let shorn = self::sheep(&mut app, Vec3::new(2.5, FEET, 0.5));
    life(&mut app, shorn).regrows_in = 100.0;
    app.update();
    kill(&mut app, shorn);
    assert!(dropped(&mut app)
        .iter()
        .all(|(item, _)| *item == Item::Mutton));
    clear_drops(&mut app);
    let lamb = self::sheep(&mut app, Vec3::new(2.5, FEET, 0.5));
    life(&mut app, lamb).grows_up_in = 100.0;
    app.update();
    kill(&mut app, lamb);
    assert!(dropped(&mut app).is_empty());
}

#[test]
fn test_shears_shear_a_grown_sheep_once_until_its_wool_grows_back() {
    let mut app = app(BlockType::Grass);
    let shears = ItemStack::new(Item::Shears, 1);
    let player = player(&mut app, Vec3::new(0.5, FEET, 0.5), Some(shears));
    let sheep = sheep(&mut app, Vec3::new(2.5, FEET, 0.5));
    app.update();
    use_on(&mut app, sheep);
    let [(Item::Wool, wool)] = dropped(&mut app)[..] else {
        panic!("shearing dropped {:?}", dropped(&mut app));
    };
    assert!((1..=3).contains(&wool));
    assert!(condition(&app, sheep).shorn);
    assert_eq!(1, held(&app, player).unwrap().wear);
    // shorn, it gives nothing more and wears nothing
    use_on(&mut app, sheep);
    assert_eq!(1, dropped(&mut app).len());
    assert_eq!(1, held(&app, player).unwrap().wear);
    // until its wool's grown back, in minutes
    let regrows = life(&mut app, sheep).regrows_in;
    assert!((240.0..360.0).contains(&regrows), "{regrows}");
    life(&mut app, sheep).regrows_in = 1.0;
    run(&mut app, 1.1);
    assert!(!condition(&app, sheep).shorn);
    use_on(&mut app, sheep);
    assert_eq!(2, dropped(&mut app).len());

    // and a lamb, or a cow, can't be shorn
    let lamb = self::sheep(&mut app, Vec3::new(0.5, FEET, 2.5));
    life(&mut app, lamb).grows_up_in = 100.0;
    let cow = spawned(&mut app, CreatureKind::Cow, Vec3::new(-1.5, FEET, 0.5));
    app.update();
    for unshorn in [lamb, cow] {
        use_on(&mut app, unshorn);
        assert!(!condition(&app, unshorn).shorn);
    }
    assert_eq!(2, dropped(&mut app).len());
}

#[test]
fn test_two_animals_fed_breed_a_young_one_that_grows_up_and_then_rest() {
    let mut app = app(BlockType::Grass);
    let wheat = ItemStack::new(Item::Wheat, 5);
    let player = player(&mut app, Vec3::new(0.5, FEET, 0.5), Some(wheat));
    let cows = [Vec3::new(2.5, FEET, -1.5), Vec3::new(2.5, FEET, 2.5)]
        .map(|at| spawned(&mut app, CreatureKind::Cow, at));
    app.update();
    for cow in cows {
        use_on(&mut app, cow);
    }
    assert_eq!(3, held(&app, player).unwrap().count);
    assert!(cows.into_iter().all(|cow| condition(&app, cow).in_love));
    let mut calf = None;
    for _ in 0..200 {
        run(&mut app, FRAME);
        calf = every_creature(&mut app)
            .into_iter()
            .find(|creature| !cows.contains(creature));
        if calf.is_some() {
            break;
        }
    }
    let calf = calf.expect("no calf in 10 s");
    app.update();
    assert_eq!(
        CreatureKind::Cow,
        app.world().get::<Creature>(calf).unwrap().0
    );
    assert!(condition(&app, calf).young);
    for cow in cows {
        assert!(!condition(&app, cow).in_love);
        // and fed again while they rest, they take nothing
        use_on(&mut app, cow);
        assert!(!condition(&app, cow).in_love);
    }
    assert_eq!(3, held(&app, player).unwrap().count);
    // the calf takes no food till it's grown
    use_on(&mut app, calf);
    assert!(!condition(&app, calf).in_love);
    // its parents can breed again once they've rested
    for cow in cows {
        life(&mut app, cow).rests = 0.0;
        use_on(&mut app, cow);
    }
    assert!(cows.into_iter().all(|cow| condition(&app, cow).in_love));
    assert_eq!(1, held(&app, player).unwrap().count);
    // and it grows up in minutes
    let growing = life(&mut app, calf).grows_up_in;
    assert!(growing > 200.0 && growing < 300.0, "{growing}");
    life(&mut app, calf).grows_up_in = 1.0;
    run(&mut app, 1.1);
    assert!(!condition(&app, calf).young);
}

#[test]
fn test_animals_take_no_food_where_their_kind_is_crowded() {
    let mut app = app(BlockType::Grass);
    let wheat = ItemStack::new(Item::Wheat, 5);
    let player = player(&mut app, Vec3::new(0.5, FEET, 0.5), Some(wheat));
    let crowd = super::tending::CROWD;
    let herd: Vec<_> = (0..crowd)
        .map(|i| {
            let at = Vec3::new(2.5 + (i % 6) as f32, FEET, -2.5 + (i / 6) as f32);
            sheep(&mut app, at)
        })
        .collect();
    app.update();
    use_on(&mut app, herd[0]);
    assert!(!condition(&app, herd[0]).in_love);
    assert_eq!(5, held(&app, player).unwrap().count);
    // one fewer, and it does
    app.world_mut().despawn(herd[crowd - 1]);
    use_on(&mut app, herd[0]);
    assert!(condition(&app, herd[0]).in_love);
}

#[test]
fn test_a_chicken_flutters_down_slowly_and_unhurt() {
    let mut app = app(BlockType::Grass);
    player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
    let chicken = spawned(
        &mut app,
        CreatureKind::Chicken,
        Vec3::new(5.5, FEET + 10.0, 0.5),
    );
    let mut last = at(&app, chicken).y;
    let mut fastest: f32 = 0.0;
    let mut frames = 0;
    while at(&app, chicken).y > FEET + 1e-4 {
        run(&mut app, FRAME);
        let y = at(&app, chicken).y;
        fastest = fastest.max((last - y) / FRAME);
        last = y;
        frames += 1;
        assert!(frames < 400, "still falling at {y}");
    }
    // a sheep would fall the 10 blocks in under a second
    assert!(fastest <= 1.5 + 1e-3, "{fastest}");
    assert!(frames as f32 * FRAME > 6.0, "{frames}");
    assert_eq!(4, health(&app, chicken));
}
