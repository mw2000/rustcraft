use bevy::prelude::*;

use crate::clock::TimeOfDay;
use crate::protocol::{TimeSync, WindClock};

const WIND_BACK_KEY: KeyCode = KeyCode::BracketLeft;
const WIND_FORWARD_KEY: KeyCode = KeyCode::BracketRight;

/// Asks the server to wind the clock back or forward while `[` or `]` is held, and to stop
/// when it's let go.
pub fn wind_clock(
    keys: Res<ButtonInput<KeyCode>>,
    mut requests: MessageWriter<WindClock>,
    mut asked: Local<i8>,
) {
    let wound = keys.pressed(WIND_FORWARD_KEY) as i8 - keys.pressed(WIND_BACK_KEY) as i8;
    if wound != *asked {
        *asked = wound;
        requests.write(WindClock(wound));
    }
}

/// Keeps a client's clock to the server's: set by each `TimeSync`, and run in between at the
/// rate it gave, so the sky moves smoothly. It stands still until the first. Not for the
/// authority, whose clock this is.
pub fn follow_clock(
    mut syncs: MessageReader<TimeSync>,
    time: Res<Time>,
    mut clock: ResMut<TimeOfDay>,
    mut rate: Local<f32>,
) {
    if let Some(sync) = syncs.read().last() {
        clock.0 = sync.time;
        *rate = sync.rate;
    }
    clock.0 += time.delta_secs() * *rate;
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin;
    use bevy::time::TimeUpdateStrategy;
    use bevy_replicon::prelude::*;

    use super::{follow_clock, wind_clock, WIND_FORWARD_KEY};
    use crate::clock::{TimeOfDay, DAY_SECONDS, START_TIME};
    use crate::protocol::{ProtocolPlugin, TimeSync};
    use crate::server::{is_authority, ServerPlugin};
    use crate::world::World;

    // As long as Bevy lets a frame be.
    const FRAME: f32 = 0.25;

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
        .init_resource::<ButtonInput<KeyCode>>()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
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

    fn time(app: &App) -> f32 {
        app.world().resource::<TimeOfDay>().0
    }

    #[test]
    fn test_a_client_keeps_to_the_servers_time_between_syncs() {
        let mut app = app(false);
        app.world_mut()
            .resource_mut::<NextState<ClientState>>()
            .set(ClientState::Connected);
        app.update();
        app.update();
        // not told the time yet
        assert_eq!(START_TIME, time(&app));

        let rate = 0.01;
        app.world_mut().write_message(TimeSync { time: 2.25, rate });
        app.update();
        let synced = time(&app);
        assert!((synced - 2.25 - rate * FRAME).abs() < 1e-5, "{synced}");
        app.update();
        assert!((time(&app) - synced - rate * FRAME).abs() < 1e-5);
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
                keys.press(WIND_FORWARD_KEY);
            }
            let before = time(&app);
            app.update();
            steps.push(time(&app) - before);
        }
        for step in &steps[..31] {
            assert!((step - FRAME / DAY_SECONDS).abs() < 1e-6, "{steps:?}");
        }
        // and winding it, from the frame after the key's pressed, as the server hears of it at
        // the end of that one
        for step in &steps[31..] {
            assert!((step - steps[31]).abs() < 1e-5, "{steps:?}");
        }
        assert!(steps[31] > 0.02, "{steps:?}");
    }
}
