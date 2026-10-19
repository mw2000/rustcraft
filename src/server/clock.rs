use std::time::Duration;

use bevy::prelude::*;
use bevy_replicon::prelude::*;

use super::Operators;
use crate::clock::{SubTick, TimeOfDay, DAY_TICKS, TICKS_PER_SECOND};
use crate::player::name::PlayerName;
use crate::player::PlayerOwner;
use crate::protocol::{CommandOutput, TimeSync, WindClock};

// How often clients are told the time, between which they run their clocks themselves. Their
// frames add up to a little more or less than the server's, so this keeps them from drifting.
const SYNC_INTERVAL: Duration = Duration::from_secs(5);
// How fast `[` and `]` wind the clock, in ticks a second: a tenth of a day.
const WIND_SPEED: f32 = DAY_TICKS as f32 / 10.0;
// How fast the clock runs unwound, in ticks a second.
const DAY_RATE: f32 = TICKS_PER_SECOND as f32;

/// How fast the clock runs, in ticks a second, backwards while it's wound back.
#[derive(Resource, PartialEq)]
pub struct ClockRate(f32);

impl Default for ClockRate {
    fn default() -> Self {
        Self(DAY_RATE)
    }
}

/// Moves the clock on with the game's time, by the whole ticks the frames add up to: the part
/// of a tick left over is carried in `SubTick` to the next frame, and draws the sky between.
/// Crops grow by the ticks it counts.
pub fn run_clock(
    time: Res<Time>,
    rate: Res<ClockRate>,
    mut clock: ResMut<TimeOfDay>,
    mut sub: ResMut<SubTick>,
) {
    clock.run(&mut sub, time.delta_secs_f64() * f64::from(rate.0));
}

/// Winds the clock back or forward as operators ask, for looking at the sky at any hour, or
/// lets it run at its own pace again. The last to ask has their way, as one player winds it at a
/// time. Anyone else is told, as they start winding, that they can't, as it's everyone's clock.
pub(super) fn wind_clock(
    mut requests: MessageReader<FromClient<WindClock>>,
    operators: Res<Operators>,
    players: Query<(&PlayerOwner, &PlayerName)>,
    mut rate: ResMut<ClockRate>,
    mut output: MessageWriter<ToClients<CommandOutput>>,
) {
    for &FromClient {
        client_id: client,
        message: WindClock(wind),
    } in requests.read()
    {
        let allowed = client == ClientId::Server
            || players
                .iter()
                .any(|(owner, name)| owner.0 == client && operators.allow(client, name));
        if !allowed {
            if wind != 0 {
                output.write(ToClients {
                    targets: SendTargets::Single(client),
                    message: CommandOutput {
                        text: "Only operators can change the time".into(),
                        error: true,
                    },
                });
            }
            continue;
        }
        let wound = f32::from(wind.clamp(-1, 1));
        rate.set_if_neq(ClockRate(DAY_RATE + wound * WIND_SPEED));
    }
}

/// Tells every client the time and how fast it runs, every `SYNC_INTERVAL`, as soon as the
/// rate changes, so the clock's wound for everyone at once, and as soon as a client joins.
pub(super) fn sync_clock(
    time: Res<Time>,
    clock: Res<TimeOfDay>,
    sub: Res<SubTick>,
    rate: Res<ClockRate>,
    joined: Query<(), Added<AuthorizedClient>>,
    mut next: Local<Duration>,
    mut syncs: MessageWriter<ToClients<TimeSync>>,
) {
    if time.elapsed() < *next && !rate.is_changed() && joined.is_empty() {
        return;
    }
    *next = time.elapsed() + SYNC_INTERVAL;
    syncs.write(ToClients {
        targets: SendTargets::All,
        message: TimeSync {
            time: clock.0,
            sub_tick: sub.0,
            rate: rate.0,
        },
    });
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::ecs::message::Messages;
    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;
    use bevy_replicon::prelude::*;

    use super::{DAY_RATE, WIND_SPEED};
    use crate::clock::{SubTick, TimeOfDay, DAY_TICKS};
    use crate::protocol::{ProtocolPlugin, TimeSync, WindClock};
    use crate::server::ServerPlugin;
    use crate::world::World;

    // As long as Bevy lets a frame be, so the frames add up to a sync's interval.
    const FRAME: Duration = Duration::from_millis(250);

    // A server alone, started.
    fn server() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins,
            ProtocolPlugin,
            ServerPlugin,
        ))
        .insert_resource(World::new(16))
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
        app.finish();
        app.update();
        app
    }

    fn time(app: &App) -> u64 {
        app.world().resource::<TimeOfDay>().0
    }

    // The clock as it's drawn, in ticks, with the part of the next.
    fn drawn(app: &App) -> f64 {
        time(app) as f64 + f64::from(app.world().resource::<SubTick>().0)
    }

    // The syncs sent since last asked, which with no transport come straight back.
    fn syncs(app: &mut App) -> Vec<TimeSync> {
        let mut syncs = app.world_mut().resource_mut::<Messages<TimeSync>>();
        syncs.drain().collect()
    }

    #[test]
    fn test_only_the_authority_runs_the_clock() {
        let mut app = server();
        let started = time(&app);
        app.update();
        assert!(time(&app) > started);

        // as a client connected to a server elsewhere, whose clock it is
        app.world_mut()
            .resource_mut::<NextState<ClientState>>()
            .set(ClientState::Connected);
        app.update();
        let connected = time(&app);
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(connected, time(&app));
    }

    #[test]
    fn test_the_clock_counts_whole_ticks_however_many_days_have_passed() {
        // a 30th of a second, a dedicated server's frame, is two thirds of a tick
        let frame = Duration::from_secs_f64(1.0 / 30.0);
        for days in [0, 1_000, 1_000_000, 100_000_000_000] {
            let mut app = server();
            app.insert_resource(TimeUpdateStrategy::ManualDuration(frame));
            app.world_mut().resource_mut::<TimeOfDay>().0 = days * DAY_TICKS;
            // the first frame at the new pace
            app.update();
            let start = time(&app);
            // as it's drawn, since then, which an `f64` holds finely at any day
            let since = |app: &App| {
                let sub = app.world().resource::<SubTick>().0;
                (time(app) - start) as f64 + f64::from(sub)
            };
            let mut last = since(&app);
            for _ in 0..300 {
                app.update();
                // every frame moves it on, as it's drawn, by as much as the rate says
                let step = since(&app) - last;
                let expected = frame.as_secs_f64() * f64::from(DAY_RATE);
                assert!((step - expected).abs() < 1e-4, "{days}: {step}");
                last = since(&app);
            }
            // 300 frames of 2/3 of a tick are 200 whole ticks, give or take the part carried
            let ran = time(&app) - start;
            assert!((199..=201).contains(&ran), "{days}: {ran}");
        }
    }

    #[test]
    fn test_sleeping_through_the_night_and_syncing_at_large_days_keep_the_whole_clock() {
        let mut app = server();
        let night = TimeOfDay::from_days(1_000_000.9);
        app.world_mut().insert_resource(night);
        app.update();
        assert!(app.world().resource::<TimeOfDay>().is_night());
        app.world_mut().write_message(WindClock(1));
        app.update();
        let sync = syncs(&mut app).pop().unwrap();
        // a frame or two of ticks on, and no more
        assert!(sync.time - night.0 < 100, "{}", sync.time);
    }

    #[test]
    fn test_clients_are_told_the_time_every_few_seconds_and_when_its_wound() {
        let mut app = server();
        assert_eq!(1, syncs(&mut app).len());
        let frames = (super::SYNC_INTERVAL.as_secs_f32() / FRAME.as_secs_f32()) as usize;
        for _ in 1..frames {
            app.update();
        }
        assert!(syncs(&mut app).is_empty());
        app.update();
        let sync = syncs(&mut app);
        assert_eq!(
            vec![time(&app)],
            sync.iter().map(|s| s.time).collect::<Vec<_>>()
        );
        assert_eq!(DAY_RATE, sync[0].rate);

        // winding is told at once, and runs the clock at that rate
        app.world_mut().write_message(WindClock(1));
        app.update();
        let wound = syncs(&mut app);
        assert_eq!(1, wound.len());
        assert_eq!(DAY_RATE + WIND_SPEED, wound[0].rate);
        app.update();
        let step = f64::from(wound[0].rate) * FRAME.as_secs_f64();
        let since = wound[0].time as f64 + f64::from(wound[0].sub_tick);
        assert!((drawn(&app) - since - step).abs() < 1e-3);
        // a request out of range winds no faster, so changes nothing
        app.world_mut().write_message(WindClock(5));
        app.update();
        assert!(syncs(&mut app).is_empty());
        app.world_mut().write_message(WindClock(0));
        app.update();
        assert_eq!(DAY_RATE, syncs(&mut app)[0].rate);
    }
}
