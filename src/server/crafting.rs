use bevy::math::I64Vec3;
use bevy::prelude::*;
use bevy_replicon::prelude::*;

use super::edits::in_reach;
use crate::crafting::Station;
use crate::player::PlayerOwner;
use crate::protocol::{CloseInventory, UseBlock};
use crate::world::World;

/// The station a player has open, and where it is, from using it until they close the
/// inventory screen.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct AtStation {
    station: Station,
    block: I64Vec3,
}

/// The station whose crafting grid a player's screen shows (§5.3): the one they have open while
/// it's still there and in reach of their feet at `feet`, and otherwise their own hands', so a
/// table broken or left behind lays and makes only what hands do.
pub(super) fn station_of(at: Option<&AtStation>, feet: Vec3, world: &World) -> Station {
    at.filter(|at| {
        Station::of_block(world.get_block_at(at.block)) == Some(at.station)
            && in_reach(feet, at.block)
    })
    .map_or(Station::Hand, |at| at.station)
}

/// Opens the stations players use, if they're one and in reach, until they close the inventory
/// screen. What's crafted at them is `Slots`', as players lay their grids (`inventory`).
pub(super) fn open_stations(
    mut uses: MessageReader<FromClient<UseBlock>>,
    mut closes: MessageReader<FromClient<CloseInventory>>,
    world: Res<World>,
    players: Query<(Entity, &PlayerOwner, &Transform)>,
    mut commands: Commands,
) {
    for &FromClient {
        client_id,
        message: UseBlock { block },
    } in uses.read()
    {
        let Some((player, _, body)) = players.iter().find(|(_, o, _)| o.0 == client_id) else {
            continue;
        };
        let station = Station::of_block(world.get_block_at(block));
        if let Some(station) = station.filter(|_| in_reach(body.translation, block)) {
            commands.entity(player).insert(AtStation { station, block });
        }
    }
    for &FromClient { client_id, .. } in closes.read() {
        if let Some((player, ..)) = players.iter().find(|(_, o, _)| o.0 == client_id) {
            commands.entity(player).remove::<AtStation>();
        }
    }
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
    use crate::inventory::slots::Place;
    use crate::inventory::{Click, Inventory, SLOTS};
    use crate::items::{Item, ItemStack};
    use crate::player::{Player, PlayerOwner};
    use crate::protocol::{CloseInventory, FillGrid, InventoryClick, ProtocolPlugin, UseBlock};
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

    fn fill(app: &mut App, recipe: Recipe, all: bool) {
        send(app, FillGrid { recipe, all });
    }

    fn take_output(app: &mut App, click: Click) {
        send(
            app,
            InventoryClick {
                container: None,
                place: Place::Output,
                click,
            },
        );
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
    fn test_crafting_by_hand_lays_the_grid_and_takes_the_output() {
        let mut app = server(&[ItemStack::new(LOG, 3)]);
        fill(&mut app, Recipe::Planks, false);
        assert_eq!(Some(ItemStack::new(LOG, 1)), inventory(&mut app).grid[0]);
        take_output(&mut app, Click::Left);
        assert_eq!(Some(ItemStack::new(PLANKS, 4)), inventory(&mut app).cursor);
        assert_eq!(
            (2, None),
            (count(&mut app, LOG), inventory(&mut app).grid[0])
        );
        send(
            &mut app,
            InventoryClick {
                container: None,
                place: Place::Own(5),
                click: Click::Left,
            },
        );
        fill(&mut app, Recipe::Sticks, false);
        take_output(&mut app, Click::Shift);
        assert_eq!(
            (2, 4),
            (count(&mut app, PLANKS), count(&mut app, Item::Stick))
        );
    }

    #[test]
    fn test_the_recipe_book_lays_nothing_without_the_ingredients() {
        let mut app = server(&[ItemStack::new(PLANKS, 3)]);
        let before = inventory(&mut app);
        fill(&mut app, Recipe::CraftingTable, false);
        fill(&mut app, Recipe::Planks, true);
        assert_eq!(before, inventory(&mut app));
        // but with some, as many as there are
        fill(&mut app, Recipe::Sticks, true);
        let grid = inventory(&mut app).grid;
        assert_eq!([Some(ItemStack::new(PLANKS, 1)); 2], [grid[0], grid[3]]);
        assert_eq!(1, count(&mut app, PLANKS));
    }

    #[test]
    fn test_shift_taking_the_output_makes_as_many_as_were_laid() {
        let mut app = server(&[ItemStack::new(LOG, 20), ItemStack::new(PLANKS, 1)]);
        fill(&mut app, Recipe::Planks, true);
        take_output(&mut app, Click::Shift);
        assert_eq!((0, 81), (count(&mut app, LOG), count(&mut app, PLANKS)));
        assert!(dropped(&mut app).is_empty());
    }

    #[test]
    fn test_closing_returns_the_grid_and_drops_what_doesnt_fit() {
        let mut app = server(&[ItemStack::new(LOG, 2)]);
        fill(&mut app, Recipe::Planks, true);
        send(&mut app, CloseInventory);
        assert_eq!(2, count(&mut app, LOG));
        assert!(inventory(&mut app).grid.iter().all(Option::is_none));
        // with the inventory filled up while it was out, it's dropped at the player
        fill(&mut app, Recipe::Planks, true);
        let sand = ItemStack::full(Item::Block(BlockType::Sand));
        let mut query = app.world_mut().query::<&mut Inventory>();
        query.single_mut(app.world_mut()).unwrap().slots = [Some(sand); SLOTS];
        send(&mut app, CloseInventory);
        assert_eq!(vec![ItemStack::new(LOG, 2)], dropped(&mut app));
        assert!(inventory(&mut app).grid.iter().all(Option::is_none));
    }

    #[test]
    fn test_table_recipes_need_a_table_opened_in_reach() {
        let mut app = server(&[ItemStack::new(PLANKS, 9), ItemStack::new(Item::Stick, 6)]);
        // by hand
        fill(&mut app, Recipe::WoodenPickaxe, false);
        assert!(inventory(&mut app).grid.iter().all(Option::is_none));
        // at a table too far off, or at a block that isn't one
        send(&mut app, UseBlock { block: FAR_TABLE });
        send(
            &mut app,
            UseBlock {
                block: TABLE + I64Vec3::Y,
            },
        );
        fill(&mut app, Recipe::WoodenPickaxe, false);
        assert!(inventory(&mut app).grid.iter().all(Option::is_none));

        send(&mut app, UseBlock { block: TABLE });
        fill(&mut app, Recipe::WoodenPickaxe, false);
        take_output(&mut app, Click::Shift);
        assert_eq!(1, count(&mut app, Item::WoodenPickaxe));
        // and hand recipes still work there
        fill(&mut app, Recipe::CraftingTable, false);
        take_output(&mut app, Click::Shift);
        assert_eq!(1, count(&mut app, Item::Block(BlockType::CraftingTable)));

        // until the screen's closed
        send(&mut app, CloseInventory);
        let mut query = app.world_mut().query::<&AtStation>();
        assert!(query.iter(app.world()).next().is_none());
        fill(&mut app, Recipe::WoodenShovel, false);
        assert!(inventory(&mut app).grid.iter().all(Option::is_none));
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
        fill(&mut app, Recipe::WoodenAxe, false);
        app.world_mut()
            .resource_mut::<World>()
            .edit(TABLE, BlockType::Air);
        take_output(&mut app, Click::Left);
        assert_eq!(None, inventory(&mut app).cursor);
        // and what was laid comes back as the screen closes
        send(&mut app, CloseInventory);
        assert_eq!(
            (9, 6),
            (count(&mut app, PLANKS), count(&mut app, Item::Stick))
        );
    }
}
