//! Monsters by place (M4.10): each climate's own at night and worse ones the deeper the band,
//! what each does that the rest don't, what they drop, and their numbers against the tiers.

use bevy_replicon::prelude::SendTargets;

use super::*;
use crate::chunks::generate::noise::NoiseGenerator;
use crate::creatures::{Missile, Stance, DEEP, DEEPEST, SHORE};
use crate::game_mode::GameMode;
use crate::server::creatures::hunting::{CLATTER, RISE_NEAR, RISING, TOUCH};
use crate::server::creatures::spawning::biome_at;

const NIGHT: f64 = 0.0;

// The new kinds, which these tests are about.
const NEW: [CreatureKind; 5] = [
    CreatureKind::Dunewight,
    CreatureKind::Rimewraith,
    CreatureKind::CanopySpitter,
    CreatureKind::Listener,
    CreatureKind::Drowner,
];

fn edit(app: &mut App, at: I64Vec3, block: BlockType) {
    app.world_mut().resource_mut::<World>().edit(at, block);
}

fn fill(app: &mut App, min: I64Vec3, max: I64Vec3, block: BlockType) {
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                edit(app, I64Vec3::new(x, y, z), block);
            }
        }
    }
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

fn cap(kind: CreatureKind) -> usize {
    kind.properties().spawns.as_ref().unwrap().cap
}

// A monster of `kind` with `stance`, its feet at `at`.
fn standing(app: &mut App, kind: CreatureKind, at: Vec3, stance: Stance) -> Entity {
    let monster = spawned(app, kind, at);
    app.world_mut().entity_mut(monster).insert(stance);
    monster
}

// Walks `player` along `velocity`, in blocks a second, for `seconds`, a frame at a time.
fn walk(app: &mut App, player: Entity, velocity: Vec3, seconds: f32) {
    for _ in 0..(seconds / FRAME).round() as usize {
        let to = at(app, player) + velocity * FRAME;
        move_to(app, player, to);
        run(app, FRAME);
    }
}

// A world, by its seed, and the middle of a chunk in it whose land within 60 blocks is all
// `biome`, looked for over a wider and coarser grid than `chunk_in`, in other worlds than the
// tests' own, and out along the meridians, as the cold climates are towards the poles.
fn chunk_far_in(biome: &str) -> (u32, I64Vec3) {
    chunk_mostly_in(biome, 1.0)
}

// The same, where at least `share` of that land is `biome`, for narrow ones like the taiga.
fn chunk_mostly_in(biome: &str, share: f32) -> (u32, I64Vec3) {
    (1..=12)
        .find_map(|seed| chunk_of_seed_in(seed, biome, share).map(|chunk| (seed, chunk)))
        .unwrap_or_else(|| panic!("no {biome} found in a dozen worlds"))
}

fn chunk_of_seed_in(seed: u32, biome: &str, share: f32) -> Option<I64Vec3> {
    let noise = NoiseGenerator::new(seed);
    let is = |x: f64, z: f64| {
        let sample = noise.sample(x, z);
        noise.biomes().name(sample.biome).unwrap_or("base") == biome
    };
    // rings round the origin out to 3 km, then north and south a kilometre at a time
    let rings = (0..=200i64).step_by(8).flat_map(|r| {
        (-r..=r).step_by(8).flat_map(move |a| {
            [(a, -r), (a, r), (-r, a), (r, a)].map(|(x, z)| I64Vec3::new(x, 0, z))
        })
    });
    let meridians = (256..=8192i64).step_by(16).flat_map(|z| {
        (-64..=64i64)
            .step_by(16)
            .flat_map(move |x| [I64Vec3::new(x, 0, z), I64Vec3::new(x, 0, -z)])
    });
    let all = 13 * 13;
    let most = (share * all as f32).ceil() as usize;
    rings.chain(meridians).find(|chunk| {
        let middle = chunk.as_dvec3() * CHUNK_SIZE as f64;
        // the middle first, which rules out most at a sample
        is(middle.x, middle.z)
            && (-6..=6)
                .flat_map(|x| (-6..=6).map(move |z| (x, z)))
                .scan(0, |missed, (x, z)| {
                    let (x, z) = (f64::from(x) * 10.0, f64::from(z) * 10.0);
                    *missed += usize::from(!is(middle.x + x, middle.z + z));
                    Some(*missed)
                })
                .all(|missed| missed <= all - most)
    })
}

// Where a player's feet stand on a field made round the chunk `centre`.
fn feet_on(centre: I64Vec3) -> Vec3 {
    let y = (centre.y + 1) * CHUNK_SIZE as i64 + 1;
    (centre * CHUNK_SIZE as i64).as_vec3().with_y(y as f32) + Vec3::new(0.5, 0.0, 0.5)
}

// Runs a night round a player on a field of `surface` round `centre`, returning the kinds that
// came, in the world of `seed`.
fn night_in(seed: u32, surface: BlockType, centre: I64Vec3) -> App {
    let mut app = field_of(seed, surface, centre);
    app.insert_resource(SpawnRate(Some(0.1)))
        .insert_resource(TimeOfDay::from_days(NIGHT));
    player(&mut app, feet_on(centre), None);
    run(&mut app, 30.0);
    app
}

#[test]
fn test_each_climate_has_its_own_monsters_at_night_and_the_shore_has_drowners() {
    // dunewights in the desert, all lying buried, and no gloamers
    let (seed, desert) = chunk_far_in("desert");
    let mut app = night_in(seed, BlockType::Sand, desert);
    assert_eq!(
        cap(CreatureKind::Dunewight),
        count(&mut app, CreatureKind::Dunewight)
    );
    let mut query = app.world_mut().query::<(&Creature, &Stance)>();
    for (creature, stance) in query.iter(app.world()) {
        assert_eq!(creature.0 == CreatureKind::Dunewight, stance.buried);
    }
    assert_eq!(0, count(&mut app, CreatureKind::Gloamer));
    assert_eq!(0, count(&mut app, CreatureKind::Rimewraith));

    // rimewraiths on snow in and round the taiga, a narrow climate, but only on the taiga's
    let (seed, taiga) = chunk_mostly_in("taiga", 0.5);
    let mut app = night_in(seed, BlockType::Snow, taiga);
    let mut query = app.world_mut().query::<(&Creature, &Transform)>();
    let wraiths: Vec<Vec3> = query
        .iter(app.world())
        .filter(|(c, _)| c.0 == CreatureKind::Rimewraith)
        .map(|(_, t)| t.translation)
        .collect();
    assert!(!wraiths.is_empty());
    let lives = CreatureKind::Rimewraith
        .properties()
        .spawns
        .as_ref()
        .unwrap();
    for wraith in wraiths {
        let biome = biome_at(app.world().resource::<World>(), wraith.xz());
        assert!(lives.biomes.contains(&biome), "{biome} at {wraith}");
    }
    assert_eq!(0, count(&mut app, CreatureKind::Dunewight));

    // on sand in the grassland, gloamers but no dunewights, and no drowners so far under the sea
    let base = chunk_in("base");
    let mut app = night_in(SEED, BlockType::Sand, base);
    assert_eq!(
        cap(CreatureKind::Gloamer),
        count(&mut app, CreatureKind::Gloamer)
    );
    assert_eq!(0, count(&mut app, CreatureKind::Dunewight));
    assert_eq!(0, count(&mut app, CreatureKind::Drowner));

    // and on a beach at sea level, drowners
    let shore = base.with_y(SHORE.start / CHUNK_SIZE as i64);
    assert!(SHORE.contains(&(feet_on(shore).y as i64)));
    let mut app = night_in(SEED, BlockType::Sand, shore);
    assert_eq!(
        cap(CreatureKind::Drowner),
        count(&mut app, CreatureKind::Drowner)
    );
}

#[test]
fn test_canopy_spitters_hang_under_the_jungles_leaves_at_night() {
    let (seed, jungle) = chunk_far_in("jungle");
    let mut app = field_of(seed, BlockType::Grass, jungle);
    app.insert_resource(SpawnRate(Some(0.1)))
        .insert_resource(TimeOfDay::from_days(NIGHT));
    let feet = feet_on(jungle);
    let canopy = feet.y as i64 + 7;
    let middle = jungle * CHUNK_SIZE as i64;
    fill(
        &mut app,
        middle + I64Vec3::new(-60, 0, -60).with_y(canopy),
        middle + I64Vec3::new(60, 0, 60).with_y(canopy),
        BlockType::Leaves,
    );
    player(&mut app, feet, None);
    run(&mut app, 30.0);
    let height = CreatureKind::CanopySpitter.properties().height;
    let mut query = app.world_mut().query::<(&Creature, &Stance, &Transform)>();
    let spitters: Vec<_> = query
        .iter(app.world())
        .filter(|(c, ..)| c.0 == CreatureKind::CanopySpitter)
        .map(|(_, s, t)| (*s, t.translation))
        .collect();
    assert_eq!(cap(CreatureKind::CanopySpitter), spitters.len());
    for (stance, body) in spitters {
        assert!(stance.clinging);
        assert!((body.y + height - canopy as f32).abs() < 1e-4, "{body}");
    }
}

// A day under ground round a player in a tunnel through the stone of a field round `centre`,
// returning the kinds that came there.
fn day_down_at(centre: I64Vec3) -> Vec<CreatureKind> {
    let mut app = field(BlockType::Sand, centre);
    app.insert_resource(SpawnRate(Some(0.1)));
    let base = centre * CHUNK_SIZE as i64;
    fill(
        &mut app,
        base + I64Vec3::new(-60, 4, -2),
        base + I64Vec3::new(60, 7, 2),
        BlockType::Air,
    );
    let feet = base.as_vec3() + Vec3::new(0.5, 4.0, 0.5);
    player(&mut app, feet, None);
    run(&mut app, 40.0);
    kinds(&mut app)
}

#[test]
fn test_the_deeper_the_band_the_worse_the_monsters() {
    let has = |kinds: &[CreatureKind], kind| kinds.contains(&kind);
    // shallow caves, just under the sea: lurkers and slingers, but no brutes or listeners
    let shallow = I64Vec3::new(0, 27, 0);
    assert!(!DEEP.contains(&(shallow.y * 16 + 4)));
    let kinds = day_down_at(shallow);
    assert!(has(&kinds, CreatureKind::Lurker), "{kinds:?}");
    assert!(!has(&kinds, CreatureKind::BarrowBrute), "{kinds:?}");
    assert!(!has(&kinds, CreatureKind::Listener), "{kinds:?}");
    // deep: brutes too
    let deep = I64Vec3::new(0, 20, 0);
    assert!(DEEP.contains(&(deep.y * 16 + 4)) && !DEEPEST.contains(&(deep.y * 16 + 4)));
    let kinds = day_down_at(deep);
    assert!(has(&kinds, CreatureKind::BarrowBrute), "{kinds:?}");
    assert!(!has(&kinds, CreatureKind::Listener), "{kinds:?}");
    // deepest: listeners besides
    let kinds = day_down_at(I64Vec3::ZERO);
    assert!(has(&kinds, CreatureKind::BarrowBrute), "{kinds:?}");
    assert!(has(&kinds, CreatureKind::Listener), "{kinds:?}");
    // and none of the surface's down there
    for kind in [
        CreatureKind::Gloamer,
        CreatureKind::Dunewight,
        CreatureKind::Rimewraith,
        CreatureKind::Drowner,
    ] {
        assert!(!has(&kinds, kind), "{kinds:?}");
    }
}

#[test]
fn test_a_dunewight_lies_buried_out_of_reach_till_someone_comes_near_then_rises_and_slows() {
    let mut app = app(BlockType::Sand);
    app.insert_resource(TimeOfDay::from_days(NIGHT));
    let buried = Stance {
        buried: true,
        ..default()
    };
    let wight = standing(
        &mut app,
        CreatureKind::Dunewight,
        Vec3::new(6.5, FEET, 0.5),
        buried,
    );
    let player = player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
    run(&mut app, 2.0);
    assert_eq!(Doing::Cling, doing(&app, wight));
    assert!(stance(&app, wight).buried);
    // in creative beside it, which it doesn't rise for, hitting it finds nothing there to hit
    app.world_mut()
        .entity_mut(player)
        .insert(GameMode::Creative);
    move_to(
        &mut app,
        player,
        Vec3::new(6.5 - RISE_NEAR + 1.0, FEET, 0.5),
    );
    run(&mut app, 0.6);
    hit(&mut app, wight);
    run(&mut app, 0.6);
    assert_eq!(20, health(&app, wight));
    assert!(stance(&app, wight).buried);

    // in survival within 5 blocks, it rises, standing while it does, then strikes, slowing
    app.world_mut()
        .entity_mut(player)
        .insert(GameMode::Survival);
    let mut frames = 0;
    while stance(&app, wight).buried {
        run(&mut app, FRAME);
        frames += 1;
        assert!(frames < 20);
    }
    assert_eq!(Doing::Hunt(player), doing(&app, wight));
    let rose_at = at(&app, wight);
    run(&mut app, RISING - 0.1);
    assert!((at(&app, wight) - rose_at).length() < 1e-3);
    assert!(blows(&app).is_empty());
    run(&mut app, 4.0);
    let landed = blows(&app);
    assert!(!landed.is_empty());
    assert_eq!(3.0, landed[0].harm.slows);
    assert_eq!(CreatureKind::Dunewight, landed[0].by);
}

#[test]
fn test_a_rimewraith_draws_off_after_each_blow_and_comes_back_chilling() {
    let mut app = app(BlockType::Snow);
    app.insert_resource(TimeOfDay::from_days(NIGHT));
    let player = player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
    let wraith = spawned(
        &mut app,
        CreatureKind::Rimewraith,
        Vec3::new(5.5, FEET, 0.5),
    );
    let mut frames = 0;
    while blows(&app).is_empty() {
        run(&mut app, FRAME);
        frames += 1;
        assert!(frames < 200);
    }
    assert_eq!(3, blows(&app)[0].harm.chills);
    // it draws off, running, for a while, and no more blows come meanwhile
    run(&mut app, 0.1);
    assert!(matches!(doing(&app, wraith), Doing::Flee { .. }));
    run(&mut app, 1.5);
    assert!(matches!(doing(&app, wraith), Doing::Flee { .. }));
    let off = (at(&app, wraith) - at(&app, player)).xz().length();
    assert!(off > 3.0, "{off}");
    assert_eq!(1, blows(&app).len());
    // then comes back for another, as soon as it next looks about
    run(&mut app, 1.6);
    assert_eq!(Doing::Hunt(player), doing(&app, wraith));
    run(&mut app, 4.0);
    assert!(blows(&app).len() >= 2);
}

#[test]
fn test_a_canopy_spitter_spits_venom_from_where_it_hangs_and_drops_on_someone_under_it() {
    let mut app = app(BlockType::Grass);
    app.insert_resource(TimeOfDay::from_days(NIGHT));
    let canopy = FLOOR + 8;
    fill(
        &mut app,
        I64Vec3::new(-14, canopy, -14),
        I64Vec3::new(14, canopy, 14),
        BlockType::Leaves,
    );
    let height = CreatureKind::CanopySpitter.properties().height;
    let hanging = Stance {
        clinging: true,
        ..default()
    };
    let spitter = standing(
        &mut app,
        CreatureKind::CanopySpitter,
        Vec3::new(0.5, canopy as f32 - height, 6.5),
        hanging,
    );
    let player = player(&mut app, Vec3::new(0.5, FEET, -1.5), None);
    let mut spat = false;
    for _ in 0..200 {
        run(&mut app, FRAME);
        let mut missiles = app.world_mut().query::<&Missile>();
        spat |= missiles
            .iter(app.world())
            .any(|m| m.by == CreatureKind::CanopySpitter);
        if !blows(&app).is_empty() {
            break;
        }
    }
    assert!(spat);
    let landed = blows(&app);
    assert_eq!(1, landed.len(), "{landed:?}");
    assert_eq!((player, 3), (landed[0].player, landed[0].harm.venom));
    // still hanging, where it was
    assert!(stance(&app, spitter).clinging);
    assert!((at(&app, spitter).y + height - canopy as f32).abs() < 1e-4);
    // and drops on someone right under it
    move_to(&mut app, player, Vec3::new(1.5, FEET, 6.5));
    run(&mut app, 0.6);
    assert!(!stance(&app, spitter).clinging);
    run(&mut app, 2.0);
    assert!((at(&app, spitter).y - FEET).abs() < 1e-3);
    assert_eq!(12, health(&app, spitter));
}

#[test]
fn test_a_listener_hears_walking_but_not_sneaking_and_seeks_where_it_last_heard() {
    let mut app = app(BlockType::Grass);
    let player = player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
    let listener = spawned(&mut app, CreatureKind::Listener, Vec3::new(8.5, FEET, 0.5));
    // standing still, and creeping, 8 blocks off, it hears nothing
    run(&mut app, 1.5);
    assert!(!matches!(doing(&app, listener), Doing::Hunt(_)));
    walk(&mut app, player, Vec3::Z * 1.3, 2.0);
    assert!(!matches!(doing(&app, listener), Doing::Hunt(_)));
    // walking, it hears them and comes
    let mut frames = 0;
    while doing(&app, listener) != Doing::Hunt(player) {
        walk(&mut app, player, Vec3::NEG_Z * 4.3, FRAME);
        frames += 1;
        assert!(frames < 40, "{:?}", doing(&app, listener));
    }
    // put far off at once, quiet, it goes to where it last heard them, and gives up there
    let heard = at(&app, player);
    move_to(&mut app, player, Vec3::new(-40.5, FEET, 0.5));
    run(&mut app, 0.6);
    let Doing::Seek(place) = doing(&app, listener) else {
        panic!("{:?}", doing(&app, listener));
    };
    assert!(place.distance(heard) < 2.0, "{place} {heard}");
    run(&mut app, 6.0);
    assert!(matches!(doing(&app, listener), Doing::Idle | Doing::Wander));

    // a block broken near it, it goes to see
    let broken = at(&app, listener).floor().as_i64vec3() + I64Vec3::new(0, -1, 10);
    const { assert!(10.0 < CLATTER) };
    app.world_mut().write_message(ToClients {
        targets: SendTargets::Single(ClientId::Server),
        message: BlockChanged {
            block: broken,
            to: BlockType::Air.into(),
            by: Some((ClientId::Server, 1)),
        },
    });
    run(&mut app, 0.1);
    assert!(matches!(doing(&app, listener), Doing::Seek(_)));

    // and finds someone standing still within reach of touch, and strikes them hard
    let near = at(&app, listener) + Vec3::X * (TOUCH - 0.6);
    let still = self::player(&mut app, near, None);
    run(&mut app, 3.0);
    assert!(blows(&app)
        .iter()
        .any(|blow| (blow.player, blow.damage) == (still, 7)));
}

#[test]
fn test_a_drowner_drags_its_prey_towards_it() {
    let mut app = app(BlockType::Sand);
    app.insert_resource(TimeOfDay::from_days(NIGHT));
    let player = player(&mut app, Vec3::new(0.5, FEET, 0.5), None);
    spawned(&mut app, CreatureKind::Drowner, Vec3::new(3.5, FEET, 0.5));
    run(&mut app, 3.0);
    let landed = blows(&app);
    assert!(!landed.is_empty());
    assert_eq!(player, landed[0].player);
    assert!(landed[0].knockback < 0.0);
    // and it swims after them rather than keeping out of the water
    assert!(super::super::walker(CreatureKind::Drowner).swims);
}

#[test]
fn test_each_new_monster_drops_only_what_its_row_says() {
    let mut app = app(BlockType::Grass);
    let sword = ItemStack::new(Item::IronSword, 1);
    player(&mut app, Vec3::new(0.5, FEET, 0.5), Some(sword));
    for kind in NEW {
        let table: Vec<Item> = kind.properties().drops.iter().map(|d| d.0).collect();
        let mut seen = Vec::new();
        for _ in 0..12 {
            let monster = spawned(&mut app, kind, Vec3::new(2.5, FEET, 0.5));
            kill(&mut app, monster);
            seen.extend(dropped(&mut app).into_iter().map(|(item, _)| item));
            clear_drops(&mut app);
        }
        assert!(!seen.is_empty(), "{kind:?}");
        for item in seen {
            assert!(table.contains(&item), "{kind:?} dropped {item:?}");
        }
    }
}

// How long a fight between a player holding `sword` and wearing `armour` and one monster of
// `kind` lasts each way, in seconds: until the player's killed it, swinging as fast as the sword
// lets them from the start, and until it's killed them, from full health through the armour,
// striking as soon as its wind-up and cooldown let it.
fn duel(kind: CreatureKind, sword: Item, armour: &[Item]) -> (f32, f32) {
    let properties = kind.properties();
    let attack = properties.attack.as_ref().unwrap();
    let hits = properties.health.div_ceil(sword.damage());
    let kills_it = f32::from(hits - 1) * sword.cooldown();
    let mut worn = Inventory::default();
    for (slot, &piece) in armour.iter().enumerate() {
        worn.armour[slot] = Some(ItemStack::new(piece, 1));
    }
    let through = worn.blunt(attack.damage);
    let blows = crate::survival::MAX_HEALTH.div_ceil(through);
    let every = attack.wind_up + attack.cooldown.max(attack.withdraws);
    let kills_them = attack.wind_up + f32::from(blows - 1) * every;
    (kills_it, kills_them)
}

#[test]
fn test_the_new_monsters_numbers_are_fair_for_the_tiers() {
    const LEATHER: [Item; 4] = [
        Item::LeatherHelmet,
        Item::LeatherChestplate,
        Item::LeatherLeggings,
        Item::LeatherBoots,
    ];
    const IRON: [Item; 4] = [
        Item::IronHelmet,
        Item::IronChestplate,
        Item::IronLeggings,
        Item::IronBoots,
    ];
    // the climates' monsters are beaten one at a time with a wooden sword and nothing worn
    for kind in [
        CreatureKind::Dunewight,
        CreatureKind::Rimewraith,
        CreatureKind::CanopySpitter,
        CreatureKind::Drowner,
    ] {
        let (it, them) = duel(kind, Item::WoodenSword, &[]);
        assert!(it * 2.0 < them, "{kind:?}: {it} against {them}");
    }
    // but three dunewights woken at once outlast that
    let (it, them) = duel(CreatureKind::Dunewight, Item::WoodenSword, &[]);
    assert!(it * 3.0 > them / 3.0);
    // a listener kills someone with a wooden sword and nothing worn first, loses to a stone
    // sword and leather, and is no match for iron
    let (it, them) = duel(CreatureKind::Listener, Item::WoodenSword, &[]);
    assert!(it > them, "{it} against {them}");
    let (it, them) = duel(CreatureKind::Listener, Item::StoneSword, &LEATHER);
    assert!(it < them, "{it} against {them}");
    let (it, them) = duel(CreatureKind::Listener, Item::IronSword, &IRON);
    assert!(it * 4.0 < them, "{it} against {them}");
    // and it's worse than a brute, which the same wooden sword beats
    let (it, them) = duel(CreatureKind::BarrowBrute, Item::WoodenSword, &[]);
    assert!(it < them, "{it} against {them}");
    for kind in NEW {
        assert!(kind.properties().spawns.is_some(), "{kind:?}");
    }
}
