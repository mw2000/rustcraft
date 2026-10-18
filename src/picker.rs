use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use bevy::window::{CursorOptions, PrimaryWindow};

use crate::block::BlockType;
use crate::client::player::LocalPlayer;
use crate::game_mode::GameMode;
use crate::hotbar::{load_icons, BlockIcons, Hotbar, SLOTS};
use crate::menu::{font, free_cursor, GameState, BUTTON_BORDER_COLOR, BUTTON_COLOR, TEXT_SHADOW};

const TOGGLE_KEY: KeyCode = KeyCode::KeyE;

// In pixels: a grid as wide as the hotbar has slots, of buttons a little bigger than them.
const TILE_SIZE: f32 = 48.0;
const TILE_BORDER: f32 = 2.0;
const TILE_GAP: f32 = 4.0;

const HINT: &str = "Click a block to put it in the selected slot";

/// In creative, E opens a panel of every block to build with, and clicking one puts it in the
/// hotbar's selected slot. The game carries on behind it, but the player stands still and the
/// cursor's let go. E or Escape closes it.
pub struct PickerPlugin;

impl Plugin for PickerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_picker.after(load_icons))
            .add_systems(Update, (toggle_picker, name_hovered_block))
            .add_systems(OnEnter(GameState::Inventory), open)
            .add_systems(OnExit(GameState::Inventory), close);
    }
}

#[derive(Component)]
struct Picker;

#[derive(Component)]
struct Tile(BlockType);

#[derive(Component)]
struct BlockName;

fn spawn_picker(mut commands: Commands, icons: Res<BlockIcons>) {
    commands
        .spawn((
            Picker,
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
            // over the debug overlay
            GlobalZIndex(1),
        ))
        .with_children(|picker| {
            picker.spawn((
                Text::new("Blocks"),
                font(24.0),
                TextColor(Color::WHITE),
                TEXT_SHADOW,
                Node {
                    margin: UiRect::bottom(Val::Px(20.0)),
                    ..default()
                },
            ));
            picker
                .spawn(Node {
                    display: Display::Grid,
                    grid_template_columns: RepeatedGridTrack::px(SLOTS as u16, TILE_SIZE),
                    row_gap: Val::Px(TILE_GAP),
                    column_gap: Val::Px(TILE_GAP),
                    ..default()
                })
                .with_children(|grid| {
                    // the solid blocks, which is all but air and water
                    for block in BlockType::ALL.into_iter().filter(|block| block.is_solid()) {
                        grid.spawn(tile(block))
                            .with_child(icons.icon(block))
                            .observe(put_in_hotbar);
                    }
                });
            picker.spawn((
                BlockName,
                Text::new(HINT),
                font(18.0),
                TEXT_SHADOW,
                Node {
                    margin: UiRect::top(Val::Px(10.0)),
                    ..default()
                },
            ));
        });
}

// A button like the menu's, which `highlight_buttons` lights up under the pointer.
fn tile(block: BlockType) -> impl Bundle {
    (
        Tile(block),
        Button,
        Hovered::default(),
        Node {
            width: Val::Px(TILE_SIZE),
            height: Val::Px(TILE_SIZE),
            border: UiRect::all(Val::Px(TILE_BORDER)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(BUTTON_COLOR),
        BorderColor::all(BUTTON_BORDER_COLOR),
    )
}

fn put_in_hotbar(
    activate: On<Activate>,
    tiles: Query<&Tile>,
    mut hotbar: Query<&mut Hotbar, With<LocalPlayer>>,
) {
    if let (Ok(tile), Ok(mut hotbar)) = (tiles.get(activate.entity), hotbar.single_mut()) {
        let selected = hotbar.selected;
        hotbar.slots[selected] = Some(tile.0);
    }
}

// Escape closes it too, see `toggle_menu`. In survival there's nothing to open until M1.1
// makes this the inventory.
fn toggle_picker(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
    player: Query<&GameMode, With<LocalPlayer>>,
) {
    if !keys.just_pressed(TOGGLE_KEY) {
        return;
    }
    match state.get() {
        GameState::Playing if player.single().is_ok_and(|mode| mode.has_every_block()) => {
            next.set(GameState::Inventory)
        }
        GameState::Inventory => next.set(GameState::Playing),
        _ => {}
    }
}

fn open(
    mut picker: Query<&mut Node, With<Picker>>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    for mut node in &mut picker {
        node.display = Display::Flex;
    }
    free_cursor(&mut cursor, true);
}

// The menu holds the cursor again as play resumes.
fn close(mut picker: Query<&mut Node, With<Picker>>) {
    for mut node in &mut picker {
        node.display = Display::None;
    }
}

fn name_hovered_block(
    changed: Query<(), (With<Tile>, Changed<Hovered>)>,
    tiles: Query<(&Tile, &Hovered)>,
    mut name: Query<&mut Text, With<BlockName>>,
) {
    if changed.is_empty() {
        return;
    }
    let hovered = tiles.iter().find(|(_, hovered)| hovered.get());
    if let Ok(mut text) = name.single_mut() {
        text.0 = hovered.map_or_else(|| HINT.into(), |(tile, _)| display_name(tile.0));
    }
}

// "stone_bricks" as "Stone bricks".
fn display_name(block: BlockType) -> String {
    let mut name = block.name().replace('_', " ");
    name[..1].make_ascii_uppercase();
    name
}

#[cfg(test)]
mod tests {
    use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
    use bevy::input::{ButtonState, InputPlugin};
    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin;
    use bevy::ui_widgets::Activate;
    use bevy::window::WindowFocused;

    use super::{display_name, PickerPlugin, Tile};
    use crate::block::BlockType;
    use crate::client::player::LocalPlayer;
    use crate::commands::{CommandBar, CommandPlugin};
    use crate::game_mode::GameMode;
    use crate::hotbar::{Hotbar, HotbarPlugin};
    use crate::menu::{GameState, MenuPlugin};
    use crate::player::Player;

    #[test]
    fn test_display_names() {
        assert_eq!("Stone", display_name(BlockType::Stone));
        assert_eq!("Crafting table", display_name(BlockType::CraftingTable));
    }

    // The game's UI, headless, with a player in `mode`.
    fn app(mode: GameMode) -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            InputPlugin,
            StatesPlugin,
            AssetPlugin::default(),
            MenuPlugin,
            CommandPlugin,
            HotbarPlugin,
            PickerPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<TextureAtlasLayout>()
        .add_message::<WindowFocused>();
        app.world_mut()
            .spawn((Player::default(), LocalPlayer, mode));
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

    #[test]
    fn test_e_opens_and_closes_the_picker() {
        let mut app = app(GameMode::Creative);
        press(&mut app, KeyCode::KeyE);
        assert_eq!(GameState::Inventory, state(&app));
        press(&mut app, KeyCode::KeyE);
        assert_eq!(GameState::Playing, state(&app));
    }

    #[test]
    fn test_escape_closes_the_picker_without_pausing() {
        let mut app = app(GameMode::Creative);
        press(&mut app, KeyCode::KeyE);
        press(&mut app, KeyCode::Escape);
        assert_eq!(GameState::Playing, state(&app));
        assert!(!app.world().resource::<Time<Virtual>>().is_paused());
    }

    #[test]
    fn test_the_command_bar_cant_open_over_the_picker() {
        let mut app = app(GameMode::Creative);
        press(&mut app, KeyCode::KeyE);
        press(&mut app, KeyCode::KeyT);
        assert!(!app.world().resource::<CommandBar>().is_open());
        assert_eq!(GameState::Inventory, state(&app));
    }

    #[test]
    fn test_survival_has_no_picker() {
        let mut app = app(GameMode::Survival);
        press(&mut app, KeyCode::KeyE);
        assert_eq!(GameState::Playing, state(&app));
    }

    #[test]
    fn test_clicking_a_block_puts_it_in_the_selected_slot() {
        let mut app = app(GameMode::Creative);
        press(&mut app, KeyCode::KeyE);
        // the slot's chosen with the picker open
        press(&mut app, KeyCode::Digit3);
        let furnace = app
            .world_mut()
            .query::<(Entity, &Tile)>()
            .iter(app.world())
            .find(|(_, tile)| tile.0 == BlockType::Furnace)
            .unwrap()
            .0;
        app.world_mut().trigger(Activate { entity: furnace });
        let hotbar = app
            .world_mut()
            .query::<&Hotbar>()
            .single(app.world())
            .unwrap();
        assert_eq!(Some(BlockType::Furnace), hotbar.slots[2]);
        assert_eq!(Some(BlockType::Furnace), hotbar.held());
    }

    #[test]
    fn test_every_solid_block_and_nothing_else_is_offered() {
        let mut app = app(GameMode::Creative);
        let offered: Vec<BlockType> = app
            .world_mut()
            .query::<&Tile>()
            .iter(app.world())
            .map(|tile| tile.0)
            .collect();
        assert!(!offered.contains(&BlockType::Air));
        assert!(!offered.contains(&BlockType::Water));
        assert_eq!(BlockType::ALL.len() - 2, offered.len());
    }
}
