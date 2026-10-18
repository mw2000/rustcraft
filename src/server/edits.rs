use std::time::Duration;

use bevy::math::I64Vec3;
use bevy::prelude::*;
use bevy_replicon::prelude::*;

use super::allowances::Allowances;
use super::containers::fill_loot;
use super::drops::{Chance, DroppedItem};
use super::sync::BlockChanges;
use crate::block::{Block, BlockType, Direction};
use crate::breaking::{break_time, harvests};
use crate::building::{needs_support, other_half, supported, toggled};
use crate::chunks::chunk::ChunkCoordinate;
use crate::clock::TimeOfDay;
use crate::collision::overlaps;
use crate::containers::Containers;
use crate::farming::{tilled, Crops};
use crate::game_mode::GameMode;
use crate::inventory::{Inventory, HOTBAR_SLOTS};
use crate::items::{ItemStack, ToolKind};
use crate::player::{Player, PlayerOwner, BODY, EYE_HEIGHT, REACH};
use crate::protocol::{BlockChanged, EditRejected, RequestEdit, StartBreaking, UseBlock};
use crate::world::World;

// How much further than `REACH` from the eye an edit can be, in blocks, for a player who's
// moved since aiming: a block for rounding, as in singleplayer, where it's the same frame, and
// two for the server's copy of a client's player trailing the client by up to a pose's
// interval and the trip here, a tenth of a second together, flying at the usual 20 blocks a
// second. Building while sprinting through the air at 80 is too fast to be worth covering.
pub(super) const REACH_SLACK: f32 = 3.0;
// Clicking as fast as anyone can is about 15 a second, so creative building at full speed never
// meets this, and a burst of a second's worth lets a quick flurry of clicks through whole. It only
// stops a client flooding the server with edits.
const EDITS_PER_SECOND: f32 = 20.0;
const EDIT_BURST: f32 = 20.0;
// How much sooner than it takes, in seconds, a block can be broken after the player started on
// it. The client times its hold from the frame it sends `StartBreaking` to the frame it sends
// the edit, and the server from the frame each arrives, so in one App the two agree to the
// frame. Over a network each goes its own way, but both take about as long, so their delay
// cancels all but its jitter, a few tens of milliseconds, and a server frame's wait; a tenth of
// a second covers those and lets no one break stone with a wooden pickaxe more than a twentieth
// quicker. A start overtaken by its edit, which would take jitter longer than the break, is
// refused, and the client's prediction rolled back.
const BREAK_SLACK: f32 = 0.1;

/// What each client may still edit straight away.
#[derive(Resource, Deref, DerefMut)]
pub(super) struct EditAllowances(Allowances);

impl Default for EditAllowances {
    fn default() -> Self {
        Self(Allowances::new(EDIT_BURST, EDITS_PER_SECOND))
    }
}

/// The block a player in survival started breaking, and when.
#[derive(Component, Debug, Clone, Copy)]
pub(super) struct BreakingStarted {
    block: I64Vec3,
    at: Duration,
}

/// Times the blocks players start breaking, to check the edits breaking them against.
pub(super) fn receive_break_starts(
    mut starts: MessageReader<FromClient<StartBreaking>>,
    time: Res<Time>,
    players: Query<(Entity, &PlayerOwner)>,
    mut commands: Commands,
) {
    for &FromClient {
        client_id,
        message: StartBreaking { block },
    } in starts.read()
    {
        if let Some((player, _)) = players.iter().find(|(_, owner)| owner.0 == client_id) {
            let at = time.elapsed();
            commands
                .entity(player)
                .insert(BreakingStarted { block, at });
        }
    }
}

type Players<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Transform,
        &'static GameMode,
        &'static PlayerOwner,
        &'static mut Inventory,
        Option<&'static BreakingStarted>,
    ),
    With<Player>,
>;

/// Makes the edits clients ask for that pass the checks, telling the clients near them, and
/// tells the client asking for one that doesn't what's there instead. In survival a block placed
/// is taken from the player's hotbar, and one broken leaves its items to pick up if it was broken
/// with a tool of the tier it needs, and wears the tool held. A container broken in either mode
/// leaves what was in it, as items are never lost with their container, and a chest the world
/// generated leaves its loot. Dirt or grass made farmland is tilled with the hoe held, which
/// wears it in survival, and a crop placed is planted, to grow from then (`farming`). A door
/// placed puts its upper block over it, and one broken takes its other block with it; and what
/// an edit leaves with nothing to hold it up, a plant on the ground or a ladder on a wall, goes.
#[allow(clippy::too_many_arguments)]
pub(super) fn receive_edits(
    mut requests: MessageReader<FromClient<RequestEdit>>,
    mut world: ResMut<World>,
    mut containers: ResMut<Containers>,
    mut crops: ResMut<Crops>,
    clock: Res<TimeOfDay>,
    mut allowances: ResMut<EditAllowances>,
    time: Res<Time>,
    mut players: Players,
    mut changed: BlockChanges,
    mut rejected: MessageWriter<ToClients<EditRejected>>,
    mut chance: ResMut<Chance>,
    mut commands: Commands,
) {
    for &FromClient {
        client_id: client,
        message:
            RequestEdit {
                block,
                to,
                slot,
                seq,
            },
    } in requests.read()
    {
        let was = world.get(block);
        // a chest the world generated is filled before it can be broken, so it leaves its loot;
        // it comes up the same whether that's now or when it's opened
        fill_loot(&world, &mut containers, block);
        // every request counts, so a flood of bad ones is stopped too
        let why_not = if allowances.take(client, time.elapsed()) {
            refusal(
                &world,
                &players,
                client,
                block,
                to,
                slot.into(),
                time.elapsed(),
            )
            .or_else(|| (!world.edit(block, to)).then_some("it's already that block"))
        } else {
            Some("too many edits")
        };
        if let Some(why) = why_not {
            debug!("refused {client}'s edit of {block} to {to}: {why}");
            rejected.write(ToClients {
                targets: SendTargets::Single(client),
                message: EditRejected {
                    seq,
                    block,
                    actual: was,
                },
            });
            continue;
        }
        changed.write(BlockChanged {
            block,
            to,
            by: Some((client, seq)),
        });
        let (player, _, mode, _, mut inventory, _) = players
            .iter_mut()
            .find(|(_, _, _, owner, ..)| owner.0 == client)
            .expect("an edit allowed has a player making it");
        let slot = usize::from(slot);
        let placed_from = held(&inventory, slot).map(|stack| stack.item);
        let tool = placed_from.and_then(|item| item.tool());
        let broken = to.kind == BlockType::Air;
        if broken {
            // the time it was held for is spent, so a block put back there takes as long
            commands.entity(player).remove::<BreakingStarted>();
        }
        let held_in_it = containers.remove(block).into_iter().flat_map(|c| c.slots);
        for stack in held_in_it.flatten() {
            commands.spawn((DroppedItem::from_block(stack, block), Replicated));
        }
        crops.remove(block);
        if to.kind.is_crop() {
            crops.plant(block, clock.0);
        }
        let drops = mode.drops_broken_blocks();
        let mut leave = |broken: BlockType, at: I64Vec3| {
            if drops && harvests(broken, tool) {
                for stack in broken.drops(&mut chance.0) {
                    commands.spawn((DroppedItem::from_block(stack, at), Replicated));
                }
            }
        };
        // what breaks at a touch doesn't wear a tool, nor does anything in creative
        if broken {
            leave(was.kind, block);
            if tool.is_some() && !mode.breaks_at_once() && was.kind.hardness() > 0.0 {
                inventory.wear(slot);
            }
        } else if tilled(was.kind) == Some(to.kind) {
            if !mode.breaks_at_once() {
                inventory.wear(slot);
            }
        } else if let Some(item) = placed_from.filter(|_| !mode.has_every_block()) {
            inventory.use_one(slot, item);
        }
        let mut set = |world: &mut World, at: I64Vec3, to: Block| {
            if world.edit(at, to) {
                changed.write(BlockChanged {
                    block: at,
                    to,
                    by: None,
                });
            }
        };
        // a door's two blocks are placed and broken together, and the item comes from one
        let door = if broken { was } else { to };
        if let Some((other, half)) = other_half(block, door) {
            set(&mut world, other, if broken { Block::AIR } else { half });
        }
        // what was held up by it goes, as a crop does with its farmland, leaving its item
        for side in Direction::ALL {
            let at = block + side.offset().as_i64vec3();
            let held_up = world.get(at);
            if needs_support(held_up.kind) && !supported(at, held_up, |at| world.get(at)) {
                set(&mut world, at, Block::AIR);
                crops.remove(at);
                leave(held_up.kind, at);
                if let Some((other, _)) = other_half(at, held_up) {
                    set(&mut world, other, Block::AIR);
                }
            }
        }
        // and farmland built on is packed back into dirt
        let below = block - I64Vec3::Y;
        if to.kind.is_solid() && world.get_block_at(below) == BlockType::Farmland {
            set(&mut world, below, BlockType::Dirt.into());
        }
    }
}

/// Opens or shuts the door or trapdoor each player uses in reach, a door's two blocks together,
/// in either game mode.
pub(super) fn use_doors(
    mut uses: MessageReader<FromClient<UseBlock>>,
    mut world: ResMut<World>,
    players: Query<(&PlayerOwner, &Transform)>,
    mut changed: BlockChanges,
) {
    for &FromClient {
        client_id,
        message: UseBlock { block },
    } in uses.read()
    {
        let reaches = players
            .iter()
            .any(|(owner, body)| owner.0 == client_id && in_reach(body.translation, block));
        let was = world.get(block);
        let Some(to) = toggled(was).filter(|_| reaches) else {
            continue;
        };
        let other = other_half(block, was).map(|(at, half)| (at, toggled(half).unwrap()));
        for (at, to) in std::iter::once((block, to)).chain(other) {
            if world.edit(at, to) {
                changed.write(BlockChanged {
                    block: at,
                    to,
                    by: None,
                });
            }
        }
    }
}

/// Whether a player with their feet at `feet` can reach `block`: whether its nearest point is
/// within `REACH` of their eye, give or take their moving since they aimed.
pub(super) fn in_reach(feet: Vec3, block: I64Vec3) -> bool {
    let eye = feet + Vec3::Y * EYE_HEIGHT;
    let nearest = eye.clamp(block.as_vec3(), block.as_vec3() + 1.0);
    eye.distance(nearest) <= REACH + REACH_SLACK
}

// What's in hotbar slot `slot`, if there's one.
fn held(inventory: &Inventory, slot: usize) -> Option<ItemStack> {
    inventory.slots[..HOTBAR_SLOTS].get(slot).copied().flatten()
}

// Why `client` can't change `block` to `to` at `now`, placing it from or breaking it with what's
// in hotbar slot `slot`, if it can't.
fn refusal(
    world: &World,
    players: &Players,
    client: ClientId,
    block: I64Vec3,
    to: Block,
    slot: usize,
    now: Duration,
) -> Option<&'static str> {
    let coord = ChunkCoordinate::containing(block);
    if !World::layers(world.height).contains(&coord.0.y) {
        return Some("it's beyond the world");
    }
    if !world.is_chunk_generated(coord) {
        return Some("its chunk isn't loaded");
    }
    let Some((_, feet, mode, _, inventory, started)) = players
        .iter()
        .find(|(_, _, _, owner, ..)| owner.0 == client)
    else {
        return Some("they have no player");
    };
    if !in_reach(feet.translation, block) {
        return Some("it's out of reach");
    }
    let was = world.get_block_at(block);
    if to.kind == BlockType::Air && !mode.breaks_at_once() {
        let tool = held(inventory, slot).and_then(|stack| stack.item.tool());
        let takes = break_time(was, tool);
        let held_for = started
            .filter(|started| started.block == block)
            .map_or(0.0, |started| now.saturating_sub(started.at).as_secs_f32());
        if held_for < takes - BREAK_SLACK {
            return Some("it's broken sooner than it takes");
        }
    }
    if tilled(was) == Some(to.kind) {
        let tool = held(inventory, slot).and_then(|stack| stack.item.tool());
        if tool.is_none_or(|tool| tool.kind != ToolKind::Hoe) {
            return Some("they aren't holding a hoe");
        }
        if world.get_block_at(block + I64Vec3::Y) != BlockType::Air {
            return Some("there's something on it");
        }
        return None;
    }
    if to.kind != BlockType::Air {
        if !was.is_replaceable() {
            return Some("it's into a solid block");
        }
        if !mode.can_place(to.kind) {
            return Some("they can't place that");
        }
        let held = held(inventory, slot);
        if !mode.has_every_block() && held.is_none_or(|stack| stack.item.places() != Some(to.kind))
        {
            return Some("they aren't holding that");
        }
        if needs_support(to.kind) && !supported(block, to, |at| world.get(at)) {
            return Some("there's nothing to hold it up");
        }
        // a door is placed by its lower block, which puts its upper one over it
        let mut cells = vec![block];
        if let Some((upper, _)) = other_half(block, to) {
            if to.state.top() {
                return Some("it's the top of a door");
            }
            if !world.get_block_at(upper).is_replaceable() {
                return Some("there's no room for the door");
            }
            cells.push(upper);
        }
        let into_player = players.iter().any(|(_, body, ..)| {
            cells
                .iter()
                .any(|&cell| overlaps(BODY, body.translation, cell))
        });
        if into_player {
            return Some("it's into a player");
        }
    }
    None
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

    use super::EDIT_BURST;
    use crate::block::BlockType;
    use crate::chunks::chunk::{ChunkCoordinate, ChunkData};
    use crate::clock::TimeOfDay;
    use crate::farming::Crops;
    use crate::game_mode::GameMode;
    use crate::inventory::Inventory;
    use crate::items::{Item, ItemStack};
    use crate::player::{Player, PlayerOwner};
    use crate::protocol::{
        BlockChanged, EditRejected, ProtocolPlugin, RequestEdit, StartBreaking, UseBlock,
    };
    use crate::server::drops::DroppedItem;
    use crate::server::ServerPlugin;
    use crate::world::World;

    // On a floor of stone at y 0, at the corner of the loaded chunks from (0, 0, -1) to
    // (1, 1, 0), so the chunk to -x isn't loaded.
    const FEET: Vec3 = Vec3::new(0.5, 1.0, 0.5);
    const FLOOR: I64Vec3 = I64Vec3::new(3, 0, 0);

    // A server alone, as a dedicated one runs, its host's player standing at `FEET`.
    fn server(mode: GameMode) -> App {
        let mut world = World::new(16);
        for x in 0..=1 {
            for y in 0..=1 {
                for z in -1..=0 {
                    let coord = ChunkCoordinate(I64Vec3::new(x, y, z));
                    world.insert_chunk(coord, ChunkData::default());
                }
            }
        }
        for x in 0..8 {
            world.edit(I64Vec3::new(x, 0, 0), BlockType::Stone);
        }
        world.edits_mut().take_changed();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins,
            ProtocolPlugin,
            ServerPlugin,
        ))
        .insert_resource(world)
        // no time passes, so the allowance of edits isn't refilled
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
        app.world_mut().spawn((
            Player::default(),
            PlayerOwner(ClientId::Server),
            mode,
            Transform::from_translation(FEET),
        ));
        // where replicon sets up its systems for the messages registered
        app.finish();
        app
    }

    // Asks for each edit, all in one frame, and returns what the server answered.
    fn request(
        app: &mut App,
        edits: &[(I64Vec3, BlockType)],
    ) -> (Vec<BlockChanged>, Vec<EditRejected>) {
        for (seq, &(block, to)) in edits.iter().enumerate() {
            app.world_mut().write_message(RequestEdit {
                block,
                to: to.into(),
                slot: 0,
                seq: seq as u32,
            });
        }
        app.update();
        (drain(app), drain(app))
    }

    fn drain<M: Message>(app: &mut App) -> Vec<M> {
        app.world_mut()
            .resource_mut::<Messages<M>>()
            .drain()
            .collect()
    }

    fn block_at(app: &App, block: I64Vec3) -> BlockType {
        app.world().resource::<World>().get_block_at(block)
    }

    // Asks for one edit that's to be refused, and checks the world's left as it was.
    fn assert_refused(app: &mut App, block: I64Vec3, to: BlockType) {
        let before = block_at(app, block);
        let (changed, rejected) = request(app, &[(block, to)]);
        assert_eq!(Vec::<BlockChanged>::new(), changed, "{block} to {to:?}");
        let refusal = EditRejected {
            seq: 0,
            block,
            actual: before.into(),
        };
        assert_eq!(vec![refusal], rejected);
        assert_eq!(before, block_at(app, block));
    }

    #[test]
    fn test_an_edit_in_reach_is_made_and_sent_to_the_host() {
        let mut app = server(GameMode::Creative);
        let (above, under_feet) = (FLOOR + I64Vec3::Y, I64Vec3::ZERO);
        let edits = [
            (FLOOR, BlockType::Air),
            (above, BlockType::Planks),
            (under_feet, BlockType::Air),
        ];
        let (changed, rejected) = request(&mut app, &edits);
        assert!(rejected.is_empty(), "{rejected:?}");
        let expected: Vec<_> = (0..)
            .zip(edits)
            .map(|(seq, (block, to))| BlockChanged {
                block,
                to: to.into(),
                by: Some((ClientId::Server, seq)),
            })
            .collect();
        assert_eq!(expected, changed);
        for (block, to) in edits {
            assert_eq!(to, block_at(&app, block));
        }
        // and recorded, for saving
        let mut world = app.world_mut().resource_mut::<World>();
        let saved = world.edits_mut().take_changed();
        assert!(saved.contains(&ChunkCoordinate(I64Vec3::ZERO)));
    }

    #[test]
    fn test_edits_out_of_reach_are_refused() {
        let mut app = server(GameMode::Creative);
        // reach is measured from the eye, 1.85 up, to the nearest point of the block, and
        // stretched for a client that's moved since it aimed
        assert_refused(&mut app, I64Vec3::new(10, 0, 0), BlockType::Stone);
        assert_refused(&mut app, I64Vec3::new(0, 12, 0), BlockType::Stone);
        let (changed, _) = request(&mut app, &[(I64Vec3::new(7, 0, 0), BlockType::Air)]);
        assert_eq!(1, changed.len());
    }

    #[test]
    fn test_blocks_arent_placed_into_players_or_solid_blocks() {
        let mut app = server(GameMode::Creative);
        assert_refused(&mut app, I64Vec3::new(0, 1, 0), BlockType::Stone);
        assert_refused(&mut app, I64Vec3::new(0, 2, 0), BlockType::Stone);
        assert_refused(&mut app, FLOOR, BlockType::Planks);
        // nor is air or water placed, which aren't building blocks
        assert_refused(&mut app, FLOOR + I64Vec3::Y, BlockType::Water);
        // but a block can go just beside the player
        let (changed, _) = request(&mut app, &[(I64Vec3::new(1, 1, 0), BlockType::Stone)]);
        assert_eq!(1, changed.len());
    }

    #[test]
    fn test_blocks_placed_into_plants_replace_them() {
        let mut app = server(GameMode::Creative);
        let above = FLOOR + I64Vec3::Y;
        let (changed, rejected) = request(&mut app, &[(above, BlockType::TallGrass)]);
        assert_eq!((1, 0), (changed.len(), rejected.len()));
        let (changed, _) = request(&mut app, &[(above, BlockType::Cobblestone)]);
        assert_eq!(1, changed.len());
        assert_eq!(BlockType::Cobblestone, block_at(&app, above));
    }

    #[test]
    fn test_plants_only_go_on_solid_ground() {
        let mut app = server(GameMode::Creative);
        // in the air over the floor, and on top of another plant
        assert_refused(&mut app, FLOOR + I64Vec3::Y * 2, BlockType::Poppy);
        request(&mut app, &[(FLOOR + I64Vec3::Y, BlockType::Poppy)]);
        assert_refused(&mut app, FLOOR + I64Vec3::Y * 2, BlockType::Dandelion);
    }

    #[test]
    fn test_breaking_the_ground_under_a_plant_takes_the_plant_too() {
        let mut app = server(GameMode::Creative);
        let above = FLOOR + I64Vec3::Y;
        request(&mut app, &[(above, BlockType::Cornflower)]);
        let (changed, _) = request(&mut app, &[(FLOOR, BlockType::Air)]);
        let gone = BlockChanged {
            block: above,
            to: BlockType::Air.into(),
            by: None,
        };
        assert_eq!(2, changed.len());
        assert_eq!(gone, changed[1]);
        assert_eq!(BlockType::Air, block_at(&app, above));
        // while breaking the plant itself leaves the ground
        request(
            &mut app,
            &[(FLOOR, BlockType::Stone), (above, BlockType::Poppy)],
        );
        request(&mut app, &[(above, BlockType::Air)]);
        assert_eq!(BlockType::Stone, block_at(&app, FLOOR));
    }

    #[test]
    fn test_torches_stand_on_solid_ground_and_drop_when_it_goes() {
        let mut app = server(GameMode::Survival);
        let above = FLOOR + I64Vec3::Y;
        inventory(&mut app).slots[0] = Some(ItemStack::new(Item::Block(BlockType::Torch), 2));
        assert_refused(&mut app, above + I64Vec3::Y, BlockType::Torch);
        let (changed, _) = request(&mut app, &[(above, BlockType::Torch)]);
        assert_eq!(1, changed.len());
        // nor does a torch hold up another, or count as ground for anything placed on it
        assert_refused(&mut app, above + I64Vec3::Y, BlockType::Torch);
        assert_refused(&mut app, above, BlockType::Stone);
        holding(&mut app, Item::WoodenPickaxe);
        hold_and_break(&mut app, FLOOR, 1.9);
        assert_eq!(BlockType::Air, block_at(&app, above));
        let mut items: Vec<_> = dropped(&mut app).into_iter().map(|(s, _)| s).collect();
        items.sort_unstable_by_key(|stack| stack.item.name());
        let one = |block| ItemStack::new(Item::Block(block), 1);
        assert_eq!(
            vec![one(BlockType::Cobblestone), one(BlockType::Torch)],
            items
        );
    }

    // Asks for `block` at `at`, by its text, and returns the changes the server sent.
    fn place(app: &mut App, at: I64Vec3, block: &str) -> Vec<BlockChanged> {
        app.world_mut().write_message(RequestEdit {
            block: at,
            to: block.parse().unwrap(),
            slot: 0,
            seq: 0,
        });
        app.update();
        drain::<EditRejected>(app);
        drain(app)
    }

    fn block_with_state(app: &App, at: I64Vec3) -> String {
        app.world().resource::<World>().get(at).to_string()
    }

    #[test]
    fn test_a_door_is_placed_with_its_upper_block_and_broken_and_opened_whole() {
        let mut app = server(GameMode::Creative);
        let (lower, upper) = (FLOOR + I64Vec3::Y, FLOOR + I64Vec3::new(0, 2, 0));
        // only its lower block can be asked for
        assert!(place(&mut app, lower, "door[half=top]").is_empty());
        let changed = place(&mut app, lower, "door[facing=east]");
        assert_eq!(2, changed.len());
        assert_eq!("door[facing=east,half=top]", block_with_state(&app, upper));
        app.world_mut().write_message(UseBlock { block: upper });
        app.update();
        assert_eq!(2, drain::<BlockChanged>(&mut app).len());
        assert_eq!("door[facing=east,open=true]", block_with_state(&app, lower));
        assert_eq!(
            "door[facing=east,half=top,open=true]",
            block_with_state(&app, upper)
        );
        let changed = place(&mut app, upper, "air");
        assert_eq!(2, changed.len());
        assert_eq!(BlockType::Air, block_at(&app, lower));
        // and a door doesn't fit under a block
        app.world_mut()
            .resource_mut::<World>()
            .edit(upper, BlockType::Stone);
        assert!(place(&mut app, lower, "door").is_empty());
    }

    #[test]
    fn test_ladders_need_a_wall_and_lanterns_a_floor_or_ceiling_and_go_with_them() {
        let mut app = server(GameMode::Survival);
        inventory(&mut app).slots[0] = Some(ItemStack::new(Item::Block(BlockType::Ladder), 4));
        let beside = FLOOR + I64Vec3::new(0, 1, 1);
        {
            let mut world = app.world_mut().resource_mut::<World>();
            world.edit(FLOOR + I64Vec3::Y, BlockType::Planks);
        }
        // against the planks to its north, not floating with nothing behind it
        assert!(place(&mut app, beside, "ladder[facing=north]").is_empty());
        assert_eq!(1, place(&mut app, beside, "ladder[facing=south]").len());
        // the wall broken, the ladder goes and leaves itself
        holding(&mut app, Item::WoodenAxe);
        hold_and_break(&mut app, FLOOR + I64Vec3::Y, 1.0);
        assert_eq!(BlockType::Air, block_at(&app, beside));
        let items: Vec<_> = dropped(&mut app).into_iter().map(|(s, _)| s.item).collect();
        assert!(items.contains(&Item::Block(BlockType::Ladder)), "{items:?}");
        let mut app = server(GameMode::Creative);
        let over = FLOOR + I64Vec3::Y;
        assert!(place(&mut app, over + I64Vec3::Y, "lantern").is_empty());
        assert_eq!(1, place(&mut app, over, "lantern").len());
        let ceiling = FLOOR + I64Vec3::new(1, 3, 0);
        app.world_mut()
            .resource_mut::<World>()
            .edit(ceiling, BlockType::Stone);
        let under = ceiling - I64Vec3::Y;
        assert!(place(&mut app, under, "lantern").is_empty());
        assert_eq!(1, place(&mut app, under, "lantern[hanging=true]").len());
    }

    fn inventory(app: &mut App) -> Mut<'_, Inventory> {
        let mut query = app.world_mut().query::<&mut Inventory>();
        query.single_mut(app.world_mut()).unwrap()
    }

    fn dropped(app: &mut App) -> Vec<(ItemStack, Vec3)> {
        let mut query = app.world_mut().query::<(&DroppedItem, &Transform)>();
        query
            .iter(app.world())
            .map(|(item, at)| (item.stack, at.translation))
            .collect()
    }

    #[test]
    fn test_survival_places_what_it_holds_and_uses_it_up() {
        let mut app = server(GameMode::Survival);
        let above = FLOOR + I64Vec3::Y;
        // with nothing in the hotbar slot placed from, or something else
        assert_refused(&mut app, above, BlockType::Bricks);
        inventory(&mut app).slots[0] = Some(ItemStack::new(Item::Block(BlockType::Sand), 2));
        assert_refused(&mut app, above, BlockType::Bricks);

        inventory(&mut app).slots[0] = Some(ItemStack::new(Item::Block(BlockType::Bricks), 2));
        let (changed, _) = request(&mut app, &[(above, BlockType::Bricks)]);
        assert_eq!(1, changed.len());
        assert_eq!(1, inventory(&mut app).slots[0].unwrap().count);
        request(&mut app, &[(above + I64Vec3::Y, BlockType::Bricks)]);
        assert_eq!(None, inventory(&mut app).slots[0]);
        assert_refused(&mut app, above + I64Vec3::Y * 2, BlockType::Bricks);
    }

    #[test]
    fn test_creative_places_anything_and_never_runs_out() {
        let mut app = server(GameMode::Creative);
        *inventory(&mut app) = Inventory::starting(GameMode::Creative);
        let before = inventory(&mut app).clone();
        // the first slot holds stone, which this isn't
        let (changed, _) = request(&mut app, &[(FLOOR + I64Vec3::Y, BlockType::Furnace)]);
        assert_eq!(1, changed.len());
        request(&mut app, &[(FLOOR + I64Vec3::Y * 2, BlockType::Stone)]);
        assert_eq!(before, *inventory(&mut app));
    }

    // Lets `seconds` pass on the server, a tenth at a time, as a frame's time is capped.
    fn pass(app: &mut App, seconds: f32) {
        let tenth = Duration::from_millis(100);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(tenth));
        for _ in 0..(seconds * 10.0).round() as usize {
            app.update();
        }
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    }

    // Starts breaking `block`, holds it for `seconds` and asks for it to be broken.
    fn hold_and_break(
        app: &mut App,
        block: I64Vec3,
        seconds: f32,
    ) -> (Vec<BlockChanged>, Vec<EditRejected>) {
        app.world_mut().write_message(StartBreaking { block });
        app.update();
        pass(app, seconds);
        request(app, &[(block, BlockType::Air)])
    }

    fn holding(app: &mut App, item: Item) {
        inventory(app).slots[0] = Some(ItemStack::new(item, 1));
    }

    #[test]
    fn test_breaking_in_survival_drops_what_the_block_drops() {
        let mut app = server(GameMode::Survival);
        let poppy = FLOOR + I64Vec3::new(1, 1, 0);
        {
            let mut world = app.world_mut().resource_mut::<World>();
            world.edit(FLOOR + I64Vec3::X, BlockType::Grass);
            world.edit(poppy, BlockType::Poppy);
        }
        // stone leaves cobblestone, grass dirt, and the poppy on it nothing
        holding(&mut app, Item::WoodenPickaxe);
        hold_and_break(&mut app, FLOOR, 1.9);
        hold_and_break(&mut app, FLOOR + I64Vec3::X, 0.9);
        let mut items: Vec<_> = dropped(&mut app);
        items.sort_unstable_by(|a, b| a.1.x.total_cmp(&b.1.x));
        let one = |block| ItemStack::new(Item::Block(block), 1);
        assert_eq!(
            vec![one(BlockType::Cobblestone), one(BlockType::Dirt)],
            items.iter().map(|(stack, _)| *stack).collect::<Vec<_>>()
        );
        // from where the block was
        assert!(items[0].1.distance(Vec3::new(3.5, 0.375, 0.5)) < 0.5);
        assert_eq!(BlockType::Air, block_at(&app, poppy));
    }

    #[test]
    fn test_breaking_in_survival_is_refused_until_its_been_held_long_enough() {
        let mut app = server(GameMode::Survival);
        // stone takes 7.5 / 4 seconds with a wooden pickaxe
        holding(&mut app, Item::WoodenPickaxe);
        assert_refused(&mut app, FLOOR, BlockType::Air);
        let (changed, rejected) = hold_and_break(&mut app, FLOOR, 1.5);
        assert_eq!((0, 1), (changed.len(), rejected.len()));
        // held on, it's long enough, give or take the slack for the network
        pass(&mut app, 0.3);
        let (changed, _) = request(&mut app, &[(FLOOR, BlockType::Air)]);
        assert_eq!(1, changed.len());
        // nor does the time held count again for a block put back there
        let mut query = app.world_mut().query::<&mut GameMode>();
        *query.single_mut(app.world_mut()).unwrap() = GameMode::Creative;
        request(&mut app, &[(FLOOR, BlockType::Stone)]);
        let mut query = app.world_mut().query::<&mut GameMode>();
        *query.single_mut(app.world_mut()).unwrap() = GameMode::Survival;
        assert_refused(&mut app, FLOOR, BlockType::Air);
        // and timing another block doesn't count for this one
        let next = FLOOR + I64Vec3::X;
        app.world_mut().write_message(StartBreaking {
            block: next + I64Vec3::X,
        });
        app.update();
        pass(&mut app, 2.0);
        assert_refused(&mut app, next, BlockType::Air);
    }

    #[test]
    fn test_plants_break_at_a_touch_in_survival() {
        let mut app = server(GameMode::Survival);
        app.world_mut()
            .resource_mut::<World>()
            .edit(FLOOR + I64Vec3::Y, BlockType::Dandelion);
        let (changed, _) = request(&mut app, &[(FLOOR + I64Vec3::Y, BlockType::Air)]);
        assert_eq!(1, changed.len());
    }

    #[test]
    fn test_a_block_broken_without_the_tier_it_needs_drops_nothing() {
        let mut app = server(GameMode::Survival);
        // stone by hand, and with an axe, takes as long as by hand
        let (changed, _) = hold_and_break(&mut app, FLOOR, 7.5);
        assert_eq!(1, changed.len());
        holding(&mut app, Item::IronAxe);
        let (changed, _) = hold_and_break(&mut app, FLOOR + I64Vec3::X, 7.5);
        assert_eq!(1, changed.len());
        // and iron ore with a stone pickaxe
        holding(&mut app, Item::StonePickaxe);
        let ore = FLOOR + I64Vec3::X * 2;
        app.world_mut()
            .resource_mut::<World>()
            .edit(ore, BlockType::IronOre);
        let (changed, _) = hold_and_break(&mut app, ore, 9.0);
        assert_eq!(1, changed.len());
        assert!(dropped(&mut app).is_empty());
    }

    #[test]
    fn test_breaking_wears_the_tool_held_until_it_breaks() {
        let mut app = server(GameMode::Survival);
        holding(&mut app, Item::IronPickaxe);
        hold_and_break(&mut app, FLOOR, 0.75);
        assert_eq!(1, inventory(&mut app).slots[0].unwrap().wear);
        // a plant broken with it doesn't wear it
        let plant = FLOOR + I64Vec3::new(1, 1, 0);
        app.world_mut()
            .resource_mut::<World>()
            .edit(plant, BlockType::Poppy);
        request(&mut app, &[(plant, BlockType::Air)]);
        assert_eq!(1, inventory(&mut app).slots[0].unwrap().wear);
        // and the last of its durability breaks it
        inventory(&mut app).slots[0].as_mut().unwrap().wear = 255;
        hold_and_break(&mut app, FLOOR + I64Vec3::X, 0.75);
        assert_eq!(None, inventory(&mut app).slots[0]);
        // having broken the stone it wore out on, as it did the first
        let stones = dropped(&mut app)
            .iter()
            .map(|(stack, _)| stack.count)
            .sum::<u8>();
        assert_eq!(2, stones);
    }

    // Puts `block` at `at`, as if it had always been there.
    fn put(app: &mut App, at: I64Vec3, block: BlockType) {
        app.world_mut().resource_mut::<World>().edit(at, block);
    }

    #[test]
    fn test_a_hoe_tills_dirt_or_grass_with_air_above_and_wears() {
        let mut app = server(GameMode::Survival);
        let (grass, dirt) = (FLOOR, FLOOR + I64Vec3::X);
        put(&mut app, grass, BlockType::Grass);
        put(&mut app, dirt, BlockType::Dirt);
        put(&mut app, dirt + I64Vec3::Y, BlockType::TallGrass);
        // not by hand, nor with a shovel
        assert_refused(&mut app, grass, BlockType::Farmland);
        holding(&mut app, Item::WoodenShovel);
        assert_refused(&mut app, grass, BlockType::Farmland);
        holding(&mut app, Item::WoodenHoe);
        let (changed, _) = request(&mut app, &[(grass, BlockType::Farmland)]);
        assert_eq!(1, changed.len());
        assert_eq!(BlockType::Farmland, block_at(&app, grass));
        assert_eq!(1, inventory(&mut app).slots[0].unwrap().wear);
        // nor with something on it, nor what isn't dirt or grass
        assert_refused(&mut app, dirt, BlockType::Farmland);
        assert_refused(&mut app, FLOOR + I64Vec3::X * 2, BlockType::Farmland);
        assert_eq!(1, inventory(&mut app).slots[0].unwrap().wear);
    }

    #[test]
    fn test_seeds_are_planted_only_on_farmland_and_used_up() {
        let mut app = server(GameMode::Survival);
        let on_stone = FLOOR + I64Vec3::Y;
        let farmland = FLOOR + I64Vec3::X;
        put(&mut app, farmland, BlockType::Farmland);
        inventory(&mut app).slots[0] = Some(ItemStack::new(Item::Seeds, 2));
        assert_refused(&mut app, on_stone, BlockType::Wheat0);
        // nor is wheat placed as a block from seeds' slot as anything but its first stage
        assert_refused(&mut app, farmland + I64Vec3::Y, BlockType::RipeWheat);
        let (changed, _) = request(&mut app, &[(farmland + I64Vec3::Y, BlockType::Wheat0)]);
        assert_eq!(1, changed.len());
        assert_eq!(1, inventory(&mut app).slots[0].unwrap().count);
        let planted = app
            .world()
            .resource::<Crops>()
            .planted(farmland + I64Vec3::Y);
        assert_eq!(Some(app.world().resource::<TimeOfDay>().0), planted);
        // and a crop isn't built over
        *inventory(&mut app) = Inventory::default();
        inventory(&mut app).slots[0] = Some(ItemStack::new(Item::Block(BlockType::Dirt), 1));
        assert_refused(&mut app, farmland + I64Vec3::Y, BlockType::Dirt);
    }

    #[test]
    fn test_wheat_drops_by_its_ripeness_and_goes_with_its_farmland() {
        let mut app = server(GameMode::Survival);
        let (ripe, unripe) = (FLOOR + I64Vec3::new(1, 1, 0), FLOOR + I64Vec3::new(2, 1, 0));
        for at in [ripe, unripe] {
            put(&mut app, at - I64Vec3::Y, BlockType::Farmland);
        }
        put(&mut app, ripe, BlockType::RipeWheat);
        put(&mut app, unripe, BlockType::Wheat4);
        app.world_mut().resource_mut::<Crops>().plant(unripe, 0.0);
        request(&mut app, &[(ripe, BlockType::Air)]);
        let count = |app: &mut App, item| {
            let stacks = dropped(app).into_iter().map(|(stack, _)| stack);
            stacks
                .filter(|s| s.item == item)
                .map(|s| s.count)
                .sum::<u8>()
        };
        let seeds = count(&mut app, Item::Seeds);
        assert!((1..=3).contains(&seeds));
        assert_eq!(1, count(&mut app, Item::Wheat));

        // the farmland under the other broken takes the crop, which leaves its seed, and the
        // farmland dirt
        let (changed, _) = hold_and_break(&mut app, unripe - I64Vec3::Y, 0.75);
        assert_eq!(2, changed.len());
        assert_eq!(BlockType::Air, block_at(&app, unripe));
        assert_eq!(None, app.world().resource::<Crops>().planted(unripe));
        assert_eq!(seeds + 1, count(&mut app, Item::Seeds));
        assert_eq!(1, count(&mut app, Item::Block(BlockType::Dirt)));
    }

    #[test]
    fn test_farmland_built_on_is_dirt_again() {
        let mut app = server(GameMode::Creative);
        let farmland = FLOOR + I64Vec3::X;
        put(&mut app, farmland, BlockType::Farmland);
        put(&mut app, FLOOR + I64Vec3::X * 2, BlockType::Farmland);
        // a torch isn't solid, so it leaves it be
        request(
            &mut app,
            &[(FLOOR + I64Vec3::new(2, 1, 0), BlockType::Torch)],
        );
        let (changed, _) = request(&mut app, &[(farmland + I64Vec3::Y, BlockType::Planks)]);
        assert_eq!(2, changed.len());
        assert_eq!(BlockType::Dirt, block_at(&app, farmland));
        assert_eq!(BlockType::Farmland, block_at(&app, FLOOR + I64Vec3::X * 2));
    }

    #[test]
    fn test_breaking_in_creative_drops_nothing() {
        let mut app = server(GameMode::Creative);
        let (changed, _) = request(&mut app, &[(FLOOR, BlockType::Air)]);
        assert_eq!(1, changed.len());
        assert!(dropped(&mut app).is_empty());
    }

    #[test]
    fn test_edits_beyond_the_world_or_its_loaded_chunks_are_refused() {
        let mut app = server(GameMode::Creative);
        // to -x, in reach of the player at the edge of the loaded chunks, in the first frame,
        // before the server has generated the chunks round the player
        let edits = [
            (I64Vec3::new(-1, 1, 0), BlockType::Stone),
            (I64Vec3::new(-1, 0, 0), BlockType::Air),
        ];
        let (changed, rejected) = request(&mut app, &edits);
        assert!(changed.is_empty(), "{changed:?}");
        assert_eq!(2, rejected.len());
        assert_refused(&mut app, I64Vec3::new(0, -1, 0), BlockType::Air);
    }

    #[test]
    fn test_edits_that_change_nothing_are_refused() {
        let mut app = server(GameMode::Creative);
        assert_refused(&mut app, FLOOR + I64Vec3::Y, BlockType::Air);
    }

    #[test]
    fn test_a_client_making_edits_too_fast_is_refused_until_its_allowance_refills() {
        let mut app = server(GameMode::Creative);
        // building and breaking a block over and over, faster than anyone clicks
        let block = FLOOR + I64Vec3::Y;
        let flurry: Vec<_> = (0..EDIT_BURST as usize + 5)
            .map(|i| (block, [BlockType::Stone, BlockType::Air][i % 2]))
            .collect();
        let (changed, rejected) = request(&mut app, &flurry);
        assert_eq!(EDIT_BURST as usize, changed.len());
        assert_eq!(5, rejected.len());
        assert_eq!(EDIT_BURST as u32, rejected[0].seq);

        // a tenth of a second gives back two
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            100,
        )));
        let (changed, rejected) = request(&mut app, &flurry[..3]);
        assert_eq!((2, 1), (changed.len(), rejected.len()));
    }
}
