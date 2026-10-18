//! The map (§5.7): a top-down view of the land the local player has explored, drawn from the
//! terrain's own sampling, as the `world_map` example draws the whole world. M opens it.

pub mod explored;
pub mod relief;
pub mod screen;
pub mod tiles;

use bevy::prelude::*;

use crate::client::command_bar::command_bar_closed;
use crate::controls::Bindings;
use crate::menu::GameState;
use crate::world::{LeaveWorld, LeaveWorldSystems, World};
use explored::{Explored, MapDir, MapFile};
use tiles::MapTiles;

/// The map: revealing it as the local player goes, keeping it in the `MapDir` if there is one,
/// rendering its tiles, and the map screen. Needs `MenuPlugin`'s states and `CommandPlugin`.
pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Bindings>()
            .init_resource::<Explored>()
            .init_resource::<MapView>()
            .init_resource::<MapTiles>()
            .add_systems(Startup, screen::spawn_map)
            .add_systems(
                Update,
                (
                    screen::toggle_map
                        .run_if(command_bar_closed)
                        .run_if(resource_exists::<World>),
                    explored::open_map_file
                        .run_if(resource_exists::<MapDir>)
                        .run_if(resource_added::<World>),
                    (explored::finish_loading, explored::autosave)
                        .chain()
                        .run_if(resource_exists::<MapFile>),
                    screen::mark_players,
                    (
                        explored::explore,
                        screen::follow_player.run_if(not(in_state(GameState::Map))),
                        screen::zoom_and_pan.run_if(in_state(GameState::Map)),
                        tiles::request_tiles,
                        tiles::finish_tiles,
                        (
                            tiles::place_tiles,
                            screen::place_markers,
                            screen::draw_scale_bar,
                            screen::show_cursor_coordinates,
                        )
                            .run_if(in_state(GameState::Map)),
                    )
                        .chain()
                        .run_if(resource_exists::<World>),
                ),
            )
            .add_systems(
                LeaveWorld,
                (explored::save_on_leaving, tiles::forget_tiles, forget_view)
                    .in_set(LeaveWorldSystems::Clear),
            )
            .add_systems(OnEnter(GameState::Map), screen::open)
            .add_systems(OnExit(GameState::Map), screen::close)
            .add_systems(Last, explored::save_on_exit);
    }
}

fn forget_view(mut view: ResMut<MapView>) {
    *view = MapView::default();
}

/// How many blocks each of the map's pixels covers at each zoom, from the closest in.
pub const ZOOMS: [f32; 4] = [0.5, 1.0, 2.0, 4.0];
const DEFAULT_ZOOM: usize = 1;

/// Where the map looks: the block column (x, z) at the middle of the screen, and how far it's
/// zoomed, as an index into `ZOOMS`. North, -z, is up. While the map's closed it stays on the
/// player, so the tiles round them are ready when it opens.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct MapView {
    pub centre: Vec2,
    pub zoom: usize,
}

impl Default for MapView {
    fn default() -> Self {
        Self {
            centre: Vec2::ZERO,
            zoom: DEFAULT_ZOOM,
        }
    }
}

impl MapView {
    pub fn blocks_per_pixel(&self) -> f32 {
        ZOOMS[self.zoom]
    }

    /// Which level of tiles it draws with: explored tiles while each of their pixels is at least
    /// one of the screen's, and coarser ones further out, so there are never more than a few
    /// hundred on screen.
    pub fn level(&self) -> u8 {
        u8::from(self.blocks_per_pixel() > 1.0)
    }

    /// Where block column `at` (x, z) is on a screen `screen` pixels across.
    pub fn to_screen(&self, at: Vec2, screen: Vec2) -> Vec2 {
        (at - self.centre) / self.blocks_per_pixel() + screen / 2.0
    }

    /// The block column at `at` on a screen `screen` pixels across.
    pub fn to_world(&self, at: Vec2, screen: Vec2) -> Vec2 {
        (at - screen / 2.0) * self.blocks_per_pixel() + self.centre
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_the_view_puts_its_centre_mid_screen_with_north_up() {
        let view = MapView {
            centre: Vec2::new(100.0, -50.0),
            zoom: 2,
        };
        let screen = Vec2::new(1280.0, 720.0);
        assert_eq!(screen / 2.0, view.to_screen(view.centre, screen));
        // 2 blocks a pixel, so 200 blocks north is 100 pixels up
        let north = view.to_screen(view.centre - Vec2::Y * 200.0, screen);
        assert_eq!(Vec2::new(640.0, 260.0), north);
        assert_eq!(view.centre - Vec2::Y * 200.0, view.to_world(north, screen));
    }
}
