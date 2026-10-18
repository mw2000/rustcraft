use bevy::prelude::*;
use bevy_replicon::prelude::*;

use super::allowances::Allowances;
use super::commands::CONSOLE_NAME;
use crate::player::name::PlayerName;
use crate::player::PlayerOwner;
use crate::protocol::{Chat, ChatLine, CommandOutput, MAX_CHAT_LENGTH};

// A few lines at once, as when answering two people, then one a second, quicker than anyone
// types a line of any length for long. It keeps one client from flooding everyone's bar, which
// shows only the last 20 lines, and the network.
const CHAT_BURST: f32 = 5.0;
const LINES_PER_SECOND: f32 = 1.0;

/// What each client may still say straight away, in chat or by `/msg`.
#[derive(Resource, Deref, DerefMut)]
pub(super) struct ChatAllowances(Allowances);

impl Default for ChatAllowances {
    fn default() -> Self {
        Self(Allowances::new(CHAT_BURST, LINES_PER_SECOND))
    }
}

/// What a client saying more than its allowance is told.
pub(super) const TOO_QUICK: &str = "You're sending messages too quickly, wait a moment";

/// Passes on what each player, or a dedicated server's console, says to everyone, by their
/// name, tidied and kept to the allowance.
pub(super) fn receive_chat(
    mut said: MessageReader<FromClient<Chat>>,
    mut allowances: ResMut<ChatAllowances>,
    time: Res<Time>,
    players: Query<(&PlayerOwner, &PlayerName)>,
    mut lines: MessageWriter<ToClients<ChatLine>>,
    mut output: MessageWriter<ToClients<CommandOutput>>,
) {
    for FromClient {
        client_id: client,
        message: Chat(text),
    } in said.read()
    {
        let text = tidy(text);
        let from = players.iter().find(|(owner, _)| owner.0 == *client);
        let from = match from {
            Some((_, from)) => from.clone(),
            // a dedicated server's console
            None if *client == ClientId::Server => PlayerName::new(CONSOLE_NAME),
            None => continue,
        };
        if text.is_empty() {
            continue;
        }
        if !allowances.take(*client, time.elapsed()) {
            output.write(ToClients {
                targets: SendTargets::Single(*client),
                message: CommandOutput {
                    text: TOO_QUICK.into(),
                    error: true,
                },
            });
            continue;
        }
        info!("<{}> {text}", from.as_str());
        lines.write(ToClients {
            targets: SendTargets::All,
            message: ChatLine { from, text },
        });
    }
}

/// `text` as it's passed on: without control characters, which could break the bar's lines,
/// trimmed and at most `MAX_CHAT_LENGTH` long, as a client can send anything.
pub(super) fn tidy(text: &str) -> String {
    let kept: String = text
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_CHAT_LENGTH)
        .collect();
    kept.trim().into()
}
