use bevy::math::I64Vec3;
use bevy::prelude::*;
use bevy_replicon::prelude::*;

use super::drops::DroppedItem;
use super::edits::in_reach;
use crate::crafting::{Recipe, Station};
use crate::inventory::Inventory;
use crate::player::{PlayerLook, PlayerOwner};
use crate::protocol::{CloseInventory, Craft, UseBlock};
use crate::world::World;

/// The station a player has open, and where it is, from using it until they close the
/// inventory screen.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AtStation {
    station: Station,
    block: I64Vec3,
}

type Players<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static PlayerOwner,
        &'static Transform,
        &'static PlayerLook,
        &'static mut Inventory,
        Option<&'static AtStation>,
    ),
>;

/// Opens the stations players use, and crafts what they ask for: a recipe made by hand
/// anywhere, and one made at a station only while they have it open, in reach, and it's still
/// there. What's made goes into the inventory, and what doesn't fit is dropped at the player.
pub(super) fn receive_crafts(
    mut uses: MessageReader<FromClient<UseBlock>>,
    mut crafts: MessageReader<FromClient<Craft>>,
    mut closes: MessageReader<FromClient<CloseInventory>>,
    world: Res<World>,
    mut players: Players,
    mut commands: Commands,
) {
    for &FromClient {
        client_id,
        message: UseBlock { block },
    } in uses.read()
    {
        let Some((player, _, body, ..)) = players.iter().find(|(_, o, ..)| o.0 == client_id) else {
            continue;
        };
        let station = Station::of_block(world.get_block_at(block));
        if let Some(station) = station.filter(|_| in_reach(body.translation, block)) {
            commands.entity(player).insert(AtStation { station, block });
        }
    }
    for &FromClient {
        client_id,
        message: Craft { recipe, all },
    } in crafts.read()
    {
        let Some((_, _, body, look, mut inventory, at)) =
            players.iter_mut().find(|(_, o, ..)| o.0 == client_id)
        else {
            continue;
        };
        if !can_make(recipe, at, body.translation, &world) {
            debug!(
                "refused {client_id}'s {}: not at its station",
                recipe.name()
            );
            continue;
        }
        // as many as there are ingredients for, if it's all, until one doesn't fit, so a full
        // inventory doesn't turn into a heap on the floor
        while let Some(made) = recipe.make(&mut inventory) {
            if let Some(left) = inventory.add(made) {
                commands.spawn((
                    DroppedItem::thrown(left, body.translation, look.forward()),
                    Replicated,
                ));
                break;
            }
            if !all {
                break;
            }
        }
    }
    for &FromClient { client_id, .. } in closes.read() {
        if let Some((player, ..)) = players.iter().find(|(_, o, ..)| o.0 == client_id) {
            commands.entity(player).remove::<AtStation>();
        }
    }
}

// Whether `recipe` can be made by a player with their feet at `feet` and the station `at` open.
fn can_make(recipe: Recipe, at: Option<&AtStation>, feet: Vec3, world: &World) -> bool {
    if recipe.station() == Station::Hand {
        return true;
    }
    at.is_some_and(|at| {
        at.station.makes(recipe.station())
            && Station::of_block(world.get_block_at(at.block)) == Some(at.station)
            && in_reach(feet, at.block)
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::math::I64Vec3;
    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;
    use bevy_replicon::prelude::*;

    use super::AtStation;
    use crate::block::BlockType;
    use crate::chunks::chunk::{ChunkCoordinate, ChunkData};
    use crate::crafting::{Recipe, Station};
    use crate::game_mode::GameMode;
    use crate::inventory::{Inventory, SLOTS};
    use crate::items::{Item, ItemStack};
    use crate::player::{Player, PlayerOwner};
    use crate::protocol::{CloseInventory, Craft, ProtocolPlugin, UseBlock};
    use crate::server::drops::DroppedItem;
    use crate::server::ServerPlugin;
    use crate::world::World;

    const LOG: Item = Item::Block(BlockType::Log);
    const PLANKS: Item = Item::Block(BlockType::Planks);
    // a crafting table beside the player, standing at the origin, and one too far off to reach
    const TABLE: I64Vec3 = I64Vec3::new(2, 1, 0);
    const FAR_TABLE: I64Vec3 = I64Vec3::new(9, 1, 0);

    // A server alone with its player in survival, carrying `stacks` in its first slots.
    fn server(stacks: &[ItemStack]) -> App {
        let mut world = World::new(16);
        world.insert_chunk(ChunkCoordinate(I64Vec3::ZERO), ChunkData::default());
        world.edit(TABLE, BlockType::CraftingTable);
        world.edit(FAR_TABLE, BlockType::CraftingTable);
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins,
            ProtocolPlugin,
            ServerPlugin,
        ))
        .insert_resource(world)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
        let mut inventory = Inventory::default();
        for (slot, &stack) in inventory.slots.iter_mut().zip(stacks) {
            *slot = Some(stack);
        }
        app.world_mut().spawn((
            Player::default(),
            PlayerOwner(ClientId::Server),
            GameMode::Survival,
            inventory,
            Transform::from_xyz(0.5, 1.0, 0.5),
        ));
        app.finish();
        app
    }

    fn send(app: &mut App, message: impl Message) {
        app.world_mut().write_message(message);
        app.update();
    }

    fn craft(app: &mut App, recipe: Recipe, all: bool) {
        send(app, Craft { recipe, all });
    }

    fn inventory(app: &mut App) -> Inventory {
        let mut query = app.world_mut().query::<&Inventory>();
        query.single(app.world()).unwrap().clone()
    }

    fn count(app: &mut App, item: Item) -> u32 {
        inventory(app).count(item)
    }

    fn dropped(app: &mut App) -> Vec<ItemStack> {
        let mut query = app.world_mut().query::<&DroppedItem>();
        query.iter(app.world()).map(|item| item.stack).collect()
    }

    #[test]
    fn test_crafting_by_hand_takes_the_ingredients_and_gives_the_output() {
        let mut app = server(&[ItemStack::new(LOG, 3)]);
        craft(&mut app, Recipe::Planks, false);
        assert_eq!((2, 4), (count(&mut app, LOG), count(&mut app, PLANKS)));
        craft(&mut app, Recipe::Sticks, false);
        assert_eq!(
            (2, 4),
            (count(&mut app, PLANKS), count(&mut app, Item::Stick))
        );
    }

    #[test]
    fn test_crafting_without_the_ingredients_makes_nothing() {
        let mut app = server(&[ItemStack::new(PLANKS, 3)]);
        let before = inventory(&mut app);
        craft(&mut app, Recipe::CraftingTable, false);
        craft(&mut app, Recipe::Planks, true);
        assert_eq!(before, inventory(&mut app));
    }

    #[test]
    fn test_shift_crafting_makes_as_many_as_there_are_ingredients_for() {
        let mut app = server(&[ItemStack::new(LOG, 20), ItemStack::new(PLANKS, 1)]);
        craft(&mut app, Recipe::Planks, true);
        assert_eq!((0, 81), (count(&mut app, LOG), count(&mut app, PLANKS)));
        assert!(dropped(&mut app).is_empty());
    }

    #[test]
    fn test_what_doesnt_fit_is_dropped_and_shift_crafting_stops_there() {
        let sand = ItemStack::full(Item::Block(BlockType::Sand));
        let mut app = server(&[ItemStack::new(LOG, 2)]);
        let mut query = app.world_mut().query::<&mut Inventory>();
        query.single_mut(app.world_mut()).unwrap().slots[1..].fill(Some(sand));
        // the first log's planks have nowhere to go, as the second's still where it was
        craft(&mut app, Recipe::Planks, true);
        assert_eq!(vec![ItemStack::new(PLANKS, 4)], dropped(&mut app));
        assert_eq!((1, 0), (count(&mut app, LOG), count(&mut app, PLANKS)));
        // and the last log's go in its place
        craft(&mut app, Recipe::Planks, false);
        let planks = Some(ItemStack::new(PLANKS, 4));
        assert_eq!(planks, inventory(&mut app).slots[0]);
        assert_eq!(SLOTS, inventory(&mut app).slots.iter().flatten().count());
    }

    #[test]
    fn test_table_recipes_need_a_table_opened_in_reach() {
        let mut app = server(&[ItemStack::new(PLANKS, 9), ItemStack::new(Item::Stick, 6)]);
        // by hand
        craft(&mut app, Recipe::WoodenPickaxe, false);
        assert_eq!(0, count(&mut app, Item::WoodenPickaxe));
        // at a table too far off, or at a block that isn't one
        send(&mut app, UseBlock { block: FAR_TABLE });
        send(
            &mut app,
            UseBlock {
                block: TABLE + I64Vec3::Y,
            },
        );
        craft(&mut app, Recipe::WoodenPickaxe, false);
        assert_eq!(0, count(&mut app, Item::WoodenPickaxe));

        send(&mut app, UseBlock { block: TABLE });
        craft(&mut app, Recipe::WoodenPickaxe, false);
        assert_eq!(1, count(&mut app, Item::WoodenPickaxe));
        // and hand recipes still work there
        craft(&mut app, Recipe::CraftingTable, false);
        assert_eq!(1, count(&mut app, Item::Block(BlockType::CraftingTable)));

        // until the screen's closed
        send(&mut app, CloseInventory);
        let mut query = app.world_mut().query::<&AtStation>();
        assert!(query.iter(app.world()).next().is_none());
        craft(&mut app, Recipe::WoodenShovel, false);
        assert_eq!(0, count(&mut app, Item::WoodenShovel));
    }

    #[test]
    fn test_a_table_broken_while_open_crafts_no_more() {
        let mut app = server(&[ItemStack::new(PLANKS, 9), ItemStack::new(Item::Stick, 6)]);
        send(&mut app, UseBlock { block: TABLE });
        let mut query = app.world_mut().query::<&AtStation>();
        assert_eq!(
            Station::CraftingTable,
            query.single(app.world()).unwrap().station
        );
        app.world_mut()
            .resource_mut::<World>()
            .edit(TABLE, BlockType::Air);
        craft(&mut app, Recipe::WoodenAxe, false);
        assert_eq!(0, count(&mut app, Item::WoodenAxe));
    }
}
