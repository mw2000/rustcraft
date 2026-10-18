//! Items lying in the world (§5.2): dropped from broken blocks and thrown by players, falling
//! onto the blocks under them, gathering into one stack where they lie together, and picked up
//! by players walking past. The server moves them and the client draws them
//! (`client::drops`). They aren't saved: a world reopened has none, as they'd have gone in a
//! few minutes anyway.

use bevy::math::I64Vec3;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::chunks::chunk::ChunkCoordinate;
use crate::collision::{move_and_collide, Aabb};
use crate::inventory::Inventory;
use crate::items::ItemStack;
use crate::player::{Player, BODY, EYE_HEIGHT};
use crate::world::World;

/// How big a dropped item is, in blocks, at its middle bottom.
pub const SIZE: f32 = 0.25;
const ITEM: Aabb = Aabb {
    min: Vec3::new(-SIZE / 2.0, 0.0, -SIZE / 2.0),
    max: Vec3::new(SIZE / 2.0, SIZE, SIZE / 2.0),
};
// Blocks a second squared, and the speed falling tops out at, as for players.
const GRAVITY: f32 = 32.0;
const TERMINAL_SPEED: f32 = 60.0;
// How fast one caught in a block rises out of it, in blocks a second.
const RISE_SPEED: f32 = 4.0;
// How quickly sliding along the ground stops, as the fraction of its speed lost a second.
const GROUND_FRICTION: f32 = 0.9999;
// The longest frame moved in one step, so a hitch doesn't drop one through a floor.
const MAX_STEP: f32 = 0.1;
// How a broken block's item hops out, in blocks a second.
const POP_SPEED: f32 = 4.0;
// How hard a thrown one leaves the eye, and how much upward it's given, in blocks a second.
const THROW_SPEED: f32 = 6.0;
const THROW_LIFT: f32 = 1.5;
// Seconds before a player can pick one up: a moment, so a block's item is seen to drop, and
// longer for one thrown, so the thrower can get it away from themselves.
const DROPPED_PICKUP_DELAY: f32 = 0.5;
const THROWN_PICKUP_DELAY: f32 = 2.0;
// How long one lies there before it goes, in seconds.
const LIFETIME: f32 = 300.0;
// How close, in blocks, one must come to a player's body to be picked up, and to another of the
// same item to gather into one stack with it.
const PICKUP_REACH: f32 = 1.5;
const MERGE_REACH: f32 = 1.0;

/// Some items lying in the world, at the middle bottom of its `Transform`. Replicated to
/// clients, which draw it, with only its stack: the rest is for the server moving it.
#[derive(Component, Debug, Serialize, Deserialize)]
#[require(Transform)]
pub struct DroppedItem {
    pub stack: ItemStack,
    #[serde(skip)]
    velocity: Vec3,
    /// How long it's lain there, in seconds.
    #[serde(skip)]
    age: f32,
    // how old it must be to be picked up
    #[serde(skip)]
    pickup_after: f32,
}

impl DroppedItem {
    /// The item a block broken at `block` leaves, hopping out of where it was.
    pub fn from_block(stack: ItemStack, block: I64Vec3) -> (Self, Transform) {
        let at = block.as_vec3() + Vec3::new(0.5, 0.5 - SIZE / 2.0, 0.5);
        let item = Self {
            stack,
            velocity: Vec3::Y * POP_SPEED,
            age: 0.0,
            pickup_after: DROPPED_PICKUP_DELAY,
        };
        (item, Transform::from_translation(at))
    }

    /// Items thrown by a player with their feet at `feet`, looking along `forward`.
    pub fn thrown(stack: ItemStack, feet: Vec3, forward: Vec3) -> (Self, Transform) {
        // from just under the eyes, so it's seen leaving
        let at = feet + Vec3::Y * (EYE_HEIGHT - 0.3);
        let item = Self {
            stack,
            velocity: forward * THROW_SPEED + Vec3::Y * THROW_LIFT,
            age: 0.0,
            pickup_after: THROWN_PICKUP_DELAY,
        };
        (item, Transform::from_translation(at))
    }
}

/// Moves dropped items under gravity, stopping them on the blocks they land on, and takes them
/// away once they've lain there for `LIFETIME`.
pub(super) fn fall(
    time: Res<Time>,
    world: Res<World>,
    mut items: Query<(Entity, &mut DroppedItem, &mut Transform)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs().min(MAX_STEP);
    // as players, the world not yet generated is taken to be solid, so nothing falls through
    // ground that's still to load
    let solid = |at: I64Vec3| {
        !world.is_chunk_generated(ChunkCoordinate::containing(at))
            || world.get_block_at(at).is_solid()
    };
    for (entity, mut item, mut transform) in &mut items {
        // Falling changes nothing clients are sent, and one lying still isn't moved, so items
        // are only replicated again as they move or their stacks change.
        let item = item.bypass_change_detection();
        item.age += time.delta_secs();
        if item.age >= LIFETIME {
            commands.entity(entity).despawn();
            continue;
        }
        let middle = transform.translation + Vec3::Y * (SIZE / 2.0);
        if solid(middle.floor().as_i64vec3()) {
            // a block placed over it, which pushes it up and out
            transform.translation.y += RISE_SPEED * dt;
            item.velocity = Vec3::ZERO;
            continue;
        }
        item.velocity.y = (item.velocity.y - GRAVITY * dt).max(-TERMINAL_SPEED);
        let (moved, hit) = move_and_collide(transform.translation, item.velocity * dt, ITEM, solid);
        if transform.translation != moved {
            transform.translation = moved;
        }
        if hit.y {
            item.velocity.y = 0.0;
            let slowed = (1.0 - GROUND_FRICTION).powf(dt);
            item.velocity.x *= slowed;
            item.velocity.z *= slowed;
        }
        if hit.x {
            item.velocity.x = 0.0;
        }
        if hit.z {
            item.velocity.z = 0.0;
        }
    }
}

/// Gathers dropped items of the same kind lying together into one stack, the older of the two,
/// as far as a stack holds. There are only ever a few dozen about, so every pair is looked at.
pub(super) fn merge(
    mut items: Query<(Entity, &mut DroppedItem, &Transform)>,
    mut commands: Commands,
) {
    let mut pairs = items.iter_combinations_mut();
    while let Some([(a, item_a, at_a), (b, item_b, at_b)]) = pairs.fetch_next() {
        // an emptied one waits for its despawn, at the end of the frame
        let together = item_a.stack.item == item_b.stack.item
            && item_a.stack.count > 0
            && item_b.stack.count > 0
            && at_a.translation.distance(at_b.translation) <= MERGE_REACH;
        if !together {
            continue;
        }
        let (mut into, mut from, emptied) = if item_a.age >= item_b.age {
            (item_a, item_b, b)
        } else {
            (item_b, item_a, a)
        };
        let moved = from.stack.count.min(into.stack.room());
        into.stack.count += moved;
        from.stack.count -= moved;
        if from.stack.count == 0 {
            commands.entity(emptied).despawn();
        }
    }
}

/// Puts dropped items near a player into their inventory, as much as it has room for.
pub(super) fn pick_up(
    mut players: Query<(&Transform, &mut Inventory), With<Player>>,
    mut items: Query<(Entity, &mut DroppedItem, &Transform)>,
    mut commands: Commands,
) {
    for (body, mut inventory) in &mut players {
        let (feet, head) = (body.translation, body.translation + Vec3::Y * BODY.max.y);
        for (entity, mut item, at) in &mut items {
            let middle = at.translation + Vec3::Y * (SIZE / 2.0);
            let nearest = middle.clamp(feet, head);
            let ready = item.age >= item.pickup_after && item.stack.count > 0;
            // asked before it's changed, so a full inventory isn't marked changed every frame
            if !ready
                || middle.distance(nearest) > PICKUP_REACH
                || !inventory.has_room_for(item.stack.item)
            {
                continue;
            }
            match inventory.add(item.stack) {
                Some(left) => item.stack = left,
                None => {
                    item.stack.count = 0;
                    commands.entity(entity).despawn();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::math::I64Vec3;
    use bevy::prelude::*;
    use bevy::time::TimeUpdateStrategy;
    use bevy_replicon::prelude::ClientId;

    use super::{fall, merge, pick_up, DroppedItem, LIFETIME, THROWN_PICKUP_DELAY};
    use crate::block::BlockType;
    use crate::chunks::chunk::{ChunkCoordinate, ChunkData};
    use crate::inventory::{Inventory, SLOTS};
    use crate::items::{Item, ItemStack};
    use crate::player::{Player, PlayerOwner};
    use crate::world::World;

    const DIRT: Item = Item::Block(BlockType::Dirt);
    const SAND: Item = Item::Block(BlockType::Sand);
    // A tenth of a second a frame.
    const FRAME: f32 = 0.1;

    // The drops' systems on a floor of stone at y 0, in the chunks from (0, 0, 0) to
    // (1, 1, 1), so the chunk to -x isn't loaded.
    fn app() -> App {
        let mut world = World::new(16);
        for x in 0..=1 {
            for y in 0..=1 {
                for z in 0..=1 {
                    let coord = ChunkCoordinate(I64Vec3::new(x, y, z));
                    world.insert_chunk(coord, ChunkData::default());
                }
            }
        }
        for x in 0..32 {
            for z in 0..32 {
                world.edit(I64Vec3::new(x, 0, z), BlockType::Stone);
            }
        }
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(world)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                FRAME,
            )))
            .add_systems(Update, (fall, merge, pick_up).chain());
        app
    }

    fn drop_at(app: &mut App, stack: ItemStack, at: Vec3) -> Entity {
        let (item, _) = DroppedItem::from_block(stack, I64Vec3::ZERO);
        app.world_mut()
            .spawn((item, Transform::from_translation(at)))
            .id()
    }

    fn run(app: &mut App, seconds: f32) {
        for _ in 0..(seconds / FRAME).round() as usize {
            app.update();
        }
    }

    fn drops(app: &mut App) -> Vec<(ItemStack, Vec3)> {
        let mut query = app.world_mut().query::<(&DroppedItem, &Transform)>();
        query
            .iter(app.world())
            .map(|(item, at)| (item.stack, at.translation))
            .collect()
    }

    #[test]
    fn test_a_broken_blocks_item_hops_out_and_lands_on_the_block_under_it() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<World>()
            .edit(I64Vec3::new(4, 0, 4), BlockType::Air);
        let (item, at) = DroppedItem::from_block(ItemStack::new(DIRT, 1), I64Vec3::new(4, 3, 4));
        app.world_mut().spawn((item, at));
        run(&mut app, 0.2);
        assert!(drops(&mut app)[0].1.y > at.translation.y, "it hops");
        // down through the hole broken in the floor, onto the stone under it
        run(&mut app, 3.0);
        let (_, landed) = drops(&mut app)[0];
        assert_eq!(Vec3::new(4.5, 0.0, 4.5), landed);
        app.world_mut()
            .resource_mut::<World>()
            .edit(I64Vec3::new(4, 0, 4), BlockType::Stone);
        run(&mut app, 1.0);
        let pushed_out = drops(&mut app)[0].1.y;
        assert!((pushed_out - 1.0).abs() < 1e-5, "{pushed_out}");
    }

    #[test]
    fn test_a_thrown_item_slides_to_a_stop() {
        let mut app = app();
        let feet = Vec3::new(8.5, 1.0, 8.5);
        let (item, at) = DroppedItem::thrown(ItemStack::new(DIRT, 1), feet, Vec3::X);
        app.world_mut().spawn((item, at));
        run(&mut app, 3.0);
        let (_, first) = drops(&mut app)[0];
        run(&mut app, 1.0);
        let (_, then) = drops(&mut app)[0];
        assert!((first.y - 1.0).abs() < 1e-5, "{first}");
        assert!(first.x > feet.x + 1.0, "{first}");
        assert!((then - first).length() < 1e-3, "{first} {then}");
    }

    #[test]
    fn test_nothing_falls_into_ground_not_loaded() {
        let mut app = app();
        // over the chunk to -x
        drop_at(&mut app, ItemStack::new(DIRT, 1), Vec3::new(-0.5, 5.0, 0.5));
        run(&mut app, 2.0);
        assert!(drops(&mut app)[0].1.y >= 5.0);
    }

    #[test]
    fn test_items_lying_together_gather_into_full_stacks() {
        let mut app = app();
        drop_at(&mut app, ItemStack::new(DIRT, 40), Vec3::new(4.5, 1.0, 4.5));
        drop_at(&mut app, ItemStack::new(DIRT, 30), Vec3::new(5.0, 1.0, 4.5));
        drop_at(&mut app, ItemStack::new(SAND, 1), Vec3::new(4.5, 1.0, 5.0));
        // too far away
        drop_at(&mut app, ItemStack::new(DIRT, 1), Vec3::new(8.5, 1.0, 4.5));
        app.update();
        let mut counts: Vec<_> = drops(&mut app).iter().map(|(s, _)| s.count).collect();
        counts.sort_unstable();
        assert_eq!(vec![1, 1, 6, 64], counts);
    }

    #[test]
    fn test_items_go_after_five_minutes() {
        let mut app = app();
        drop_at(&mut app, ItemStack::new(DIRT, 1), Vec3::new(4.5, 1.0, 4.5));
        run(&mut app, LIFETIME - 1.0);
        assert_eq!(1, drops(&mut app).len());
        run(&mut app, 1.5);
        assert!(drops(&mut app).is_empty());
    }

    fn player(app: &mut App, feet: Vec3, inventory: Inventory) -> Entity {
        app.world_mut()
            .spawn((
                Player::default(),
                PlayerOwner(ClientId::Server),
                inventory,
                Transform::from_translation(feet),
            ))
            .id()
    }

    fn inventory(app: &App, player: Entity) -> &Inventory {
        app.world().get::<Inventory>(player).unwrap()
    }

    #[test]
    fn test_players_pick_up_what_they_walk_by_into_their_stacks_first() {
        let mut app = app();
        let mut carried = Inventory::default();
        carried.slots[5] = Some(ItemStack::new(DIRT, 60));
        let player = player(&mut app, Vec3::new(4.5, 1.0, 4.5), carried);
        drop_at(&mut app, ItemStack::new(DIRT, 10), Vec3::new(5.5, 1.0, 5.5));
        drop_at(&mut app, ItemStack::new(SAND, 3), Vec3::new(9.5, 1.0, 4.5));
        // not at once, so a block's item is seen to drop
        app.update();
        assert_eq!(2, drops(&mut app).len());
        run(&mut app, 0.5);
        let slots = inventory(&app, player).slots;
        assert_eq!(Some(ItemStack::new(DIRT, 64)), slots[5]);
        assert_eq!(Some(ItemStack::new(DIRT, 6)), slots[0]);
        // and the sand, out of reach, is left
        assert_eq!(
            vec![ItemStack::new(SAND, 3)],
            drops(&mut app).iter().map(|d| d.0).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_a_full_inventory_leaves_what_it_cant_take() {
        let mut app = app();
        let mut full = Inventory {
            slots: [Some(ItemStack::full(SAND)); SLOTS],
            cursor: None,
        };
        full.slots[0] = Some(ItemStack::new(DIRT, 62));
        let player = player(&mut app, Vec3::new(4.5, 1.0, 4.5), full);
        drop_at(&mut app, ItemStack::new(DIRT, 5), Vec3::new(4.5, 1.0, 5.5));
        run(&mut app, 1.0);
        assert_eq!(
            Some(ItemStack::full(DIRT)),
            inventory(&app, player).slots[0]
        );
        assert_eq!(ItemStack::new(DIRT, 3), drops(&mut app)[0].0);
    }

    #[test]
    fn test_a_thrown_item_isnt_picked_straight_back_up() {
        let mut app = app();
        let feet = Vec3::new(4.5, 1.0, 4.5);
        let player = player(&mut app, feet, Inventory::default());
        // straight up, so it comes down on the thrower
        let (item, at) = DroppedItem::thrown(ItemStack::new(DIRT, 1), feet, Vec3::ZERO);
        app.world_mut().spawn((item, at));
        run(&mut app, THROWN_PICKUP_DELAY - 0.5);
        assert_eq!(1, drops(&mut app).len());
        run(&mut app, 1.0);
        assert!(drops(&mut app).is_empty());
        assert_eq!(
            Some(ItemStack::new(DIRT, 1)),
            inventory(&app, player).slots[0]
        );
    }
}
