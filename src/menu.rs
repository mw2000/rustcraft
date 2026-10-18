use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow, WindowFocused};

const TOGGLE_KEY: KeyCode = KeyCode::Escape;
const INVENTORY_KEY: KeyCode = KeyCode::KeyE;

// Minecraft's: grey stone buttons, lighter with a white edge under the pointer.
pub(crate) const BUTTON_COLOR: Color = Color::srgb(0.42, 0.42, 0.42);
const BUTTON_HOVER_COLOR: Color = Color::srgb(0.5, 0.55, 0.7);
pub(crate) const BUTTON_BORDER_COLOR: Color = Color::BLACK;
const BUTTON_HOVER_BORDER_COLOR: Color = Color::WHITE;
pub(crate) const TEXT_SHADOW: TextShadow = TextShadow {
    offset: Vec2::splat(2.0),
    color: Color::srgb(0.24, 0.24, 0.24),
};

/// Whether the game's being played, paused under the menu, or carrying on under the inventory:
/// the block picker in creative, and the inventory screen in survival.
#[derive(States, Default, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GameState {
    #[default]
    Playing,
    Paused,
    Inventory,
}

/// Escape opens a menu over the game, like Minecraft's, and pauses it: the clock stops, the
/// player can't move and the cursor's let go. Escape again, or Back to Game, carries on. E opens
/// and closes the inventory, which Escape closes too.
pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<GameState>()
            .add_systems(Startup, spawn_menu)
            .add_systems(
                Update,
                (toggle_menu, pause_when_unfocused, highlight_buttons),
            )
            .add_systems(OnEnter(GameState::Paused), pause)
            .add_systems(OnEnter(GameState::Playing), resume);
    }
}

#[derive(Component)]
struct Menu;

fn spawn_menu(mut commands: Commands) {
    commands
        .spawn((
            Menu,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(10.0),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
            // over the debug overlay, the block picker and the hotbar
            GlobalZIndex(3),
        ))
        .with_children(|menu| {
            menu.spawn((
                Text::new("Game Menu"),
                font(24.0),
                TextColor(Color::WHITE),
                TEXT_SHADOW,
                Node {
                    margin: UiRect::bottom(Val::Px(30.0)),
                    ..default()
                },
            ));
            menu.spawn(button())
                .with_child(label("Back to Game"))
                .observe(|_: On<Activate>, mut state: ResMut<NextState<GameState>>| {
                    state.set(GameState::Playing);
                });
            menu.spawn(button()).with_child(label("Quit Game")).observe(
                |_: On<Activate>, mut exit: MessageWriter<AppExit>| {
                    exit.write(AppExit::Success);
                },
            );
        });
}

pub(crate) fn font(size: f32) -> TextFont {
    TextFont {
        font_size: bevy::text::FontSize::Px(size),
        ..default()
    }
}

fn label(text: &str) -> impl Bundle {
    (Text::new(text), font(18.0), TEXT_SHADOW)
}

// A button, for a label to go in.
fn button() -> impl Bundle {
    (
        Button,
        Hovered::default(),
        Node {
            width: Val::Px(400.0),
            height: Val::Px(40.0),
            border: UiRect::all(Val::Px(2.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(BUTTON_COLOR),
        BorderColor::all(BUTTON_BORDER_COLOR),
    )
}

fn toggle_menu(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
) {
    if keys.just_pressed(TOGGLE_KEY) {
        // which closes the inventory rather than pausing over it
        next.set(match state.get() {
            GameState::Playing => GameState::Paused,
            GameState::Paused | GameState::Inventory => GameState::Playing,
        });
    } else if keys.just_pressed(INVENTORY_KEY) {
        match state.get() {
            GameState::Playing => next.set(GameState::Inventory),
            GameState::Inventory => next.set(GameState::Playing),
            GameState::Paused => {}
        }
    }
}

// Like Minecraft, clicking away from the window pauses it, rather than leaving the player
// running about with no cursor. Not in the stats runs, which run unfocused.
//
// Only the latest event counts, and only once the window has had focus: opening, it reports
// losing focus and gaining it in the same frame, which isn't clicking away.
fn pause_when_unfocused(
    mut focus: MessageReader<WindowFocused>,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
    mut was_focused: Local<bool>,
) {
    let Some(focused) = focus.read().last().map(|event| event.focused) else {
        return;
    };
    let lost = *was_focused && !focused;
    *was_focused = focused;
    if lost && *state.get() == GameState::Playing && std::env::var("RC_STATS").is_err() {
        next.set(GameState::Paused);
    }
}

fn pause(
    mut time: ResMut<Time<Virtual>>,
    mut menu: Query<&mut Node, With<Menu>>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    time.pause();
    show(&mut menu, true);
    free_cursor(&mut cursor, true);
}

fn resume(
    mut time: ResMut<Time<Virtual>>,
    mut menu: Query<&mut Node, With<Menu>>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    time.unpause();
    show(&mut menu, false);
    free_cursor(&mut cursor, false);
}

fn show(menu: &mut Query<&mut Node, With<Menu>>, shown: bool) {
    for mut node in menu.iter_mut() {
        node.display = if shown { Display::Flex } else { Display::None };
    }
}

/// Lets the cursor go, for clicking on things, or holds it for looking about.
pub(crate) fn free_cursor(cursor: &mut Query<&mut CursorOptions, With<PrimaryWindow>>, free: bool) {
    for mut cursor in cursor.iter_mut() {
        cursor.visible = free;
        cursor.grab_mode = if free {
            CursorGrabMode::None
        } else {
            CursorGrabMode::Locked
        };
    }
}

#[allow(clippy::type_complexity)]
fn highlight_buttons(
    mut buttons: Query<
        (&Hovered, &mut BackgroundColor, &mut BorderColor),
        (With<Button>, Changed<Hovered>),
    >,
) {
    for (hovered, mut background, mut border) in &mut buttons {
        let (color, edge) = if hovered.get() {
            (BUTTON_HOVER_COLOR, BUTTON_HOVER_BORDER_COLOR)
        } else {
            (BUTTON_COLOR, BUTTON_BORDER_COLOR)
        };
        background.0 = color;
        *border = BorderColor::all(edge);
    }
}
