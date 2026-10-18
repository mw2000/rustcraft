use bevy::picking::events::PointerPress;
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui_widgets::Button;
use bevy::window::{CursorOptions, PrimaryWindow};

use super::chest::{show_chest, spawn_chest};
use super::furnace::{
    container_stack, follow_container, show_furnace, spawn_furnace, ContainerSlot, ContainerView,
};
use super::icons::{load_icons, stack_view, ItemIcons, StackView};
use super::recipes::{describe, show_recipes, spawn_recipes, CraftingStation, RecipeCard};
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
// between the slots and the recipes beside them
const RECIPES_GAP: f32 = 24.0;
// half an icon, so the stack carried is held by its middle
const CARRIED_OFFSET: f32 = 16.0;

const HINT: &str = "Right click takes half a stack or puts down one, and shift-click moves a \
                    stack across or crafts all you can";

/// In survival, the inventory (E) is the inventory screen: the main inventory's 27 slots over
/// the hotbar's 9, as in Minecraft, beside the recipes that can be made by hand, or at the
/// crafting table it was opened at, or beside the furnace's slots if it was opened at one, or
/// under a chest's if it was opened at one of those.
/// Clicks on the slots ask the server to move stacks about, and the stack picked up follows the
/// mouse until it's put down. The game carries on behind it, as behind the block picker, and
/// closing it puts the stack carried back.
pub struct InventoryScreenPlugin;

impl Plugin for InventoryScreenPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CraftingStation>()
            .init_resource::<ContainerView>()
            .add_systems(Startup, spawn_screen.after(load_icons))
            .add_systems(
                Update,
                (
                    follow_container,
                    (
                        carry_to_mouse,
                        name_hovered_item,
                        show_recipes,
                        show_furnace,
                        show_chest,
                    )
                        .run_if(in_state(GameState::Inventory)),
                ),
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

fn spawn_screen(
    mut commands: Commands,
    icons: Res<ItemIcons>,
    mut images: ResMut<Assets<Image>>,
    mut layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
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
            screen
                .spawn(Node {
                    column_gap: Val::Px(RECIPES_GAP),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|slots| {
                        spawn_chest(slots, &icons);
                        spawn_slots(slots, &icons, HOTBAR_SLOTS..SLOTS, 0.0);
                        spawn_slots(slots, &icons, 0..HOTBAR_SLOTS, HOTBAR_GAP);
                    });
                    spawn_recipes(row, &icons);
                    spawn_furnace(row, &icons, &mut images, &mut layouts);
                });
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
    screen.spawn(slot_grid(gap)).with_children(|grid| {
        for slot in slots {
            grid.spawn((Slot(slot), slot_button()))
                .with_child(stack_view(icons, StackView::Slot(slot)))
                .observe(click_slot);
        }
    });
}

/// A grid of slots nine to a row, as wide as the hotbar, `gap` pixels below what's above it.
pub(super) fn slot_grid(gap: f32) -> Node {
    Node {
        display: Display::Grid,
        grid_template_columns: RepeatedGridTrack::px(HOTBAR_SLOTS as u16, SLOT_SIZE),
        row_gap: Val::Px(SLOT_GAP),
        column_gap: Val::Px(SLOT_GAP),
        margin: UiRect::top(Val::Px(gap)),
        ..default()
    }
}

/// A slot's button, for an inventory's slot or a container's.
pub(super) fn slot_button() -> impl Bundle {
    (
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
    )
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
            container: None,
            slot: slot.0 as u8,
            click,
        });
    }
}

pub(super) fn click_of(button: PointerButton, shift: bool) -> Option<Click> {
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

// The hovered stack's name, or what the hovered recipe makes and from what.
#[allow(clippy::type_complexity)]
fn name_hovered_item(
    changed: Query<
        (),
        (
            Or<(With<Slot>, With<ContainerSlot>, With<RecipeCard>)>,
            Changed<Hovered>,
        ),
    >,
    player: Query<Ref<Inventory>, With<LocalPlayer>>,
    view: Res<ContainerView>,
    slots: Query<(&Slot, &Hovered)>,
    container_slots: Query<(&ContainerSlot, &Hovered)>,
    recipes: Query<(&RecipeCard, &Hovered)>,
    mut name: Query<&mut Text, With<ItemName>>,
) {
    let Ok(inventory) = player.single() else {
        return;
    };
    if changed.is_empty() && !inventory.is_changed() && !view.is_changed() {
        return;
    }
    let slot = slots.iter().find(|(_, hovered)| hovered.get());
    let in_container = container_slots.iter().find(|(_, hovered)| hovered.get());
    let stack = slot
        .and_then(|(slot, _)| inventory.slots[slot.0])
        .or_else(|| in_container.and_then(|(slot, _)| container_stack(&view, slot.0)));
    let recipe = recipes.iter().find(|(_, hovered)| hovered.get());
    let text = match (stack, recipe) {
        (Some(stack), _) => stack.item.display_name(),
        (None, Some((card, _))) => describe(card.0),
        (None, None) => HINT.into(),
    };
    if let Ok(mut name) = name.single_mut() {
        name.0 = text;
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

// The menu holds the cursor again as play resumes. The next time it opens, it's by hand,
// unless it's at a station or a container again.
fn close(
    mut screen: Query<&mut Node, With<Screen>>,
    mut closes: MessageWriter<CloseInventory>,
    mut station: ResMut<CraftingStation>,
    mut view: ResMut<ContainerView>,
) {
    *station = CraftingStation::default();
    *view = ContainerView::default();
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
    use crate::client::command_bar::CommandPlugin;
    use crate::client::player::LocalPlayer;
    use crate::crafting::{Recipe, Station};
    use crate::game_mode::GameMode;
    use crate::hotbar::HotbarPlugin;
    use crate::inventory::recipes::{CraftingStation, Dimmed, RecipeCard};
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
        .insert_state(GameState::Playing)
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
            container: None,
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

    // The recipes listed, and whether each is bright, in the table's order.
    fn listed(app: &mut App) -> Vec<(Recipe, bool)> {
        let mut cards = app.world_mut().query::<(&RecipeCard, &Node, &Children)>();
        let mut dims = app
            .world_mut()
            .query_filtered::<&Visibility, With<Dimmed>>();
        let world = app.world();
        let mut listed: Vec<_> = cards
            .iter(world)
            .filter(|(_, node, _)| node.display != Display::None)
            .map(|(card, _, children)| {
                let dim = dims.get(world, *children.last().unwrap()).unwrap();
                (card.0, *dim == Visibility::Hidden)
            })
            .collect();
        listed.sort_by_key(|(recipe, _)| Recipe::ALL.iter().position(|r| r == recipe));
        listed
    }

    #[test]
    fn test_the_screen_lists_the_recipes_of_its_station_bright_if_theyre_makeable() {
        let mut app = app(GameMode::Survival);
        press(&mut app, KeyCode::KeyE);
        // by hand, with only dirt to make anything with
        let by_hand: Vec<_> = Recipe::ALL
            .iter()
            .filter(|recipe| recipe.station() == Station::Hand)
            .map(|&recipe| (recipe, false))
            .collect();
        assert_eq!(5, by_hand.len());
        assert_eq!(by_hand, listed(&mut app));
        // and with a log there are planks to be made
        let mut query = app.world_mut().query::<&mut Inventory>();
        query.single_mut(app.world_mut()).unwrap().slots[1] =
            Some(ItemStack::new(Item::Block(BlockType::Log), 1));
        app.update();
        assert!(listed(&mut app).contains(&(Recipe::Planks, true)));
        press(&mut app, KeyCode::KeyE);

        // at a crafting table, everything but smelting
        app.insert_resource(CraftingStation(Station::CraftingTable));
        press(&mut app, KeyCode::KeyE);
        let crafted = Recipe::ALL
            .iter()
            .filter(|r| r.station() != Station::Furnace);
        assert_eq!(crafted.count(), listed(&mut app).len());
        // and closing it goes back to hand
        press(&mut app, KeyCode::KeyE);
        assert_eq!(
            CraftingStation(Station::Hand),
            *app.world().resource::<CraftingStation>()
        );
    }
}
