use bevy::math::I64Vec3;

use super::{Fluids, FLUID_TICK};
use crate::block::{Block, BlockType};
use crate::chunks::chunk::{ChunkCoordinate, ChunkData, CHUNK_SIZE};
use crate::edits::Edits;
use crate::fluids::{fluid_of, Fluid, Level};
use crate::world::World;

// The floor everything stands on, in the second layer of chunks, so nothing's near the bottom
// of the world.
const FLOOR: i64 = 20;
const ON: i64 = FLOOR + 1;

// A world of empty chunks 5 across and 4 high round the origin, with a stone floor 25 across,
// and fluids that have seen none of it.
fn empty() -> (World, Fluids) {
    let mut world = World::from_save(1, Edits::default(), 16);
    for x in -2..=2 {
        for y in 0..4 {
            for z in -2..=2 {
                let coord = ChunkCoordinate(I64Vec3::new(x, y, z));
                world.insert_chunk(coord, ChunkData::default());
            }
        }
    }
    for x in -12..=12 {
        for z in -12..=12 {
            world.edit(I64Vec3::new(x, FLOOR, z), BlockType::Stone);
        }
    }
    world.take_fluid_wakes();
    (world, Fluids::default())
}

fn at(x: i64, y: i64, z: i64) -> I64Vec3 {
    I64Vec3::new(x, y, z)
}

// Puts `block` at `at`, as an edit, which wakes the fluids by it.
fn put(world: &mut World, at: I64Vec3, block: impl Into<Block>) {
    world.edit(at, block);
}

// Steps the fluids until nothing's left to do but wait for chunks to load, returning how many
// ticks that took.
fn settle(world: &mut World, fluids: &mut Fluids) -> usize {
    fluids.wake(world);
    fluids.thaw(world);
    for ticks in 0..5000 {
        if fluids.queued.is_empty() {
            return ticks;
        }
        fluids.step(world);
    }
    panic!("the fluids never settled");
}

fn steps(world: &mut World, fluids: &mut Fluids, ticks: usize) {
    fluids.wake(world);
    for _ in 0..ticks {
        fluids.step(world);
    }
}

fn level(world: &World, at: I64Vec3) -> Option<(Fluid, Level)> {
    fluid_of(world.get(at))
}

#[test]
fn test_water_runs_seven_blocks_from_its_source_a_level_a_block() {
    let (mut world, mut fluids) = empty();
    put(&mut world, at(0, ON, 0), Fluid::Water.source());
    settle(&mut world, &mut fluids);
    for x in -9..=9_i64 {
        for z in -9..=9_i64 {
            let distance = (x.abs() + z.abs()) as u8;
            let expected = match distance {
                0 => Some((Fluid::Water, Level::Source)),
                1..=7 => Some((Fluid::Water, Level::Flowing(distance))),
                _ => None,
            };
            assert_eq!(expected, level(&world, at(x, ON, z)), "{x}, {z}");
        }
    }
    // and nothing runs over the floor's edge, as there's no edge near
    assert_eq!(None, level(&world, at(0, ON + 1, 0)));
}

#[test]
fn test_water_steps_every_five_ticks_and_lava_every_thirty() {
    let (mut world, mut fluids) = empty();
    put(&mut world, at(0, ON, 0), Fluid::Water.source());
    put(&mut world, at(8, ON, 8), Fluid::Lava.source());
    steps(&mut world, &mut fluids, 4);
    assert_eq!(None, level(&world, at(1, ON, 0)));
    steps(&mut world, &mut fluids, 1);
    assert_eq!(
        Some((Fluid::Water, Level::Flowing(1))),
        level(&world, at(1, ON, 0))
    );
    assert_eq!(None, level(&world, at(2, ON, 0)), "a block a step");
    steps(&mut world, &mut fluids, 24);
    assert_eq!(None, level(&world, at(9, ON, 8)));
    steps(&mut world, &mut fluids, 1);
    assert_eq!(
        Some((Fluid::Lava, Level::Flowing(2))),
        level(&world, at(9, ON, 8))
    );
}

#[test]
fn test_lava_runs_three_blocks_two_levels_a_block() {
    let (mut world, mut fluids) = empty();
    put(&mut world, at(0, ON, 0), Fluid::Lava.source());
    settle(&mut world, &mut fluids);
    let levels: Vec<_> = (0..=4).map(|x| level(&world, at(x, ON, 0))).collect();
    let lava = |level| Some((Fluid::Lava, level));
    assert_eq!(
        vec![
            lava(Level::Source),
            lava(Level::Flowing(2)),
            lava(Level::Flowing(4)),
            lava(Level::Flowing(6)),
            None
        ],
        levels
    );
    assert_eq!(lava(Level::Flowing(6)), level(&world, at(1, ON, 2)));
}

#[test]
fn test_water_falls_first_and_spreads_from_where_it_lands() {
    let (mut world, mut fluids) = empty();
    // walled in on a ledge, so it only falls
    for side in crate::fluids::HORIZONTAL {
        put(&mut world, at(0, ON + 4, 0) + side, BlockType::Stone);
    }
    put(&mut world, at(0, ON + 4, 0), Fluid::Water.source());
    settle(&mut world, &mut fluids);
    for y in ON..ON + 4 {
        assert_eq!(
            Some((Fluid::Water, Level::Falling)),
            level(&world, at(0, y, 0)),
            "{y}"
        );
    }
    // a fall spreads only from its bottom, as a source does
    assert_eq!(None, level(&world, at(1, ON + 3, 0)));
    assert_eq!(
        Some((Fluid::Water, Level::Flowing(1))),
        level(&world, at(1, ON, 0))
    );
    assert_eq!(
        Some((Fluid::Water, Level::Flowing(7))),
        level(&world, at(7, ON, 0))
    );

    // and once the source is gone the fall and the pool drain away
    put(&mut world, at(0, ON + 4, 0), BlockType::Air);
    settle(&mut world, &mut fluids);
    for x in 0..8 {
        for y in ON..ON + 4 {
            assert_eq!(None, level(&world, at(x, y, 0)), "{x}, {y}");
        }
    }
}

#[test]
fn test_water_flows_towards_the_nearest_drop() {
    let (mut world, mut fluids) = empty();
    // a hole in the floor three blocks east
    put(&mut world, at(3, FLOOR, 0), BlockType::Air);
    world.take_fluid_wakes();
    put(&mut world, at(0, ON, 0), Fluid::Water.source());
    steps(&mut world, &mut fluids, 5);
    assert_eq!(
        Some((Fluid::Water, Level::Flowing(1))),
        level(&world, at(1, ON, 0))
    );
    for other in [at(-1, ON, 0), at(0, ON, 1), at(0, ON, -1)] {
        assert_eq!(None, level(&world, other), "{other}");
    }
    settle(&mut world, &mut fluids);
    // it pours down the hole, and the source spreads every way once it's no nearer one way
    assert_eq!(
        Some((Fluid::Water, Level::Falling)),
        level(&world, at(3, FLOOR, 0))
    );
    assert!(level(&world, at(3, FLOOR - 1, 0)).is_some());
}

#[test]
fn test_two_water_sources_make_another_between_them_and_lava_never_does() {
    let (mut world, mut fluids) = empty();
    put(&mut world, at(0, ON, 0), Fluid::Water.source());
    put(&mut world, at(2, ON, 0), Fluid::Water.source());
    settle(&mut world, &mut fluids);
    assert_eq!(
        Some((Fluid::Water, Level::Source)),
        level(&world, at(1, ON, 0))
    );
    // so a source taken from between two fills again
    put(&mut world, at(1, ON, 0), BlockType::Air);
    settle(&mut world, &mut fluids);
    assert_eq!(
        Some((Fluid::Water, Level::Source)),
        level(&world, at(1, ON, 0))
    );

    let (mut world, mut fluids) = empty();
    put(&mut world, at(0, ON, 0), Fluid::Lava.source());
    put(&mut world, at(2, ON, 0), Fluid::Lava.source());
    settle(&mut world, &mut fluids);
    assert_eq!(
        Some((Fluid::Lava, Level::Flowing(2))),
        level(&world, at(1, ON, 0))
    );
    // but not over air
    let (mut world, mut fluids) = empty();
    put(&mut world, at(1, FLOOR, 0), BlockType::Air);
    put(&mut world, at(1, FLOOR - 1, 0), BlockType::Stone);
    put(&mut world, at(0, ON, 0), Fluid::Water.source());
    put(&mut world, at(2, ON, 0), Fluid::Water.source());
    settle(&mut world, &mut fluids);
    assert_eq!(
        Some((Fluid::Water, Level::Falling)),
        level(&world, at(1, FLOOR, 0))
    );
    assert_ne!(
        Some((Fluid::Water, Level::Source)),
        level(&world, at(1, ON, 0))
    );
}

#[test]
fn test_water_meeting_a_lava_source_makes_obsidian_and_flowing_lava_cobblestone() {
    let (mut world, mut fluids) = empty();
    put(&mut world, at(0, ON, 0), Fluid::Lava.source());
    put(&mut world, at(2, ON, 0), Fluid::Water.source());
    steps(&mut world, &mut fluids, 5);
    assert_eq!(BlockType::Obsidian, world.get_block_at(at(0, ON, 0)));

    // lava runs a while, then water reaches the end of it
    let (mut world, mut fluids) = empty();
    put(&mut world, at(0, ON, 0), Fluid::Lava.source());
    steps(&mut world, &mut fluids, 60);
    assert_eq!(
        Some((Fluid::Lava, Level::Flowing(4))),
        level(&world, at(2, ON, 0))
    );
    put(&mut world, at(4, ON, 0), Fluid::Water.source());
    steps(&mut world, &mut fluids, 5);
    assert_eq!(BlockType::Cobblestone, world.get_block_at(at(2, ON, 0)));
    assert_eq!(
        Some((Fluid::Lava, Level::Source)),
        level(&world, at(0, ON, 0)),
        "the water's yet to reach the source"
    );
}

#[test]
fn test_lava_falling_onto_water_turns_it_to_stone() {
    let (mut world, mut fluids) = empty();
    // a pool walled in, with lava held over it
    for side in crate::fluids::HORIZONTAL {
        put(&mut world, at(0, ON, 0) + side, BlockType::Stone);
    }
    put(&mut world, at(0, ON, 0), Fluid::Water.source());
    put(&mut world, at(0, ON + 2, 0), Fluid::Lava.source());
    settle(&mut world, &mut fluids);
    assert_eq!(BlockType::Stone, world.get_block_at(at(0, ON, 0)));
    assert_eq!(
        Some((Fluid::Lava, Level::Falling)),
        level(&world, at(0, ON + 1, 0))
    );
}

#[test]
fn test_a_flood_too_big_for_a_tick_carries_over_and_ends_the_same() {
    let flood = |steps_per_tick| {
        let (mut world, mut fluids) = empty();
        fluids.steps_per_tick = steps_per_tick;
        put(&mut world, at(0, ON + 3, 0), Fluid::Water.source());
        put(&mut world, at(-5, ON, 4), Fluid::Water.source());
        let ticks = settle(&mut world, &mut fluids);
        let blocks: Vec<Block> = (-10..=10)
            .flat_map(|x| (ON..ON + 4).flat_map(move |y| (-10..=10).map(move |z| at(x, y, z))))
            .map(|block| world.get(block))
            .collect();
        (ticks, blocks)
    };
    let (quick, all) = flood(super::STEPS_PER_TICK);
    let (slow, carried) = flood(3);
    assert!(slow > quick, "{slow} ticks against {quick}");
    assert!(all == carried, "a flood held back ends as one that isn't");
}

#[test]
fn test_a_tick_steps_no_more_than_its_budget() {
    let (mut world, mut fluids) = empty();
    fluids.steps_per_tick = 2;
    for x in [-6, -3, 0, 3, 6] {
        put(&mut world, at(x, ON, 0), Fluid::Water.source());
    }
    // all five are due on the fifth tick, and two are stepped then, two the tick after
    let spread = |world: &World, x| level(world, at(x, ON, 1)).is_some();
    steps(&mut world, &mut fluids, 5);
    assert_eq!(
        [true, true, false, false, false],
        [-6, -3, 0, 3, 6].map(|x| spread(&world, x))
    );
    steps(&mut world, &mut fluids, 1);
    assert_eq!(
        [true, true, true, true, false],
        [-6, -3, 0, 3, 6].map(|x| spread(&world, x))
    );
}

#[test]
fn test_generated_and_saved_water_stands_still_until_disturbed() {
    // a chunk of sea as generation makes it, sources all, and saved edits of flowing water
    let mut sea = ChunkData::default();
    for x in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            for y in 0..CHUNK_SIZE {
                let block = if y < 8 {
                    BlockType::Stone
                } else if y < 12 {
                    BlockType::Water
                } else {
                    BlockType::Air
                };
                sea.set_block_at(bevy::math::U16Vec3::new(x, y, z), block);
            }
        }
    }
    let mut edits = Edits::default();
    let shore = at(0, 16 + 12, 0);
    edits.set(shore, Fluid::Water.at(Level::Flowing(3)));
    let mut world = World::from_save(1, edits, 16);
    for x in -1..=1 {
        for z in -1..=1 {
            world.insert_chunk(ChunkCoordinate(I64Vec3::new(x, 1, z)), sea.clone());
            world.insert_chunk(ChunkCoordinate(I64Vec3::new(x, 2, z)), ChunkData::default());
        }
    }
    let mut fluids = Fluids::default();
    assert_eq!(Vec::<I64Vec3>::new(), world.take_fluid_wakes());
    fluids.run(&mut world, FLUID_TICK * 200);
    assert_eq!(0, fluids.pending());
    assert!(fluids.take_changed(&world).is_empty());
    assert_eq!(
        Some((Fluid::Water, Level::Flowing(3))),
        level(&world, shore)
    );

    // and breaking the sea's floor sets it flowing
    let floor = at(4, 16 + 7, 4);
    put(&mut world, floor, BlockType::Air);
    put(&mut world, floor - I64Vec3::Y, BlockType::Air);
    steps(&mut world, &mut fluids, 10);
    assert!(level(&world, floor - I64Vec3::Y).is_some());
    assert!(!fluids.take_changed(&world).is_empty());
}

#[test]
fn test_fluids_wait_at_chunks_not_loaded_and_go_on_once_they_are() {
    let (mut world, mut fluids) = empty();
    // the chunk to the east of the floor's edge isn't loaded
    let east = ChunkCoordinate(I64Vec3::new(3, 1, 0));
    for x in 32..48 {
        for z in 0..16 {
            put(&mut world, at(x, FLOOR, z), BlockType::Stone);
        }
    }
    put(&mut world, at(46, ON, 8), Fluid::Water.source());
    settle(&mut world, &mut fluids);
    assert_eq!(
        Some((Fluid::Water, Level::Flowing(1))),
        level(&world, at(47, ON, 8))
    );
    assert_eq!(None, level(&world, at(48, ON, 8)));
    assert!(!world.is_chunk_generated(east));
    assert!(fluids.pending() > 0, "waiting for the chunk");
    world.insert_chunk(east, ChunkData::default());
    put(&mut world, at(48, FLOOR, 8), BlockType::Stone);
    world.take_fluid_wakes();
    settle(&mut world, &mut fluids);
    assert_eq!(
        Some((Fluid::Water, Level::Flowing(2))),
        level(&world, at(48, ON, 8))
    );
}

// What a dam broken into a big cavern costs the server, tick by tick: a lake 48 across and 6
// deep, walled off from a cavern 72 by 96 whose floor lies 16 blocks below the lake's bed, with
// holes in it every 8 blocks down to a second floor 8 lower. Run with
// `cargo test --release flood_cost -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn flood_cost() {
    let mut world = World::from_save(1, Edits::default(), 16);
    for x in -4..=4 {
        for y in 0..6 {
            for z in -4..=4 {
                let coord = ChunkCoordinate(I64Vec3::new(x, y, z));
                world.insert_chunk(coord, ChunkData::default());
            }
        }
    }
    let (bed, upper, lower) = (64, 48, 40);
    for x in -48..0 {
        for z in -24..24 {
            world.edit(at(x, bed, z), BlockType::Stone);
            for y in bed + 1..bed + 7 {
                world.edit(at(x, y, z), Fluid::Water.source());
            }
        }
    }
    for z in -24..24 {
        for y in bed..bed + 8 {
            world.edit(at(0, y, z), BlockType::Stone);
        }
    }
    for x in 0..72 {
        for z in -48_i64..48 {
            let hole = x % 8 == 4 && z.rem_euclid(8) == 4;
            if !hole {
                world.edit(at(x, upper, z), BlockType::Stone);
            }
            world.edit(at(x, lower, z), BlockType::Stone);
        }
    }
    world.take_fluid_wakes();
    let mut fluids = Fluids::default();
    // the wall gives way
    for z in -24..24 {
        for y in bed + 1..bed + 7 {
            world.edit(at(0, y, z), BlockType::Air);
        }
    }
    fluids.wake(&mut world);
    let (mut ticks, mut most, mut total) = (0, 0.0_f64, 0.0_f64);
    let (mut changed, mut busiest, mut messages) = (0, 0, 0);
    while !fluids.queued.is_empty() && ticks < 20_000 {
        let start = std::time::Instant::now();
        fluids.step(&mut world);
        let sent = fluids.take_changed(&world);
        let took = start.elapsed().as_secs_f64() * 1000.0;
        let blocks: usize = sent.iter().map(|(_, blocks)| blocks.len()).sum();
        (most, total) = (most.max(took), total + took);
        (changed, busiest, messages) =
            (changed + blocks, busiest.max(blocks), messages + sent.len());
        ticks += 1;
    }
    println!(
        "flood: {ticks} ticks ({:.1} s of game), {changed} blocks changed, at most {busiest} \
         in a tick, {messages} chunk messages; {:.3} ms a tick on average, {most:.3} ms at most",
        ticks as f64 / 20.0,
        total / ticks as f64
    );
}
