use std::time::Duration;

use bevy::asset::AssetPlugin;
use bevy::math::I64Vec3;
use bevy::state::app::StatesPlugin;
use bevy::time::TimeUpdateStrategy;
use rand::rngs::StdRng;
use rand::SeedableRng;

use super::birds::{
    flock_heading, migration_course, skein_place, steering, step_flock, Goose, Starling, SPEED,
    STARLINGS,
};
use super::glow::{nearest_lit, FireflyLight};
use super::insects::{butterfly_at, firefly_at, firefly_blink, Butterfly, Firefly, FIREFLIES};
use super::*;
use crate::block::BlockType;
use crate::chunks::chunk::{ChunkCoordinate, ChunkData, CELLS, CHUNK_SIZE};
use crate::chunks::generate::noise::NoiseGenerator;
use crate::client::creatures::{creature_hit, SeenCreature};
use crate::collision::{raycast, Aabb};
use crate::edits::Edits;
use crate::menu::GameState;
use crate::player::{PlayerLook, REACH};
use crate::preferences::Preferences;
use crate::world::World;

const FRAME: f32 = 0.05;
const SEED: u32 = 1;
// The top of the ground, and the camera's height over it.
const FLOOR: i64 = 16;
const EYE: f32 = FLOOR as f32 + 2.6;

const NOON: f64 = 0.5;
const MIDNIGHT: f64 = 0.0;
const DUSK_TIME: f64 = 0.71;

// The nearest chunk to the origin, on a coarse grid, whose land within 60 blocks is all `biome`.
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

// What a field's ground is: grass, with `plant` on it, and water in a strip along one side of
// every chunk if it's `wet`.
#[derive(Clone, Copy)]
struct Ground {
    plant: Option<BlockType>,
    wet: bool,
}

const BARE: Ground = Ground {
    plant: None,
    wet: false,
};

// A field 9 chunks across round the chunk `centre`, its top at `FLOOR`, over stone, with
// ambient life about the camera over its middle at `time`, and the dice seeded.
fn field(centre: I64Vec3, ground: Ground, time: f64) -> App {
    let mut world = World::from_save(SEED, Edits::default(), 16);
    let size = CHUNK_SIZE as usize;
    let top = ChunkData::from_cells((0..CELLS).map(|i| {
        let (x, y) = (i / (size * size), i % size);
        match y {
            0 if ground.wet && x < 4 => BlockType::Water,
            0 => BlockType::Grass,
            1 if !(ground.wet && x < 4) => ground.plant.unwrap_or(BlockType::Air),
            _ => BlockType::Air,
        }
    }));
    for x in -4..=4 {
        for z in -4..=4 {
            let coord = |y| ChunkCoordinate(centre + I64Vec3::new(x, y, z));
            world.insert_chunk(coord(0), ChunkData::from_cells([BlockType::Stone; CELLS]));
            world.insert_chunk(coord(1), top.clone());
            world.insert_chunk(coord(2), ChunkData::default());
        }
    }
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        StatesPlugin,
        AssetPlugin::default(),
        AmbientLifePlugin,
    ))
    .init_asset::<Mesh>()
    .init_asset::<StandardMaterial>()
    .init_asset::<Image>()
    .insert_state(GameState::Playing)
    .insert_resource(world)
    .insert_resource(TimeOfDay(time))
    .insert_resource(CritterDice(StdRng::seed_from_u64(3)))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
        FRAME,
    )));
    let eye = (centre * CHUNK_SIZE as i64).as_vec3().with_y(EYE) + Vec3::new(8.0, 0.0, 8.0);
    let camera = Transform::from_translation(eye).looking_to(Vec3::X, Vec3::Y);
    app.world_mut()
        .spawn((Camera3d::default(), camera, GlobalTransform::from(camera)));
    app
}

fn run(app: &mut App, seconds: f32) {
    for _ in 0..(seconds / FRAME) as usize {
        app.update();
    }
}

fn count<T: Component>(app: &mut App) -> usize {
    app.world_mut()
        .query_filtered::<(), With<T>>()
        .iter(app.world())
        .count()
}

fn eye_of(app: &mut App) -> Vec3 {
    app.world_mut()
        .query_filtered::<&GlobalTransform, With<Camera3d>>()
        .single(app.world())
        .unwrap()
        .translation()
}

fn flowers() -> Ground {
    Ground {
        plant: Some(BlockType::Poppy),
        wet: false,
    }
}

#[test]
fn test_butterflies_come_out_over_flowers_by_day_and_not_at_night_or_over_bare_grass() {
    let base = chunk_in("base");
    let mut day = field(base, flowers(), NOON);
    run(&mut day, 3.0);
    let butterflies = count::<Butterfly>(&mut day);
    assert!(
        (1..=insects::BUTTERFLIES).contains(&butterflies),
        "{butterflies}"
    );
    // each by a flower, near the camera
    let eye = eye_of(&mut day);
    for at in day
        .world_mut()
        .query_filtered::<&Transform, With<Butterfly>>()
        .iter(day.world())
    {
        assert!(at.translation.distance(eye) < 30.0, "{at:?}");
        assert!(
            (at.translation.y - (FLOOR as f32 + 2.2)).abs() < 1.5,
            "{at:?}"
        );
    }

    let mut night = field(base, flowers(), MIDNIGHT);
    run(&mut night, 3.0);
    assert_eq!(count::<Butterfly>(&mut night), 0);

    let mut bare = field(base, BARE, NOON);
    run(&mut bare, 3.0);
    assert_eq!(count::<Butterfly>(&mut bare), 0);
}

#[test]
fn test_butterflies_fly_off_as_night_falls() {
    let mut app = field(chunk_in("base"), flowers(), NOON);
    run(&mut app, 3.0);
    assert!(count::<Butterfly>(&mut app) > 0);
    app.insert_resource(TimeOfDay(MIDNIGHT));
    run(&mut app, 2.0);
    assert_eq!(count::<Butterfly>(&mut app), 0);
}

#[test]
fn test_fireflies_come_out_at_night_by_water_and_in_the_woods_but_not_on_dry_open_grass() {
    let wet = Ground {
        plant: None,
        wet: true,
    };
    let mut by_water = field(chunk_in("base"), wet, MIDNIGHT);
    run(&mut by_water, 3.0);
    let fireflies = count::<Firefly>(&mut by_water);
    assert!((1..=FIREFLIES).contains(&fireflies), "{fireflies}");

    let mut in_woods = field(chunk_in("forest"), BARE, MIDNIGHT);
    run(&mut in_woods, 3.0);
    assert!(count::<Firefly>(&mut in_woods) > 0);

    let mut dry = field(chunk_in("base"), BARE, MIDNIGHT);
    run(&mut dry, 3.0);
    assert_eq!(count::<Firefly>(&mut dry), 0);

    let mut by_day = field(chunk_in("base"), wet, NOON);
    run(&mut by_day, 3.0);
    assert_eq!(count::<Firefly>(&mut by_day), 0);
}

#[test]
fn test_no_more_fireflies_light_the_ground_than_the_cap_and_none_by_day() {
    let wet = Ground {
        plant: None,
        wet: true,
    };
    let mut app = field(chunk_in("base"), wet, MIDNIGHT);
    let mut most = 0;
    for _ in 0..200 {
        run(&mut app, FRAME);
        let lights = count::<FireflyLight>(&mut app);
        assert!(lights <= MAX_FIREFLY_LIGHTS, "{lights}");
        most = most.max(lights);
    }
    assert!(count::<Firefly>(&mut app) > MAX_FIREFLY_LIGHTS);
    assert_eq!(most, MAX_FIREFLY_LIGHTS);
    // every light at a firefly lit up, or hidden
    let fireflies: Vec<Vec3> = app
        .world_mut()
        .query_filtered::<&Transform, With<Firefly>>()
        .iter(app.world())
        .map(|at| at.translation)
        .collect();
    for (light, at, visibility) in app
        .world_mut()
        .query::<(&PointLight, &Transform, &Visibility)>()
        .iter(app.world())
    {
        if *visibility != Visibility::Hidden {
            assert!(light.intensity > 0.0);
            assert!(fireflies.iter().any(|f| f.distance(at.translation) < 1e-3));
        }
    }

    app.insert_resource(TimeOfDay(NOON));
    run(&mut app, 2.0);
    assert_eq!(count::<Firefly>(&mut app), 0);
    assert_eq!(count::<FireflyLight>(&mut app), 0);
}

#[test]
fn test_starlings_gather_at_dusk_over_open_land_and_not_by_day_or_in_the_woods() {
    let mut open = field(chunk_in("base"), BARE, DUSK_TIME);
    run(&mut open, 1.0);
    assert_eq!(count::<Starling>(&mut open), STARLINGS);
    // and leave once dusk's over, fading as they go
    open.insert_resource(TimeOfDay(MIDNIGHT));
    run(&mut open, 4.0);
    assert_eq!(count::<Starling>(&mut open), 0);

    let mut noon = field(chunk_in("base"), BARE, NOON);
    run(&mut noon, 1.0);
    assert_eq!(count::<Starling>(&mut noon), 0);

    let mut woods = field(chunk_in("forest"), BARE, DUSK_TIME);
    run(&mut woods, 1.0);
    assert_eq!(count::<Starling>(&mut woods), 0);
}

#[test]
fn test_geese_pass_high_over_by_day_the_days_way() {
    let base = chunk_in("base");
    let mut app = field(base, BARE, NOON);
    run(&mut app, 13.0);
    let geese: Vec<(Vec3, Vec3)> = app
        .world_mut()
        .query::<(&Goose, &Transform)>()
        .iter(app.world())
        .map(|(goose, at)| (goose.velocity, at.translation))
        .collect();
    assert!((7..12).contains(&geese.len()), "{}", geese.len());
    let day = migration_course(SEED, NOON.floor() as i64);
    for (velocity, at) in geese {
        assert!(velocity.normalize().angle_between(day) < 0.26, "{velocity}");
        assert!(at.y > EYE + 30.0, "{at}");
    }

    let mut night = field(base, BARE, MIDNIGHT);
    run(&mut night, 13.0);
    assert_eq!(count::<Goose>(&mut night), 0);
}

#[test]
fn test_turning_ambient_life_off_takes_them_all_away_and_brings_no_more() {
    let wet = Ground {
        plant: Some(BlockType::Poppy),
        wet: true,
    };
    let mut app = field(chunk_in("base"), wet, MIDNIGHT);
    app.insert_resource(Preferences::default());
    run(&mut app, 2.0);
    assert!(count::<AmbientCritter>(&mut app) > 0);
    app.world_mut()
        .resource_mut::<Preferences>()
        .video
        .ambient_life = false;
    run(&mut app, FRAME);
    assert!(!app.world().resource::<AmbientLife>().on);
    for time in [MIDNIGHT, NOON, DUSK_TIME] {
        app.insert_resource(TimeOfDay(time));
        run(&mut app, 13.0);
        assert_eq!(count::<AmbientCritter>(&mut app), 0, "{time}");
    }
    // and back on, they come back
    app.world_mut()
        .resource_mut::<Preferences>()
        .video
        .ambient_life = true;
    run(&mut app, 1.0);
    assert!(count::<Starling>(&mut app) > 0);
}

#[test]
fn test_rain_sends_them_to_shelter() {
    let mut app = field(chunk_in("base"), flowers(), NOON);
    run(&mut app, 3.0);
    assert!(count::<Butterfly>(&mut app) > 0);
    app.world_mut().resource_mut::<AmbientLife>().rain = 1.0;
    run(&mut app, 2.0);
    assert_eq!(count::<Butterfly>(&mut app), 0);
}

#[test]
fn test_leaving_the_world_takes_them_with_it() {
    let mut app = field(chunk_in("base"), flowers(), DUSK_TIME);
    run(&mut app, 2.0);
    assert!(count::<AmbientCritter>(&mut app) > 0);
    app.world_mut().run_schedule(LeaveWorld);
    assert_eq!(count::<AmbientCritter>(&mut app), 0);
    // and with the world gone, no more come
    app.world_mut().remove_resource::<World>();
    run(&mut app, 2.0);
    assert_eq!(count::<AmbientCritter>(&mut app), 0);
}

#[test]
fn test_none_on_the_title_screen() {
    let mut app = field(chunk_in("base"), flowers(), DUSK_TIME);
    app.insert_state(GameState::Title);
    run(&mut app, 3.0);
    assert_eq!(count::<AmbientCritter>(&mut app), 0);
}

#[test]
fn test_critters_are_never_aimed_at_or_hit() {
    let mut app = field(chunk_in("base"), flowers(), NOON);
    run(&mut app, 3.0);
    // a butterfly right in front of the eye, looking down through it at the flowers
    let eye = eye_of(&mut app);
    let butterfly = app
        .world_mut()
        .query_filtered::<Entity, With<Butterfly>>()
        .iter(app.world())
        .next()
        .unwrap();
    let in_front = eye + Vec3::new(0.0, -1.0, 0.0);
    app.world_mut()
        .entity_mut(butterfly)
        .insert(Transform::from_translation(in_front));
    let look = PlayerLook {
        yaw: 0.0,
        pitch: -std::f32::consts::FRAC_PI_2,
    };
    let world = app.world().resource::<World>();
    let hit = raycast(eye, look.forward(), REACH, |at| {
        world.get_block_at(at).is_tangible()
    });
    assert_eq!(hit.unwrap().block.y, FLOOR + 1, "the flower under it");
    // only creatures the server replicates are seen to be hit, and no critter is one
    assert_eq!(count::<SeenCreature>(&mut app), 0);
    let none: [(&SeenCreature, &Transform, Aabb); 0] = [];
    assert!(creature_hit(eye, look.forward(), 5.0, none).is_none());
}

#[test]
fn test_butterflies_keep_near_their_flower() {
    let home = Vec3::new(10.0, 20.0, -4.0);
    for i in 0..2000 {
        let at = butterfly_at(home, 1.3, 1.1, i as f32 * 0.05);
        assert!(at.with_y(0.0).distance(home.with_y(0.0)) < 2.6, "{at}");
        assert!((at.y - home.y).abs() < 1.0, "{at}");
    }
}

#[test]
fn test_fireflies_drift_near_home_and_blink_out_of_step() {
    let home = Vec3::new(0.0, 20.0, 0.0);
    let (mut lit_together, mut lit) = (0, 0);
    for i in 0..4000 {
        let t = i as f32 * 0.05;
        assert!(firefly_at(home, 0.7, t).distance(home) < 3.0);
        let (a, b) = (firefly_blink(0.7, t), firefly_blink(2.9, t));
        assert!((0.0..=1.0).contains(&a));
        if a > 0.5 {
            lit += 1;
            if b > 0.5 {
                lit_together += 1;
            }
        }
    }
    // dark more than lit, and not in time with each other
    assert!(lit < 4000 / 3, "{lit}");
    assert!(lit_together < lit, "{lit_together} of {lit}");
}

#[test]
fn test_the_nearest_lit_fireflies_are_the_ones_lit_and_no_more_than_asked() {
    let fireflies = (0..60).map(|i| (Vec3::X * i as f32, if i % 3 == 0 { 0.0 } else { 0.5 }));
    let lit = nearest_lit(Vec3::ZERO, fireflies, MAX_FIREFLY_LIGHTS);
    assert_eq!(lit.len(), MAX_FIREFLY_LIGHTS);
    assert!(lit.iter().all(|&(_, lit)| lit > 0.0));
    assert_eq!(lit[0].0, Vec3::X);
    assert!(lit.windows(2).all(|w| w[0].0.x < w[1].0.x));
    assert!(nearest_lit(Vec3::ZERO, std::iter::empty(), MAX_FIREFLY_LIGHTS).is_empty());
}

#[test]
fn test_crowded_birds_push_apart_and_lone_ones_head_home() {
    let target = Vec3::new(100.0, 0.0, 0.0);
    let lone = [(Vec3::ZERO, Vec3::X * 10.0)];
    assert!(steering(0, &lone, target).x > 0.0);
    let crowded = [
        (Vec3::ZERO, Vec3::Z * 10.0),
        (Vec3::new(0.0, 0.0, 0.5), Vec3::Z * 10.0),
    ];
    // the one behind is pushed back, away from the one just ahead
    assert!(steering(0, &crowded, Vec3::ZERO).z < 0.0);
}

#[test]
fn test_neighbours_turn_a_bird_to_their_heading() {
    let mut birds = vec![(Vec3::ZERO, Vec3::X * 10.0)];
    for k in 0..6 {
        birds.push((Vec3::new(k as f32 * 0.4 - 1.0, 2.0, 0.0), Vec3::Z * 10.0));
    }
    let steer = steering(0, &birds, Vec3::ZERO);
    assert!(steer.z > 0.0 && steer.x < 0.0, "{steer}");
}

#[test]
fn test_a_flock_keeps_together_round_its_heading_at_its_speeds_and_over_the_ground() {
    let eye = Vec3::new(0.0, 500.0, 0.0);
    let ground = 480.0;
    let mut dice = CritterDice(StdRng::seed_from_u64(9));
    let start = flock_heading(eye, 0.0).with_y(eye.y + 30.0);
    let mut birds: Vec<(Vec3, Vec3)> = (0..STARLINGS)
        .map(|_| {
            let at = start + Vec3::new(dice.range(-4.0..4.0), dice.range(-2.0..2.0), 0.0);
            (at, Vec3::X * SPEED.0)
        })
        .collect();
    let dt = 1.0 / 30.0;
    for frame in 0..(120.0 / dt) as usize {
        let t = frame as f32 * dt;
        let target = flock_heading(eye, t).with_y(eye.y + 34.0);
        step_flock(&mut birds, target, ground, dt);
        for &(at, velocity) in &birds {
            let speed = velocity.length();
            assert!(
                (SPEED.0 - 1e-3..=SPEED.1 + 1e-3).contains(&speed),
                "{speed}"
            );
            assert!(at.y > ground, "{at}");
            // never strays far from the camera, so it's always seen
            assert!(
                at.with_y(0.0).distance(eye.with_y(0.0)) < 150.0,
                "{at} at {t}"
            );
        }
    }
    // still one flock: its birds within a few dozen blocks of their middle
    let middle = birds.iter().map(|b| b.0).sum::<Vec3>() / birds.len() as f32;
    let spread = birds
        .iter()
        .map(|b| b.0.distance(middle))
        .fold(0.0, f32::max);
    assert!(spread < 60.0, "{spread}");
}

#[test]
fn test_a_days_geese_all_fly_one_way_and_another_day_another() {
    let course = migration_course(7, 3);
    assert_eq!(course, migration_course(7, 3));
    assert!((course.length() - 1.0).abs() < 1e-5 && course.y == 0.0);
    let days: Vec<Vec3> = (0..20).map(|day| migration_course(7, day)).collect();
    assert!(days.iter().any(|&other| other.angle_between(course) > 1.0));
}

#[test]
fn test_a_skein_is_a_v_behind_its_leader() {
    let course = Vec3::Z;
    assert_eq!(skein_place(0, course), Vec3::ZERO);
    for k in 1..11 {
        let place = skein_place(k, course);
        assert!(place.z < 0.0, "behind");
        // a goose and its partner across the V are mirrored
        if !k.is_multiple_of(2) {
            let partner = skein_place(k + 1, course);
            assert!((place.x + partner.x).abs() < 1e-5 && place.z == partner.z);
        }
    }
}

#[test]
fn test_dusk_is_the_late_afternoon_into_sunset() {
    let at = |time| dusk(&TimeOfDay(time));
    assert_eq!(at(NOON), 0.0);
    assert_eq!(at(MIDNIGHT), 0.0);
    assert_eq!(at(0.3), 0.0);
    assert_eq!(at(DUSK_TIME), 1.0);
    assert!(at(DUSK_TIME + 3.0) == 1.0, "on any day");
    assert!(daylight(&TimeOfDay(NOON)) == 1.0 && daylight(&TimeOfDay(MIDNIGHT)) == 0.0);
}
