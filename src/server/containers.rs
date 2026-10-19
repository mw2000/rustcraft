//! Containers on the server (§5.2): opening them, filling chests the world generated from their
//! loot tables (§5.6), running furnaces, and sending players the one they have open as it
//! changes. Clicks on a container's slots are inventory requests (`server::inventory`), and
//! breaking one drops what's in it (`server::edits`).

use std::time::Duration;

use bevy::math::I64Vec3;
use bevy::prelude::*;
use bevy_replicon::prelude::*;

use super::edits::in_reach;
use crate::block::BlockType;
use crate::containers::{Container, Containers};
use crate::loot::generated_table;
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
        if !in_reach(body.translation, at) {
            continue;
        }
        fill_loot(&world, &mut containers, at);
        if containers.open(at, world.get_block_at(at)).is_some() {
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

/// Fills the chest at `at` from its structure's loot table if it's one the world generated and
/// nobody's opened, as it's opened or broken: one with no container, which a player didn't
/// place, as a player's chest is an edit. It's rolled the same whenever that is, from the seed
/// and where it is, and from then on it's a container like any other, saved with its chunk, so
/// what's taken from it stays taken. One no structure accounts for is left empty.
pub(super) fn fill_loot(world: &World, containers: &mut Containers, at: I64Vec3) {
    let unopened = world.get_block_at(at) == BlockType::Chest
        && world.edits().get(at).is_none()
        && containers
            .get(at)
            .is_none_or(|container| container.block != BlockType::Chest);
    if !unopened {
        return;
    }
    let table = generated_table(&world.region_store, at);
    if let Some((chest, table)) = containers.open(at, BlockType::Chest).zip(table) {
        chest.slots = table.roll(world.seed(), at, chest.slots.len());
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
    use crate::chunks::generate::generator::generate_chunk;
    use crate::containers::{Container, Containers, FUEL, INPUT, OUTPUT};
    use crate::crafting::SMELT_TIME;
    use crate::edits::Edits;
    use crate::game_mode::GameMode;
    use crate::inventory::slots::Place;
    use crate::inventory::{Click, Inventory};
    use crate::items::{Item, ItemStack};
    use crate::loot::{watchtower_chest, LootTable};
    use crate::player::{Player, PlayerOwner};
    use crate::protocol::{
        CloseInventory, ContainerChanged, InventoryClick, ProtocolPlugin, RequestEdit,
        StartBreaking, UseBlock,
    };
    use crate::server::drops::DroppedItem;
    use crate::server::ServerPlugin;
    use crate::world::World;

    const IRON_ORE: Item = Item::Block(BlockType::IronOre);
    // a furnace and a chest beside the player, standing at the origin, and a chest the world
    // generated there that no structure accounts for
    const FURNACE: I64Vec3 = I64Vec3::new(2, 1, 0);
    const CHEST: I64Vec3 = I64Vec3::new(2, 1, 2);
    const STRAY: I64Vec3 = I64Vec3::new(0, 1, 2);

    // A server of seed 3 alone with its player in `mode`, carrying iron ore and coal, a second a
    // frame.
    fn server(mode: GameMode) -> App {
        let mut world = World::from_save(3, Edits::default(), 16);
        let mut generated = ChunkData::default();
        generated.set_block_at(STRAY.as_u16vec3(), BlockType::Chest);
        world.insert_chunk(ChunkCoordinate(I64Vec3::ZERO), generated);
        world.edit(FURNACE, BlockType::Furnace);
        world.edit(CHEST, BlockType::Chest);
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
        // a container's slot when one's named, as the screen sends them
        let place = match container {
            Some(_) => Place::Container(slot),
            None => Place::Own(slot),
        };
        InventoryClick {
            container,
            place,
            click,
        }
    }

    fn furnace(app: &App) -> Option<Container> {
        container(app, FURNACE)
    }

    fn container(app: &App, at: I64Vec3) -> Option<Container> {
        app.world().resource::<Containers>().get(at).cloned()
    }

    // A second player joins, beside the first, carrying a coal.
    fn join(app: &mut App) -> ClientId {
        let guest = ClientId::Client(Entity::from_raw_u32(7).unwrap());
        let mut inventory = Inventory::default();
        inventory.slots[0] = Some(ItemStack::new(Item::Coal, 1));
        app.world_mut().spawn((
            Player::default(),
            PlayerOwner(guest),
            GameMode::Survival,
            inventory,
            Transform::from_xyz(0.5, 1.0, 1.5),
        ));
        guest
    }

    fn inventory(app: &mut App, owner: ClientId) -> Inventory {
        let mut query = app.world_mut().query::<(&PlayerOwner, &Inventory)>();
        let mut players = query.iter(app.world());
        players.find(|(o, _)| o.0 == owner).unwrap().1.clone()
    }

    fn dropped(app: &mut App) -> Vec<ItemStack> {
        let mut query = app.world_mut().query::<&DroppedItem>();
        let mut stacks: Vec<_> = query.iter(app.world()).map(|item| item.stack).collect();
        stacks.sort_unstable_by_key(|stack| (stack.item.name(), stack.count));
        stacks
    }

    fn break_block(app: &mut App, block: I64Vec3) {
        send(app, StartBreaking { block });
        // long enough to break a chest by hand
        for _ in 0..4 {
            app.update();
        }
        send(
            app,
            RequestEdit {
                block,
                to: crate::block::Block::AIR,
                slot: 8,
                seq: 0,
            },
        );
    }

    // Generates seed 3's watchtower nearest its spawn and moves the player beside the chest at
    // its top, returning where that is and what its loot table rolls there.
    fn watchtower(app: &mut App) -> (I64Vec3, Vec<Option<ItemStack>>) {
        let mut world = app.world_mut().resource_mut::<World>();
        let at = watchtower_chest(&world.region_store);
        let coord = ChunkCoordinate::containing(at);
        let chunk = generate_chunk(world.region_store.clone(), coord);
        world.insert_chunk(coord, chunk);
        let loot = LootTable::named("watchtower").unwrap().roll(3, at, 27);
        let mut query = app
            .world_mut()
            .query_filtered::<&mut Transform, With<Player>>();
        query.single_mut(app.world_mut()).unwrap().translation = at.as_vec3() + Vec3::X;
        (at, loot)
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
                to: BlockType::Air.into(),
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

    #[test]
    fn test_a_chest_opened_takes_stacks_shift_clicked_either_way() {
        let mut app = server(GameMode::Survival);
        send(&mut app, UseBlock { block: CHEST });
        // a chest a player placed starts empty
        assert_eq!(Container::new(BlockType::Chest), container(&app, CHEST));
        send(&mut app, click(None, 0, Click::Shift));
        send(&mut app, click(None, 1, Click::Left));
        send(&mut app, click(Some(CHEST), 20, Click::Right));
        let chest = container(&app, CHEST).unwrap();
        assert_eq!(Some(ItemStack::new(IRON_ORE, 3)), chest.slots[0]);
        assert_eq!(Some(ItemStack::new(Item::Coal, 1)), chest.slots[20]);
        assert_eq!(chest, sent(&mut app).pop().unwrap().container.unwrap());
        send(&mut app, click(Some(CHEST), 0, Click::Shift));
        assert_eq!(None, container(&app, CHEST).unwrap().slots[0]);
        let inventory = inventory(&mut app, ClientId::Server);
        assert_eq!(Some(ItemStack::new(IRON_ORE, 3)), inventory.slots[0]);
    }

    #[test]
    fn test_two_players_with_a_chest_open_see_each_others_changes() {
        let mut app = server(GameMode::Survival);
        let guest = join(&mut app);
        let open = |client_id| FromClient {
            client_id,
            message: UseBlock { block: CHEST },
        };
        app.world_mut().write_message(open(ClientId::Server));
        app.world_mut().write_message(open(guest));
        app.update();
        sent(&mut app);
        app.world_mut().write_message(FromClient {
            client_id: guest,
            message: click(None, 0, Click::Shift),
        });
        app.update();
        let chest = container(&app, CHEST).unwrap();
        assert_eq!(Some(ItemStack::new(Item::Coal, 1)), chest.slots[0]);
        // the host's sent what the guest put in, and so's the guest
        assert_eq!(Some(&chest), sent(&mut app)[0].container.as_ref());
        let mut query = app.world_mut().query::<&OpenContainer>();
        for open in query.iter(app.world()) {
            assert_eq!((CHEST, Some(&chest)), (open.at, open.sent.as_ref()));
        }
        assert_eq!(None, inventory(&mut app, guest).slots[0]);
    }

    #[test]
    fn test_a_chest_broken_leaves_itself_and_whats_in_it() {
        let mut app = server(GameMode::Survival);
        send(&mut app, UseBlock { block: CHEST });
        send(&mut app, click(None, 0, Click::Shift));
        send(&mut app, click(None, 1, Click::Shift));
        break_block(&mut app, CHEST);
        assert_eq!(
            BlockType::Air,
            app.world().resource::<World>().get_block_at(CHEST)
        );
        assert_eq!(None, container(&app, CHEST));
        let chest = ItemStack::new(Item::Block(BlockType::Chest), 1);
        let expected = vec![
            chest,
            ItemStack::new(Item::Coal, 1),
            ItemStack::new(IRON_ORE, 3),
        ];
        assert_eq!(expected, dropped(&mut app));
    }

    #[test]
    fn test_a_generated_chest_rolls_its_loot_once_as_its_first_opened() {
        let mut app = server(GameMode::Survival);
        let (loot, rolled) = watchtower(&mut app);
        assert_eq!(None, container(&app, loot));
        send(&mut app, UseBlock { block: loot });
        assert_eq!(rolled, container(&app, loot).unwrap().slots);
        // something taken out stays taken, closed and opened again
        let slot = rolled.iter().position(Option::is_some).unwrap();
        send(&mut app, click(Some(loot), slot as u8, Click::Shift));
        send(&mut app, CloseInventory);
        send(&mut app, UseBlock { block: loot });
        let opened = container(&app, loot).unwrap();
        assert_eq!(None, opened.slots[slot]);
        // and as it's saved and loaded, a container like any other
        let mut reloaded = server(GameMode::Survival);
        watchtower(&mut reloaded);
        let mut containers = reloaded.world_mut().resource_mut::<Containers>();
        containers.put(loot, opened.clone());
        send(&mut reloaded, UseBlock { block: loot });
        assert_eq!(Some(opened), container(&reloaded, loot));
    }

    #[test]
    fn test_a_generated_chest_no_structure_accounts_for_opens_empty() {
        let mut app = server(GameMode::Survival);
        send(&mut app, UseBlock { block: STRAY });
        assert_eq!(Container::new(BlockType::Chest), container(&app, STRAY));
    }

    #[test]
    fn test_a_generated_chest_broken_unopened_leaves_its_loot_and_one_put_back_is_empty() {
        let mut app = server(GameMode::Survival);
        let (loot, rolled) = watchtower(&mut app);
        break_block(&mut app, loot);
        let mut expected: Vec<_> = rolled.into_iter().flatten().collect();
        expected.push(ItemStack::new(Item::Block(BlockType::Chest), 1));
        expected.sort_unstable_by_key(|stack| (stack.item.name(), stack.count));
        assert_eq!(expected, dropped(&mut app));
        // a chest a player puts there is theirs, and empty
        let mut world = app.world_mut().resource_mut::<World>();
        world.edit(loot, BlockType::Chest);
        send(&mut app, UseBlock { block: loot });
        assert_eq!(Container::new(BlockType::Chest), container(&app, loot));
    }
}
