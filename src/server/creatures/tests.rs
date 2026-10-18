use std::time::{Duration, Instant};

use bevy::math::I64Vec3;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy_replicon::prelude::{ClientId, FromClient};
use rand::rngs::StdRng;
use rand::SeedableRng;

use super::{
    creature, move_creatures, receive_hits, sleep, spawn, take_paths, think, wake, Doing, Dormant,
    Health, Mind, Route, SpawnRate,
};
use crate::block::BlockType;
use crate::chunks::chunk::{ChunkCoordinate, ChunkData, CELLS, CHUNK_SIZE};
use crate::clock::TimeOfDay;
use crate::creatures::{Creature, CreatureKind, Flinches};
use crate::inventory::Inventory;
use crate::items::{Item, ItemStack};
use crate::player::{Player, PlayerOwner};
use crate::protocol::HitCreature;
use crate::server::drops::{Chance, DroppedItem};
use crate::server::HeldItem;
use crate::world::World;

const FRAME: f32 = 0.05;
// The top of the ground, the field's floor, and where feet stand on it.
const FLOOR: i64 = 16;
const FEET: f32 = 17.0;

// A field of `surface` 9 chunks across round the origin, its top at `FLOOR`, over stone, with
// the creatures' systems running on it by day, and none appearing by themselves.
fn app(surface: BlockType) -> App {
    let mut world = World::new(16);
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
            let coord = |y| ChunkCoordinate(I64Vec3::new(x, y, z));
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
        .add_message::<FromClient<HitCreature>>()
        .add_systems(
            Update,
            (
                wake,
                spawn,
                take_paths,
                think,
                move_creatures,
                sleep,
                receive_hits,
            )
                .chain(),
        );
    app
}

fn sheep(app: &mut App, at: Vec3) -> Entity {
    let health = CreatureKind::Sheep.properties().health;
    app.world_mut()
        .spawn(creature(CreatureKind::Sheep, at, health, 0.0))
        .id()
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
fn test_a_sheep_killed_drops_mutton_and_wears_the_tool() {
    let mut app = app(BlockType::Grass);
    let axe = ItemStack::new(Item::IronAxe, 1);
    let player = player(&mut app, Vec3::new(0.5, FEET, 0.5), Some(axe));
    let sheep = sheep(&mut app, Vec3::new(2.5, FEET, 0.5));
    app.update();
    // an iron axe hits for 5
    hit(&mut app, sheep);
    app.update();
    assert_eq!(3, health(&app, sheep));
    run(&mut app, 0.6);
    hit(&mut app, sheep);
    app.update();
    assert!(app.world().get_entity(sheep).is_err());
    let mut drops = app.world_mut().query::<&DroppedItem>();
    let mutton: Vec<_> = drops.iter(app.world()).map(|drop| drop.stack).collect();
    assert_eq!(1, mutton.len());
    assert_eq!(Item::Mutton, mutton[0].item);
    assert!((1..=2).contains(&mutton[0].count));
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
fn test_sheep_appear_on_grass_out_of_sight_by_day_up_to_their_cap() {
    let mut app = app(BlockType::Grass);
    app.insert_resource(SpawnRate(Some(0.1)));
    let feet = Vec3::new(0.5, FEET, 0.5);
    player(&mut app, feet, None);
    while creatures(&mut app).is_empty() {
        app.update();
    }
    for sheep in creatures(&mut app) {
        let off = (sheep - feet).xz();
        assert!((19.0..53.0).contains(&off.length()), "{sheep}");
        // looking along -z, which they're never put near
        assert!(off.angle_to(Vec2::NEG_Y).abs() > 0.75, "{sheep}");
        assert_eq!(FEET, sheep.y);
    }
    run(&mut app, 10.0);
    let cap = CreatureKind::Sheep
        .properties()
        .spawns
        .as_ref()
        .unwrap()
        .cap;
    assert_eq!(cap, creatures(&mut app).len());
}

#[test]
fn test_sheep_dont_appear_at_night_or_away_from_grass() {
    for (surface, time) in [(BlockType::Grass, 0.0), (BlockType::Sand, 0.5)] {
        let mut app = app(surface);
        app.insert_resource(SpawnRate(Some(0.1)))
            .insert_resource(TimeOfDay(time));
        player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
        run(&mut app, 3.0);
        assert!(creatures(&mut app).is_empty(), "{surface:?} at {time}");
    }
}
