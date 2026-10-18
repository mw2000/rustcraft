//! The pause menu, over the game: back to it, the settings, opening the world to friends, and
//! saving and quitting to the title screen.

use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate};
use bevy_replicon::prelude::{ClientId, ClientState, RepliconChannels};

use super::title::open;
use super::widgets::{column, first, heading, note, text_button, WIDE};
use super::{back_button, GameState, Page, SettingsTab};
use crate::player::name::PlayerName;
use crate::player::PlayerOwner;
use crate::protocol::DEFAULT_PORT;
use crate::server::host;
use crate::session::{self, Hosting};

#[derive(Component)]
struct FriendsNote;

pub(super) fn build(app: &mut App) {
    app.add_systems(Update, show_friends.run_if(in_state(GameState::Paused)));
}

pub(super) fn spawn(In(root): In<Entity>, client: Res<State<ClientState>>, mut commands: Commands) {
    // a client of a server elsewhere can't open it to anyone, and only disconnects
    let joined = *client.get() != ClientState::Disconnected;
    let quit = if joined {
        "Disconnect"
    } else {
        "Save and Quit to Title"
    };
    commands.entity(root).with_children(|root| {
        root.spawn(column()).with_children(|page| {
            page.spawn(heading("Game Menu"));
            page.spawn((back_button("Back to Game", WIDE), first()));
            page.spawn((
                text_button("Settings", WIDE),
                observe(open(Page::Settings(SettingsTab::Video))),
            ));
            if !joined {
                page.spawn((
                    text_button("Open to Friends", WIDE),
                    observe(open_to_friends),
                ));
            }
            page.spawn((
                text_button(quit, WIDE),
                observe(|_: On<Activate>, mut commands: Commands| {
                    commands.run_system_cached(session::leave);
                }),
            ));
            page.spawn((FriendsNote, note("", WIDE)));
        });
    });
}

// Hosts the world on the default port, unless it's hosted already, for friends to join at the
// address the menu then shows.
fn open_to_friends(
    _: On<Activate>,
    hosting: Option<Res<Hosting>>,
    channels: Res<RepliconChannels>,
    mut commands: Commands,
) {
    if hosting.is_some() {
        return;
    }
    match host(&mut commands, &channels, DEFAULT_PORT) {
        Ok(address) => {
            info!("open to friends at {address}");
            commands.insert_resource(Hosting(Ok(address)));
        }
        Err(error) => {
            error!("couldn't open the world to friends: {error}");
            commands.insert_resource(Hosting(Err(error.to_string())));
        }
    }
}

// Says where friends can join, and who has.
fn show_friends(
    hosting: Option<Res<Hosting>>,
    players: Query<(&PlayerOwner, &PlayerName)>,
    mut notes: Query<&mut Text, With<FriendsNote>>,
) {
    let text = match hosting.as_deref() {
        None => String::new(),
        Some(Hosting(Err(error))) => format!("Couldn't open to friends: {error}"),
        Some(Hosting(Ok(address))) => {
            let mut joined: Vec<&str> = players
                .iter()
                .filter(|(owner, _)| owner.0 != ClientId::Server)
                .map(|(_, name)| name.as_str())
                .collect();
            joined.sort_unstable();
            let who = if joined.is_empty() {
                "Nobody's joined yet.".to_owned()
            } else {
                format!("Here: {}", joined.join(", "))
            };
            format!("Open to friends at {address}\n{who}")
        }
    };
    for mut note in &mut notes {
        if note.0 != text {
            note.0.clone_from(&text);
        }
    }
}
