//! The map screen: the explored land over a dark ground, an arrow for the player and the way
//! they face, markers for the others, the world's spawn and the player's bed, a scale bar and
//! the coordinates under the cursor. Scrolling zooms about the cursor and dragging pans.

use bevy::asset::RenderAssetUsages;
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::{CursorOptions, PrimaryWindow};

use super::tiles::TileLayer;
use super::{MapView, ZOOMS};
use crate::client::others::SeenPlayer;
use crate::client::player::LocalPlayer;
use crate::controls::{Action, Controls};
use crate::menu::{font, free_cursor, GameState, TEXT_SHADOW};
use crate::player::name::PlayerName;
use crate::player::PlayerLook;
use crate::survival::SpawnBed;
use crate::world::World;

// Unexplored land: dark, like old paper in shadow.
const UNEXPLORED: Color = Color::srgb(0.13, 0.12, 0.1);
// How many pixels a touchpad scrolls for a notch of a mouse's wheel.
const PIXELS_PER_NOTCH: f32 = 40.0;

const ARROW_SIZE: f32 = 20.0;
const MARKER_SIZE: f32 = 10.0;
const SPAWN_COLOUR: Color = Color::srgb(0.95, 0.78, 0.2);
const BED_COLOUR: Color = Color::srgb(0.8, 0.15, 0.15);
const PLAYER_COLOUR: Color = Color::srgb(0.3, 0.6, 1.0);
// Wide enough for any name, which is centred in it under its marker.
const LABEL_WIDTH: f32 = 200.0;

// The lengths the scale bar can show, in blocks, the longest that fits `SCALE_BAR_MAX` taken.
const SCALE_LENGTHS: [f32; 8] = [20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0];
const SCALE_BAR_MAX: f32 = 160.0;

#[derive(Component)]
pub(super) struct MapScreen;

/// Something marked on the map.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    /// The local player, as an arrow the way they face.
    Me,
    Spawn,
    /// The local player's bed, if they've slept in one.
    Bed,
    /// Another player, on the map with what `MarkerOf` says this client sees of them.
    Player,
}

/// On another player's marker, the `SeenPlayer` it marks, with which it goes.
#[derive(Component)]
#[relationship(relationship_target = MarkedBy)]
pub struct MarkerOf(pub Entity);

#[derive(Component)]
#[relationship_target(relationship = MarkerOf, linked_spawn)]
pub struct MarkedBy(Entity);

#[derive(Component)]
pub(super) struct ScaleBar;

#[derive(Component)]
pub(super) struct ScaleLabel;

#[derive(Component)]
pub(super) struct CursorCoordinates;

pub(super) fn spawn_map(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let arrow = images.add(arrow_image(32));
    let root = commands
        .spawn((
            MapScreen,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                display: Display::None,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(UNEXPLORED),
            // over the debug overlay, as the inventory is; the hotbar's hidden while it's open
            GlobalZIndex(1),
        ))
        .id();
    commands.spawn((
        TileLayer,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(root),
    ));
    commands.spawn((
        marker(Marker::Spawn, SPAWN_COLOUR, BorderRadius::ZERO, "Spawn"),
        ChildOf(root),
    ));
    commands.spawn((
        marker(Marker::Bed, BED_COLOUR, BorderRadius::ZERO, "Bed"),
        ChildOf(root),
    ));
    // last, so it's over the others
    commands.spawn((
        Marker::Me,
        ImageNode::new(arrow),
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(ARROW_SIZE),
            height: Val::Px(ARROW_SIZE),
            ..default()
        },
        ChildOf(root),
    ));

    commands.spawn((
        Text::new("Map"),
        font(24.0),
        TextColor(Color::WHITE),
        TEXT_SHADOW,
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(16.0),
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        TextLayout::justify(Justify::Center),
        ChildOf(root),
    ));
    commands.spawn((
        Text::new("Scroll to zoom, drag to look around, M to close"),
        font(18.0),
        TEXT_SHADOW,
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(16.0),
            width: Val::Percent(100.0),
            ..default()
        },
        TextLayout::justify(Justify::Center),
        ChildOf(root),
    ));
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(16.0),
            bottom: Val::Px(16.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        },
        ChildOf(root),
        children![
            (ScaleLabel, Text::default(), font(18.0), TEXT_SHADOW),
            (
                ScaleBar,
                Node {
                    height: Val::Px(6.0),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(Color::WHITE),
                BorderColor::all(Color::BLACK),
            ),
        ],
    ));
    commands.spawn((
        CursorCoordinates,
        Text::default(),
        font(18.0),
        TEXT_SHADOW,
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(16.0),
            bottom: Val::Px(16.0),
            ..default()
        },
        ChildOf(root),
    ));
}

// A marker with corners `rounded` this much, and a label under it.
fn marker(marker: Marker, colour: Color, rounded: BorderRadius, label: &str) -> impl Bundle {
    (
        marker,
        Node {
            position_type: PositionType::Absolute,
            width: Val::Px(MARKER_SIZE),
            height: Val::Px(MARKER_SIZE),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: rounded,
            ..default()
        },
        BackgroundColor(colour),
        BorderColor::all(Color::BLACK),
        children![(
            Text::new(label),
            font(14.0),
            TEXT_SHADOW,
            TextLayout::justify(Justify::Center),
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(MARKER_SIZE + 2.0),
                left: Val::Px((MARKER_SIZE - LABEL_WIDTH) / 2.0),
                width: Val::Px(LABEL_WIDTH),
                ..default()
            },
        )],
    )
}

/// An arrow pointing up, white edged with black, `size` pixels square.
fn arrow_image(size: u32) -> Image {
    // its corners about the middle: the tip, the right barb, the notch and the left barb
    const ARROW: [Vec2; 4] = [
        Vec2::new(0.0, -0.9),
        Vec2::new(0.7, 0.8),
        Vec2::new(0.0, 0.4),
        Vec2::new(-0.7, 0.8),
    ];
    let inside = |p: Vec2, scale: f32| {
        // even-odd: crossings of a ray to the right
        let mut inside = false;
        for (a, b) in ARROW.iter().zip(ARROW.iter().cycle().skip(1)) {
            let (a, b) = (*a * scale, *b * scale);
            if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                inside = !inside;
            }
        }
        inside
    };
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let p = (Vec2::new(x as f32, y as f32) + 0.5) / size as f32 * 2.0 - 1.0;
            rgba.extend(if inside(p, 0.8) {
                [255; 4]
            } else if inside(p, 1.0) {
                [0, 0, 0, 255]
            } else {
                [0; 4]
            });
        }
    }
    Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// M opens the map over the game and closes it again, as Escape does.
pub(super) fn toggle_map(
    controls: Controls,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
) {
    if controls.just_pressed(Action::Map) {
        match state.get() {
            GameState::Playing => next.set(GameState::Map),
            GameState::Map => next.set(GameState::Playing),
            _ => {}
        }
    }
}

/// Opens the map on the player, with the cursor let go, for dragging the map about.
pub(super) fn open(
    player: Query<&Transform, With<LocalPlayer>>,
    mut view: ResMut<MapView>,
    mut screen: Query<&mut Node, With<MapScreen>>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    if let Ok(body) = player.single() {
        view.centre = body.translation.xz();
    }
    for mut node in &mut screen {
        node.display = Display::Flex;
    }
    free_cursor(&mut cursor, true);
}

pub(super) fn close(mut screen: Query<&mut Node, With<MapScreen>>) {
    for mut node in &mut screen {
        node.display = Display::None;
    }
}

/// Keeps the closed map's view on the tile the player's in, changing it only as they leave it.
pub(super) fn follow_player(player: Query<&Transform, With<LocalPlayer>>, view: ResMut<MapView>) {
    if let Ok(body) = player.single() {
        let tile = super::explored::Explored::tile_of(body.translation.xz());
        let middle = (tile.as_vec2() + 0.5) * super::explored::TILE_BLOCKS as f32;
        view.map_unchanged(|view| &mut view.centre)
            .set_if_neq(middle);
    }
}

/// Scrolling zooms in and out about the point under the cursor, and dragging moves the map.
pub(super) fn zoom_and_pan(
    window: Single<&Window, With<PrimaryWindow>>,
    scroll: Res<AccumulatedMouseScroll>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut view: ResMut<MapView>,
    // how far the wheel's turned towards the next notch
    mut notches: Local<f32>,
    mut dragged_from: Local<Option<Vec2>>,
) {
    let (screen, cursor) = (window.size(), window.cursor_position());
    *notches += match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / PIXELS_PER_NOTCH,
    };
    let turned = notches.trunc();
    if turned != 0.0 {
        *notches -= turned;
        // scrolling up zooms in
        let zoom = (view.zoom as i32 - turned as i32).clamp(0, ZOOMS.len() as i32 - 1) as usize;
        if zoom != view.zoom {
            let about = cursor.unwrap_or(screen / 2.0);
            let held = view.to_world(about, screen);
            view.zoom = zoom;
            view.centre = held - (about - screen / 2.0) * view.blocks_per_pixel();
        }
    }

    let dragging = mouse.pressed(MouseButton::Left).then_some(cursor).flatten();
    if let (Some(from), Some(to)) = (*dragged_from, dragging) {
        if from != to {
            let scale = view.blocks_per_pixel();
            view.centre -= (to - from) * scale;
        }
    }
    *dragged_from = dragging;
}

/// Sees another player's marker onto the map as this client sees them.
pub(super) fn mark_players(
    seen: Query<(Entity, &SeenPlayer), Added<SeenPlayer>>,
    names: Query<&PlayerName>,
    screen: Single<Entity, With<MapScreen>>,
    mut commands: Commands,
) {
    for (entity, seen) in &seen {
        let name = names.get(seen.0).map_or("", PlayerName::as_str);
        commands.spawn((
            marker(Marker::Player, PLAYER_COLOUR, BorderRadius::MAX, name),
            MarkerOf(entity),
            ChildOf(*screen),
        ));
    }
}

/// Puts each marker where what it marks is, and hides those off the screen or with nowhere to
/// be, such as a bed not yet slept in.
#[allow(clippy::type_complexity)]
pub(super) fn place_markers(
    view: Res<MapView>,
    window: Single<&Window, With<PrimaryWindow>>,
    world: Res<World>,
    me: Query<(&Transform, &PlayerLook, &SpawnBed), With<LocalPlayer>>,
    seen: Query<(&Transform, &Visibility), With<SeenPlayer>>,
    mut markers: Query<(&Marker, Option<&MarkerOf>, &mut Node, &mut UiTransform)>,
) {
    let screen = window.size();
    let me = me.single().ok();
    for (marker, of, mut node, mut turned) in &mut markers {
        let at = match marker {
            Marker::Me => me.map(|(body, ..)| body.translation.xz()),
            Marker::Spawn => Some(world.spawn().xz()),
            Marker::Bed => me
                .and_then(|(.., bed)| bed.0)
                .map(|bed| bed.as_vec3().xz() + 0.5),
            Marker::Player => of
                .and_then(|of| seen.get(of.0).ok())
                .filter(|(_, shown)| **shown != Visibility::Hidden)
                .map(|(body, _)| body.translation.xz()),
        };
        let at = at.map(|at| view.to_screen(at, screen));
        let on_screen = at.filter(|at| at.cmpge(Vec2::ZERO).all() && at.cmplt(screen).all());
        let size = if *marker == Marker::Me {
            ARROW_SIZE
        } else {
            MARKER_SIZE
        };
        let display = if on_screen.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        let corner = on_screen.unwrap_or_default() - size / 2.0;
        if node.display != display
            || node.left != Val::Px(corner.x)
            || node.top != Val::Px(corner.y)
        {
            node.display = display;
            node.left = Val::Px(corner.x);
            node.top = Val::Px(corner.y);
        }
        if let (Marker::Me, Some((_, look, _))) = (marker, me) {
            // facing -z at no yaw, up the map, and turning anticlockwise as yaw grows
            turned.set_if_neq(UiTransform::from_rotation(Rot2::radians(-look.yaw)));
        }
    }
}

/// Sizes the scale bar to the longest round distance that fits, and names it.
pub(super) fn draw_scale_bar(
    view: Res<MapView>,
    mut bar: Single<&mut Node, With<ScaleBar>>,
    mut label: Single<&mut Text, With<ScaleLabel>>,
) {
    if !view.is_changed() {
        return;
    }
    let scale = view.blocks_per_pixel();
    let blocks = SCALE_LENGTHS
        .into_iter()
        .rfind(|blocks| blocks / scale <= SCALE_BAR_MAX)
        .unwrap_or(SCALE_LENGTHS[0]);
    bar.width = Val::Px(blocks / scale);
    label.0 = if blocks >= 1000.0 {
        format!("{} km", blocks / 1000.0)
    } else {
        format!("{blocks} m")
    };
}

/// Shows the block column under the cursor.
pub(super) fn show_cursor_coordinates(
    view: Res<MapView>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut text: Single<&mut Text, With<CursorCoordinates>>,
) {
    let shown = window.cursor_position().map_or_else(String::new, |cursor| {
        let at = view.to_world(cursor, window.size()).floor();
        format!("x {}, z {}", at.x, at.y)
    });
    if text.0 != shown {
        text.0 = shown;
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
    use bevy::input::mouse::MouseWheel;
    use bevy::input::touch::TouchPhase;
    use bevy::input::{ButtonState, InputPlugin};
    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin;
    use bevy::window::{PrimaryWindow, WindowFocused};
    use bevy_replicon::prelude::ClientId;
    use bevy_replicon::RepliconPlugins;

    use super::{MapScreen, Marker, ARROW_SIZE, MARKER_SIZE};
    use crate::client::command_bar::CommandPlugin;
    use crate::client::player::LocalPlayer;
    use crate::hotbar::HotbarPlugin;
    use crate::inventory::Hotbar;
    use crate::map::explored::Explored;
    use crate::map::{MapPlugin, MapView};
    use crate::menu::{GameState, MenuPlugin};
    use crate::player::{Player, PlayerLook, PlayerOwner};
    use crate::protocol::ProtocolPlugin;
    use crate::survival::SpawnBed;
    use crate::world::World;

    const BED: IVec3 = IVec3::new(-100, 470, 40);

    // The map with the menu, the command bar and the hotbar, headless, with a local player at
    // the origin, who's slept in a bed, in a window of the default size.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            InputPlugin,
            StatesPlugin,
            AssetPlugin::default(),
            MenuPlugin,
            RepliconPlugins,
            ProtocolPlugin,
            CommandPlugin,
            HotbarPlugin,
            MapPlugin,
        ))
        .insert_state(GameState::Playing)
        .init_asset::<Image>()
        .init_asset::<TextureAtlasLayout>()
        .add_message::<WindowFocused>()
        .insert_resource(World::new(16));
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        app.world_mut().spawn((
            Player::default(),
            PlayerOwner(ClientId::Server),
            LocalPlayer,
            SpawnBed(Some(BED.as_i64vec3())),
        ));
        app.finish();
        app.update();
        app
    }

    fn press(app: &mut App, key_code: KeyCode) {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            app.world_mut().write_message(KeyboardInput {
                key_code,
                logical_key: Key::Unidentified(NativeKey::Unidentified),
                state,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
        }
    }

    fn state(app: &App) -> GameState {
        app.world().resource::<State<GameState>>().get().clone()
    }

    fn shown(app: &mut App) -> bool {
        let mut screen = app.world_mut().query_filtered::<&Node, With<MapScreen>>();
        screen.single(app.world()).unwrap().display == Display::Flex
    }

    #[test]
    fn test_m_opens_the_map_and_m_or_escape_closes_it() {
        let mut app = app();
        press(&mut app, KeyCode::KeyM);
        assert_eq!(GameState::Map, state(&app));
        assert!(shown(&mut app));
        press(&mut app, KeyCode::KeyM);
        assert_eq!(GameState::Playing, state(&app));
        assert!(!shown(&mut app));

        press(&mut app, KeyCode::KeyM);
        press(&mut app, KeyCode::Escape);
        // back to the game, not paused over it
        assert_eq!(GameState::Playing, state(&app));
        assert!(!shown(&mut app));
    }

    #[test]
    fn test_the_map_doesnt_open_over_the_menu_or_while_typing() {
        let mut app = app();
        press(&mut app, KeyCode::Escape);
        press(&mut app, KeyCode::KeyM);
        assert_eq!(GameState::Paused, state(&app));
        press(&mut app, KeyCode::Escape);

        // typing an m
        press(&mut app, KeyCode::KeyT);
        press(&mut app, KeyCode::KeyM);
        assert_eq!(GameState::Playing, state(&app));
    }

    #[test]
    fn test_the_wheel_zooms_the_map_not_the_hotbar() {
        let mut app = app();
        press(&mut app, KeyCode::KeyM);
        let zoom = app.world().resource::<MapView>().zoom;
        app.world_mut().write_message(MouseWheel {
            unit: bevy::input::mouse::MouseScrollUnit::Line,
            x: 0.0,
            y: -1.0,
            window: Entity::PLACEHOLDER,
            phase: TouchPhase::Moved,
        });
        app.update();
        assert_eq!(zoom + 1, app.world().resource::<MapView>().zoom);
        let mut hotbar = app.world_mut().query::<&Hotbar>();
        assert_eq!(0, hotbar.single(app.world()).unwrap().selected);
    }

    #[test]
    fn test_markers_go_where_what_they_mark_is_on_the_map() {
        let mut app = app();
        press(&mut app, KeyCode::KeyM);
        let mut player = app
            .world_mut()
            .query_filtered::<(&mut Transform, &mut PlayerLook), With<LocalPlayer>>();
        let (mut body, mut look) = player.single_mut(app.world_mut()).unwrap();
        body.translation = Vec3::new(30.0, 500.0, -20.0);
        look.yaw = std::f32::consts::FRAC_PI_2;
        let view = MapView {
            centre: Vec2::new(-50.0, 0.0),
            zoom: 0,
        };
        app.insert_resource(view);
        app.update();

        let screen = Vec2::new(1280.0, 720.0);
        let spawn = app.world().resource::<World>().spawn().xz();
        let mut markers = app.world_mut().query::<(&Marker, &Node, &UiTransform)>();
        for (marker, node, turned) in markers.iter(app.world()) {
            let (at, size) = match marker {
                Marker::Me => (Vec2::new(30.0, -20.0), ARROW_SIZE),
                Marker::Spawn => (spawn, MARKER_SIZE),
                Marker::Bed => (BED.as_vec3().xz() + 0.5, MARKER_SIZE),
                Marker::Player => unreachable!("there's no one else"),
            };
            let middle = view.to_screen(at, screen);
            let corner = middle - size / 2.0;
            if middle.cmpge(Vec2::ZERO).all() && middle.cmplt(screen).all() {
                assert_eq!(Display::Flex, node.display, "{marker:?}");
                assert_eq!(
                    (Val::Px(corner.x), Val::Px(corner.y)),
                    (node.left, node.top),
                    "{marker:?}"
                );
            } else {
                assert_eq!(Display::None, node.display, "{marker:?}");
            }
            if *marker == Marker::Me {
                // facing west, to the left of the map
                assert!((turned.rotation * Vec2::Y).x.abs() > 0.99);
            }
        }
        // the bed's 50 blocks west of the middle, 100 pixels at this zoom
        let bed = view.to_screen(BED.as_vec3().xz() + 0.5, screen);
        assert_eq!(Vec2::new(640.0 - 99.0, 360.0 + 81.0), bed);
    }

    #[test]
    fn test_walking_reveals_the_map() {
        let mut app = app();
        assert!(app.world().resource::<Explored>().contains(IVec2::ZERO));
        let far = IVec2::new(100, 0);
        assert!(!app.world().resource::<Explored>().contains(far));
        let mut player = app
            .world_mut()
            .query_filtered::<&mut Transform, With<LocalPlayer>>();
        player.single_mut(app.world_mut()).unwrap().translation.x = 6420.0;
        app.update();
        assert!(app.world().resource::<Explored>().contains(far));
    }
}
