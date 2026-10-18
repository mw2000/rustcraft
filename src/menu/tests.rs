//! The menus headless: their widgets, pages and keys, with the widgets' events triggered as
//! clicks and key presses would.

use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input::{ButtonState, InputPlugin};
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::text::{apply_text_edits, EditableText, FontCx, LayoutCx, TextEdit};
use bevy::ui::Selected;
use bevy::ui_widgets::{Activate, ActiveDescendant, ListBoxPlugin, ListItem};
use bevy::window::WindowFocused;

use super::widgets::{digits_only, list, list_row, select_row, selected_row, text_field, typed};
use super::*;
use crate::controls::Binding;
use crate::preferences::Preferences;
use crate::save::worlds::WorldsDir;
use crate::settings::Settings;

// The menus on their own, in `state`.
fn app(state: GameState) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        StatesPlugin,
        InputPlugin,
        ListBoxPlugin,
        MenuPlugin,
    ))
    .insert_state(state)
    .init_state::<bevy_replicon::prelude::ClientState>()
    .add_message::<WindowFocused>()
    .insert_resource(Settings::default())
    .insert_resource(WorldsDir(
        std::env::temp_dir().join("rustcraft-menu-tests-none"),
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

fn pages(app: &App) -> Vec<Page> {
    app.world().resource::<Menu>().0.clone()
}

fn state(app: &App) -> GameState {
    app.world().resource::<State<GameState>>().get().clone()
}

// The button whose label says `text`.
fn button_saying(app: &mut App, text: &str) -> Entity {
    let label = app
        .world_mut()
        .query::<(&Text, &ChildOf)>()
        .iter(app.world())
        .find(|(label, _)| label.0 == text)
        .map(|(_, parent)| parent.parent())
        .unwrap_or_else(|| panic!("no button says {text}"));
    label
}

fn click(app: &mut App, button: Entity) {
    app.world_mut().trigger(Activate { entity: button });
    app.update();
}

#[test]
fn test_escape_goes_back_a_page_and_never_off_the_title_screen() {
    let mut app = app(GameState::Title);
    assert_eq!(vec![Page::Title], pages(&app));
    let singleplayer = button_saying(&mut app, "Singleplayer");
    click(&mut app, singleplayer);
    assert_eq!(vec![Page::Title, Page::Worlds], pages(&app));
    let create = button_saying(&mut app, "Create New World");
    click(&mut app, create);
    assert_eq!(
        Some(&Page::CreateWorld),
        app.world().resource::<Menu>().top()
    );
    press(&mut app, KeyCode::Escape);
    press(&mut app, KeyCode::Escape);
    assert_eq!(vec![Page::Title], pages(&app));
    press(&mut app, KeyCode::Escape);
    assert_eq!(vec![Page::Title], pages(&app));
    // the Back buttons do the same
    let settings = button_saying(&mut app, "Settings");
    click(&mut app, settings);
    let done = button_saying(&mut app, "Done");
    click(&mut app, done);
    assert_eq!(vec![Page::Title], pages(&app));
}

#[test]
fn test_escape_pauses_and_goes_back_through_the_pause_menu_to_the_game() {
    let mut app = app(GameState::Playing);
    assert!(pages(&app).is_empty());
    press(&mut app, KeyCode::Escape);
    assert_eq!(GameState::Paused, state(&app));
    assert_eq!(vec![Page::Pause], pages(&app));
    let settings = button_saying(&mut app, "Settings");
    click(&mut app, settings);
    assert_eq!(2, pages(&app).len());
    press(&mut app, KeyCode::Escape);
    assert_eq!(vec![Page::Pause], pages(&app));
    press(&mut app, KeyCode::Escape);
    assert_eq!(GameState::Playing, state(&app));
    assert!(pages(&app).is_empty());
}

#[test]
fn test_tabs_switch_the_settings_shown() {
    let mut app = app(GameState::Title);
    app.world_mut()
        .resource_mut::<Menu>()
        .open(Page::Settings(SettingsTab::Video));
    app.update();
    assert!(button_saying(&mut app, "Clouds: On") != Entity::PLACEHOLDER);
    let controls = button_saying(&mut app, "Controls");
    click(&mut app, controls);
    assert_eq!(
        Some(&Page::Settings(SettingsTab::Controls)),
        app.world().resource::<Menu>().top()
    );
    // in place of the tab shown, so Done still goes back to the title screen
    assert_eq!(2, pages(&app).len());
    assert!(button_saying(&mut app, "Invert Mouse: Off") != Entity::PLACEHOLDER);
    let video = button_saying(&mut app, "Video");
    click(&mut app, video);
    let clouds = button_saying(&mut app, "Clouds: On");
    click(&mut app, clouds);
    app.update();
    assert!(!app.world().resource::<Preferences>().video.clouds);
    assert!(button_saying(&mut app, "Clouds: Off") == clouds);
}

#[test]
fn test_a_key_rebound_in_the_settings_is_the_one_that_works() {
    let mut app = app(GameState::Paused);
    app.world_mut()
        .resource_mut::<Menu>()
        .open(Page::Settings(SettingsTab::Controls));
    app.update();
    let inventory = button_saying(&mut app, "E");
    click(&mut app, inventory);
    assert!(button_saying(&mut app, "> Press a key <") == inventory);
    // Escape cancels without going back a page
    press(&mut app, KeyCode::Escape);
    assert_eq!(2, pages(&app).len());
    assert!(button_saying(&mut app, "E") == inventory);
    click(&mut app, inventory);
    press(&mut app, KeyCode::KeyI);
    let bindings = app
        .world()
        .resource::<Preferences>()
        .controls
        .bindings
        .clone();
    assert_eq!(Binding::Key(KeyCode::KeyI), bindings.get(Action::Inventory));
    assert!(button_saying(&mut app, "I") == inventory);

    // and once Bindings follows the settings, as `PreferencesPlugin` has it do, I opens the
    // inventory and E no longer does
    app.insert_resource(bindings);
    press(&mut app, KeyCode::Escape);
    press(&mut app, KeyCode::Escape);
    assert_eq!(GameState::Playing, state(&app));
    press(&mut app, KeyCode::KeyE);
    assert_eq!(GameState::Playing, state(&app));
    press(&mut app, KeyCode::KeyI);
    assert_eq!(GameState::Inventory, state(&app));
}

#[test]
fn test_lists_select_the_row_clicked_or_moved_to() {
    let mut app = app(GameState::Title);
    let list = app.world_mut().spawn(list(600.0, 300.0)).id();
    let rows: Vec<Entity> = (0..3)
        .map(|_| app.world_mut().spawn((list_row(), ChildOf(list))).id())
        .collect();
    let selected = |app: &mut App| {
        app.world_mut()
            .run_system_cached_with(
                |In(list): In<Entity>,
                 children: Query<&Children>,
                 rows: Query<(), (With<ListItem>, With<Selected>)>| {
                    selected_row(list, &children, &rows)
                },
                list,
            )
            .unwrap()
    };
    assert_eq!(None, selected(&mut app));
    let row = rows[1];
    app.world_mut()
        .commands()
        .queue(move |world: &mut bevy::ecs::world::World| {
            select_row(&mut world.commands(), list, row);
        });
    app.update();
    assert_eq!(Some(rows[1]), selected(&mut app));
    // moving to a row with the arrow keys selects it too
    app.world_mut()
        .entity_mut(list)
        .insert(ActiveDescendant(Some(rows[2])));
    app.update();
    assert_eq!(Some(rows[2]), selected(&mut app));
}

#[test]
fn test_text_fields_take_typing_up_to_their_length_and_only_what_they_allow() {
    let mut world = bevy::ecs::world::World::new();
    world.init_resource::<FontCx>();
    // the game's font, which a field's laid out in to edit it
    let font = bevy::text::Font::from_bytes(bevy::text::DEFAULT_FONT_DATA.to_vec());
    world
        .resource_mut::<FontCx>()
        .collection
        .register_fonts(font.data, None);
    world.init_resource::<LayoutCx>();
    world.init_resource::<bevy::clipboard::Clipboard>();
    let seed = world.spawn((text_field("", 10, 400.0), digits_only())).id();
    let name = world.spawn(text_field("New World", 10, 400.0)).id();
    let mut edit = |entity, edits: Vec<TextEdit>| {
        let mut field = world.get_mut::<EditableText>(entity).unwrap();
        for e in edits {
            field.queue_edit(e);
        }
        world.run_system_cached(apply_text_edits).unwrap();
    };
    // as it's typed, a key at a time, and a paste with anything else in it is refused whole
    let typing = "12ab3"
        .chars()
        .map(|c| TextEdit::Insert(c.to_string().into()));
    edit(seed, typing.collect());
    edit(seed, vec![TextEdit::Insert("4x".into())]);
    edit(
        name,
        vec![TextEdit::Insert("A".into()), TextEdit::Insert("B".into())],
    );
    let value =
        |world: &bevy::ecs::world::World, entity| typed(world.get::<EditableText>(entity).unwrap());
    assert_eq!("123", value(&world, seed));
    // and the second letter would make it longer than 10
    let name = value(&world, name);
    assert_eq!(10, name.len());
    assert!(name.contains('A') && !name.contains('B'), "{name}");
}
