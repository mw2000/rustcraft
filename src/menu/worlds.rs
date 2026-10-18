//! Singleplayer: the worlds saved, to play, rename, duplicate or delete, and making new ones.

use std::path::PathBuf;
use std::time::SystemTime;

use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::{InteractionDisabled, Selected};
use bevy::ui_widgets::{observe, Activate, ListItem};

use super::title::open;
use super::widgets::{
    choice, choice_text, column, digits_only, first, gap, heading, label, list, list_row, note,
    relabel, row, selected_row, text_button, text_field, typed, RowChosen, ERROR_TEXT, QUIET_TEXT,
};
use super::{back_button, Confirm, Confirmed, Menu, Page};
use crate::game_mode::GameMode;
use crate::save::worlds::{self, allowed_in_name, SavedWorld, WorldsDir, MAX_NAME};
use crate::session::Session;

// The list and the buttons under it, in pixels: two to a row, or four.
const WIDTH: f32 = 600.0;
const HALF: f32 = 296.0;
const QUARTER: f32 = 144.0;
const LIST_HEIGHT: f32 = 300.0;

/// A row of the list, for the world saved in this directory.
#[derive(Component)]
struct WorldRow(PathBuf);

#[derive(Component)]
struct WorldList;

#[derive(Component)]
struct NameField;

#[derive(Component)]
struct SeedField;

/// The game mode a new world's to be made with, on the button choosing it.
#[derive(Component)]
struct ModeChoice(GameMode);

pub(super) fn spawn_list(In(root): In<Entity>, dir: Res<WorldsDir>, mut commands: Commands) {
    let saved = worlds::list(&dir.0);
    let none = saved.is_empty();
    commands.entity(root).with_children(|root| {
        root.spawn(column()).with_children(|page| {
            page.spawn(heading("Singleplayer"));
            page.spawn((WorldList, list(WIDTH, LIST_HEIGHT)))
                .insert_if(first(), || !none)
                .observe(play_chosen)
                .with_children(|list| {
                    if none {
                        list.spawn(note("No worlds yet: create one to start playing.", WIDTH));
                    }
                    for (i, world) in saved.iter().enumerate() {
                        let mut row = list.spawn((WorldRow(world.dir.clone()), list_row()));
                        if i == 0 {
                            row.insert(Selected);
                        }
                        row.with_children(|row| world_lines(row, world));
                    }
                });
            page.spawn(row()).with_children(|buttons| {
                buttons
                    .spawn((
                        text_button("Play Selected World", HALF),
                        observe(play_selected),
                    ))
                    .insert_if(InteractionDisabled, || none);
                buttons
                    .spawn((
                        text_button("Create New World", HALF),
                        observe(open(Page::CreateWorld)),
                    ))
                    .insert_if(first(), || none);
            });
            page.spawn(row()).with_children(|buttons| {
                buttons
                    .spawn((text_button("Rename", QUARTER), observe(rename_selected)))
                    .insert_if(InteractionDisabled, || none);
                buttons
                    .spawn((
                        text_button("Duplicate", QUARTER),
                        observe(duplicate_selected),
                    ))
                    .insert_if(InteractionDisabled, || none);
                buttons
                    .spawn((text_button("Delete", QUARTER), observe(delete_selected)))
                    .insert_if(InteractionDisabled, || none);
                buttons.spawn(back_button("Back", QUARTER));
            });
        });
    });
}

// A world's name, and its seed, game mode and when it was last played, or why it can't be.
fn world_lines(row: &mut ChildSpawnerCommands, world: &SavedWorld) {
    row.spawn(label(&world.name));
    let (details, colour) = match &world.file {
        Ok(file) => (
            format!(
                "Seed {}, {}, {}",
                file.seed,
                mode_name(world.game_mode().unwrap_or_default()),
                played(world.last_played)
            ),
            QUIET_TEXT,
        ),
        Err(error) => (format!("Can't be played: {error}"), ERROR_TEXT),
    };
    row.spawn((
        Text::new(details),
        super::font(15.0),
        TextColor(colour),
        bevy::picking::Pickable::IGNORE,
    ));
}

/// When a world was last played, as a player reads it.
fn played(when: Option<SystemTime>) -> String {
    let Some(ago) = when.and_then(|when| SystemTime::now().duration_since(when).ok()) else {
        return "never played".into();
    };
    let minutes = ago.as_secs() / 60;
    let (count, unit) = match minutes {
        0 => return "played just now".into(),
        1..60 => (minutes, "minute"),
        60..1440 => (minutes / 60, "hour"),
        _ => (minutes / 1440, "day"),
    };
    let plural = if count == 1 { "" } else { "s" };
    format!("played {count} {unit}{plural} ago")
}

/// The world selected in the Singleplayer list.
#[derive(bevy::ecs::system::SystemParam)]
struct Selection<'w, 's> {
    lists: Query<'w, 's, Entity, With<WorldList>>,
    children: Query<'w, 's, &'static Children>,
    selected: Query<'w, 's, (), (With<ListItem>, With<Selected>)>,
    rows: Query<'w, 's, &'static WorldRow>,
}

impl Selection<'_, '_> {
    fn dir(&self) -> Option<PathBuf> {
        let list = self.lists.single().ok()?;
        let row = selected_row(list, &self.children, &self.selected)?;
        self.rows.get(row).ok().map(|row| row.0.clone())
    }
}

fn play(dir: PathBuf, commands: &mut Commands) {
    Session::Singleplayer {
        dir,
        seed: None,
        time_of_day: None,
        host: None,
    }
    .start(commands);
}

fn play_selected(_: On<Activate>, selection: Selection, mut commands: Commands) {
    if let Some(dir) = selection.dir() {
        play(dir, &mut commands);
    }
}

// Enter on a row, or a double click on it, plays it.
fn play_chosen(chosen: On<RowChosen>, rows: Query<&WorldRow>, mut commands: Commands) {
    if let Ok(row) = rows.get(chosen.row) {
        play(row.0.clone(), &mut commands);
    }
}

fn rename_selected(_: On<Activate>, selection: Selection, mut menu: ResMut<Menu>) {
    if let Some(dir) = selection.dir() {
        menu.open(Page::RenameWorld(dir));
    }
}

fn duplicate_selected(_: On<Activate>, selection: Selection, mut menu: ResMut<Menu>) {
    let Some(dir) = selection.dir() else {
        return;
    };
    match worlds::duplicate(&dir) {
        // drawn again with the copy
        Ok(_) => menu.set_changed(),
        Err(error) => menu.open(problem("Couldn't duplicate the world", error)),
    }
}

fn delete_selected(_: On<Activate>, selection: Selection, mut menu: ResMut<Menu>) {
    let Some(dir) = selection.dir() else {
        return;
    };
    let name = dir.file_name().unwrap_or_default().to_string_lossy();
    menu.open(Page::Confirm(Confirm {
        question: format!("Delete {name}?"),
        detail: "It'll be gone for good: everything built and everything carried.".into(),
        yes: "Delete",
        then: Confirmed::DeleteWorld(dir),
    }));
}

/// Deletes the world in `dir`, as it's been confirmed, back to the list.
pub(super) fn delete(In(dir): In<PathBuf>, mut menu: ResMut<Menu>) {
    menu.0.pop();
    if let Err(error) = worlds::delete(&dir) {
        menu.open(problem("Couldn't delete the world", error));
    }
}

fn problem(title: &str, error: impl std::fmt::Display) -> Page {
    Page::Notice {
        title: title.into(),
        text: error.to_string(),
    }
}

pub(super) fn spawn_create(In(root): In<Entity>, dir: Res<WorldsDir>, mut commands: Commands) {
    let name = worlds::unused_name(&dir.0, "New World");
    let mode = GameMode::default();
    commands.entity(root).with_child((
        column(),
        children![
            heading("Create New World"),
            field_label("World name"),
            (
                NameField,
                text_field(&name, MAX_NAME, super::widgets::WIDE),
                bevy::text::EditableTextFilter::new(allowed_in_name),
                first(),
            ),
            field_label("Seed, or blank for a random one"),
            (
                SeedField,
                text_field("", 10, super::widgets::WIDE),
                digits_only()
            ),
            gap(4.0),
            (
                ModeChoice(mode),
                choice("Game Mode", mode_name(mode), super::widgets::WIDE),
                observe(switch_mode),
            ),
            gap(4.0),
            (
                row(),
                children![
                    (
                        text_button("Create World", super::widgets::HALF),
                        observe(create)
                    ),
                    back_button("Cancel", super::widgets::HALF),
                ],
            ),
        ],
    ));
}

fn field_label(text: &str) -> impl Bundle {
    (
        Text::new(text),
        super::font(15.0),
        TextColor(QUIET_TEXT),
        Node {
            width: Val::Px(super::widgets::WIDE),
            margin: UiRect::top(Val::Px(6.0)),
            ..default()
        },
    )
}

fn mode_name(mode: GameMode) -> &'static str {
    match mode {
        GameMode::Survival => "Survival",
        GameMode::Creative => "Creative",
    }
}

fn switch_mode(
    activate: On<Activate>,
    mut choices: Query<&mut ModeChoice>,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
) {
    let Ok(mut choice) = choices.get_mut(activate.entity) else {
        return;
    };
    choice.0 = match choice.0 {
        GameMode::Survival => GameMode::Creative,
        GameMode::Creative => GameMode::Survival,
    };
    let text = choice_text("Game Mode", mode_name(choice.0));
    relabel(activate.entity, text, &children, &mut texts);
}

// Makes the world as the page has it and plays it.
fn create(
    _: On<Activate>,
    dir: Res<WorldsDir>,
    name: Query<&EditableText, With<NameField>>,
    seed: Query<&EditableText, With<SeedField>>,
    mode: Query<&ModeChoice>,
    mut menu: ResMut<Menu>,
    mut commands: Commands,
) {
    let name = name.single().map(typed).unwrap_or_default();
    let seed = seed.single().map(typed).unwrap_or_default();
    let mode = mode.single().map_or(GameMode::default(), |choice| choice.0);
    let seed = match seed.as_str() {
        "" => None,
        digits => match digits.parse() {
            Ok(seed) => Some(seed),
            Err(_) => {
                menu.open(problem("That seed's too big", "Seeds go up to 4294967295."));
                return;
            }
        },
    };
    match worlds::create(&dir.0, &name, seed, mode) {
        Ok(world) => play(world, &mut commands),
        Err(error) => menu.open(problem("Couldn't create the world", error)),
    }
}

pub(super) fn spawn_rename(In(root): In<Entity>, menu: Res<Menu>, mut commands: Commands) {
    let Some(Page::RenameWorld(dir)) = menu.top() else {
        return;
    };
    let name = dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    commands.entity(root).with_child((
        column(),
        children![
            heading("Rename World"),
            field_label("World name"),
            (
                NameField,
                text_field(&name, MAX_NAME, super::widgets::WIDE),
                bevy::text::EditableTextFilter::new(allowed_in_name),
                first(),
            ),
            gap(4.0),
            (
                row(),
                children![
                    (text_button("Rename", super::widgets::HALF), observe(rename)),
                    back_button("Cancel", super::widgets::HALF),
                ],
            ),
        ],
    ));
}

fn rename(_: On<Activate>, name: Query<&EditableText, With<NameField>>, mut menu: ResMut<Menu>) {
    let Some(Page::RenameWorld(dir)) = menu.top().cloned() else {
        return;
    };
    let name = name.single().map(typed).unwrap_or_default();
    menu.0.pop();
    if let Err(error) = worlds::rename(&dir, &name) {
        menu.open(problem("Couldn't rename the world", error));
    }
}
