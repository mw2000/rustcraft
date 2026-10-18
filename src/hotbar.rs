use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;

use crate::client::command_bar::command_bar_closed;
use crate::client::player::LocalPlayer;
use crate::controls::{Action, Bindings, Controls};
use crate::game_mode::GameMode;
use crate::inventory::icons::{draw_stacks, load_icons, stack_view, ItemIcons, StackView};
use crate::inventory::{Hotbar, HOTBAR_SLOTS};
use crate::menu::{GameState, InWorld};
use crate::protocol::DropItem;

// Touchpads scroll in pixels, which are gathered up into notches of a mouse wheel.
const PIXELS_PER_NOTCH: f32 = 40.0;

// In pixels: a strip of slots, each with an item's icon in it.
const SLOT_SIZE: f32 = 40.0;
const SLOT_BORDER: f32 = 2.0;
const SLOT_GAP: f32 = 2.0;
const MARGIN: f32 = 6.0;
/// How far up from the bottom of the screen the hotbar reaches, in pixels, for UI to go above.
pub const HOTBAR_TOP: f32 = MARGIN + SLOT_SIZE + 2.0 * SLOT_GAP;

// Like the command bar's, with the selected slot framed in white.
const STRIP_COLOR: Color = Color::srgba(0.0, 0.0, 0.0, 0.5);
const SLOT_BORDER_COLOR: Color = Color::srgba(0.55, 0.55, 0.55, 0.6);
const SELECTED_BORDER_COLOR: Color = Color::WHITE;
const SELECTED_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.15);

/// The player's hotbar on screen, choosing a slot in it and dropping what's in it, and the
/// icons of items it and the other screens draw.
pub struct HotbarPlugin;

impl Plugin for HotbarPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Bindings>()
            .add_systems(Startup, (load_icons, spawn_hotbar).chain())
            .add_systems(
                Update,
                (
                    // and while the block picker's open, to choose the slot a block goes in, but
                    // not while the wheel zooms the map
                    select_slot
                        .run_if(not(in_state(GameState::Paused)))
                        .run_if(not(in_state(GameState::Map)))
                        .run_if(command_bar_closed),
                    (draw_selection, draw_stacks),
                )
                    .chain(),
            )
            .add_systems(
                Update,
                drop_held
                    .run_if(in_state(GameState::Playing))
                    .run_if(command_bar_closed),
            )
            .add_systems(OnEnter(GameState::Inventory), hide_hotbar)
            .add_systems(OnExit(GameState::Inventory), show_hotbar)
            .add_systems(OnEnter(GameState::Map), hide_hotbar)
            .add_systems(OnExit(GameState::Map), show_hotbar);
    }
}

#[derive(Component)]
struct Strip;

#[derive(Component)]
struct Slot(usize);

fn spawn_hotbar(mut commands: Commands, icons: Res<ItemIcons>) {
    commands
        .spawn((
            Strip,
            InWorld,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(MARGIN),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            // over the block picker, so its slots can be seen filling
            GlobalZIndex(2),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    padding: UiRect::all(Val::Px(SLOT_GAP)),
                    column_gap: Val::Px(SLOT_GAP),
                    ..default()
                },
                BackgroundColor(STRIP_COLOR),
            ))
            .with_children(|strip| {
                for slot in 0..HOTBAR_SLOTS {
                    strip
                        .spawn((
                            Slot(slot),
                            Node {
                                width: Val::Px(SLOT_SIZE),
                                height: Val::Px(SLOT_SIZE),
                                border: UiRect::all(Val::Px(SLOT_BORDER)),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BorderColor::all(SLOT_BORDER_COLOR),
                            BackgroundColor(Color::NONE),
                        ))
                        .with_child(stack_view(&icons, StackView::Slot(slot)));
                }
            });
        });
}

fn select_slot(
    controls: Controls,
    scroll: Res<AccumulatedMouseScroll>,
    mut hotbar: Query<&mut Hotbar, With<LocalPlayer>>,
    // how far the wheel's turned towards the next notch
    mut notches: Local<f32>,
) {
    let Ok(mut hotbar) = hotbar.single_mut() else {
        return;
    };
    if let Some(slot) = Action::SLOTS
        .iter()
        .position(|&slot| controls.just_pressed(slot))
    {
        hotbar.selected = slot;
    }
    *notches += match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / PIXELS_PER_NOTCH,
    };
    let turned = notches.trunc();
    if turned != 0.0 {
        *notches -= turned;
        // scrolling down moves to the right, as in Minecraft
        hotbar.scroll(-turned as i32);
    }
}

// Q throws one of what's held, and Ctrl+Q the whole stack, as in Minecraft.
fn drop_held(
    controls: Controls,
    keys: Res<ButtonInput<KeyCode>>,
    hotbar: Query<&Hotbar, With<LocalPlayer>>,
    mut drops: MessageWriter<DropItem>,
) {
    let Ok(hotbar) = hotbar.single() else {
        return;
    };
    if controls.just_pressed(Action::Drop) {
        drops.write(DropItem {
            slot: hotbar.selected as u8,
            all: keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
        });
    }
}

fn draw_selection(
    hotbar: Query<&Hotbar, (With<LocalPlayer>, Changed<Hotbar>)>,
    mut slots: Query<(&Slot, &mut BorderColor, &mut BackgroundColor)>,
) {
    let Ok(hotbar) = hotbar.single() else {
        return;
    };
    for (slot, mut border, mut background) in &mut slots {
        let selected = slot.0 == hotbar.selected;
        *border = BorderColor::all(if selected {
            SELECTED_BORDER_COLOR
        } else {
            SLOT_BORDER_COLOR
        });
        background.0 = if selected {
            SELECTED_COLOR
        } else {
            Color::NONE
        };
    }
}

// Hidden behind the survival inventory screen, which has the hotbar's slots in it, but not the
// creative block picker, where it shows the slots filling.
fn hide_hotbar(
    player: Query<&GameMode, With<LocalPlayer>>,
    mut strip: Query<&mut Visibility, With<Strip>>,
) {
    if player.single().is_ok_and(|mode| !mode.has_every_block()) {
        for mut visibility in &mut strip {
            *visibility = Visibility::Hidden;
        }
    }
}

fn show_hotbar(mut strip: Query<&mut Visibility, With<Strip>>) {
    for mut visibility in &mut strip {
        *visibility = Visibility::Inherited;
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
    use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
    use bevy::input::touch::TouchPhase;
    use bevy::input::{ButtonState, InputPlugin};
    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin;
    use bevy_replicon::RepliconPlugins;

    use super::{drop_held, Hotbar, HotbarPlugin, HOTBAR_SLOTS};
    use crate::block::BlockType;
    use crate::client::command_bar::CommandPlugin;
    use crate::client::player::LocalPlayer;
    use crate::game_mode::GameMode;
    use crate::inventory::icons::StackCount;
    use crate::inventory::Inventory;
    use crate::items::{Item, ItemStack};
    use crate::menu::GameState;
    use crate::player::Player;
    use crate::protocol::DropItem;
    use crate::protocol::ProtocolPlugin;

    #[test]
    fn test_scrolling_wraps_round() {
        let mut hotbar = Hotbar::default();
        hotbar.scroll(-1);
        assert_eq!(HOTBAR_SLOTS - 1, hotbar.selected);
        hotbar.scroll(2);
        assert_eq!(1, hotbar.selected);
        hotbar.scroll(HOTBAR_SLOTS as i32 * 3);
        assert_eq!(1, hotbar.selected);
    }

    #[test]
    fn test_picking_selects_the_hotbar_slot_holding_a_block() {
        let mut hotbar = Hotbar::default();
        let mut inventory = Inventory::starting(GameMode::Creative);
        assert!(hotbar.pick(BlockType::Bricks, &inventory));
        assert_eq!(3, hotbar.selected);
        let bricks = Some(ItemStack::full(Item::Block(BlockType::Bricks)));
        assert_eq!(bricks, hotbar.held(&inventory));
        // but not one in the main inventory
        inventory.slots[20] = Some(ItemStack::new(Item::Block(BlockType::Furnace), 1));
        assert!(!hotbar.pick(BlockType::Furnace, &inventory));
        assert_eq!(3, hotbar.selected);
    }

    // The plugin, headless, with a player.
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            InputPlugin,
            StatesPlugin,
            AssetPlugin::default(),
            RepliconPlugins,
            ProtocolPlugin,
            CommandPlugin,
            HotbarPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<TextureAtlasLayout>()
        .insert_state(GameState::Playing);
        app.world_mut().spawn((
            Player::default(),
            LocalPlayer,
            GameMode::Creative,
            Inventory::starting(GameMode::Creative),
        ));
        app.finish();
        app.update();
        app
    }

    fn selected(app: &mut App) -> usize {
        app.world_mut()
            .query::<&Hotbar>()
            .single(app.world())
            .unwrap()
            .selected
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

    fn scroll(app: &mut App, y: f32, unit: MouseScrollUnit) {
        app.world_mut().write_message(MouseWheel {
            unit,
            x: 0.0,
            y,
            window: Entity::PLACEHOLDER,
            phase: TouchPhase::Moved,
        });
        app.update();
    }

    #[test]
    fn test_number_keys_select_a_slot() {
        let mut app = app();
        press(&mut app, KeyCode::Digit5);
        assert_eq!(4, selected(&mut app));
        press(&mut app, KeyCode::Digit1);
        assert_eq!(0, selected(&mut app));
    }

    #[test]
    fn test_the_wheel_selects_the_next_slot_wrapping_round() {
        let mut app = app();
        scroll(&mut app, 1.0, MouseScrollUnit::Line);
        assert_eq!(HOTBAR_SLOTS - 1, selected(&mut app));
        scroll(&mut app, -2.0, MouseScrollUnit::Line);
        assert_eq!(1, selected(&mut app));
        // a touchpad's pixels add up to a notch
        scroll(&mut app, -25.0, MouseScrollUnit::Pixel);
        assert_eq!(1, selected(&mut app));
        scroll(&mut app, -25.0, MouseScrollUnit::Pixel);
        assert_eq!(2, selected(&mut app));
    }

    #[test]
    fn test_nothing_is_selected_while_paused() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Paused);
        app.update();
        scroll(&mut app, -1.0, MouseScrollUnit::Line);
        assert_eq!(0, selected(&mut app));
    }

    #[test]
    fn test_q_drops_one_of_the_held_stack_and_ctrl_q_all_of_it() {
        #[derive(Resource, Default)]
        struct Dropped(Vec<DropItem>);
        let mut app = app();
        app.init_resource::<Dropped>().add_systems(
            Update,
            (|mut drops: MessageReader<DropItem>, mut dropped: ResMut<Dropped>| {
                dropped.0.extend(drops.read().copied());
            })
            .after(drop_held),
        );
        press(&mut app, KeyCode::Digit4);
        press(&mut app, KeyCode::KeyQ);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ControlLeft);
        press(&mut app, KeyCode::KeyQ);
        let drop = |all| DropItem { slot: 3, all };
        assert_eq!(
            vec![drop(false), drop(true)],
            app.world().resource::<Dropped>().0
        );
    }

    fn counts(app: &mut App) -> Vec<String> {
        let mut texts = app.world_mut().query_filtered::<&Text, With<StackCount>>();
        texts.iter(app.world()).map(|text| text.0.clone()).collect()
    }

    #[test]
    fn test_counts_are_shown_in_survival_for_stacks_of_more_than_one() {
        let mut app = app();
        assert!(counts(&mut app).iter().all(String::is_empty));
        let mut query = app.world_mut().query::<(&mut GameMode, &mut Inventory)>();
        let (mut mode, mut inventory) = query.single_mut(app.world_mut()).unwrap();
        *mode = GameMode::Survival;
        inventory.slots[1] = Some(ItemStack::new(Item::Block(BlockType::Dirt), 1));
        app.update();
        let mut shown = counts(&mut app);
        shown.sort_unstable();
        assert_eq!(vec!["64"; 8], shown[1..]);
        assert_eq!("", shown[0]);
    }
}
