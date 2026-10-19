//! Monsters (M3.3): where they appear, how they hunt and fight, and how they go.

use super::*;
use crate::creatures::{Missile, Stance, Vanishing};
use crate::game_mode::GameMode;
use crate::server::survival::{Combat, Footing, Surroundings};

// Midnight and noon.
const NIGHT: f64 = 0.0;
const NOON: f64 = 0.5;

fn edit(app: &mut App, at: I64Vec3, block: BlockType) {
    app.world_mut().resource_mut::<World>().edit(at, block);
}

// Fills the box from `min` to `max`, both in it, with `block`.
fn fill(app: &mut App, min: I64Vec3, max: I64Vec3, block: BlockType) {
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                edit(app, I64Vec3::new(x, y, z), block);
            }
        }
    }
}

// A tunnel through the stone under the field, 5 wide and 4 tall from y 4 along x from `from`
// to `to`, with its floor of stone at y 3 and its ceiling at y 8.
fn tunnel(app: &mut App, from: i64, to: i64) {
    let (min, max) = (I64Vec3::new(from, 4, -2), I64Vec3::new(to, 7, 2));
    fill(app, min, max, BlockType::Air);
}

fn doing(app: &App, creature: Entity) -> Doing {
    app.world().get::<Mind>(creature).unwrap().doing
}

fn stance(app: &App, creature: Entity) -> Stance {
    *app.world().get::<Stance>(creature).unwrap()
}

fn blows(app: &App) -> Vec<Blow> {
    app.world().resource::<Blows>().0.clone()
}

fn move_to(app: &mut App, entity: Entity, to: Vec3) {
    app.world_mut()
        .get_mut::<Transform>(entity)
        .unwrap()
        .translation = to;
}

fn count(app: &mut App, kind: CreatureKind) -> usize {
    kinds(app).into_iter().filter(|&k| k == kind).count()
}

fn missiles(app: &mut App) -> usize {
    let mut query = app.world_mut().query::<&Missile>();
    query.iter(app.world()).count()
}

// A monster hanging from the ceiling with its body's top at `top`.
fn clinging(app: &mut App, kind: CreatureKind, x: f32, top: f32, z: f32) -> Entity {
    let height = kind.properties().height;
    let monster = spawned(app, kind, Vec3::new(x, top - height, z));
    app.world_mut().entity_mut(monster).insert(Stance {
        clinging: true,
        ..default()
    });
    monster
}

#[test]
fn test_monsters_appear_at_night_out_of_reach_up_to_their_caps_and_gloamers_go_by_day() {
    let mut app = app(BlockType::Grass);
    app.insert_resource(SpawnRate(Some(0.1)))
        .insert_resource(TimeOfDay::from_days(NIGHT));
    let feet = Vec3::new(0.5, FEET, 0.5);
    player(&mut app, feet, None);
    // never nearer than 24 blocks as they appear
    while creatures(&mut app).is_empty() {
        app.update();
    }
    for creature in creatures(&mut app) {
        assert!((creature - feet).xz().length() > 23.0, "{creature}");
    }
    run(&mut app, 30.0);
    // in the open at night, the gloamers and slingers, as many as their caps, and none of the
    // cave's, as there are no caves
    let caps = |kind: CreatureKind| kind.properties().spawns.as_ref().unwrap().cap;
    assert_eq!(
        caps(CreatureKind::Gloamer),
        count(&mut app, CreatureKind::Gloamer)
    );
    assert_eq!(
        caps(CreatureKind::Slinger),
        count(&mut app, CreatureKind::Slinger)
    );
    assert_eq!(0, count(&mut app, CreatureKind::Lurker));
    assert_eq!(0, count(&mut app, CreatureKind::BarrowBrute));
    // and no animals
    assert!(kinds(&mut app).into_iter().all(is_monster));

    // at sunrise the gloamers vanish, saying so first, and the slingers stay
    app.insert_resource(TimeOfDay::from_days(NOON))
        .insert_resource(SpawnRate(None));
    app.update();
    let mut vanishing = app
        .world_mut()
        .query_filtered::<&Creature, With<Vanishing>>();
    let gloamers = vanishing.iter(app.world()).count();
    assert_eq!(caps(CreatureKind::Gloamer), gloamers);
    app.update();
    assert_eq!(0, count(&mut app, CreatureKind::Gloamer));
    assert_eq!(
        caps(CreatureKind::Slinger),
        count(&mut app, CreatureKind::Slinger)
    );
}

#[test]
fn test_no_monster_appears_where_torches_light_the_ground() {
    let mut app = app(BlockType::Grass);
    app.insert_resource(SpawnRate(Some(0.1)))
        .insert_resource(TimeOfDay::from_days(NIGHT));
    // a torch every 12 blocks, so nowhere's further than 12 from one, and lit
    for x in -5..=5 {
        for z in -5..=5 {
            edit(
                &mut app,
                I64Vec3::new(x * 12, FLOOR + 1, z * 12),
                BlockType::Torch,
            );
        }
    }
    let corner = I64Vec3::new(6, FLOOR + 1, 6);
    assert!(app.world().resource::<World>().block_light(corner) > 0);
    player(&mut app, Vec3::new(0.5, FEET, 3.5), None);
    run(&mut app, 20.0);
    assert!(creatures(&mut app).is_empty(), "{:?}", kinds(&mut app));
}

#[test]
fn test_the_caves_have_lurkers_on_their_ceilings_and_brutes_deep_down_by_day_but_not_lit() {
    for lit in [false, true] {
        // sand, which nothing appears on by day
        let mut app = app(BlockType::Sand);
        app.insert_resource(SpawnRate(Some(0.1)));
        tunnel(&mut app, -60, 60);
        if lit {
            for x in -6..=6 {
                edit(&mut app, I64Vec3::new(x * 10, 4, 0), BlockType::Torch);
            }
        }
        player(&mut app, Vec3::new(0.5, 4.0, 0.5), None);
        run(&mut app, 40.0);
        if lit {
            assert!(kinds(&mut app).is_empty(), "{:?}", kinds(&mut app));
            continue;
        }
        // by day, under ground: lurkers hanging, the brute, slingers, and no gloamers
        let mut query = app.world_mut().query::<(&Creature, &Stance, &Transform)>();
        let seen: Vec<_> = query
            .iter(app.world())
            .map(|(c, s, t)| (c.0, *s, t.translation))
            .collect();
        let lurkers: Vec<_> = seen
            .iter()
            .filter(|s| s.0 == CreatureKind::Lurker)
            .collect();
        assert!(!lurkers.is_empty(), "{seen:?}");
        for (_, stance, body) in lurkers {
            // its top against the ceiling's underside
            assert!(stance.clinging);
            assert!((body.y + CreatureKind::Lurker.properties().height - 8.0).abs() < 1e-4);
        }
        assert_eq!(1, count(&mut app, CreatureKind::BarrowBrute), "{seen:?}");
        assert_eq!(0, count(&mut app, CreatureKind::Gloamer));
    }
}

#[test]
fn test_a_spawner_brings_out_its_kind_in_the_dark_up_to_a_few_and_not_lit_or_broken() {
    let mut app = app(BlockType::Sand);
    app.insert_resource(SpawnRate(Some(0.5)));
    // a tunnel too short for monsters to appear in by themselves, out of reach of the player
    tunnel(&mut app, -8, 16);
    let spawner = I64Vec3::new(10, 4, 0);
    edit(&mut app, spawner, BlockType::Spawner);
    player(&mut app, Vec3::new(0.5, 4.0, 0.5), None);
    let kind = CreatureKind::guarding(SEED, spawner);
    let mut frames = 0;
    while creatures(&mut app).is_empty() {
        app.update();
        frames += 1;
        assert!(frames < 1000, "nothing came out in 50 s");
    }
    for creature in creatures(&mut app) {
        let off = creature - (spawner.as_vec3() + Vec3::new(0.5, 0.0, 0.5));
        assert!(
            off.xz().abs().max_element() <= 4.0 && off.y.abs() <= 2.0,
            "{off}"
        );
    }
    run(&mut app, 120.0);
    let all = kinds(&mut app);
    assert!(all.iter().all(|&k| k == kind), "{all:?}");
    assert!((1..=4).contains(&all.len()), "{all:?}");

    // lit, it brings out no more, and broken neither
    for block in [BlockType::Torch, BlockType::Air] {
        for creature in every_creature(&mut app) {
            app.world_mut().despawn(creature);
        }
        if block == BlockType::Torch {
            edit(&mut app, spawner + I64Vec3::new(0, 1, 1), BlockType::Torch);
        } else {
            edit(&mut app, spawner, BlockType::Air);
            edit(&mut app, spawner + I64Vec3::new(0, 1, 1), BlockType::Air);
        }
        run(&mut app, 45.0);
        assert!(creatures(&mut app).is_empty(), "{block:?}");
    }
}

#[test]
fn test_a_gloamer_hunts_a_player_in_survival_it_sees_and_strikes_after_its_cooldown() {
    let mut app = app(BlockType::Grass);
    app.insert_resource(TimeOfDay::from_days(NIGHT));
    let player = player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
    let gloamer = spawned(&mut app, CreatureKind::Gloamer, Vec3::new(12.5, FEET, 0.5));
    run(&mut app, 0.6);
    assert_eq!(Doing::Hunt(player), doing(&app, gloamer));
    run(&mut app, 6.0);
    let blows = blows(&app);
    assert!(blows.len() >= 2, "{blows:?}");
    for blow in &blows {
        assert_eq!(
            (player, 3, CreatureKind::Gloamer),
            (blow.player, blow.damage, blow.by)
        );
    }
    // no more often than its cooldown, 1.3 s and its wind-up, about 30 frames
    assert!(blows.len() <= 6, "{}", blows.len());

    // a player in creative's left alone
    app.world_mut()
        .entity_mut(player)
        .insert(GameMode::Creative);
    run(&mut app, 1.0);
    assert!(!matches!(doing(&app, gloamer), Doing::Hunt(_)));
}

#[test]
fn test_a_brute_winds_up_for_a_second_and_misses_a_player_who_steps_away() {
    let mut app = app(BlockType::Grass);
    let player = player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
    let brute = spawned(
        &mut app,
        CreatureKind::BarrowBrute,
        Vec3::new(2.5, FEET, 0.5),
    );
    let mut frames = 0;
    while !stance(&app, brute).winding {
        run(&mut app, FRAME);
        frames += 1;
        assert!(frames < 40);
    }
    // it shows the blow coming for a second before it lands
    run(&mut app, 0.9);
    assert!(stance(&app, brute).winding && blows(&app).is_empty());
    run(&mut app, 0.15);
    let landed = blows(&app);
    assert_eq!(1, landed.len());
    assert_eq!((8, 9.0), (landed[0].damage, landed[0].knockback));
    // and, once it's cooled down, winds up again, which stepping back from makes it miss
    let mut frames = 0;
    while !stance(&app, brute).winding {
        run(&mut app, FRAME);
        frames += 1;
        assert!(frames < 80);
    }
    assert!(frames as f32 * FRAME > 2.0, "{frames}");
    move_to(&mut app, player, Vec3::new(-6.5, FEET, 0.5));
    run(&mut app, 1.1);
    assert_eq!(1, blows(&app).len());
}

#[test]
fn test_a_slinger_backs_off_and_throws_stones_that_fly_and_strike_but_not_unseen() {
    let mut app = app(BlockType::Grass);
    let player = player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
    let slinger = spawned(&mut app, CreatureKind::Slinger, Vec3::new(3.5, FEET, 0.5));
    run(&mut app, 3.0);
    let off = (at(&app, slinger) - Vec3::new(0.5, FEET, 0.5))
        .xz()
        .length();
    assert!(off > 4.5, "{off}");
    app.world_mut().resource_mut::<Blows>().0.clear();
    // from its range it throws, and a stone lands
    let mut thrown = false;
    for _ in 0..200 {
        run(&mut app, FRAME);
        thrown |= missiles(&mut app) > 0;
        if !blows(&app).is_empty() {
            break;
        }
    }
    assert!(thrown);
    let landed = blows(&app);
    assert_eq!(1, landed.len(), "{landed:?}");
    assert_eq!(
        (player, CreatureKind::Slinger),
        (landed[0].player, landed[0].by)
    );

    // shut in a hut of stone, they're out of its sight, and nothing's thrown
    let hut = (
        I64Vec3::new(-2, FLOOR + 1, -2),
        I64Vec3::new(2, FLOOR + 4, 2),
    );
    fill(&mut app, hut.0, hut.1, BlockType::Stone);
    let inside = (
        I64Vec3::new(-1, FLOOR + 1, -1),
        I64Vec3::new(1, FLOOR + 3, 1),
    );
    fill(&mut app, inside.0, inside.1, BlockType::Air);
    run(&mut app, 1.0);
    app.world_mut().resource_mut::<Blows>().0.clear();
    let mut thrown = 0;
    for _ in 0..100 {
        run(&mut app, FRAME);
        thrown += missiles(&mut app);
    }
    assert_eq!(0, thrown);
    assert!(blows(&app).is_empty());
}

#[test]
fn test_a_lurker_hangs_till_someone_comes_under_it_then_drops_unhurt_and_strikes() {
    let mut app = app(BlockType::Grass);
    // a roof over the field, 4 blocks up
    let roof = FLOOR + 5;
    fill(
        &mut app,
        I64Vec3::new(-12, roof, -12),
        I64Vec3::new(12, roof, 12),
        BlockType::Stone,
    );
    let player = player(&mut app, Vec3::new(-8.5, FEET, 0.5), None);
    let lurker = clinging(&mut app, CreatureKind::Lurker, 0.5, roof as f32, 0.5);
    run(&mut app, 3.0);
    assert_eq!(Doing::Cling, doing(&app, lurker));
    assert!((at(&app, lurker).y + 0.6 - roof as f32).abs() < 1e-4);
    move_to(&mut app, player, Vec3::new(1.5, FEET, 0.5));
    run(&mut app, 0.6);
    assert!(!stance(&app, lurker).clinging);
    assert_eq!(Doing::Hunt(player), doing(&app, lurker));
    run(&mut app, 3.0);
    assert!((at(&app, lurker).y - FEET).abs() < 1e-3);
    assert_eq!(12, health(&app, lurker));
    assert!(blows(&app)
        .iter()
        .any(|blow| blow.by == CreatureKind::Lurker));
}

#[test]
fn test_a_lurker_runs_from_torchlight_and_from_a_torch_held_near() {
    let mut app = app(BlockType::Grass);
    app.insert_resource(TimeOfDay::from_days(NIGHT));
    let torch = I64Vec3::new(0, FLOOR + 1, 0);
    edit(&mut app, torch, BlockType::Torch);
    player(&mut app, Vec3::new(-40.5, FEET, 0.5), None);
    let lurker = spawned(&mut app, CreatureKind::Lurker, Vec3::new(2.5, FEET, 0.5));
    run(&mut app, 3.0);
    // to where the light's dim, from 2 blocks off
    let away = (at(&app, lurker) - torch.as_vec3()).xz().length();
    assert!(away > 4.5, "{away}");

    // and from a player holding one, rather than going after them
    edit(&mut app, torch, BlockType::Air);
    let torch = ItemStack::new(Item::Block(BlockType::Torch), 1);
    let near = at(&app, lurker) + Vec3::X * 3.0;
    let holder = self::player(&mut app, near, Some(torch));
    run(&mut app, 0.6);
    assert!(matches!(doing(&app, lurker), Doing::Flee { .. }));
    app.world_mut().entity_mut(holder).insert(HeldItem(None));
    run(&mut app, 0.6);
    assert_eq!(Doing::Hunt(holder), doing(&app, lurker));
}

#[test]
fn test_gloamers_join_one_of_their_pack_on_a_hunt_and_give_up_on_someone_far_off() {
    let mut app = app(BlockType::Grass);
    app.insert_resource(TimeOfDay::from_days(NIGHT));
    let player = player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
    // a wall between the player and one of them only
    let wall = (
        I64Vec3::new(4, FLOOR + 1, 2),
        I64Vec3::new(5, FLOOR + 3, 12),
    );
    fill(&mut app, wall.0, wall.1, BlockType::Stone);
    let behind = Vec3::new(8.5, FEET, 8.5);
    let alone = spawned(&mut app, CreatureKind::Gloamer, behind);
    run(&mut app, 1.0);
    assert!(!matches!(doing(&app, alone), Doing::Hunt(_)));
    app.world_mut().despawn(alone);
    let hidden = spawned(&mut app, CreatureKind::Gloamer, behind);
    let seeing = spawned(&mut app, CreatureKind::Gloamer, Vec3::new(8.5, FEET, 0.5));
    run(&mut app, 0.1);
    assert_eq!(Doing::Hunt(player), doing(&app, seeing));
    run(&mut app, 0.6);
    assert_eq!(Doing::Hunt(player), doing(&app, hidden));

    // one beyond half again as far as it sees is let go
    move_to(&mut app, player, Vec3::new(-40.5, FEET, 0.5));
    move_to(&mut app, seeing, Vec3::new(0.5, FEET, 0.5));
    run(&mut app, 0.6);
    assert_ne!(Doing::Hunt(player), doing(&app, seeing));
}

#[test]
fn test_a_monster_hit_turns_on_whoever_hit_it_and_drops_off_its_ceiling() {
    let mut app = app(BlockType::Grass);
    let roof = FLOOR + 4;
    fill(
        &mut app,
        I64Vec3::new(-6, roof, -6),
        I64Vec3::new(6, roof, 6),
        BlockType::Stone,
    );
    let player = player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
    let lurker = clinging(&mut app, CreatureKind::Lurker, 3.5, roof as f32, 0.5);
    app.update();
    hit(&mut app, lurker);
    app.update();
    assert_eq!(11, health(&app, lurker));
    assert!(!stance(&app, lurker).clinging);
    assert_eq!(Doing::Hunt(player), doing(&app, lurker));
}

#[test]
fn test_a_hit_coming_down_from_a_jump_is_critical() {
    let mut app = app(BlockType::Grass);
    let sword = ItemStack::new(Item::WoodenSword, 1);
    let player = player(&mut app, Vec3::new(0.5, FEET, 0.5), Some(sword));
    let sheep = sheep(&mut app, Vec3::new(2.5, FEET, 0.5));
    app.update();
    let mut footing = Footing::default();
    let ground = Surroundings {
        on_ground: true,
        ..default()
    };
    footing.step(Vec3::new(0.5, FEET, 0.5), ground, FRAME);
    footing.step(
        Vec3::new(0.5, FEET + 1.0, 0.5),
        Surroundings::default(),
        FRAME,
    );
    footing.step(
        Vec3::new(0.5, FEET + 0.5, 0.5),
        Surroundings::default(),
        FRAME,
    );
    app.world_mut().entity_mut(player).insert(footing);
    hit(&mut app, sheep);
    app.update();
    // a wooden sword's 4, half as much again
    assert_eq!(8 - 6, health(&app, sheep));
    assert!(app.world().get::<Combat>(player).unwrap().ready_at > 0.0);
}

#[test]
fn test_monsters_despawn_far_from_everyone_unsaved_while_animals_sleep() {
    let mut app = app(BlockType::Grass);
    app.insert_resource(TimeOfDay::from_days(NIGHT));
    let player = player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
    app.world_mut()
        .entity_mut(player)
        .insert(GameMode::Creative);
    let gloamer = spawned(&mut app, CreatureKind::Gloamer, Vec3::new(10.5, FEET, 0.5));
    let sheep = sheep(&mut app, Vec3::new(10.5, FEET, 4.5));
    run(&mut app, 0.5);
    // over 64 blocks from the gloamer, and under 128 from the sheep
    move_to(&mut app, player, Vec3::new(-60.5, FEET, 0.5));
    run(&mut app, 0.1);
    assert!(app.world().get_entity(gloamer).is_err());
    assert!(app.world().get_entity(sheep).is_ok());
    move_to(&mut app, player, Vec3::new(200.5, FEET, 0.5));
    app.update();
    let dormant = &app.world().resource::<Dormant>().0;
    let asleep: Vec<_> = dormant.values().flatten().map(|c| c.kind).collect();
    assert_eq!(vec![CreatureKind::Sheep], asleep);

    // a gloamer under a roof by day stays, and one under the sky vanishes
    let mut app = self::app(BlockType::Grass);
    app.insert_resource(TimeOfDay::from_days(NOON));
    let player = self::player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
    app.world_mut()
        .entity_mut(player)
        .insert(GameMode::Creative);
    fill(
        &mut app,
        I64Vec3::new(8, FLOOR + 3, -2),
        I64Vec3::new(12, FLOOR + 3, 2),
        BlockType::Stone,
    );
    let roofed = spawned(&mut app, CreatureKind::Gloamer, Vec3::new(10.5, FEET, 0.5));
    let open = spawned(&mut app, CreatureKind::Gloamer, Vec3::new(-10.5, FEET, 0.5));
    app.update();
    assert!(app.world().get::<Vanishing>(open).is_some());
    app.update();
    assert!(app.world().get_entity(open).is_err());
    assert!(app.world().get_entity(roofed).is_ok());
}

#[test]
fn test_a_throw_aims_its_arc_to_land_where_its_aimed_and_lines_of_sight_stop_at_walls() {
    use super::super::hunting::{aim, sees};
    let from = Vec3::new(0.0, 1.0, 0.0);
    let to = Vec3::new(9.0, 1.5, 4.0);
    let velocity = aim(from, to, 16.0).unwrap();
    assert!((velocity.length() - 16.0).abs() < 1e-3);
    let missile = Missile { from, velocity };
    let t = (to - from).xz().length() / velocity.xz().length();
    assert!(missile.at(t).distance(to) < 1e-3, "{}", missile.at(t));
    assert_eq!(None, aim(from, from + Vec3::X * 60.0, 16.0));

    let mut app = app(BlockType::Grass);
    let eye = Vec3::new(0.5, FEET + 1.0, 0.5);
    let there = Vec3::new(8.5, FEET + 1.0, 0.5);
    assert!(sees(app.world().resource::<World>(), eye, there));
    edit(&mut app, I64Vec3::new(4, FLOOR + 2, 0), BlockType::Stone);
    assert!(!sees(app.world().resource::<World>(), eye, there));
    // glass is seen through
    edit(&mut app, I64Vec3::new(4, FLOOR + 2, 0), BlockType::Glass);
    assert!(sees(app.world().resource::<World>(), eye, there));
}

#[test]
fn test_a_dungeons_spawner_is_built_as_someone_comes_near_and_not_again_once_broken() {
    use crate::chunks::generate::structures::SPAWNER;
    use crate::protocol::BlockChanged;
    use bevy::math::I64Vec2;
    use bevy_replicon::prelude::ToClients;

    // the spawner of the dungeon nearest the origin, where the world's generation leaves air
    let marker = {
        let world = World::from_save(SEED, Edits::default(), 16);
        let reach = I64Vec2::splat(4096);
        let mut dungeons: Vec<_> = world
            .region_store
            .structures_near(-reach, reach)
            .into_iter()
            .filter(|placed| placed.name() == "dungeon")
            .collect();
        dungeons.sort_by_key(|placed| placed.min.xz().length_squared());
        let dungeon = dungeons.first().expect("a dungeon within 4 km");
        let mut markers = dungeon.markers().filter(|&(kind, _)| kind == SPAWNER);
        markers.next().expect("a dungeon has a spawner").1
    };
    let chunk = ChunkCoordinate::containing(marker).0;
    let mut app = field(BlockType::Stone, chunk - I64Vec3::Y);
    app.add_message::<ToClients<BlockChanged>>()
        .add_systems(Update, super::super::build_spawners);
    // its chunk as generation leaves it, empty round the marker
    app.world_mut().resource_mut::<World>().insert_chunk(
        ChunkCoordinate(chunk),
        ChunkData::from_cells([BlockType::Air; CELLS]),
    );
    let block = |app: &App| app.world().resource::<World>().get_block_at(marker);
    let edited = |app: &App| app.world().resource::<World>().edits().get(marker);

    // nobody near, it isn't built
    let far = player(&mut app, marker.as_vec3() + Vec3::X * 60.0, None);
    app.update();
    assert_eq!((BlockType::Air, None), (block(&app), edited(&app)));
    // and as someone comes within 32 blocks it is, as an edit, and clients are told
    move_to(&mut app, far, marker.as_vec3() + Vec3::X * 20.0);
    app.update();
    assert_eq!(BlockType::Spawner, block(&app));
    assert_eq!(Some(BlockType::Spawner.into()), edited(&app));
    let told = app.world().resource::<Messages<ToClients<BlockChanged>>>();
    assert_eq!(1, told.len());
    // broken, it's air, and stays so
    edit(&mut app, marker, BlockType::Air);
    run(&mut app, 1.0);
    assert_eq!(BlockType::Air, block(&app));
}
