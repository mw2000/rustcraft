//! Multiplayer: the servers saved, to join, add, change and remove, joining one by its address,
//! and the name to play under. Whether a server's up shows as it's joined, as the protocol has
//! no cheap way to ask without joining.

use bevy::prelude::*;
use bevy::text::{EditableText, TextEditChange};
use bevy::ui::{InteractionDisabled, Selected};
use bevy::ui_widgets::{observe, Activate, ListItem};

use super::title::open;
use super::widgets::{
    column, first, gap, heading, label, list, list_row, note, row, selected_row, text_button,
    text_field, typed, RowChosen, HALF, QUIET_TEXT, WIDE,
};
use super::{back_button, Confirm, Confirmed, Menu, Page};
use crate::client::servers::{SavedServer, ServerList};
use crate::player::name::MAX_NAME_LENGTH;
use crate::preferences::Preferences;
use crate::session::Session;

const WIDTH: f32 = 600.0;
const LIST_HEIGHT: f32 = 220.0;
const THIRD: f32 = 194.0;
// `host:port`, with room for a long host name
const MAX_ADDRESS: usize = 100;

#[derive(Component)]
struct ServerRow(usize);

#[derive(Component)]
struct ServerListBox;

#[derive(Component)]
struct NameField;

#[derive(Component)]
struct AddressField;

#[derive(Component)]
struct ServerNameField;

pub(super) fn spawn_list(
    In(root): In<Entity>,
    servers: Res<ServerList>,
    preferences: Res<Preferences>,
    mut commands: Commands,
) {
    let none = servers.servers.is_empty();
    let name = preferences.player_name();
    commands.entity(root).with_children(|root| {
        root.spawn(column()).with_children(|page| {
            page.spawn(heading("Multiplayer"));
            page.spawn(row()).with_children(|row| {
                row.spawn((label("Your name"), TextColor(QUIET_TEXT)));
                row.spawn((
                    NameField,
                    text_field(name.as_str(), MAX_NAME_LENGTH, WIDTH - 120.0),
                ))
                .observe(keep_name);
            });
            page.spawn((ServerListBox, list(WIDTH, LIST_HEIGHT)))
                .insert_if(first(), || !none)
                .observe(join_chosen)
                .with_children(|list| {
                    if none {
                        list.spawn(note(
                            "No servers saved: add a friend's, or join one by its address below.",
                            WIDTH,
                        ));
                    }
                    for (i, server) in servers.servers.iter().enumerate() {
                        let mut row = list.spawn((ServerRow(i), list_row()));
                        if i == 0 {
                            row.insert(Selected);
                        }
                        row.with_children(|row| {
                            row.spawn(label(&server.name));
                            row.spawn((
                                Text::new(server.address.clone()),
                                super::font(15.0),
                                TextColor(QUIET_TEXT),
                                bevy::picking::Pickable::IGNORE,
                            ));
                        });
                    }
                });
            page.spawn(row()).with_children(|buttons| {
                buttons
                    .spawn((text_button("Join Server", 296.0), observe(join_selected)))
                    .insert_if(InteractionDisabled, || none);
                buttons
                    .spawn((
                        text_button("Add Server", 296.0),
                        observe(open(Page::EditServer(None))),
                    ))
                    .insert_if(first(), || none);
            });
            page.spawn(row()).with_children(|buttons| {
                buttons
                    .spawn((text_button("Edit", THIRD), observe(edit_selected)))
                    .insert_if(InteractionDisabled, || none);
                buttons
                    .spawn((text_button("Remove", THIRD), observe(remove_selected)))
                    .insert_if(InteractionDisabled, || none);
                buttons.spawn(back_button("Back", THIRD));
            });
            page.spawn((
                Text::new("Or join by address"),
                super::font(15.0),
                TextColor(QUIET_TEXT),
                Node {
                    width: Val::Px(WIDTH),
                    margin: UiRect::top(Val::Px(12.0)),
                    ..default()
                },
            ));
            page.spawn(row()).with_children(|row| {
                row.spawn((AddressField, text_field("", MAX_ADDRESS, WIDTH - 128.0)));
                row.spawn((text_button("Join", 120.0), observe(join_typed)));
            });
        });
    });
}

// The name typed is the one played under from now on.
fn keep_name(
    changed: On<TextEditChange>,
    fields: Query<&EditableText>,
    mut preferences: ResMut<Preferences>,
) {
    if let Ok(field) = fields.get(changed.event_target()) {
        let name = typed(field);
        if preferences.name != name {
            preferences.name = name;
        }
    }
}

fn join(address: String, commands: &mut Commands) {
    Session::Join { address }.start(commands);
}

/// The server selected in the Multiplayer list.
#[derive(bevy::ecs::system::SystemParam)]
struct Selection<'w, 's> {
    lists: Query<'w, 's, Entity, With<ServerListBox>>,
    children: Query<'w, 's, &'static Children>,
    selected: Query<'w, 's, (), (With<ListItem>, With<Selected>)>,
    rows: Query<'w, 's, &'static ServerRow>,
}

impl Selection<'_, '_> {
    fn at(&self) -> Option<usize> {
        let list = self.lists.single().ok()?;
        let row = selected_row(list, &self.children, &self.selected)?;
        self.rows.get(row).ok().map(|row| row.0)
    }
}

fn join_selected(
    _: On<Activate>,
    selection: Selection,
    servers: Res<ServerList>,
    mut commands: Commands,
) {
    if let Some(server) = selection.at().and_then(|at| servers.servers.get(at)) {
        join(server.address.clone(), &mut commands);
    }
}

// Enter on a row, or a double click on it, joins it.
fn join_chosen(
    chosen: On<RowChosen>,
    rows: Query<&ServerRow>,
    servers: Res<ServerList>,
    mut commands: Commands,
) {
    let server = rows
        .get(chosen.row)
        .ok()
        .and_then(|row| servers.servers.get(row.0));
    if let Some(server) = server {
        join(server.address.clone(), &mut commands);
    }
}

fn join_typed(
    _: On<Activate>,
    address: Query<&EditableText, With<AddressField>>,
    mut commands: Commands,
) {
    let address = address.single().map(typed).unwrap_or_default();
    if !address.is_empty() {
        join(address, &mut commands);
    }
}

fn edit_selected(_: On<Activate>, selection: Selection, mut menu: ResMut<Menu>) {
    if let Some(at) = selection.at() {
        menu.open(Page::EditServer(Some(at)));
    }
}

fn remove_selected(
    _: On<Activate>,
    selection: Selection,
    servers: Res<ServerList>,
    mut menu: ResMut<Menu>,
) {
    let Some((at, server)) = selection
        .at()
        .and_then(|at| Some((at, servers.servers.get(at)?)))
    else {
        return;
    };
    menu.open(Page::Confirm(Confirm {
        question: format!("Remove {}?", server.name),
        detail: format!(
            "It can be added again with its address, {}.",
            server.address
        ),
        yes: "Remove",
        then: Confirmed::RemoveServer(at),
    }));
}

/// Removes the server at `at` in the list, as it's been confirmed, back to the list.
pub(super) fn remove(In(at): In<usize>, mut servers: ResMut<ServerList>, mut menu: ResMut<Menu>) {
    menu.0.pop();
    if at < servers.servers.len() {
        servers.servers.remove(at);
        servers.save();
    }
}

pub(super) fn spawn_edit(
    In(root): In<Entity>,
    menu: Res<Menu>,
    servers: Res<ServerList>,
    mut commands: Commands,
) {
    let Some(&Page::EditServer(at)) = menu.top() else {
        return;
    };
    let editing = at.and_then(|at| servers.servers.get(at));
    let (name, address) = editing.map_or(("A rustcraft server", ""), |server| {
        (server.name.as_str(), server.address.as_str())
    });
    commands.entity(root).with_child((
        column(),
        children![
            heading(if editing.is_some() {
                "Edit Server"
            } else {
                "Add Server"
            }),
            field_label("Server name"),
            (ServerNameField, text_field(name, 32, WIDE), first()),
            field_label("Address, such as 192.168.1.20 or example.org:7777"),
            (AddressField, text_field(address, MAX_ADDRESS, WIDE)),
            gap(4.0),
            (
                row(),
                children![
                    (text_button("Done", HALF), observe(save_server)),
                    back_button("Cancel", HALF),
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
            width: Val::Px(WIDE),
            margin: UiRect::top(Val::Px(6.0)),
            ..default()
        },
    )
}

// Keeps the server as the page has it, back to the list, unless it's been given no address.
fn save_server(
    _: On<Activate>,
    name: Query<&EditableText, With<ServerNameField>>,
    address: Query<&EditableText, With<AddressField>>,
    mut servers: ResMut<ServerList>,
    mut menu: ResMut<Menu>,
) {
    let Some(&Page::EditServer(at)) = menu.top() else {
        return;
    };
    let address = address.single().map(typed).unwrap_or_default();
    if address.is_empty() {
        return;
    }
    let name = Some(name.single().map(typed).unwrap_or_default())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| address.clone());
    let server = SavedServer { name, address };
    match at.filter(|&at| at < servers.servers.len()) {
        Some(at) => servers.servers[at] = server,
        None => servers.servers.push(server),
    }
    servers.save();
    menu.0.pop();
}
