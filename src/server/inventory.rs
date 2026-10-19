use bevy::prelude::*;
use bevy_replicon::prelude::*;

use super::containers::OpenContainer;
use super::drops::DroppedItem;
use crate::containers::Containers;
use crate::game_mode::GameMode;
use crate::inventory::slots::{Place, Slots};
use crate::inventory::{Inventory, HOTBAR_SLOTS};
use crate::items::ItemStack;
use crate::player::{PlayerLook, PlayerOwner};
use crate::protocol::{CloseInventory, DropItem, InventoryClick, InventoryDrag, PickBlock};

type Players<'w, 's> = Query<
    'w,
    's,
    (
        &'static PlayerOwner,
        &'static Transform,
        &'static PlayerLook,
        &'static GameMode,
        &'static mut Inventory,
        Option<&'static OpenContainer>,
    ),
>;

/// Changes players' inventories and the containers they have open as they ask: clicks and drags
/// in the inventory screen, closing it, and dropping and picking items. A request from a client with no
/// player, or for a slot there isn't, or a container they don't have open, changes nothing.
#[allow(clippy::too_many_arguments)]
pub(super) fn receive_inventory_requests(
    mut containers: ResMut<Containers>,
    mut clicks: MessageReader<FromClient<InventoryClick>>,
    mut drags: MessageReader<FromClient<InventoryDrag>>,
    mut closes: MessageReader<FromClient<CloseInventory>>,
    mut drops: MessageReader<FromClient<DropItem>>,
    mut picks: MessageReader<FromClient<PickBlock>>,
    mut players: Players,
    mut commands: Commands,
) {
    for &FromClient {
        client_id,
        message:
            InventoryClick {
                container,
                slot,
                click,
            },
    } in clicks.read()
    {
        let Some((body, look, _, mut inventory, open)) = player_of(&mut players, client_id) else {
            continue;
        };
        let open = open.map(|open| open.at);
        if container.is_some() && container != open {
            continue;
        }
        let place = match container {
            Some(_) => Place::Container(slot),
            None => Place::Own(slot),
        };
        let mut slots = Slots {
            inventory: &mut inventory,
            container: open.and_then(|at| containers.get_mut(at)),
        };
        if let Some(thrown) = slots.click(place, click) {
            commands.spawn((
                DroppedItem::thrown(thrown, body.translation, look.forward()),
                Replicated,
            ));
        }
    }
    for FromClient {
        client_id,
        message:
            InventoryDrag {
                container,
                places,
                spread,
            },
    } in drags.read()
    {
        let Some((.., mut inventory, open)) = player_of(&mut players, *client_id) else {
            continue;
        };
        let open = open.map(|open| open.at);
        let crosses_container = places.iter().any(|p| matches!(p, Place::Container(_)));
        if (container.is_some() || crosses_container) && *container != open {
            continue;
        }
        let mut slots = Slots {
            inventory: &mut inventory,
            container: container.and_then(|at| containers.get_mut(at)),
        };
        slots.drag(places, *spread);
    }
    for &FromClient { client_id, .. } in closes.read() {
        if let Some((body, look, _, mut inventory, _)) = player_of(&mut players, client_id) {
            // what there's no room for is dropped, as Minecraft does
            if let Some(left) = inventory.put_back_cursor() {
                commands.spawn((
                    DroppedItem::thrown(left, body.translation, look.forward()),
                    Replicated,
                ));
            }
        }
    }
    for &FromClient {
        client_id,
        message: DropItem { slot, all },
    } in drops.read()
    {
        if let Some((body, look, _, mut inventory, _)) = player_of(&mut players, client_id) {
            if let Some(stack) = inventory.drop_from(slot.into(), all) {
                commands.spawn((
                    DroppedItem::thrown(stack, body.translation, look.forward()),
                    Replicated,
                ));
            }
        }
    }
    for &FromClient {
        client_id,
        message: PickBlock { slot, item },
    } in picks.read()
    {
        let Some((_, _, mode, mut inventory, _)) = player_of(&mut players, client_id) else {
            continue;
        };
        let slot = usize::from(slot);
        let placeable = item.places().is_some_and(|block| mode.can_place(block));
        if mode.has_every_block() && placeable && slot < HOTBAR_SLOTS {
            inventory.slots[slot] = Some(ItemStack::full(item));
        }
    }
}

// The player `client` plays as: where they are, which way they look, their mode, their
// inventory to change and the container they have open.
type PlayerOf<'a> = (
    Transform,
    PlayerLook,
    GameMode,
    Mut<'a, Inventory>,
    Option<&'a OpenContainer>,
);

fn player_of<'a>(players: &'a mut Players, client: ClientId) -> Option<PlayerOf<'a>> {
    players
        .iter_mut()
        .find(|(owner, ..)| owner.0 == client)
        .map(|(_, body, look, mode, inventory, open)| (*body, *look, *mode, inventory, open))
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin;
    use bevy_replicon::prelude::*;

    use crate::block::BlockType;
    use crate::game_mode::GameMode;
    use crate::inventory::slots::{Place, Spread};
    use crate::inventory::{Click, Inventory, SLOTS};
    use crate::items::{Item, ItemStack};
    use crate::player::{Player, PlayerOwner};
    use crate::protocol::{
        CloseInventory, DropItem, InventoryClick, InventoryDrag, PickBlock, ProtocolPlugin,
    };
    use crate::server::drops::DroppedItem;
    use crate::server::ServerPlugin;
    use crate::world::World;

    const DIRT: Item = Item::Block(BlockType::Dirt);

    // A server alone with its host's player in `mode`, holding 10 dirt in slot 0.
    fn server(mode: GameMode) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins,
            ProtocolPlugin,
            ServerPlugin,
        ))
        .insert_resource(World::new(16));
        let mut inventory = Inventory::default();
        inventory.slots[0] = Some(ItemStack::new(DIRT, 10));
        app.world_mut().spawn((
            Player::default(),
            PlayerOwner(ClientId::Server),
            mode,
            inventory,
        ));
        app.finish();
        app
    }

    fn inventory(app: &mut App) -> Inventory {
        let mut query = app.world_mut().query::<&Inventory>();
        query.single(app.world()).unwrap().clone()
    }

    fn dropped(app: &mut App) -> Vec<ItemStack> {
        let mut query = app.world_mut().query::<&DroppedItem>();
        query.iter(app.world()).map(|item| item.stack).collect()
    }

    #[test]
    fn test_clicks_change_the_inventory_in_the_frame_theyre_made() {
        let mut app = server(GameMode::Survival);
        let click = |slot, click| InventoryClick {
            container: None,
            slot,
            click,
        };
        app.world_mut().write_message(click(0, Click::Right));
        app.world_mut().write_message(click(4, Click::Left));
        app.world_mut().write_message(click(0, Click::Shift));
        app.update();
        let inventory = inventory(&mut app);
        assert_eq!(Some(ItemStack::new(DIRT, 5)), inventory.slots[4]);
        assert_eq!(Some(ItemStack::new(DIRT, 5)), inventory.slots[9]);
        assert_eq!((None, None), (inventory.slots[0], inventory.cursor));
    }

    #[test]
    fn test_a_drag_is_made_whole_in_one_request_or_refused() {
        let mut app = server(GameMode::Survival);
        let pick_up = InventoryClick {
            container: None,
            slot: 0,
            click: Click::Left,
        };
        app.world_mut().write_message(pick_up);
        app.world_mut().write_message(InventoryDrag {
            container: None,
            places: vec![Place::Own(9), Place::Own(10), Place::Own(11)],
            spread: Spread::Even,
        });
        app.update();
        let inventory = inventory(&mut app);
        let three = Some(ItemStack::new(DIRT, 3));
        assert_eq!(
            [three; 3],
            [inventory.slots[9], inventory.slots[10], inventory.slots[11]]
        );
        assert_eq!(Some(ItemStack::new(DIRT, 1)), inventory.cursor);

        // one crossing a container the player doesn't have open changes nothing
        app.world_mut().write_message(InventoryDrag {
            container: None,
            places: vec![Place::Own(12), Place::Container(0)],
            spread: Spread::One,
        });
        app.update();
        assert_eq!(inventory, self::inventory(&mut app));
    }

    #[test]
    fn test_q_over_a_slot_in_the_screen_throws_from_it() {
        let mut app = server(GameMode::Survival);
        app.world_mut().write_message(InventoryClick {
            container: None,
            slot: 0,
            click: Click::Throw { all: false },
        });
        app.update();
        assert_eq!(Some(ItemStack::new(DIRT, 9)), inventory(&mut app).slots[0]);
        assert_eq!(vec![ItemStack::new(DIRT, 1)], dropped(&mut app));
    }

    #[test]
    fn test_closing_the_screen_puts_the_cursors_stack_back_or_drops_it() {
        let mut app = server(GameMode::Survival);
        app.world_mut().write_message(InventoryClick {
            container: None,
            slot: 0,
            click: Click::Left,
        });
        app.world_mut().write_message(CloseInventory);
        app.update();
        assert_eq!(Some(ItemStack::new(DIRT, 10)), inventory(&mut app).slots[0]);
        assert!(dropped(&mut app).is_empty());

        // with no room for it, it's dropped
        app.world_mut().write_message(InventoryClick {
            container: None,
            slot: 0,
            click: Click::Left,
        });
        app.update();
        let mut query = app.world_mut().query::<&mut Inventory>();
        query.single_mut(app.world_mut()).unwrap().slots =
            [Some(ItemStack::full(Item::Block(BlockType::Sand))); SLOTS];
        app.world_mut().write_message(CloseInventory);
        app.update();
        assert_eq!(None, inventory(&mut app).cursor);
        assert_eq!(vec![ItemStack::new(DIRT, 10)], dropped(&mut app));
    }

    #[test]
    fn test_q_drops_one_and_ctrl_q_the_stack() {
        let mut app = server(GameMode::Survival);
        app.world_mut().write_message(DropItem {
            slot: 0,
            all: false,
        });
        app.update();
        assert_eq!(Some(ItemStack::new(DIRT, 9)), inventory(&mut app).slots[0]);
        app.world_mut()
            .write_message(DropItem { slot: 0, all: true });
        // and from an empty slot, or one there isn't, nothing
        app.world_mut()
            .write_message(DropItem { slot: 1, all: true });
        app.world_mut().write_message(DropItem {
            slot: SLOTS as u8,
            all: true,
        });
        app.update();
        assert_eq!(None, inventory(&mut app).slots[0]);
        let mut counts: Vec<_> = dropped(&mut app).iter().map(|s| s.count).collect();
        counts.sort_unstable();
        assert_eq!(vec![1, 9], counts);
    }

    #[test]
    fn test_only_creative_picks_blocks_into_the_hotbar() {
        let pick = PickBlock {
            slot: 2,
            item: Item::Block(BlockType::Furnace),
        };
        let mut app = server(GameMode::Creative);
        app.world_mut().write_message(pick);
        app.update();
        let furnaces = Some(ItemStack::full(Item::Block(BlockType::Furnace)));
        assert_eq!(furnaces, inventory(&mut app).slots[2]);

        let mut app = server(GameMode::Survival);
        app.world_mut().write_message(pick);
        app.update();
        assert_eq!(None, inventory(&mut app).slots[2]);
        // nor water, nor what places no block, nor into the main inventory, in either mode
        let mut app = server(GameMode::Creative);
        for (slot, item) in [
            (1, Item::Block(BlockType::Water)),
            (3, Item::IronPickaxe),
            (9, Item::Block(BlockType::Stone)),
        ] {
            app.world_mut().write_message(PickBlock { slot, item });
        }
        // but seeds, which are planted, are
        app.world_mut().write_message(PickBlock {
            slot: 4,
            item: Item::Seeds,
        });
        app.update();
        let slots = inventory(&mut app).slots;
        assert_eq!([None, None, None], [slots[1], slots[3], slots[9]]);
        assert_eq!(Some(ItemStack::full(Item::Seeds)), slots[4]);
    }
}
