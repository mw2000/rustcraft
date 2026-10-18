use std::time::Duration;

use bevy::math::I64Vec3;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy_replicon::prelude::*;

use super::drops::DroppedItem;
use super::sync::BlockChanges;
use crate::block::BlockType;
use crate::chunks::chunk::ChunkCoordinate;
use crate::game_mode::GameMode;
use crate::interaction::{overlaps, REACH};
use crate::inventory::{Inventory, HOTBAR_SLOTS};
use crate::items::{Item, ItemStack};
use crate::player::{Player, PlayerOwner, BODY, EYE_HEIGHT};
use crate::protocol::{BlockChanged, EditRejected, RequestEdit};
use crate::world::World;

// How much further than `REACH` from the eye an edit can be, in blocks, for a player who's
// moved since aiming. In singleplayer they haven't, as it's the same frame, so this only covers
// rounding until M0.6 adds a network's delay.
const REACH_SLACK: f32 = 1.0;
// Clicking as fast as anyone can is about 15 a second, so creative building at full speed never
// meets this, and a burst of a second's worth lets a quick flurry of clicks through whole. It only
// stops a client flooding the server with edits.
const EDITS_PER_SECOND: f32 = 20.0;
const EDIT_BURST: f32 = 20.0;

/// What each client may still edit straight away: a burst of edits, used up by each request
/// and refilled at a steady rate.
#[derive(Resource, Default)]
pub(super) struct EditAllowances(HashMap<ClientId, Allowance>);

struct Allowance {
    edits: f32,
    at: Duration,
}

impl EditAllowances {
    // Takes an edit from the client's allowance at `now`, if it has one left.
    fn take(&mut self, client: ClientId, now: Duration) -> bool {
        let allowance = self.0.entry(client).or_insert(Allowance {
            edits: EDIT_BURST,
            at: now,
        });
        let refilled = (now - allowance.at).as_secs_f32() * EDITS_PER_SECOND;
        allowance.edits = (allowance.edits + refilled).min(EDIT_BURST);
        allowance.at = now;
        let allowed = allowance.edits >= 1.0;
        if allowed {
            allowance.edits -= 1.0;
        }
        allowed
    }

    /// Forgets a client that's left.
    pub(super) fn forget(&mut self, client: ClientId) {
        self.0.remove(&client);
    }
}

type Players<'w, 's> = Query<
    'w,
    's,
    (
        &'static Transform,
        &'static GameMode,
        &'static PlayerOwner,
        &'static mut Inventory,
    ),
    With<Player>,
>;

/// Makes the edits clients ask for that pass the checks, telling the clients near them, and
/// tells the client asking for one that doesn't what's there instead. In survival a block placed
/// is taken from the player's hotbar, and one broken leaves its item to pick up.
#[allow(clippy::too_many_arguments)]
pub(super) fn receive_edits(
    mut requests: MessageReader<FromClient<RequestEdit>>,
    mut world: ResMut<World>,
    mut allowances: ResMut<EditAllowances>,
    time: Res<Time>,
    mut players: Players,
    mut changed: BlockChanges,
    mut rejected: MessageWriter<ToClients<EditRejected>>,
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
        let was = world.get_block_at(block);
        // every request counts, so a flood of bad ones is stopped too
        let why_not = if allowances.take(client, time.elapsed()) {
            refusal(&world, &players, client, block, to, slot.into())
                .or_else(|| (!world.edit(block, to)).then_some("it's already that block"))
        } else {
            Some("too many edits")
        };
        if let Some(why) = why_not {
            debug!("refused {client}'s edit of {block} to {}: {why}", to.name());
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
        let (_, mode, _, mut inventory) = players
            .iter_mut()
            .find(|(_, _, owner, _)| owner.0 == client)
            .expect("an edit allowed has a player making it");
        let drops = mode.drops_broken_blocks();
        let mut leave = |broken: BlockType, at: I64Vec3| {
            if let Some(item) = broken.drops().filter(|_| drops) {
                commands.spawn((
                    DroppedItem::from_block(ItemStack::new(item, 1), at),
                    Replicated,
                ));
            }
        };
        if to == BlockType::Air {
            leave(was, block);
        } else if !mode.has_every_block() {
            inventory.use_one(slot.into(), Item::Block(to));
        }
        // a plant goes with the ground it stood on
        let above = block + I64Vec3::Y;
        let plant = world.get_block_at(above);
        if !to.is_solid() && plant.is_plant() {
            world.edit(above, BlockType::Air);
            leave(plant, above);
            changed.write(BlockChanged {
                block: above,
                to: BlockType::Air,
                by: None,
            });
        }
    }
}

// Why `client` can't change `block` to `to`, placing it from hotbar slot `slot`, if it can't.
fn refusal(
    world: &World,
    players: &Players,
    client: ClientId,
    block: I64Vec3,
    to: BlockType,
    slot: usize,
) -> Option<&'static str> {
    let coord = ChunkCoordinate::containing(block);
    if !World::layers(world.height).contains(&coord.0.y) {
        return Some("it's beyond the world");
    }
    if !world.is_chunk_generated(coord) {
        return Some("its chunk isn't loaded");
    }
    let Some((feet, mode, _, inventory)) =
        players.iter().find(|(_, _, owner, _)| owner.0 == client)
    else {
        return Some("they have no player");
    };
    let eye = feet.translation + Vec3::Y * EYE_HEIGHT;
    let nearest = eye.clamp(block.as_vec3(), block.as_vec3() + 1.0);
    if eye.distance(nearest) > REACH + REACH_SLACK {
        return Some("it's out of reach");
    }
    if to != BlockType::Air {
        if !world.get_block_at(block).is_replaceable() {
            return Some("it's into a solid block");
        }
        if !mode.can_place(to) {
            return Some("they can't place that");
        }
        let held = inventory.slots[..HOTBAR_SLOTS].get(slot).copied().flatten();
        if !mode.has_every_block() && held.is_none_or(|stack| stack.item != Item::Block(to)) {
            return Some("they aren't holding that");
        }
        if to.is_plant() && !world.get_block_at(block - I64Vec3::Y).is_solid() {
            return Some("it's a plant with nothing solid under it");
        }
        let into_player = players
            .iter()
            .any(|(body, ..)| overlaps(BODY, body.translation, block));
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
    use crate::game_mode::GameMode;
    use crate::inventory::Inventory;
    use crate::items::{Item, ItemStack};
    use crate::player::{Player, PlayerOwner};
    use crate::protocol::{BlockChanged, EditRejected, ProtocolPlugin, RequestEdit};
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
                to,
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
            actual: before,
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
                to,
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
        // reach is measured from the eye, 1.85 up, to the nearest point of the block
        assert_refused(&mut app, I64Vec3::new(7, 0, 0), BlockType::Air);
        assert_refused(&mut app, I64Vec3::new(0, 9, 0), BlockType::Stone);
        let (changed, _) = request(&mut app, &[(I64Vec3::new(6, 0, 0), BlockType::Air)]);
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
            to: BlockType::Air,
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
        request(
            &mut app,
            &[
                (FLOOR, BlockType::Air),
                (FLOOR + I64Vec3::X, BlockType::Air),
            ],
        );
        let mut items: Vec<_> = dropped(&mut app);
        items.sort_unstable_by(|a, b| a.1.x.total_cmp(&b.1.x));
        let one = |block| ItemStack::new(Item::Block(block), 1);
        assert_eq!(
            vec![one(BlockType::Cobblestone), one(BlockType::Dirt)],
            items.iter().map(|(stack, _)| *stack).collect::<Vec<_>>()
        );
        // from where the block was
        assert_eq!(Vec3::new(3.5, 0.375, 0.5), items[0].1);
        assert_eq!(BlockType::Air, block_at(&app, poppy));
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
