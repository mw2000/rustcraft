use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use bevy::window::{CursorOptions, PrimaryWindow};

use crate::block::BlockType;
use crate::client::player::LocalPlayer;
use crate::game_mode::GameMode;
use crate::inventory::icons::{load_icons, ItemIcons};
use crate::inventory::Hotbar;
use crate::inventory::HOTBAR_SLOTS;
use crate::items::Item;
use crate::menu::{font, free_cursor, GameState, BUTTON_BORDER_COLOR, BUTTON_COLOR, TEXT_SHADOW};
use crate::protocol::PickBlock;

// In pixels: a grid as wide as the hotbar has slots, of buttons a little bigger than them.
const TILE_SIZE: f32 = 48.0;
const TILE_BORDER: f32 = 2.0;
const TILE_GAP: f32 = 4.0;

const HINT: &str = "Click a block to put it in the selected slot";

/// In creative, the inventory (E) is a panel of every block to build with, and of seeds to plant,
/// and clicking one puts a stack of it in the hotbar's selected slot. The game carries on behind it, but the player
/// stands still and the cursor's let go. E or Escape closes it.
pub struct PickerPlugin;

impl Plugin for PickerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_picker.after(load_icons))
            .add_systems(Update, name_hovered_block)
            .add_systems(OnEnter(GameState::Inventory), open)
            .add_systems(OnExit(GameState::Inventory), close);
    }
}

#[derive(Component)]
struct Picker;

#[derive(Component)]
struct Tile(Item);

// What's offered: every block but air, water and wheat that's still growing, which only grows
// from seeds, and the seeds, after the blocks.
fn offered() -> impl Iterator<Item = Item> {
    let growing = |block: BlockType| block.is_crop() && block != BlockType::RipeWheat;
    Item::all().filter(move |item| match item.block() {
        Some(block) => !growing(block),
        None => item.places().is_some(),
    })
}

#[derive(Component)]
struct BlockName;

fn spawn_picker(mut commands: Commands, icons: Res<ItemIcons>) {
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
                    grid_template_columns: RepeatedGridTrack::px(HOTBAR_SLOTS as u16, TILE_SIZE),
                    row_gap: Val::Px(TILE_GAP),
                    column_gap: Val::Px(TILE_GAP),
                    ..default()
                })
                .with_children(|grid| {
                    for item in offered() {
                        grid.spawn(tile(item))
                            .with_child(icons.icon(item))
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
fn tile(item: Item) -> impl Bundle {
    (
        Tile(item),
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

// The server puts the stack there, as it does all changes to inventories (INV-4).
fn put_in_hotbar(
    activate: On<Activate>,
    tiles: Query<&Tile>,
    hotbar: Query<&Hotbar, With<LocalPlayer>>,
    mut picks: MessageWriter<PickBlock>,
) {
    if let (Ok(tile), Ok(hotbar)) = (tiles.get(activate.entity), hotbar.single()) {
        picks.write(PickBlock {
            slot: hotbar.selected as u8,
            item: tile.0,
        });
    }
}

// In survival the inventory is the inventory screen instead.
fn open(
    player: Query<&GameMode, With<LocalPlayer>>,
    mut picker: Query<&mut Node, With<Picker>>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    if player.single().is_ok_and(|mode| mode.has_every_block()) {
        for mut node in &mut picker {
            node.display = Display::Flex;
        }
        free_cursor(&mut cursor, true);
    }
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
        text.0 = hovered.map_or_else(|| HINT.into(), |(tile, _)| tile.0.display_name());
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
    use bevy::input::{ButtonState, InputPlugin};
    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin;
    use bevy::ui_widgets::Activate;
    use bevy::window::WindowFocused;
    use bevy_replicon::prelude::ClientId;
    use bevy_replicon::RepliconPlugins;

    use super::{Picker, PickerPlugin, Tile};
    use crate::block::BlockType;
    use crate::client::command_bar::{CommandBar, CommandPlugin};
    use crate::client::player::LocalPlayer;
    use crate::game_mode::GameMode;
    use crate::hotbar::HotbarPlugin;
    use crate::inventory::Hotbar;
    use crate::inventory::Inventory;
    use crate::items::{Item, ItemStack};
    use crate::menu::{GameState, MenuPlugin};
    use crate::player::{Player, PlayerOwner};
    use crate::protocol::ProtocolPlugin;
    use crate::server::ServerPlugin;
    use crate::world::World;

    // The game's UI, headless, with the server and a player in `mode`.
    fn app(mode: GameMode) -> App {
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
            PickerPlugin,
            ServerPlugin,
        ))
        .insert_state(GameState::Playing)
        .init_asset::<Image>()
        .init_asset::<TextureAtlasLayout>()
        .add_message::<WindowFocused>()
        .insert_resource(World::new(16));
        app.world_mut().spawn((
            Player::default(),
            PlayerOwner(ClientId::Server),
            LocalPlayer,
            mode,
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

    fn shown(app: &mut App) -> bool {
        let mut picker = app.world_mut().query_filtered::<&Node, With<Picker>>();
        picker.single(app.world()).unwrap().display == Display::Flex
    }

    #[test]
    fn test_survival_has_no_picker() {
        let mut creative = app(GameMode::Creative);
        press(&mut creative, KeyCode::KeyE);
        assert!(shown(&mut creative));
        let mut survival = app(GameMode::Survival);
        press(&mut survival, KeyCode::KeyE);
        assert!(!shown(&mut survival));
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
            .find(|(_, tile)| tile.0 == Item::Block(BlockType::Furnace))
            .unwrap()
            .0;
        app.world_mut().trigger(Activate { entity: furnace });
        app.update();
        let (hotbar, inventory) = app
            .world_mut()
            .query::<(&Hotbar, &Inventory)>()
            .single(app.world())
            .unwrap();
        let furnaces = Some(ItemStack::full(Item::Block(BlockType::Furnace)));
        assert_eq!(furnaces, inventory.slots[2]);
        assert_eq!(furnaces, hotbar.held(inventory));
    }

    #[test]
    fn test_every_block_to_build_with_and_seeds_are_offered() {
        let mut app = app(GameMode::Creative);
        let offered: Vec<Item> = app
            .world_mut()
            .query::<&Tile>()
            .iter(app.world())
            .map(|tile| tile.0)
            .collect();
        let block = Item::Block;
        for wanted in [
            block(BlockType::Stone),
            block(BlockType::Farmland),
            block(BlockType::RipeWheat),
            Item::Seeds,
        ] {
            assert!(offered.contains(&wanted), "{wanted:?}");
        }
        for unwanted in [block(BlockType::Wheat3), Item::Wheat, Item::IronHoe] {
            assert!(!offered.contains(&unwanted), "{unwanted:?}");
        }
        // all but air, water and the seven stages wheat grows through, and seeds
        assert_eq!(BlockType::ALL.len() - 2 - 7 + 1, offered.len());
    }
}
