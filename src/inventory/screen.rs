use bevy::picking::events::PointerPress;
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui_widgets::Button;
use bevy::window::{CursorOptions, PrimaryWindow};

use super::icons::{load_icons, stack_view, ItemIcons, StackView};
use super::{Click, Inventory, HOTBAR_SLOTS, SLOTS};
use crate::client::player::LocalPlayer;
use crate::game_mode::GameMode;
use crate::menu::{font, free_cursor, GameState, BUTTON_BORDER_COLOR, BUTTON_COLOR, TEXT_SHADOW};
use crate::protocol::{CloseInventory, InventoryClick};

// In pixels: slots like the block picker's buttons, and the gap between the main inventory and
// the hotbar under it.
const SLOT_SIZE: f32 = 48.0;
const SLOT_BORDER: f32 = 2.0;
const SLOT_GAP: f32 = 4.0;
const HOTBAR_GAP: f32 = 16.0;
// half an icon, so the stack carried is held by its middle
const CARRIED_OFFSET: f32 = 16.0;

const HINT: &str =
    "Right click takes half a stack or puts down one, shift-click moves a stack across";

/// In survival, the inventory (E) is the inventory screen: the main inventory's 27 slots over
/// the hotbar's 9, as in Minecraft. Clicks on them ask the server to move stacks about, and the
/// stack picked up follows the mouse until it's put down. The game carries on behind it, as
/// behind the block picker, and closing it puts the stack carried back.
pub struct InventoryScreenPlugin;

impl Plugin for InventoryScreenPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_screen.after(load_icons))
            .add_systems(
                Update,
                (carry_to_mouse, name_hovered_item).run_if(in_state(GameState::Inventory)),
            )
            .add_systems(OnEnter(GameState::Inventory), open)
            .add_systems(OnExit(GameState::Inventory), close);
    }
}

#[derive(Component)]
struct Screen;

#[derive(Component)]
struct Slot(usize);

// The stack on the cursor.
#[derive(Component)]
struct Carried;

#[derive(Component)]
struct ItemName;

fn spawn_screen(mut commands: Commands, icons: Res<ItemIcons>) {
    commands
        .spawn((
            Screen,
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
            // over the debug overlay, as the block picker is
            GlobalZIndex(1),
        ))
        .with_children(|screen| {
            screen.spawn((
                Text::new("Inventory"),
                font(24.0),
                TextColor(Color::WHITE),
                TEXT_SHADOW,
                Node {
                    margin: UiRect::bottom(Val::Px(20.0)),
                    ..default()
                },
            ));
            spawn_slots(screen, &icons, HOTBAR_SLOTS..SLOTS, 0.0);
            spawn_slots(screen, &icons, 0..HOTBAR_SLOTS, HOTBAR_GAP);
            screen.spawn((
                ItemName,
                Text::new(HINT),
                font(18.0),
                TEXT_SHADOW,
                Node {
                    margin: UiRect::top(Val::Px(10.0)),
                    ..default()
                },
            ));
            // last, so it's drawn over the slots
            screen.spawn((
                Carried,
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                Pickable::IGNORE,
                children![stack_view(&icons, StackView::Cursor)],
            ));
        });
}

// A grid of `slots`, nine to a row, `gap` pixels below what's above it. Each is a button like
// the block picker's, which `highlight_buttons` lights up under the pointer.
fn spawn_slots(
    screen: &mut ChildSpawnerCommands,
    icons: &ItemIcons,
    slots: std::ops::Range<usize>,
    gap: f32,
) {
    screen
        .spawn(Node {
            display: Display::Grid,
            grid_template_columns: RepeatedGridTrack::px(HOTBAR_SLOTS as u16, SLOT_SIZE),
            row_gap: Val::Px(SLOT_GAP),
            column_gap: Val::Px(SLOT_GAP),
            margin: UiRect::top(Val::Px(gap)),
            ..default()
        })
        .with_children(|grid| {
            for slot in slots {
                grid.spawn((
                    Slot(slot),
                    Button,
                    Hovered::default(),
                    Node {
                        width: Val::Px(SLOT_SIZE),
                        height: Val::Px(SLOT_SIZE),
                        border: UiRect::all(Val::Px(SLOT_BORDER)),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(BUTTON_COLOR),
                    BorderColor::all(BUTTON_BORDER_COLOR),
                ))
                .with_child(stack_view(icons, StackView::Slot(slot)))
                .observe(click_slot);
            }
        });
}

// The server makes the change, as it does all changes to inventories (INV-4), in this frame in
// singleplayer.
fn click_slot(
    press: On<PointerPress>,
    slots: Query<&Slot>,
    keys: Res<ButtonInput<KeyCode>>,
    mut clicks: MessageWriter<InventoryClick>,
) {
    let Ok(slot) = slots.get(press.entity) else {
        return;
    };
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if let Some(click) = click_of(press.button, shift) {
        clicks.write(InventoryClick {
            slot: slot.0 as u8,
            click,
        });
    }
}

fn click_of(button: PointerButton, shift: bool) -> Option<Click> {
    match button {
        PointerButton::Primary if shift => Some(Click::Shift),
        PointerButton::Primary => Some(Click::Left),
        PointerButton::Secondary => Some(Click::Right),
        PointerButton::Middle => None,
    }
}

// Moved only when the mouse is, so the layout isn't redone every frame.
fn carry_to_mouse(
    window: Query<&Window, With<PrimaryWindow>>,
    mut carried: Query<&mut Node, With<Carried>>,
) {
    let Some(at) = window.single().ok().and_then(Window::cursor_position) else {
        return;
    };
    let (left, top) = (
        Val::Px(at.x - CARRIED_OFFSET),
        Val::Px(at.y - CARRIED_OFFSET),
    );
    for mut node in &mut carried {
        if node.left != left || node.top != top {
            node.left = left;
            node.top = top;
        }
    }
}

fn name_hovered_item(
    changed: Query<(), (With<Slot>, Changed<Hovered>)>,
    player: Query<Ref<Inventory>, With<LocalPlayer>>,
    slots: Query<(&Slot, &Hovered)>,
    mut name: Query<&mut Text, With<ItemName>>,
) {
    let Ok(inventory) = player.single() else {
        return;
    };
    if changed.is_empty() && !inventory.is_changed() {
        return;
    }
    let hovered = slots.iter().find(|(_, hovered)| hovered.get());
    let item = hovered.and_then(|(slot, _)| inventory.slots[slot.0]);
    if let Ok(mut text) = name.single_mut() {
        text.0 = item.map_or_else(|| HINT.into(), |stack| stack.item.display_name());
    }
}

// In creative the inventory is the block picker instead.
fn open(
    player: Query<&GameMode, With<LocalPlayer>>,
    mut screen: Query<&mut Node, With<Screen>>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    if player.single().is_ok_and(|mode| !mode.has_every_block()) {
        for mut node in &mut screen {
            node.display = Display::Flex;
        }
        free_cursor(&mut cursor, true);
    }
}

// The menu holds the cursor again as play resumes.
fn close(mut screen: Query<&mut Node, With<Screen>>, mut closes: MessageWriter<CloseInventory>) {
    for mut node in &mut screen {
        if node.display == Display::Flex {
            node.display = Display::None;
            closes.write(CloseInventory);
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
    use bevy::input::{ButtonState, InputPlugin};
    use bevy::picking::pointer::PointerButton;
    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin;
    use bevy::window::WindowFocused;
    use bevy_replicon::prelude::ClientId;
    use bevy_replicon::RepliconPlugins;

    use super::{click_of, InventoryScreenPlugin, Screen};
    use crate::block::BlockType;
    use crate::client::player::LocalPlayer;
    use crate::commands::CommandPlugin;
    use crate::game_mode::GameMode;
    use crate::hotbar::HotbarPlugin;
    use crate::inventory::{Click, Inventory};
    use crate::items::{Item, ItemStack};
    use crate::menu::{GameState, MenuPlugin};
    use crate::picker::PickerPlugin;
    use crate::player::{Player, PlayerOwner};
    use crate::protocol::{InventoryClick, ProtocolPlugin};
    use crate::server::ServerPlugin;
    use crate::world::World;

    // The game's UI and the server, headless, with a player in `mode` carrying some dirt.
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
            InventoryScreenPlugin,
            ServerPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<TextureAtlasLayout>()
        .add_message::<WindowFocused>()
        .insert_resource(World::new(16));
        let mut inventory = Inventory::default();
        inventory.slots[0] = Some(ItemStack::new(Item::Block(BlockType::Dirt), 5));
        app.world_mut().spawn((
            Player::default(),
            PlayerOwner(ClientId::Server),
            LocalPlayer,
            mode,
            inventory,
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

    fn shown(app: &mut App) -> bool {
        let mut screen = app.world_mut().query_filtered::<&Node, With<Screen>>();
        screen.single(app.world()).unwrap().display == Display::Flex
    }

    fn inventory(app: &mut App) -> Inventory {
        let mut query = app.world_mut().query::<&Inventory>();
        query.single(app.world()).unwrap().clone()
    }

    #[test]
    fn test_e_opens_the_inventory_screen_in_survival_and_the_picker_in_creative() {
        let mut survival = app(GameMode::Survival);
        press(&mut survival, KeyCode::KeyE);
        assert_eq!(
            GameState::Inventory,
            *survival.world().resource::<State<GameState>>().get()
        );
        assert!(shown(&mut survival));
        press(&mut survival, KeyCode::KeyE);
        assert!(!shown(&mut survival));

        let mut creative = app(GameMode::Creative);
        press(&mut creative, KeyCode::KeyE);
        assert!(!shown(&mut creative));
    }

    #[test]
    fn test_clicks_are_left_right_or_shift() {
        assert_eq!(Some(Click::Left), click_of(PointerButton::Primary, false));
        assert_eq!(Some(Click::Shift), click_of(PointerButton::Primary, true));
        assert_eq!(
            Some(Click::Right),
            click_of(PointerButton::Secondary, false)
        );
        assert_eq!(None, click_of(PointerButton::Middle, false));
    }

    #[test]
    fn test_closing_puts_the_stack_carried_back() {
        let mut app = app(GameMode::Survival);
        press(&mut app, KeyCode::KeyE);
        app.world_mut().write_message(InventoryClick {
            slot: 0,
            click: Click::Left,
        });
        app.update();
        assert!(inventory(&mut app).cursor.is_some());
        press(&mut app, KeyCode::Escape);
        let dirt = Some(ItemStack::new(Item::Block(BlockType::Dirt), 5));
        assert_eq!(
            (dirt, None),
            (inventory(&mut app).slots[0], inventory(&mut app).cursor)
        );
    }
}
