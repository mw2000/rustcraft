//! The weather on the server (M3.5): the seed's spells, one after another as the clock runs,
//! told to clients as each starts and as they join.

use bevy::prelude::*;
use bevy_replicon::prelude::*;

use crate::clock::TimeOfDay;
use crate::protocol::WeatherSync;
use crate::weather::Weather;
use crate::world::World;

/// Moves the weather on to the seed's next spell as each ends.
pub(super) fn run_weather(world: Res<World>, clock: Res<TimeOfDay>, mut weather: ResMut<Weather>) {
    let mut next = *weather;
    if next.run(world.seed(), *clock) {
        *weather = next;
    }
}

/// Tells every client the weather as it changes, and as anyone joins. Between, each works out
/// how far its rain has got from its own clock.
pub(super) fn sync_weather(
    weather: Res<Weather>,
    joined: Query<(), Added<AuthorizedClient>>,
    mut syncs: MessageWriter<ToClients<WeatherSync>>,
) {
    if !weather.is_changed() && joined.is_empty() {
        return;
    }
    syncs.write(ToClients {
        targets: SendTargets::All,
        message: WeatherSync(*weather),
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

    use crate::clock::TimeOfDay;
    use crate::protocol::{ProtocolPlugin, WeatherSync};
    use crate::server::ServerPlugin;
    use crate::weather::Weather;
    use crate::world::World;

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
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            250,
        )));
        app.finish();
        app.update();
        app
    }

    // The syncs sent since last asked, which with no transport come straight back.
    fn syncs(app: &mut App) -> Vec<WeatherSync> {
        let mut syncs = app.world_mut().resource_mut::<Messages<WeatherSync>>();
        syncs.drain().collect()
    }

    #[test]
    fn test_the_weather_moves_on_as_each_spell_ends_and_clients_are_told() {
        let mut app = server();
        let seed = app.world().resource::<World>().seed();
        let now = *app.world().resource::<TimeOfDay>();
        app.world_mut().insert_resource(Weather::new(seed, now));
        app.update();
        syncs(&mut app);
        app.update();
        // nothing's changed, so nothing's said
        assert!(syncs(&mut app).is_empty());
        let until = app.world().resource::<Weather>().until;
        app.world_mut().insert_resource(TimeOfDay(until));
        app.update();
        let told = syncs(&mut app);
        let weather = *app.world().resource::<Weather>();
        assert_eq!(1, weather.spell);
        assert_eq!(until, weather.since);
        assert_eq!(vec![WeatherSync(weather)], told);
        // as the seed has it, whoever runs it
        let mut expected = Weather::new(seed, now);
        expected.run(seed, TimeOfDay(until));
        assert_eq!(expected, weather);
    }
}
