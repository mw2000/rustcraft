use bevy::prelude::*;

use crate::clock::{SubTick, TimeOfDay};
use crate::controls::{Action, Controls};
use crate::protocol::{TimeSync, WindClock};

/// Asks the server to wind the clock back or forward while `[` or `]` is held, and to stop
/// when it's let go.
pub fn wind_clock(
    controls: Controls,
    mut requests: MessageWriter<WindClock>,
    mut asked: Local<i8>,
) {
    let wound =
        controls.pressed(Action::WindForward) as i8 - controls.pressed(Action::WindBack) as i8;
    if wound != *asked {
        *asked = wound;
        requests.write(WindClock(wound));
    }
}

/// Keeps a client's clock to the server's: set by each `TimeSync`, to the tick and the part of
/// the next, and run in between at the rate it gave, so the sky moves smoothly. It stands still
/// until the first. Not for the authority, whose clock this is.
pub fn follow_clock(
    mut syncs: MessageReader<TimeSync>,
    time: Res<Time>,
    mut clock: ResMut<TimeOfDay>,
    mut sub: ResMut<SubTick>,
    mut rate: Local<f32>,
) {
    if let Some(sync) = syncs.read().last() {
        clock.0 = sync.time;
        sub.0 = sync.sub_tick;
        *rate = sync.rate;
    }
    clock.run(&mut sub, time.delta_secs_f64() * f64::from(*rate));
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;
    use bevy_replicon::prelude::*;

    use super::{follow_clock, wind_clock};
    use crate::clock::{SubTick, TimeOfDay, DAY_TICKS, TICKS_PER_SECOND};
    use crate::controls::Bindings;
    use crate::protocol::{ProtocolPlugin, TimeSync};
    use crate::server::{is_authority, ServerPlugin};
    use crate::world::World;

    // As long as Bevy lets a frame be.
    const FRAME: f64 = 0.25;
    // How far the clock runs in a frame at its own pace, in ticks.
    const FRAME_TICKS: f64 = FRAME * TICKS_PER_SECOND as f64;

    // The clock's systems as the game has them, with or without the server.
    fn app(server: bool) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            StatesPlugin,
            RepliconPlugins,
            ProtocolPlugin,
        ))
        .init_resource::<TimeOfDay>()
        .init_resource::<SubTick>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Bindings>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            FRAME,
        )))
        .add_systems(Update, (wind_clock, follow_clock.run_if(not(is_authority))));
        if server {
            app.add_plugins(ServerPlugin)
                .insert_resource(World::new(16));
        }
        app.finish();
        app
    }

    // A client connected to a server elsewhere, which hasn't told it the time yet.
    fn connected() -> App {
        let mut app = app(false);
        app.world_mut()
            .resource_mut::<NextState<ClientState>>()
            .set(ClientState::Connected);
        app.update();
        app
    }

    // The clock as it's drawn, in ticks, with the part of the next.
    fn time(app: &App) -> f64 {
        let sub = app.world().resource::<SubTick>().0;
        app.world().resource::<TimeOfDay>().0 as f64 + f64::from(sub)
    }

    // A sync to `time` and `sub_tick` at the clock's own pace.
    fn sync(time: u64, sub_tick: f32) -> TimeSync {
        TimeSync {
            time,
            sub_tick,
            rate: TICKS_PER_SECOND as f32,
        }
    }

    #[test]
    fn test_a_client_keeps_to_the_servers_time_between_syncs() {
        let mut app = connected();
        app.update();
        // not told the time yet
        assert_eq!(TimeOfDay::default(), *app.world().resource::<TimeOfDay>());

        let rate = 7.0;
        app.world_mut().write_message(TimeSync {
            rate,
            ..sync(27_000, 0.5)
        });
        app.update();
        let synced = time(&app);
        let step = f64::from(rate) * FRAME;
        assert!((synced - 27_000.5 - step).abs() < 1e-5, "{synced}");
        app.update();
        assert!((time(&app) - synced - step).abs() < 1e-5);
    }

    #[test]
    fn test_the_sky_moves_smoothly_in_singleplayer() {
        let mut app = app(true);
        app.update();
        // through a sync, every frame moves the clock on as far
        let mut steps = Vec::new();
        for frame in 0..40 {
            if frame == 30 {
                let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keys.press(KeyCode::BracketRight);
            }
            let before = time(&app);
            app.update();
            steps.push(time(&app) - before);
        }
        for step in &steps[..31] {
            assert!((step - FRAME_TICKS).abs() < 1e-4, "{steps:?}");
        }
        // and winding it, from the frame after the key's pressed, as the server hears of it at
        // the end of that one
        for step in &steps[31..] {
            assert!((step - steps[31]).abs() < 1e-3, "{steps:?}");
        }
        assert!(steps[31] > 0.02 * DAY_TICKS as f64, "{steps:?}");
    }

    #[test]
    fn test_a_client_stays_smooth_following_syncs_at_large_days() {
        for days in [1_000, 1_000_000, 100_000_000_000] {
            let mut app = connected();
            let start = days * DAY_TICKS + 3;
            app.world_mut().write_message(sync(start, 0.0));
            app.update();
            let mut last = time(&app);
            assert!((last - start as f64 - FRAME_TICKS).abs() < 1e-4, "{days}");
            for frame in 0..40 {
                if frame == 20 {
                    // the next sync, 5 seconds on, lands where the client had got to
                    let at = start + (FRAME_TICKS * 21.0) as u64;
                    app.world_mut().write_message(sync(at, 0.0));
                }
                app.update();
                let step = time(&app) - last;
                assert!((step - FRAME_TICKS).abs() < 1e-4, "{days}: {frame}: {step}");
                last = time(&app);
            }
        }
    }

    #[test]
    fn test_the_sky_moves_smoothly_in_singleplayer_at_large_days() {
        for days in [1_000, 1_000_000, 100_000_000_000] {
            let mut app = app(true);
            app.update();
            app.world_mut().resource_mut::<TimeOfDay>().0 = days * DAY_TICKS;
            app.update();
            for _ in 0..40 {
                let before = time(&app);
                app.update();
                let step = time(&app) - before;
                assert!((step - FRAME_TICKS).abs() < 1e-4, "{days}: {step}");
            }
        }
    }

    #[test]
    fn test_what_is_drawn_moves_every_frame_though_the_clock_moves_by_whole_ticks() {
        // at 120 frames a second, a sixth of a tick a frame, the time drawn moves on every
        // frame while the clock itself moves on only every sixth
        let mut app = connected();
        let frame = Duration::from_secs_f64(1.0 / 120.0);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(frame));
        app.world_mut().write_message(sync(1_000, 0.0));
        let (mut drawn, mut ticks) = (Vec::new(), Vec::new());
        for _ in 0..13 {
            app.update();
            drawn.push(time(&app));
            ticks.push(app.world().resource::<TimeOfDay>().0);
        }
        assert!(drawn.windows(2).all(|w| w[1] > w[0]), "{drawn:?}");
        assert_eq!((1_000, 1_002), (ticks[0], ticks[12]), "{ticks:?}");
    }
}
