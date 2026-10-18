//! Growing crops on the server (§5.5a, INV-4): each crop in a loaded chunk is looked at when
//! it's due to reach its next stage, rather than every crop every frame, and brought up to the
//! stage its planting time and the clock say, which after a long time away may be several at
//! once. Tilling and planting are edits clients ask for (`server::edits`).

use bevy::prelude::*;

use super::sync::BlockChanges;
use crate::clock::TimeOfDay;
use crate::farming::{grown, is_watered, reaches, stage, Crops, WHEAT};
use crate::protocol::BlockChanged;
use crate::world::World;

// The most crops looked at in a frame, so a big field come back into reach after a long time
// catches up over a few frames, its changes sent a few hundred at a time, rather than in one.
const CROPS_PER_FRAME: usize = 256;

/// Grows the crops that are due in the chunks the server has loaded, telling the clients near
/// them.
pub(super) fn grow_crops(
    clock: Res<TimeOfDay>,
    mut world: ResMut<World>,
    mut crops: ResMut<Crops>,
    mut changed: BlockChanges,
) {
    let now = clock.0;
    let due = crops.due(
        now,
        |coord| world.is_chunk_generated(coord),
        CROPS_PER_FRAME,
    );
    let seed = world.seed();
    for (at, planted) in due {
        // its block's been changed by something that didn't forget it, such as a command
        let Some(shown) = stage(world.get_block_at(at)) else {
            crops.grew(at, None);
            continue;
        };
        let wet = is_watered(&world, at);
        let stage = grown(seed, at, planted, now, wet).max(shown);
        if stage > shown {
            world.edit(at, WHEAT[stage]);
            changed.write(BlockChanged {
                block: at,
                to: WHEAT[stage],
                by: None,
            });
        }
        let next = (stage + 1 < WHEAT.len()).then(|| reaches(seed, at, planted, stage + 1, wet));
        crops.grew(at, next);
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

    use crate::block::BlockType;
    use crate::chunks::chunk::{ChunkCoordinate, ChunkData};
    use crate::clock::TimeOfDay;
    use crate::edits::Edits;
    use crate::farming::{grown, stage, Crops};
    use crate::protocol::{BlockChanged, ProtocolPlugin};
    use crate::server::ServerPlugin;
    use crate::world::World;

    // A row of crops along x on farmland, across two chunks, all planted at day 1, with water
    // at the end of the row, within reach of the last four.
    const ROW: i64 = 24;
    const PLANTED: f32 = 1.0;

    fn crop(i: i64) -> I64Vec3 {
        I64Vec3::new(i, 1, 3)
    }

    // A server with no players and seed 7, its chunks loaded if `loaded`. Its clock moves only as it's set.
    fn server(loaded: bool) -> App {
        let mut world = World::from_save(7, Edits::default(), 16);
        if loaded {
            load(&mut world);
        }
        let mut crops = Crops::default();
        for i in 0..ROW {
            world.edit(crop(i) - I64Vec3::Y, BlockType::Farmland);
            world.edit(crop(i), BlockType::Wheat0);
            crops.plant(crop(i), PLANTED);
        }
        world.edit(crop(ROW) - I64Vec3::Y, BlockType::Water);
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins,
            ProtocolPlugin,
            ServerPlugin,
        ))
        .insert_resource(world)
        .insert_resource(crops)
        .insert_resource(TimeOfDay(PLANTED))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
        app.finish();
        app.update();
        app
    }

    fn load(world: &mut World) {
        for x in 0..=1 {
            world.insert_chunk(ChunkCoordinate(I64Vec3::new(x, 0, 0)), ChunkData::default());
        }
    }

    // Sets the clock to `day` and lets the server run a frame.
    fn at(app: &mut App, day: f32) {
        app.world_mut().resource_mut::<TimeOfDay>().0 = day;
        app.update();
    }

    // Each crop's stage.
    fn stages(app: &App) -> Vec<usize> {
        let world = app.world().resource::<World>();
        (0..ROW)
            .map(|i| stage(world.get_block_at(crop(i))).expect("a crop's gone"))
            .collect()
    }

    fn sent(app: &mut App) -> usize {
        let mut sent = app.world_mut().resource_mut::<Messages<BlockChanged>>();
        sent.drain().count()
    }

    #[test]
    fn test_wheat_grows_stage_by_stage_quicker_by_water_and_is_let_go_of_ripe() {
        let mut app = server(true);
        let seed = app.world().resource::<World>().seed();
        let mut was = stages(&app);
        assert_eq!(vec![0; ROW as usize], was);
        for tenth in 1..=36 {
            let day = PLANTED + tenth as f32 / 10.0;
            at(&mut app, day);
            let now = stages(&app);
            // never going back, and as each crop's own time says
            assert!(was.iter().zip(&now).all(|(was, now)| was <= now));
            let dry = |i: i64| grown(seed, crop(i), PLANTED, day, false);
            assert_eq!((0..4).map(dry).collect::<Vec<_>>(), now[..4]);
            was = now;
            // half way, the watered end is well ahead of the dry
            if tenth == 12 {
                let wet: usize = was[ROW as usize - 4..].iter().sum();
                let dry: usize = was[..4].iter().sum();
                assert!(wet > dry + 4, "{wet} wet against {dry} dry");
            }
        }
        // ripe within three and a half days, with nothing more to grow
        assert_eq!(vec![7; ROW as usize], was);
        assert_eq!(0, app.world().resource::<Crops>().chunks().count());
    }

    #[test]
    fn test_a_field_left_unloaded_catches_up_to_where_it_would_have_grown() {
        let day = PLANTED + 1.3;
        let mut live = server(true);
        for hundredth in 1..=130 {
            at(&mut live, PLANTED + hundredth as f32 / 100.0);
        }
        // the clock jumped, as when the world's slept through
        let mut jumped = server(true);
        at(&mut jumped, day);
        assert_eq!(stages(&live), stages(&jumped));
        assert!(stages(&live).iter().any(|&stage| stage > 0 && stage < 7));

        // and away from everyone, nothing grows until its chunks are loaded again
        let mut away = server(false);
        sent(&mut away);
        at(&mut away, day);
        assert_eq!(0, sent(&mut away));
        load(&mut away.world_mut().resource_mut::<World>());
        assert_eq!(vec![0; ROW as usize], stages(&away));
        away.update();
        assert_eq!(stages(&live), stages(&away));
        // a change sent for each crop that's grown, all at once
        let grown = stages(&away).iter().filter(|&&stage| stage > 0).count();
        assert_eq!(grown, sent(&mut away));
    }
}
