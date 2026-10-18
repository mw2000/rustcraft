use bevy::prelude::*;
use bevy_replicon::prelude::*;

use super::drops::DroppedItem;
use crate::game_mode::GameMode;
use crate::inventory::{Inventory, HOTBAR_SLOTS};
use crate::items::{Item, ItemStack};
use crate::player::{PlayerLook, PlayerOwner};
use crate::protocol::{CloseInventory, DropItem, InventoryClick, PickBlock};

type Players<'w, 's> = Query<
    'w,
    's,
    (
        &'static PlayerOwner,
        &'static Transform,
        &'static PlayerLook,
        &'static GameMode,
        &'static mut Inventory,
    ),
>;

/// Changes players' inventories as they ask: clicks in the inventory screen, closing it, and
/// dropping and picking items. A request from a client with no player, or for a slot there
/// isn't, changes nothing.
pub(super) fn receive_inventory_requests(
    mut clicks: MessageReader<FromClient<InventoryClick>>,
    mut closes: MessageReader<FromClient<CloseInventory>>,
    mut drops: MessageReader<FromClient<DropItem>>,
    mut picks: MessageReader<FromClient<PickBlock>>,
    mut players: Players,
    mut commands: Commands,
) {
    for &FromClient {
        client_id,
        message: InventoryClick { slot, click },
    } in clicks.read()
    {
        if let Some((.., mut inventory)) = player_of(&mut players, client_id) {
            inventory.click(slot.into(), click);
        }
    }
    for &FromClient { client_id, .. } in closes.read() {
        if let Some((body, look, _, mut inventory)) = player_of(&mut players, client_id) {
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
        if let Some((body, look, _, mut inventory)) = player_of(&mut players, client_id) {
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
        message: PickBlock { slot, block },
    } in picks.read()
    {
        let Some((_, _, mode, mut inventory)) = player_of(&mut players, client_id) else {
            continue;
        };
        let slot = usize::from(slot);
        if mode.has_every_block() && mode.can_place(block) && slot < HOTBAR_SLOTS {
            inventory.slots[slot] = Some(ItemStack::full(Item::Block(block)));
        }
    }
}

// The player `client` plays as: where they are, which way they look, their mode and their
// inventory to change.
fn player_of<'a>(
    players: &'a mut Players,
    client: ClientId,
) -> Option<(Transform, PlayerLook, GameMode, Mut<'a, Inventory>)> {
    players
        .iter_mut()
        .find(|(owner, ..)| owner.0 == client)
        .map(|(_, body, look, mode, inventory)| (*body, *look, *mode, inventory))
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin;
    use bevy_replicon::prelude::*;

    use crate::block::BlockType;
    use crate::game_mode::GameMode;
    use crate::inventory::{Click, Inventory, SLOTS};
    use crate::items::{Item, ItemStack};
    use crate::player::{Player, PlayerOwner};
    use crate::protocol::{CloseInventory, DropItem, InventoryClick, PickBlock, ProtocolPlugin};
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
        let click = |slot, click| InventoryClick { slot, click };
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
    fn test_closing_the_screen_puts_the_cursors_stack_back_or_drops_it() {
        let mut app = server(GameMode::Survival);
        app.world_mut().write_message(InventoryClick {
            slot: 0,
            click: Click::Left,
        });
        app.world_mut().write_message(CloseInventory);
        app.update();
        assert_eq!(Some(ItemStack::new(DIRT, 10)), inventory(&mut app).slots[0]);
        assert!(dropped(&mut app).is_empty());

        // with no room for it, it's dropped
        app.world_mut().write_message(InventoryClick {
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
            block: BlockType::Furnace,
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
        // nor water, nor into the main inventory, in either mode
        let mut app = server(GameMode::Creative);
        app.world_mut().write_message(PickBlock {
            slot: 1,
            block: BlockType::Water,
        });
        app.world_mut().write_message(PickBlock {
            slot: 9,
            block: BlockType::Stone,
        });
        app.update();
        assert_eq!(
            (None, None),
            (inventory(&mut app).slots[1], inventory(&mut app).slots[9])
        );
    }
}
