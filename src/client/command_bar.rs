//! The command bar, which is also where chat is typed and shown.

use bevy::input::keyboard::KeyboardInput;
use bevy::input::{ButtonState, InputSystems};
use bevy::prelude::*;
use bevy_replicon::prelude::ServerSystems;

use crate::controls::{just_pressed, Action, Bindings};
use crate::hotbar::HOTBAR_TOP;
use crate::menu::{font, GameState, InWorld};
use crate::protocol::{Chat, ChatLine, CommandOutput, RunCommand, MAX_CHAT_LENGTH};
use crate::world::{LeaveWorld, LeaveWorldSystems};

// Minecraft's: T (`Action::Chat`) or Enter opens the bar, and / (`Action::Command`) opens it
// with the slash already typed.
const OPEN_KEYS: [KeyCode; 2] = [KeyCode::Enter, KeyCode::NumpadEnter];

// How long a line stays up once the bar's closed. The latest are all shown while it's open,
// as many as chat with a few people fills in a while.
const LINE_SECONDS: f32 = 10.0;
const MAX_LINES: usize = 20;

const TEXT_COLOR: Color = Color::WHITE;
const ERROR_COLOR: Color = Color::srgb(1.0, 0.33, 0.33);
const BAR_COLOR: Color = Color::srgba(0.0, 0.0, 0.0, 0.5);

/// A command bar like Minecraft's chat: T, Enter or / opens it, and Enter sends what's typed to
/// the server, a command to run if it starts with / or else a line of chat, and Escape closes
/// it. It prints what the server answers and what everyone says, `<name> text`. While it's open,
/// keys type into it instead of moving the player. Needs `ProtocolPlugin`.
pub struct CommandPlugin;

impl Plugin for CommandPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CommandBar>()
            .init_resource::<Bindings>()
            .add_systems(Startup, spawn_bar)
            // straight after input's read, so the keys it takes never reach the game
            .add_systems(PreUpdate, type_into_bar.after(InputSystems))
            // as soon as replicon hands the answer over, which in singleplayer is in the frame
            // the command was typed
            .add_systems(PostUpdate, print_output.after(ServerSystems::Send))
            .add_systems(Update, draw_bar)
            .add_systems(LeaveWorld, forget_lines.in_set(LeaveWorldSystems::Clear));
    }
}

// What was said in the world left isn't shown in the next.
fn forget_lines(mut bar: ResMut<CommandBar>) {
    bar.open = false;
    bar.input.clear();
    bar.lines.clear();
}

#[derive(Resource, Default)]
pub struct CommandBar {
    open: bool,
    input: String,
    // what's been printed, newest last, with when
    lines: Vec<Line>,
    // how many lines have ever been printed, so drawing can tell when there are new ones
    printed: usize,
}

struct Line {
    text: String,
    error: bool,
    at: f32,
}

impl CommandBar {
    pub fn is_open(&self) -> bool {
        self.open
    }

    fn print(&mut self, text: impl Into<String>, error: bool, at: f32) {
        self.lines.push(Line {
            text: text.into(),
            error,
            at,
        });
        self.printed += 1;
        let excess = self.lines.len().saturating_sub(MAX_LINES);
        self.lines.drain(..excess);
    }
}

/// Whether the bar's closed, so the player can move and look about.
pub fn command_bar_closed(bar: Res<CommandBar>) -> bool {
    !bar.is_open()
}

#[derive(Component)]
struct Output;

#[derive(Component)]
struct Input;

fn spawn_bar(mut commands: Commands) {
    let font = font(16.0);
    commands
        .spawn((
            InWorld,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(6.0),
                right: Val::Px(6.0),
                bottom: Val::Px(HOTBAR_TOP + 6.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(4.0),
                ..default()
            },
        ))
        .with_children(|bar| {
            bar.spawn((
                Output,
                Text::default(),
                font.clone(),
                Node {
                    max_width: Val::Px(600.0),
                    padding: UiRect::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(BAR_COLOR),
                Visibility::Hidden,
            ));
            bar.spawn((
                Input,
                Text::default(),
                font,
                TextColor(TEXT_COLOR),
                Node {
                    padding: UiRect::all(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(BAR_COLOR),
                Visibility::Hidden,
            ));
        });
}

#[allow(clippy::too_many_arguments)]
fn type_into_bar(
    mut bar: ResMut<CommandBar>,
    mut typed: MessageReader<KeyboardInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    bindings: Res<Bindings>,
    mut run: MessageWriter<RunCommand>,
    mut chat: MessageWriter<Chat>,
    state: Res<State<GameState>>,
) {
    if !bar.open {
        // not over the menu or the block picker
        if *state.get() != GameState::Playing {
            return;
        }
        let pressed = |action| just_pressed(&bindings, action, &keys, &mouse);
        let slash = pressed(Action::Command);
        if slash || pressed(Action::Chat) || keys.any_just_pressed(OPEN_KEYS) {
            bar.open = true;
            bar.input = if slash { "/".into() } else { String::new() };
            // the key that opened it would type itself too
            typed.clear();
            keys.reset_all();
        }
        return;
    }

    let modified = keys.any_pressed([
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);
    for key in typed.read() {
        if key.state != ButtonState::Pressed {
            continue;
        }
        match key.key_code {
            KeyCode::Escape => {
                bar.open = false;
                break;
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                let line = std::mem::take(&mut bar.input);
                if line.trim_start().starts_with('/') {
                    run.write(RunCommand(line));
                } else if !line.trim().is_empty() {
                    chat.write(Chat(line));
                }
                bar.open = false;
                break;
            }
            KeyCode::Backspace => {
                bar.input.pop();
            }
            _ if modified => {}
            _ => {
                if let Some(text) = &key.text {
                    let room = MAX_CHAT_LENGTH.saturating_sub(bar.input.chars().count());
                    let typed = text.chars().filter(|c| !c.is_control()).take(room);
                    bar.input.extend(typed);
                }
            }
        }
    }
    // nothing typed moves the player, toggles the overlay or turns the sun
    keys.reset_all();
}

fn print_output(
    mut output: MessageReader<CommandOutput>,
    mut chat: MessageReader<ChatLine>,
    mut bar: ResMut<CommandBar>,
    time: Res<Time<Real>>,
) {
    let now = time.elapsed_secs();
    for CommandOutput { text, error } in output.read() {
        bar.print(text.as_str(), *error, now);
    }
    for ChatLine { from, text } in chat.read() {
        bar.print(format!("<{}> {text}", from.as_str()), false, now);
    }
}

#[allow(clippy::type_complexity)]
fn draw_bar(
    bar: Res<CommandBar>,
    time: Res<Time<Real>>,
    mut commands: Commands,
    mut output: Query<(Entity, &mut Visibility), (With<Output>, Without<Input>)>,
    mut input: Query<(&mut Text, &mut Visibility), (With<Input>, Without<Output>)>,
    // the lines drawn last, as how many had been printed and how many of the newest were up
    mut drawn: Local<(usize, usize)>,
) {
    let now = time.elapsed_secs();
    if let Ok((mut text, mut visibility)) = input.single_mut() {
        let wanted = format!("{}_", bar.input);
        if text.0 != wanted {
            text.0 = wanted;
        }
        visibility.set_if_neq(if bar.open {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }

    // the lines still up, as spans so errors can be coloured
    let shown: Vec<&Line> = bar
        .lines
        .iter()
        .filter(|line| bar.open || now - line.at < LINE_SECONDS)
        .collect();
    let Ok((entity, mut visibility)) = output.single_mut() else {
        return;
    };
    visibility.set_if_neq(if shown.is_empty() {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    });
    if *drawn == (bar.printed, shown.len()) {
        return;
    }
    *drawn = (bar.printed, shown.len());
    let mut entity = commands.entity(entity);
    entity.despawn_related::<Children>();
    entity.with_children(|output| {
        for (i, line) in shown.iter().enumerate() {
            let newline = if i + 1 < shown.len() { "\n" } else { "" };
            output.spawn((
                TextSpan::new(format!("{}{newline}", line.text)),
                TextColor(if line.error { ERROR_COLOR } else { TEXT_COLOR }),
            ));
        }
    });
}

// The whole plugin, headless and played in singleplayer: key presses in, run by the server, the
// player's position and the bar's output out.
#[cfg(test)]
mod tests {
    use bevy::input::keyboard::{Key, KeyboardInput};
    use bevy::input::{ButtonState, InputPlugin};
    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin;
    use bevy_replicon::prelude::*;

    use super::{CommandBar, CommandPlugin};
    use crate::client::player::LocalPlayer;
    use crate::game_mode::{GameMode, GameModePlugin};
    use crate::menu::GameState;
    use crate::player::name::PlayerName;
    use crate::player::{Movement, Player, PlayerOwner};
    use crate::protocol::{ProtocolPlugin, RunCommand};
    use crate::server::{Operators, ServerPlugin};
    use crate::world::World;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            InputPlugin,
            StatesPlugin,
            RepliconPlugins,
            ProtocolPlugin,
            ServerPlugin,
            CommandPlugin,
            GameModePlugin,
        ))
        .insert_state(GameState::Playing)
        .insert_resource(World::new(16));
        app.world_mut().spawn((
            Player::default(),
            PlayerOwner(ClientId::Server),
            PlayerName::new("host"),
            LocalPlayer,
            GameMode::Creative,
            Transform::default(),
        ));
        app.finish();
        app.update();
        app
    }

    fn press(app: &mut App, key_code: KeyCode, text: Option<&str>) {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            app.world_mut().write_message(KeyboardInput {
                key_code,
                logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
                state,
                text: text
                    .filter(|_| state == ButtonState::Pressed)
                    .map(Into::into),
                repeat: false,
                window: Entity::PLACEHOLDER,
            });
            app.update();
        }
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            let key = match c {
                ' ' => KeyCode::Space,
                '-' => KeyCode::Minus,
                _ => KeyCode::KeyA,
            };
            press(app, key, Some(&c.to_string()));
        }
    }

    fn run(app: &mut App, command: &str) {
        press(app, KeyCode::Slash, Some("/"));
        type_text(app, command);
        press(app, KeyCode::Enter, None);
    }

    fn player(app: &mut App) -> Vec3 {
        app.world_mut()
            .query_filtered::<&Transform, With<LocalPlayer>>()
            .single(app.world())
            .unwrap()
            .translation
    }

    fn mode(app: &App, player: Entity) -> GameMode {
        *app.world().get::<GameMode>(player).unwrap()
    }

    fn printed(app: &App) -> Vec<&str> {
        let bar = app.world().resource::<CommandBar>();
        bar.lines.iter().map(|line| line.text.as_str()).collect()
    }

    #[test]
    fn test_slash_opens_the_bar_and_enter_teleports() {
        let mut app = app();
        press(&mut app, KeyCode::Slash, Some("/"));
        assert!(app.world().resource::<CommandBar>().open);
        assert_eq!("/", app.world().resource::<CommandBar>().input);
        type_text(&mut app, "tp 12 34 -56");
        assert_eq!("/tp 12 34 -56", app.world().resource::<CommandBar>().input);
        press(&mut app, KeyCode::Enter, None);
        assert!(!app.world().resource::<CommandBar>().open);
        assert_eq!(Vec3::new(12.0, 34.0, -56.0), player(&mut app));
        assert_eq!(vec!["Teleported to 12.0, 34.0, -56.0"], printed(&app));
    }

    #[test]
    fn test_gamemode_switches_the_players_mode_and_lands_them() {
        let mut app = app();
        let host = app
            .world_mut()
            .query_filtered::<Entity, With<LocalPlayer>>()
            .single(app.world())
            .unwrap();
        run(&mut app, "gamemode s");
        assert_eq!(GameMode::Survival, mode(&app, host));
        let movement = app.world().get::<Player>(host).unwrap().movement;
        assert_eq!(Movement::Walk, movement);
    }

    #[test]
    fn test_a_command_runs_for_the_player_who_sent_it_and_answers_only_them() {
        let mut app = app();
        let client = ClientId::Client(app.world_mut().spawn_empty().id());
        let guest = app
            .world_mut()
            .spawn((
                Player::default(),
                PlayerOwner(client),
                PlayerName::new("guest"),
                Transform::default(),
            ))
            .id();
        app.insert_resource(Operators::new(vec!["guest".into()]));
        app.world_mut().write_message(FromClient {
            client_id: client,
            message: RunCommand("/gamemode s".into()),
        });
        app.update();
        assert_eq!(GameMode::Survival, mode(&app, guest));
        let host = app
            .world_mut()
            .query_filtered::<&GameMode, With<LocalPlayer>>()
            .single(app.world())
            .unwrap();
        assert_eq!(GameMode::Creative, *host);
        assert!(printed(&app).is_empty());

        // and the host's own are answered in their bar, errors and all
        run(&mut app, "seed");
        run(&mut app, "fly");
        let seed = app.world().resource::<World>().seed();
        assert_eq!(
            vec![
                format!("Seed: {seed}").as_str(),
                "Unknown command /fly, try /help"
            ],
            printed(&app)
        );
        let bar = app.world().resource::<CommandBar>();
        assert!(!bar.lines[0].error && bar.lines[1].error);
    }

    #[test]
    fn test_chat_in_singleplayer_comes_back_by_name() {
        let mut app = app();
        press(&mut app, KeyCode::Enter, None);
        type_text(&mut app, "hello there");
        press(&mut app, KeyCode::Enter, None);
        assert_eq!(vec!["<host> hello there"], printed(&app));
    }

    #[test]
    fn test_typing_doesnt_reach_the_game_and_escape_cancels() {
        let mut app = app();
        press(&mut app, KeyCode::KeyT, Some("t"));
        assert_eq!("", app.world().resource::<CommandBar>().input);
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::KeyW,
            logical_key: Key::Character("w".into()),
            state: ButtonState::Pressed,
            text: Some("w".into()),
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
        assert!(!app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW));
        press(&mut app, KeyCode::Backspace, None);
        type_text(&mut app, "/tp 1 2 3");
        press(&mut app, KeyCode::Escape, None);
        assert!(!app.world().resource::<CommandBar>().open);
        assert_eq!(Vec3::ZERO, player(&mut app));
    }
}
