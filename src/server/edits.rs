use std::time::Duration;

use bevy::math::I64Vec3;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy_replicon::prelude::*;

use crate::block::BlockType;
use crate::chunks::chunk::ChunkCoordinate;
use crate::game_mode::GameMode;
use crate::interaction::{overlaps, REACH};
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
}

type Players<'w, 's> =
    Query<'w, 's, (&'static Transform, &'static GameMode, &'static PlayerOwner), With<Player>>;

/// Makes the edits clients ask for that pass the checks, telling every client, and tells the
/// client asking for one that doesn't what's there instead.
pub(super) fn receive_edits(
    mut requests: MessageReader<FromClient<RequestEdit>>,
    mut world: ResMut<World>,
    mut allowances: ResMut<EditAllowances>,
    time: Res<Time>,
    players: Players,
    mut changed: MessageWriter<ToClients<BlockChanged>>,
    mut rejected: MessageWriter<ToClients<EditRejected>>,
) {
    for &FromClient {
        client_id: client,
        message: RequestEdit { block, to, seq },
    } in requests.read()
    {
        // every request counts, so a flood of bad ones is stopped too
        let why_not = if allowances.take(client, time.elapsed()) {
            refusal(&world, &players, client, block, to)
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
                    actual: world.get_block_at(block),
                },
            });
        } else {
            changed.write(ToClients {
                targets: SendTargets::All,
                message: BlockChanged {
                    block,
                    to,
                    by: Some((client, seq)),
                },
            });
        }
    }
}

// Why `client` can't change `block` to `to`, if it can't.
fn refusal(
    world: &World,
    players: &Players,
    client: ClientId,
    block: I64Vec3,
    to: BlockType,
) -> Option<&'static str> {
    let coord = ChunkCoordinate::containing(block);
    if !World::layers(world.height).contains(&coord.0.y) {
        return Some("it's beyond the world");
    }
    if !world.is_chunk_generated(coord) {
        return Some("its chunk isn't loaded");
    }
    let Some((feet, mode)) = player_of(client, players) else {
        return Some("they have no player");
    };
    let eye = feet.translation + Vec3::Y * EYE_HEIGHT;
    let nearest = eye.clamp(block.as_vec3(), block.as_vec3() + 1.0);
    if eye.distance(nearest) > REACH + REACH_SLACK {
        return Some("it's out of reach");
    }
    if to != BlockType::Air {
        if world.get_block_at(block).is_solid() {
            return Some("it's into a solid block");
        }
        if !mode.can_place(to) {
            return Some("they can't place that");
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

// The player `client` plays as. A search, as there are only ever a few players.
fn player_of<'a>(client: ClientId, players: &'a Players) -> Option<(&'a Transform, &'a GameMode)> {
    players
        .iter()
        .find(|(.., owner)| owner.0 == client)
        .map(|(body, mode, _)| (body, mode))
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
    use crate::player::{Player, PlayerOwner};
    use crate::protocol::{BlockChanged, EditRejected, ProtocolPlugin, RequestEdit};
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
    fn test_an_edit_in_reach_is_made_and_sent_to_every_client() {
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
    fn test_survival_places_blocks_as_creative_does_for_now() {
        let mut app = server(GameMode::Survival);
        let (changed, _) = request(&mut app, &[(FLOOR + I64Vec3::Y, BlockType::Bricks)]);
        assert_eq!(1, changed.len());
    }

    #[test]
    fn test_edits_beyond_the_world_or_its_loaded_chunks_are_refused() {
        let mut app = server(GameMode::Creative);
        assert_refused(&mut app, I64Vec3::new(0, -1, 0), BlockType::Air);
        // to -x, in reach of the player at the edge of the loaded chunks
        assert_refused(&mut app, I64Vec3::new(-1, 1, 0), BlockType::Stone);
        assert_refused(&mut app, I64Vec3::new(-1, 0, 0), BlockType::Air);
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
