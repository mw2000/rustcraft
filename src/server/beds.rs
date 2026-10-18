//! Beds (§5.5): right-clicking one makes it where its user respawns, and at night lies them
//! down in it. Once everyone in the world is asleep, the night's skipped to morning.

use bevy::prelude::*;
use bevy_replicon::prelude::*;

use super::clock::ClockRate;
use super::edits::in_reach;
use crate::block::BlockType;
use crate::clock::{is_night, morning_after, TimeOfDay};
use crate::player::{Player, PlayerOwner};
use crate::protocol::{CommandOutput, UseBlock};
use crate::survival::{Asleep, SpawnBed, Vitals};
use crate::world::World;

// How long everyone must have been asleep for the night to pass, in seconds, as their screens
// fade to black.
const SLEEP_SECONDS: f32 = 2.0;
// How far a sleeper moves from where they lay down to get up, in blocks.
const GET_UP_DISTANCE: f32 = 0.5;

/// Sets the respawn point of each player who uses a bed in reach, and lies them down in it if
/// it's night.
pub(super) fn use_beds(
    mut uses: MessageReader<FromClient<UseBlock>>,
    world: Res<World>,
    clock: Res<TimeOfDay>,
    mut players: Query<(Entity, &PlayerOwner, &Transform, &Vitals, &mut SpawnBed)>,
    mut output: MessageWriter<ToClients<CommandOutput>>,
    mut commands: Commands,
) {
    for &FromClient {
        client_id,
        message: UseBlock { block },
    } in uses.read()
    {
        let Some((player, _, body, vitals, mut bed)) = players
            .iter_mut()
            .find(|(_, owner, ..)| owner.0 == client_id)
        else {
            continue;
        };
        let feet = body.translation;
        if world.get_block_at(block) != BlockType::Bed || !in_reach(feet, block) || vitals.is_dead()
        {
            continue;
        }
        bed.0 = Some(block);
        if is_night(clock.0) {
            commands.entity(player).insert(Asleep { at: feet });
        } else {
            output.write(ToClients {
                targets: SendTargets::Single(client_id),
                message: CommandOutput {
                    text: "You'll respawn at this bed. You can only sleep in it at night".into(),
                    error: false,
                },
            });
        }
    }
}

/// Wakes sleepers who've moved from where they lay down, or died.
pub(super) fn get_up(
    sleepers: Query<(Entity, &Transform, &Vitals, &Asleep)>,
    mut commands: Commands,
) {
    for (player, body, vitals, asleep) in &sleepers {
        if vitals.is_dead() || body.translation.distance(asleep.at) > GET_UP_DISTANCE {
            commands.entity(player).remove::<Asleep>();
        }
    }
}

/// Skips the night to morning once every player has been asleep for `SLEEP_SECONDS`, and wakes
/// them, telling every client the time at once.
pub(super) fn skip_night(
    time: Res<Time>,
    players: Query<(Entity, Has<Asleep>), With<Player>>,
    mut clock: ResMut<TimeOfDay>,
    mut rate: ResMut<ClockRate>,
    mut slept: Local<f32>,
    mut commands: Commands,
) {
    let everyone = !players.is_empty() && players.iter().all(|(_, asleep)| asleep);
    *slept = if everyone {
        *slept + time.delta_secs()
    } else {
        0.0
    };
    if *slept < SLEEP_SECONDS {
        return;
    }
    *slept = 0.0;
    clock.0 = morning_after(clock.0);
    // which tells clients the time, as it's the rate that's usually changed
    rate.set_changed();
    for (player, _) in &players {
        commands.entity(player).remove::<Asleep>();
    }
    info!("everyone slept through the night");
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::math::I64Vec3;
    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;
    use bevy_replicon::prelude::*;

    use super::SLEEP_SECONDS;
    use crate::block::BlockType;
    use crate::chunks::chunk::{ChunkCoordinate, ChunkData};
    use crate::clock::{is_night, TimeOfDay};
    use crate::game_mode::GameMode;
    use crate::player::{Player, PlayerOwner};
    use crate::protocol::{ProtocolPlugin, UseBlock};
    use crate::server::ServerPlugin;
    use crate::survival::{Asleep, SpawnBed};
    use crate::world::World;

    const FRAME: f32 = 0.1;
    const BED: I64Vec3 = I64Vec3::new(2, 0, 0);
    const OTHER_BED: I64Vec3 = I64Vec3::new(-2, 0, 0);

    // A server with the host and another player, standing between two beds at midnight.
    fn server() -> (App, Entity, (ClientId, Entity)) {
        let mut world = World::new(16);
        world.insert_chunk(ChunkCoordinate(I64Vec3::ZERO), ChunkData::default());
        world.insert_chunk(ChunkCoordinate(I64Vec3::NEG_X), ChunkData::default());
        world.edit(BED, BlockType::Bed);
        world.edit(OTHER_BED, BlockType::Bed);
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins,
            ProtocolPlugin,
            ServerPlugin,
        ))
        .insert_resource(world)
        .insert_resource(TimeOfDay(10.0))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
            FRAME,
        )));
        let mut spawn = |owner| {
            let body = Transform::from_xyz(0.5, 0.0, 0.5);
            let player = (Player::default(), PlayerOwner(owner), GameMode::Survival);
            app.world_mut().spawn((player, body)).id()
        };
        let host = spawn(ClientId::Server);
        let guest = ClientId::Client(Entity::from_raw_u32(7).unwrap());
        let other = spawn(guest);
        app.finish();
        app.update();
        (app, host, (guest, other))
    }

    fn sleep(app: &mut App, client_id: ClientId, block: I64Vec3) {
        app.world_mut().write_message(FromClient {
            client_id,
            message: UseBlock { block },
        });
        app.update();
    }

    fn run(app: &mut App, seconds: f32) {
        for _ in 0..(seconds / FRAME).round() as usize {
            app.update();
        }
    }

    fn time(app: &App) -> f64 {
        app.world().resource::<TimeOfDay>().0
    }

    fn asleep(app: &App, player: Entity) -> bool {
        app.world().entity(player).contains::<Asleep>()
    }

    #[test]
    fn test_the_night_passes_only_once_everyone_sleeps() {
        let (mut app, host, (guest, other)) = server();
        sleep(&mut app, ClientId::Server, BED);
        assert!(asleep(&app, host));
        assert_eq!(
            SpawnBed(Some(BED)),
            *app.world().get::<SpawnBed>(host).unwrap()
        );
        run(&mut app, SLEEP_SECONDS * 2.0);
        assert!(is_night(time(&app)) && asleep(&app, host));

        sleep(&mut app, guest, OTHER_BED);
        run(&mut app, SLEEP_SECONDS + FRAME);
        assert!(!is_night(time(&app)), "{}", time(&app));
        assert!((10.26..10.3).contains(&time(&app)), "{}", time(&app));
        assert!(!asleep(&app, host) && !asleep(&app, other));
    }

    #[test]
    fn test_getting_up_stops_the_night_passing() {
        let (mut app, host, (guest, other)) = server();
        sleep(&mut app, ClientId::Server, BED);
        sleep(&mut app, guest, OTHER_BED);
        run(&mut app, SLEEP_SECONDS / 2.0);
        app.world_mut()
            .get_mut::<Transform>(other)
            .unwrap()
            .translation
            .x += 1.0;
        run(&mut app, SLEEP_SECONDS * 2.0);
        assert!(is_night(time(&app)));
        assert!(asleep(&app, host) && !asleep(&app, other));
    }

    #[test]
    fn test_a_bed_by_day_sets_the_respawn_point_without_sleeping() {
        let (mut app, host, _) = server();
        app.world_mut().resource_mut::<TimeOfDay>().0 = 10.5;
        // nor does what isn't a bed, or a bed out of reach
        let far = BED + I64Vec3::X * 9;
        app.world_mut()
            .resource_mut::<World>()
            .edit(far, BlockType::Bed);
        sleep(&mut app, ClientId::Server, BED + I64Vec3::Y);
        sleep(&mut app, ClientId::Server, far);
        assert_eq!(SpawnBed(None), *app.world().get::<SpawnBed>(host).unwrap());
        sleep(&mut app, ClientId::Server, BED);
        assert_eq!(
            SpawnBed(Some(BED)),
            *app.world().get::<SpawnBed>(host).unwrap()
        );
        assert!(!asleep(&app, host));
    }
}
