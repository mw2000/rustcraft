use std::time::Duration;

use bevy::prelude::*;
use bevy_replicon::prelude::*;

use super::Operators;
use crate::clock::{TimeOfDay, DAY_SECONDS};
use crate::player::name::PlayerName;
use crate::player::PlayerOwner;
use crate::protocol::{CommandOutput, TimeSync, WindClock};

// How often clients are told the time, between which they run their clocks themselves. Their
// frames add up to a little more or less than the server's, so this keeps them from drifting.
const SYNC_INTERVAL: Duration = Duration::from_secs(5);
// How fast `[` and `]` wind the clock, in days a second.
const WIND_SPEED: f32 = 0.1;
// How fast the clock runs unwound, in days a second.
const DAY_RATE: f32 = 1.0 / DAY_SECONDS;

/// How fast the clock runs, in days a second.
#[derive(Resource, PartialEq)]
pub struct ClockRate(f32);

impl Default for ClockRate {
    fn default() -> Self {
        Self(DAY_RATE)
    }
}

/// Moves the clock on with the game's time. It's counted in an `f64`, kept with what it last
/// set the clock to, as an `f32` of a thousand days can't count a frame's worth: a server left
/// running for a week would otherwise stop its clock, and crops grow by the days it counts.
pub fn run_clock(
    time: Res<Time>,
    rate: Res<ClockRate>,
    mut clock: ResMut<TimeOfDay>,
    mut counted: Local<(f64, f32)>,
) {
    let (days, set) = &mut *counted;
    // or as it's been set since, by sleeping through the night or opening a world
    if clock.0 != *set {
        *days = f64::from(clock.0);
    }
    *days += time.delta_secs_f64() * f64::from(rate.0);
    clock.0 = *days as f32;
    *set = clock.0;
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
        let wound = wind.clamp(-1, 1) as f32;
        rate.set_if_neq(ClockRate(DAY_RATE + wound * WIND_SPEED));
    }
}

/// Tells every client the time and how fast it runs, every `SYNC_INTERVAL`, as soon as the
/// rate changes, so the clock's wound for everyone at once, and as soon as a client joins.
pub(super) fn sync_clock(
    time: Res<Time>,
    clock: Res<TimeOfDay>,
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
    use crate::clock::TimeOfDay;
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

    fn time(app: &App) -> f32 {
        app.world().resource::<TimeOfDay>().0
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
    fn test_the_clock_runs_on_however_many_days_have_passed() {
        let mut app = server();
        // a 30th of a second, a dedicated server's frame, is less than half of what an `f32`
        // of 2,000 days can count
        let frame = Duration::from_secs_f32(1.0 / 30.0);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(frame));
        app.world_mut().resource_mut::<TimeOfDay>().0 = 2000.0;
        for _ in 0..300 {
            app.update();
        }
        let ran = 300.0 * frame.as_secs_f32() * DAY_RATE;
        assert!((time(&app) - (2000.0 + ran)).abs() < 2e-4, "{}", time(&app));
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
        let step = wound[0].rate * FRAME.as_secs_f32();
        assert!((time(&app) - wound[0].time - step).abs() < 1e-5);
        // a request out of range winds no faster, so changes nothing
        app.world_mut().write_message(WindClock(5));
        app.update();
        assert!(syncs(&mut app).is_empty());
        app.world_mut().write_message(WindClock(0));
        app.update();
        assert_eq!(DAY_RATE, syncs(&mut app)[0].rate);
    }
}
