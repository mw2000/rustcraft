//! The menus (M-U1): the title screen the game opens on, the worlds to play and the servers to
//! join, the settings, the loading screen, and over the game the pause menu. One page shows at a
//! time, the top of a stack of them, and Escape or a Back button goes back down it.

mod loading;
mod pause;
mod servers;
mod settings;
#[cfg(test)]
mod tests;
mod title;
pub mod widgets;
mod worlds;

use std::path::PathBuf;

use bevy::input_focus::tab_navigation::TabNavigationPlugin;
use bevy::input_focus::InputFocusPlugin;
use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate};
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow, WindowFocused};

use crate::client::player::OwnSkin;
use crate::controls::{Action, Bindings, Controls};
use crate::preferences::Preferences;
use crate::session;
pub use settings::SettingsTab;
use widgets::{column, first, gap, heading, note, row, text_button, HALF, WIDE};
pub(crate) use widgets::{font, BUTTON_BORDER_COLOR, BUTTON_COLOR, TEXT_SHADOW};

/// Where the game's got to: in the menus on the title screen, loading a world, or in one,
/// being played, paused under the menu, carrying on under the inventory (the block picker in
/// creative, and the inventory screen in survival) or the map, or carrying on with the player
/// dead, under the death screen until they respawn.
#[derive(States, Default, Debug, Clone, PartialEq, Eq, Hash)]
pub enum GameState {
    #[default]
    Title,
    Loading,
    Playing,
    Paused,
    Inventory,
    Map,
    Dead,
}

impl GameState {
    /// Whether there's a world being played, which there isn't in the menus or while it loads.
    pub fn in_world(&self) -> bool {
        !matches!(self, GameState::Title | GameState::Loading)
    }
}

/// The pages of the menu open, the one shown last.
#[derive(Resource, Default, Debug)]
pub struct Menu(Vec<Page>);

/// A page of the menus.
#[derive(Debug, Clone, PartialEq)]
pub enum Page {
    Title,
    Worlds,
    CreateWorld,
    RenameWorld(PathBuf),
    Servers,
    /// Adding a server, or changing the one at this place in the list.
    EditServer(Option<usize>),
    Settings(SettingsTab),
    Pause,
    Loading,
    Confirm(Confirm),
    /// Something to tell the player, such as why a server couldn't be joined.
    Notice {
        title: String,
        text: String,
    },
}

/// A question to be sure of before doing something that can't be undone.
#[derive(Debug, Clone, PartialEq)]
pub struct Confirm {
    pub question: String,
    pub detail: String,
    /// What the button that does it says.
    pub yes: &'static str,
    pub then: Confirmed,
}

/// What's done once it's confirmed.
#[derive(Debug, Clone, PartialEq)]
pub enum Confirmed {
    DeleteWorld(PathBuf),
    RemoveServer(usize),
}

impl Menu {
    pub fn top(&self) -> Option<&Page> {
        self.0.last()
    }

    pub fn open(&mut self, page: Page) {
        self.0.push(page);
    }

    /// Shows `page` in place of the one shown.
    pub fn replace(&mut self, page: Page) {
        self.0.pop();
        self.0.push(page);
    }

    /// Shows these pages, from the bottom.
    pub fn show(&mut self, pages: Vec<Page>) {
        self.0 = pages;
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// UI shown only in a world, such as the hotbar, hidden in the menus.
#[derive(Component)]
pub struct InWorld;

/// The root of the page shown.
#[derive(Component)]
struct PageRoot;

/// The title screen and every page of the menus, and over the game Escape opening the pause
/// menu, which pauses it: the clock stops, the player can't move and the cursor's let go.
/// Escape again, or Back to Game, carries on. E opens and closes the inventory, which Escape
/// closes too, as it does the map.
pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<InputFocusPlugin>() {
            app.add_plugins(InputFocusPlugin);
        }
        app.init_state::<GameState>()
            .init_resource::<Menu>()
            .init_resource::<Bindings>()
            .init_resource::<Preferences>()
            .init_resource::<OwnSkin>()
            .add_plugins((TabNavigationPlugin, widgets::WidgetsPlugin))
            .add_systems(
                Update,
                (
                    (escape, toggle_inventory, pause_when_unfocused),
                    draw_menu.run_if(resource_changed::<Menu>),
                )
                    .chain(),
            )
            .add_systems(Update, show_in_world.run_if(state_changed::<GameState>))
            .add_systems(OnEnter(GameState::Title), (open_title, free))
            .add_systems(OnEnter(GameState::Loading), (open_loading, free))
            .add_systems(OnEnter(GameState::Paused), pause)
            .add_systems(OnExit(GameState::Paused), unpause)
            .add_systems(OnEnter(GameState::Playing), resume);
        title::build(app);
        loading::build(app);
        settings::build(app);
        pause::build(app);
    }
}

fn open_title(mut menu: ResMut<Menu>) {
    // unless it's been opened with something to say over it, as a world's left
    if menu.0.first() != Some(&Page::Title) {
        menu.show(vec![Page::Title]);
    }
}

fn open_loading(mut menu: ResMut<Menu>) {
    menu.show(vec![Page::Loading]);
}

fn free(mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>) {
    free_cursor(&mut cursor, true);
}

/// Goes back a page, as Escape and the Back buttons do: out of the pause menu to the game, and
/// out of the loading screen to the title screen, leaving the world loading. The title screen
/// stays.
pub fn back(
    mut menu: ResMut<Menu>,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
    mut commands: Commands,
) {
    match state.get() {
        GameState::Paused if menu.len() <= 1 => next.set(GameState::Playing),
        GameState::Loading => commands.run_system_cached(session::leave),
        _ if menu.len() > 1 => {
            menu.0.pop();
        }
        _ => {}
    }
}

/// A button that goes back a page.
pub fn back_button(text: &str, width: f32) -> impl Bundle {
    (
        text_button(text, width),
        observe(|_: On<Activate>, mut commands: Commands| {
            commands.run_system_cached(back);
        }),
    )
}

// Escape goes back: from the game into the pause menu, out of the inventory or the map, and back
// a page in the menus, but does nothing for the dead, who can only respawn or quit, nor while a
// key's being bound, which it cancels.
fn escape(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
    binding: Option<Res<settings::Rebinding>>,
    mut commands: Commands,
) {
    if !keys.just_pressed(KeyCode::Escape) || binding.is_some() {
        return;
    }
    match state.get() {
        GameState::Playing => next.set(GameState::Paused),
        GameState::Inventory | GameState::Map => next.set(GameState::Playing),
        GameState::Dead => {}
        GameState::Title | GameState::Loading | GameState::Paused => {
            commands.run_system_cached(back);
        }
    }
}

fn toggle_inventory(
    controls: Controls,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
) {
    if controls.just_pressed(Action::Inventory) {
        match state.get() {
            GameState::Playing => next.set(GameState::Inventory),
            GameState::Inventory => next.set(GameState::Playing),
            _ => {}
        }
    }
}

// Shows the page on top of the menu, taking down the one shown before.
fn draw_menu(
    menu: Res<Menu>,
    state: Res<State<GameState>>,
    roots: Query<Entity, With<PageRoot>>,
    mut commands: Commands,
) {
    for root in &roots {
        commands.entity(root).despawn();
    }
    let Some(page) = menu.top() else {
        return;
    };
    // the title screen's scenery shows through, and the game through the pause menu's pages,
    // but not the world loading, which may have nothing to show yet
    let backdrop = match page {
        Page::Title => Color::NONE,
        Page::Loading => Color::srgb(0.1, 0.09, 0.08),
        _ if state.in_world() => Color::srgba(0.0, 0.0, 0.0, 0.5),
        _ => Color::srgba(0.0, 0.0, 0.0, 0.4),
    };
    let root = commands
        .spawn((
            PageRoot,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(backdrop),
            // over the debug overlay, the inventory, the hotbar and the death screen
            GlobalZIndex(4),
        ))
        .id();
    match page {
        Page::Title => commands.run_system_cached_with(title::spawn, root),
        Page::Worlds => commands.run_system_cached_with(worlds::spawn_list, root),
        Page::CreateWorld => commands.run_system_cached_with(worlds::spawn_create, root),
        Page::RenameWorld(_) => commands.run_system_cached_with(worlds::spawn_rename, root),
        Page::Servers => commands.run_system_cached_with(servers::spawn_list, root),
        Page::EditServer(_) => commands.run_system_cached_with(servers::spawn_edit, root),
        Page::Settings(_) => commands.run_system_cached_with(settings::spawn, root),
        Page::Pause => commands.run_system_cached_with(pause::spawn, root),
        Page::Loading => commands.run_system_cached_with(loading::spawn, root),
        Page::Confirm(_) => commands.run_system_cached_with(spawn_confirm, root),
        Page::Notice { .. } => commands.run_system_cached_with(spawn_notice, root),
    }
}

fn spawn_confirm(In(root): In<Entity>, menu: Res<Menu>, mut commands: Commands) {
    let Some(Page::Confirm(confirm)) = menu.top() else {
        return;
    };
    let then = confirm.then.clone();
    commands.entity(root).with_child((
        column(),
        children![
            heading(&confirm.question),
            note(&confirm.detail, WIDE),
            gap(12.0),
            (
                row(),
                children![
                    (
                        text_button(confirm.yes, HALF),
                        observe(move |_: On<Activate>, mut commands: Commands| {
                            match then.clone() {
                                Confirmed::DeleteWorld(dir) => {
                                    commands.run_system_cached_with(worlds::delete, dir)
                                }
                                Confirmed::RemoveServer(at) => {
                                    commands.run_system_cached_with(servers::remove, at)
                                }
                            }
                        }),
                    ),
                    (back_button("Cancel", HALF), first()),
                ],
            ),
        ],
    ));
}

fn spawn_notice(In(root): In<Entity>, menu: Res<Menu>, mut commands: Commands) {
    let Some(Page::Notice { title, text }) = menu.top() else {
        return;
    };
    commands.entity(root).with_child((
        column(),
        children![
            heading(title),
            note(text, WIDE + 100.0),
            gap(12.0),
            (back_button("Back", WIDE), first()),
        ],
    ));
}

// Hides the UI that belongs in a world while the menus have none.
fn show_in_world(state: Res<State<GameState>>, mut nodes: Query<&mut Node, With<InWorld>>) {
    let display = if state.in_world() {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut nodes {
        node.display = display;
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
    mut menu: ResMut<Menu>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    time.pause();
    menu.show(vec![Page::Pause]);
    free_cursor(&mut cursor, true);
}

fn unpause(mut time: ResMut<Time<Virtual>>) {
    time.unpause();
}

fn resume(mut menu: ResMut<Menu>, mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>) {
    if !menu.is_empty() {
        menu.show(Vec::new());
    }
    free_cursor(&mut cursor, false);
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
