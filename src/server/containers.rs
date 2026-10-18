//! Containers on the server (§5.2): opening them, running furnaces, and sending players the one
//! they have open as it changes. Clicks on a container's slots are inventory requests
//! (`server::inventory`), and breaking one drops what's in it (`server::edits`).

use std::time::Duration;

use bevy::math::I64Vec3;
use bevy::prelude::*;
use bevy_replicon::prelude::*;

use super::edits::in_reach;
use crate::containers::{Container, Containers};
use crate::player::PlayerOwner;
use crate::protocol::{CloseInventory, ContainerChanged, UseBlock};
use crate::world::World;

// The most often a container's sent to a player who has it open while only its fire changes,
// which is every frame a furnace burns: often enough for its flame and arrow to move smoothly.
// A change to its slots is sent at once.
const FIRE_INTERVAL: Duration = Duration::from_millis(100);

/// The container a player has open, from using its block until they close the inventory screen,
/// and what they were last sent of it.
#[derive(Component, Debug)]
pub(super) struct OpenContainer {
    pub(super) at: I64Vec3,
    sent: Option<Container>,
    sent_at: Duration,
}

/// Opens the containers players use, if they're in reach, and closes them as they close the
/// inventory screen.
pub(super) fn open_containers(
    mut uses: MessageReader<FromClient<UseBlock>>,
    mut closes: MessageReader<FromClient<CloseInventory>>,
    world: Res<World>,
    mut containers: ResMut<Containers>,
    players: Query<(Entity, &PlayerOwner, &Transform)>,
    mut commands: Commands,
) {
    let player_of = |client: ClientId| players.iter().find(|(_, owner, _)| owner.0 == client);
    for &FromClient {
        client_id,
        message: UseBlock { block: at },
    } in uses.read()
    {
        let Some((player, _, body)) = player_of(client_id) else {
            continue;
        };
        if in_reach(body.translation, at) && containers.open(at, world.get_block_at(at)).is_some() {
            commands.entity(player).insert(OpenContainer {
                at,
                sent: None,
                sent_at: Duration::ZERO,
            });
        }
    }
    for &FromClient { client_id, .. } in closes.read() {
        if let Some((player, ..)) = player_of(client_id) {
            commands.entity(player).remove::<OpenContainer>();
        }
    }
}

/// Runs every furnace that's burning, wherever it is (see `Containers`).
pub(super) fn run_furnaces(time: Res<Time>, mut containers: ResMut<Containers>) {
    containers.run(time.delta_secs());
}

/// Sends each player the container they have open as it changes, and closes it for them,
/// telling them so, once it's gone or out of their reach.
pub(super) fn send_containers(
    time: Res<Time>,
    containers: Res<Containers>,
    mut players: Query<(Entity, &PlayerOwner, &Transform, &mut OpenContainer)>,
    mut changes: MessageWriter<ToClients<ContainerChanged>>,
    mut commands: Commands,
) {
    let now = time.elapsed();
    for (player, owner, body, mut open) in &mut players {
        let at = open.at;
        let container = containers
            .get(at)
            .filter(|_| in_reach(body.translation, at));
        let send = |container: Option<Container>| ToClients {
            targets: SendTargets::Single(owner.0),
            message: ContainerChanged { at, container },
        };
        let Some(container) = container else {
            changes.write(send(None));
            commands.entity(player).remove::<OpenContainer>();
            continue;
        };
        let due = match &open.sent {
            None => true,
            Some(sent) => {
                sent.slots != container.slots
                    || (sent.fire != container.fire && now >= open.sent_at + FIRE_INTERVAL)
            }
        };
        if due {
            changes.write(send(Some(container.clone())));
            open.sent = Some(container.clone());
            open.sent_at = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::ecs::message::Messages;
    use bevy::math::I64Vec3;
    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;
    use bevy_replicon::prelude::*;

    use super::OpenContainer;
    use crate::block::BlockType;
    use crate::chunks::chunk::{ChunkCoordinate, ChunkData};
    use crate::containers::{Container, Containers, FUEL, INPUT, OUTPUT};
    use crate::crafting::SMELT_TIME;
    use crate::game_mode::GameMode;
    use crate::inventory::{Click, Inventory};
    use crate::items::{Item, ItemStack};
    use crate::player::{Player, PlayerOwner};
    use crate::protocol::{
        CloseInventory, ContainerChanged, InventoryClick, ProtocolPlugin, RequestEdit, UseBlock,
    };
    use crate::server::drops::DroppedItem;
    use crate::server::ServerPlugin;
    use crate::world::World;

    const IRON_ORE: Item = Item::Block(BlockType::IronOre);
    // a furnace beside the player, standing at the origin
    const FURNACE: I64Vec3 = I64Vec3::new(2, 1, 0);

    // A server alone with its player in `mode`, carrying iron ore and coal, a second a frame.
    fn server(mode: GameMode) -> App {
        let mut world = World::new(16);
        world.insert_chunk(ChunkCoordinate(I64Vec3::ZERO), ChunkData::default());
        world.edit(FURNACE, BlockType::Furnace);
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins,
            ProtocolPlugin,
            ServerPlugin,
        ))
        .insert_resource(world)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(1)));
        let mut inventory = Inventory::default();
        inventory.slots[0] = Some(ItemStack::new(IRON_ORE, 3));
        inventory.slots[1] = Some(ItemStack::new(Item::Coal, 1));
        app.world_mut().spawn((
            Player::default(),
            PlayerOwner(ClientId::Server),
            mode,
            inventory,
            Transform::from_xyz(0.5, 1.0, 0.5),
        ));
        // frames of a whole second, which virtual time clamps to a quarter otherwise
        let mut time = app.world_mut().resource_mut::<Time<Virtual>>();
        time.set_max_delta(Duration::from_secs(1));
        app.finish();
        app
    }

    fn send(app: &mut App, message: impl Message) {
        app.world_mut().write_message(message);
        app.update();
    }

    fn click(container: Option<I64Vec3>, slot: u8, click: Click) -> InventoryClick {
        InventoryClick {
            container,
            slot,
            click,
        }
    }

    fn furnace(app: &App) -> Option<Container> {
        app.world().resource::<Containers>().get(FURNACE).cloned()
    }

    // What the host's been sent of its open container, oldest first.
    fn sent(app: &mut App) -> Vec<ContainerChanged> {
        let mut messages = app.world_mut().resource_mut::<Messages<ContainerChanged>>();
        messages.drain().collect()
    }

    #[test]
    fn test_a_furnace_opened_smelts_what_its_given_and_its_sent_as_it_does() {
        let mut app = server(GameMode::Survival);
        // a click on a furnace that isn't open does nothing
        send(&mut app, click(Some(FURNACE), INPUT as u8, Click::Left));
        assert_eq!(None, furnace(&app));
        send(&mut app, UseBlock { block: FURNACE });
        assert_eq!(Container::new(BlockType::Furnace), furnace(&app));
        let opened = sent(&mut app);
        assert_eq!(1, opened.len());
        assert_eq!(furnace(&app), opened[0].container);

        // the ore picked up and put in, and the coal shift-clicked onto the fire
        send(&mut app, click(None, 0, Click::Left));
        send(&mut app, click(Some(FURNACE), INPUT as u8, Click::Left));
        send(&mut app, click(None, 1, Click::Shift));
        // lit the next frame
        app.update();
        let lit = furnace(&app).unwrap();
        assert_eq!(None, lit.slots[FUEL]);
        assert!(lit.fire.left > 0.0);
        sent(&mut app);
        let mut sent_since = Vec::new();
        for _ in 0..SMELT_TIME as usize * 3 {
            app.update();
            sent_since.extend(sent(&mut app));
        }
        let done = furnace(&app).unwrap();
        assert_eq!(Some(ItemStack::new(Item::IronIngot, 3)), done.slots[OUTPUT]);
        assert_eq!(None, done.slots[INPUT]);
        // sent every frame, as its fire changes every second
        assert_eq!(SMELT_TIME as usize * 3, sent_since.len());
        assert_eq!(Some(done), sent_since.last().unwrap().container);

        // and it smelts on once it's closed, with nobody looking
        send(&mut app, CloseInventory);
        let mut query = app.world_mut().query::<&OpenContainer>();
        assert!(query.iter(app.world()).next().is_none());
    }

    #[test]
    fn test_a_furnace_out_of_reach_or_broken_closes_and_breaking_it_drops_whats_in_it() {
        let mut app = server(GameMode::Creative);
        send(&mut app, UseBlock { block: FURNACE });
        send(&mut app, click(None, 0, Click::Left));
        send(&mut app, click(Some(FURNACE), INPUT as u8, Click::Left));
        sent(&mut app);
        send(
            &mut app,
            RequestEdit {
                block: FURNACE,
                to: BlockType::Air,
                slot: 0,
                seq: 0,
            },
        );
        assert_eq!(None, furnace(&app));
        let mut query = app.world_mut().query::<&DroppedItem>();
        let dropped: Vec<_> = query.iter(app.world()).map(|item| item.stack).collect();
        assert_eq!(vec![ItemStack::new(IRON_ORE, 3)], dropped);
        let closed = ContainerChanged {
            at: FURNACE,
            container: None,
        };
        assert_eq!(vec![closed], sent(&mut app));

        // one walked away from is closed too
        let mut app = server(GameMode::Survival);
        send(&mut app, UseBlock { block: FURNACE });
        sent(&mut app);
        let mut query = app.world_mut().query::<&mut Transform>();
        query.single_mut(app.world_mut()).unwrap().translation.x = 20.0;
        app.update();
        assert_eq!(None, sent(&mut app)[0].container);
        // and a crafting table or a furnace out of reach isn't opened
        send(&mut app, UseBlock { block: FURNACE });
        assert!(sent(&mut app).is_empty());
    }

    #[test]
    fn test_a_furnace_far_from_anyone_in_a_chunk_not_loaded_smelts_on() {
        let mut app = server(GameMode::Survival);
        let far = I64Vec3::new(5000, 450, -5000);
        let world = app.world().resource::<World>();
        assert!(!world.is_chunk_generated(ChunkCoordinate::containing(far)));
        let mut containers = app.world_mut().resource_mut::<Containers>();
        let furnace = containers.open(far, BlockType::Furnace).unwrap();
        furnace.slots[INPUT] = Some(ItemStack::new(IRON_ORE, 2));
        furnace.slots[FUEL] = Some(ItemStack::new(Item::Stick, 1));
        for _ in 0..SMELT_TIME as usize {
            app.update();
        }
        let containers = app.world().resource::<Containers>();
        let output = containers.get(far).unwrap().slots[OUTPUT];
        // a stick burns for half an item, which starts again once there's fuel for it
        assert_eq!(None, output);
        assert_eq!(0.0, containers.get(far).unwrap().fire.left);
        // and with more fuel, it finishes it
        let mut containers = app.world_mut().resource_mut::<Containers>();
        containers.get_mut(far).unwrap().slots[FUEL] = Some(ItemStack::new(Item::Coal, 1));
        for _ in 0..SMELT_TIME as usize {
            app.update();
        }
        let containers = app.world().resource::<Containers>();
        let output = containers.get(far).unwrap().slots[OUTPUT];
        assert_eq!(Some(ItemStack::new(Item::IronIngot, 1)), output);
    }
}
