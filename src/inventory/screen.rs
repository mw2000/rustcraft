use bevy::picking::events::PointerPress;
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};

use super::chest::{show_chest, spawn_chest};
use super::furnace::{follow_container, show_furnace, spawn_furnace, ContainerView};
use super::icons::{load_icons, stack_view, ItemIcons, StackView};
use super::look::{self, light_slots, panel, slot_frame, slot_grid, title, SlotAt, QUIET};
use super::recipes::{show_recipes, spawn_recipes, CraftingStation};
use super::slots::{Place, Slots, Spread};
use super::tooltip::{show_tooltip, spawn_tooltip};
use super::trade::{
    follow_offers, name_villager, show_trade, spawn_label, spawn_trade, talk, TradeView,
};
use super::{Click, Inventory, ARMOUR_SLOTS, HOTBAR_SLOTS, SLOTS};
use crate::client::player::LocalPlayer;
use crate::controls::{Action, Controls};
use crate::game_mode::GameMode;
use crate::interaction::TargetCreature;
use crate::menu::{font, free_cursor, GameState, TEXT_SHADOW};
use crate::protocol::{CloseInventory, InventoryClick, InventoryDrag};

// In pixels: between the main inventory and the hotbar under it, and between the panels.
const HOTBAR_GAP: f32 = 10.0;
const PANEL_GAP: f32 = 16.0;
// half an icon, so the stack carried is held by its middle
const CARRIED_OFFSET: f32 = 16.0;
// How soon a second click on a slot makes a double click, in seconds.
const DOUBLE_CLICK: f32 = 0.3;

const HINT_COLOUR: Color = Color::srgb(0.8, 0.82, 0.86);
const HINT_BACKING: Color = Color::srgba(0.05, 0.05, 0.07, 0.6);
const HINT: &str = "Drag to share a stack · double-click gathers · shift-click moves across · \
                    1-9 swaps with the hotbar · Q drops";

/// In survival, the inventory (E) is the inventory screen, in one look for every station
/// (`look`): a panel with the main inventory's 27 slots over the hotbar's 9, as in Minecraft,
/// and over them the armour worn, or the chest's slots, or the furnace's, if it was opened at
/// one; beside it, a panel of the recipes that can be made by hand or at the crafting table it
/// was opened at, or of a villager's trades. Clicks, drags and keys over the slots ask the
/// server to move stacks about (`slots::Slots`), the stack picked up follows the mouse until
/// it's put down, and what's hovered is described beside the pointer. The game carries on
/// behind it, as behind the block picker, and closing it puts the stack carried back.
pub struct InventoryScreenPlugin;

impl Plugin for InventoryScreenPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CraftingStation>()
            .init_resource::<ContainerView>()
            .init_resource::<TradeView>()
            .init_resource::<Dragging>()
            .init_resource::<LastPress>()
            .add_systems(Startup, (spawn_screen.after(load_icons), spawn_label))
            .add_systems(
                Update,
                (
                    follow_container,
                    follow_offers,
                    // with the creature looked at, as the game has it
                    (name_villager, talk.run_if(in_state(GameState::Playing)))
                        .run_if(resource_exists::<TargetCreature>),
                    (
                        (drag, keys_over_slots).chain(),
                        carry_to_mouse,
                        show_recipes,
                        show_furnace,
                        show_chest,
                        show_trade,
                        show_armour,
                        light_slots,
                        show_tooltip,
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

// The stack on the cursor.
#[derive(Component)]
struct Carried;

// The armour worn and what it's worth, over the inventory when no container's open.
#[derive(Component)]
struct ArmourSection;

#[derive(Component)]
struct ArmourWorth;

// The label over the player's own slots.
#[derive(Component)]
struct InventoryLabel;

/// A drag in progress across the slots, from a press over a slot with a stack on the cursor
/// until the button's let go: the slots crossed so far, in order, and how it shares.
#[derive(Resource, Default, Debug)]
pub struct Dragging(Option<(MouseButton, Spread, Vec<Place>)>);

impl Dragging {
    /// Whether a drag's going on.
    pub fn active(&self) -> bool {
        self.0.is_some()
    }

    /// Whether the drag's crossed the slot at `place`, to be lit. A drag of one slot is still
    /// only a click, and isn't lit.
    pub fn crosses(&self, place: Place) -> bool {
        self.0
            .as_ref()
            .is_some_and(|(_, _, places)| places.len() > 1 && places.contains(&place))
    }
}

// The last press on a slot, and when, to tell a double click by.
#[derive(Resource, Default)]
struct LastPress(Option<(Place, f32)>);

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
                row_gap: Val::Px(12.0),
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            // over the debug overlay, as the block picker is
            GlobalZIndex(1),
        ))
        .with_children(|screen| {
            screen
                .spawn(Node {
                    column_gap: Val::Px(PANEL_GAP),
                    align_items: AlignItems::Start,
                    ..default()
                })
                .with_children(|row| {
                    // beside the inventory, where Minecraft keeps its recipe book
                    spawn_recipes(row, &icons);
                    spawn_trade(row);
                    row.spawn(panel()).with_children(|main| {
                        spawn_armour(main, &icons);
                        spawn_chest(main, &icons);
                        spawn_furnace(main, &icons, &mut images, &mut layouts);
                        main.spawn((
                            InventoryLabel,
                            title("Inventory"),
                            Node {
                                margin: UiRect::top(Val::Px(4.0)),
                                ..default()
                            },
                        ));
                        spawn_slots(main, &icons, HOTBAR_SLOTS..SLOTS, 0.0);
                        spawn_slots(main, &icons, 0..HOTBAR_SLOTS, HOTBAR_GAP - 6.0);
                    });
                });
            // on a dark strip, so it reads over bright sand and snow
            screen.spawn((
                Text::new(HINT),
                font(15.0),
                TextColor(HINT_COLOUR),
                TEXT_SHADOW,
                Node {
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(HINT_BACKING),
                Pickable::IGNORE,
            ));
            spawn_tooltip(screen);
            // last, so it's drawn over the slots
            screen.spawn((
                Carried,
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                Pickable::IGNORE,
                GlobalZIndex(4),
                children![stack_view(&icons, StackView::Cursor)],
            ));
        });
}

// The four armour slots in a row, and what they're worth beside them. M4.2's crafting grid goes
// beside them too.
fn spawn_armour(main: &mut ChildSpawnerCommands, icons: &ItemIcons) {
    main.spawn((
        ArmourSection,
        Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(look::SLOT_GAP),
            margin: UiRect::bottom(Val::Px(4.0)),
            ..default()
        },
    ))
    .with_children(|row| {
        for slot in ARMOUR_SLOTS {
            spawn_slot(row, icons, Place::Own(slot as u8));
        }
        row.spawn((
            ArmourWorth,
            Text::new(""),
            font(15.0),
            TextColor(QUIET),
            TEXT_SHADOW,
            Node {
                margin: UiRect::left(Val::Px(12.0)),
                ..default()
            },
            Pickable::IGNORE,
        ));
    });
}

// A grid of `slots`, nine to a row, `gap` pixels below what's above it.
fn spawn_slots(
    parent: &mut ChildSpawnerCommands,
    icons: &ItemIcons,
    slots: std::ops::Range<usize>,
    gap: f32,
) {
    let grid = Node {
        margin: UiRect::top(Val::Px(gap)),
        ..slot_grid(HOTBAR_SLOTS as u16)
    };
    parent.spawn(grid).with_children(|grid| {
        for slot in slots {
            spawn_slot(grid, icons, Place::Own(slot as u8));
        }
    });
}

/// A slot at `place` with its stack in it, which presses on ask the server to change.
pub(super) fn spawn_slot(parent: &mut ChildSpawnerCommands, icons: &ItemIcons, place: Place) {
    let shows = match place {
        Place::Own(slot) => StackView::Slot(usize::from(slot)),
        Place::Container(slot) => StackView::Container(usize::from(slot)),
    };
    parent
        .spawn(slot_frame(place))
        .with_child(stack_view(icons, shows))
        .observe(press_slot);
}

// The click a press on a slot makes, to send it as it's pressed: picking up, a shift-click or
// a double click, all at once. A press with a stack on the cursor starts a drag instead, which
// is a click after all if it ends without crossing another slot (`drag`).
#[allow(clippy::too_many_arguments)]
fn press_slot(
    press: On<PointerPress>,
    slots: Query<&SlotAt>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    player: Query<&Inventory, With<LocalPlayer>>,
    view: Res<ContainerView>,
    mut last: ResMut<LastPress>,
    mut dragging: ResMut<Dragging>,
    mut clicks: MessageWriter<InventoryClick>,
) {
    let Ok(&SlotAt(place)) = slots.get(press.entity) else {
        return;
    };
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let now = time.elapsed_secs();
    let double = last
        .0
        .is_some_and(|(at, when)| at == place && now - when < DOUBLE_CLICK);
    let carrying = player.single().is_ok_and(|i| i.cursor.is_some());
    let (button, spread, click) = match press.button {
        PointerButton::Primary => (MouseButton::Left, Spread::Even, Click::Left),
        PointerButton::Secondary => (MouseButton::Right, Spread::One, Click::Right),
        PointerButton::Middle => return,
    };
    let primary = press.button == PointerButton::Primary;
    last.0 = (primary && !double).then_some((place, now));
    let click = if primary && shift {
        Click::Shift
    } else if primary && double {
        Click::Gather
    } else if carrying {
        dragging.0 = Some((button, spread, vec![place]));
        return;
    } else {
        click
    };
    send_click(&mut clicks, &view, place, click);
}

fn send_click(
    clicks: &mut MessageWriter<InventoryClick>,
    view: &ContainerView,
    place: Place,
    click: Click,
) {
    // the server makes the change, as it does all changes to inventories (INV-4), naming the
    // container so a click meant for one that's since closed changes nothing
    let (container, slot) = match place {
        Place::Own(slot) => (None, slot),
        Place::Container(slot) => match view.at {
            Some(at) => (Some(at), slot),
            None => return,
        },
    };
    clicks.write(InventoryClick {
        container,
        slot,
        click,
    });
}

// Adds the slots the pointer crosses to the drag, while the button's held, those that would take
// some of the cursor's stack and no more of them than there are items, as the server will check;
// and once it's let go sends the drag, all of it at once, or the click it was if it crossed no
// other slot.
#[allow(clippy::too_many_arguments)]
fn drag(
    mouse: Res<ButtonInput<MouseButton>>,
    hovered: Query<(&SlotAt, &Hovered)>,
    player: Query<&Inventory, With<LocalPlayer>>,
    view: Res<ContainerView>,
    mut dragging: ResMut<Dragging>,
    mut clicks: MessageWriter<InventoryClick>,
    mut drags: MessageWriter<InventoryDrag>,
) {
    let Some((button, spread, places)) = &mut dragging.0 else {
        return;
    };
    let Ok(inventory) = player.single() else {
        return;
    };
    if let Some(carried) = inventory.cursor {
        let mut inventory = inventory.clone();
        let mut container = view.container.clone();
        let slots = Slots {
            inventory: &mut inventory,
            container: container.as_mut(),
        };
        for (&SlotAt(place), hover) in &hovered {
            let room = places.len() < usize::from(carried.count);
            if hover.get() && room && !places.contains(&place) && slots.takes(place, carried) {
                places.push(place);
            }
        }
    }
    if mouse.pressed(*button) {
        return;
    }
    match places.as_slice() {
        [] => {}
        [place] => {
            let click = match spread {
                Spread::Even => Click::Left,
                Spread::One => Click::Right,
            };
            send_click(&mut clicks, &view, *place, click);
        }
        _ => {
            drags.write(InventoryDrag {
                container: view.at,
                places: places.clone(),
                spread: *spread,
            });
        }
    }
    dragging.0 = None;
}

// Over a slot, 1 to 9 swap it with that hotbar slot, and Q throws one from it, or the stack with
// Ctrl, as in Minecraft.
fn keys_over_slots(
    controls: Controls,
    keys: Res<ButtonInput<KeyCode>>,
    hovered: Query<(&SlotAt, &Hovered)>,
    view: Res<ContainerView>,
    dragging: Res<Dragging>,
    mut clicks: MessageWriter<InventoryClick>,
) {
    if dragging.0.is_some() {
        return;
    }
    let Some(place) = hovered.iter().find(|(_, h)| h.get()).map(|(at, _)| at.0) else {
        return;
    };
    let swap = Action::SLOTS.iter().position(|&s| controls.just_pressed(s));
    let click = match swap {
        Some(hotbar) => Click::Swap(hotbar as u8),
        None if controls.just_pressed(Action::Drop) => Click::Throw {
            all: keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
        },
        None => return,
    };
    send_click(&mut clicks, &view, place, click);
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

// The armour slots and what they're worth, when no container's open, and the label over the
// player's slots, when one is.
#[allow(clippy::type_complexity)]
fn show_armour(
    view: Res<ContainerView>,
    player: Query<Ref<Inventory>, With<LocalPlayer>>,
    mut section: Query<&mut Node, With<ArmourSection>>,
    mut worth: Query<&mut Text, With<ArmourWorth>>,
) {
    let display = match view.at {
        None => Display::Flex,
        Some(_) => Display::None,
    };
    for mut node in &mut section {
        if node.display != display {
            node.display = display;
        }
    }
    let Ok(inventory) = player.single() else {
        return;
    };
    if !inventory.is_changed() {
        return;
    }
    let text = match inventory.armour_points() {
        0 => "No armour worn".to_string(),
        points => format!(
            "Armour {points}\ntakes {:.0}% of a blow",
            inventory.protection() * 100.0
        ),
    };
    for mut worth in &mut worth {
        if worth.0 != text {
            worth.0.clone_from(&text);
        }
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
// unless it's at a station or a container again. A drag still going is dropped, and the server
// puts what's on the cursor back.
#[allow(clippy::too_many_arguments)]
fn close(
    mut screen: Query<&mut Node, With<Screen>>,
    mut closes: MessageWriter<CloseInventory>,
    mut station: ResMut<CraftingStation>,
    mut view: ResMut<ContainerView>,
    mut trade: ResMut<TradeView>,
    mut dragging: ResMut<Dragging>,
    mut last: ResMut<LastPress>,
) {
    *station = CraftingStation::default();
    *view = ContainerView::default();
    *trade = TradeView::default();
    dragging.0 = None;
    last.0 = None;
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
    use bevy::prelude::*;
    use bevy::state::app::StatesPlugin;
    use bevy::window::WindowFocused;
    use bevy_replicon::prelude::ClientId;
    use bevy_replicon::RepliconPlugins;

    use super::{Dragging, InventoryScreenPlugin, Screen};
    use crate::block::BlockType;
    use crate::client::command_bar::CommandPlugin;
    use crate::client::player::LocalPlayer;
    use crate::crafting::{Recipe, Station};
    use crate::game_mode::GameMode;
    use crate::hotbar::HotbarPlugin;
    use crate::inventory::recipes::{CraftingStation, Dimmed, RecipeCard};
    use crate::inventory::slots::{Place, Spread};
    use crate::inventory::{Click, Hotbar, Inventory};
    use crate::items::{Item, ItemStack};
    use crate::menu::{GameState, MenuPlugin};
    use crate::picker::PickerPlugin;
    use crate::player::{Player, PlayerOwner};
    use crate::protocol::{InventoryClick, ProtocolPlugin};
    use crate::server::ServerPlugin;
    use crate::world::World;

    const DIRT: Item = Item::Block(BlockType::Dirt);

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
        inventory.slots[0] = Some(ItemStack::new(DIRT, 5));
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
        let dirt = Some(ItemStack::new(DIRT, 5));
        assert_eq!(
            (dirt, None),
            (inventory(&mut app).slots[0], inventory(&mut app).cursor)
        );
    }

    #[test]
    fn test_a_drag_let_go_is_sent_whole_and_one_of_a_single_slot_is_a_click() {
        let mut app = app(GameMode::Survival);
        press(&mut app, KeyCode::KeyE);
        app.world_mut().write_message(InventoryClick {
            container: None,
            slot: 0,
            click: Click::Left,
        });
        app.update();
        // as the press and the pointer crossing two more slots would leave it, the button up
        app.insert_resource(Dragging(Some((
            MouseButton::Left,
            Spread::Even,
            vec![Place::Own(9), Place::Own(10)],
        ))));
        app.update();
        let inventory = inventory(&mut app);
        let two = Some(ItemStack::new(DIRT, 2));
        assert_eq!([two, two], [inventory.slots[9], inventory.slots[10]]);
        assert_eq!(Some(ItemStack::new(DIRT, 1)), inventory.cursor);
        assert!(app.world().resource::<Dragging>().0.is_none());
        // a right drag that crossed nothing else puts one down, as a right click
        app.insert_resource(Dragging(Some((
            MouseButton::Right,
            Spread::One,
            vec![Place::Own(20)],
        ))));
        app.update();
        let inventory = self::inventory(&mut app);
        assert_eq!(Some(ItemStack::new(DIRT, 1)), inventory.slots[20]);
        assert_eq!(None, inventory.cursor);
    }

    #[test]
    fn test_number_keys_in_the_screen_dont_change_the_slot_held() {
        let mut app = app(GameMode::Survival);
        press(&mut app, KeyCode::KeyE);
        press(&mut app, KeyCode::Digit4);
        let mut hotbar = app.world_mut().query::<&Hotbar>();
        assert_eq!(0, hotbar.single(app.world()).unwrap().selected);
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
        assert_eq!(6, by_hand.len());
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
