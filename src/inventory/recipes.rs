//! The recipe book beside the inventory screen's slots (M4.2, D-11), where Minecraft keeps its
//! own: every recipe made at the station the screen's open at, by hand in the inventory and both
//! a hand's and a table's at a crafting table, each a card of what it makes, bright if what's
//! carried makes it and dimmed if not. Typing in its search field narrows it to what's called
//! that, and its toggle to what can be made now. Clicking a card asks the server to lay the
//! recipe's ingredients in the crafting grid (`FillGrid`), and shift-clicking as many sets as
//! there are; without the ingredients it's shown as a ghost in the grid instead (`grid::Ghost`).
//! It scrolls, with the wheel or the arrow keys over it, once there are more than fit.

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::picking::events::PointerPress;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::EditableText;

use super::furnace::ContainerView;
use super::grid::Ghost;
use super::icons::{counted_icon, ItemIcons};
use super::look::{card, panel, shade, title, QUIET, SLOT_GAP};
use super::Inventory;
use crate::client::player::LocalPlayer;
use crate::crafting::{Recipe, Station};
use crate::menu::widgets::{scroller, text_field, typed};
use crate::menu::{font, TEXT_SHADOW};
use crate::protocol::FillGrid;

// In pixels: a card, an icon and the bevelled edge and padding round it, and the gap between
// them, so seven columns of a crafting table's hundred-odd recipes sit beside the inventory,
// scrolling down it.
const CARD_SIZE: f32 = 46.0;
const COLUMNS: u16 = 7;
const BOOK_WIDTH: f32 = COLUMNS as f32 * CARD_SIZE + (COLUMNS - 1) as f32 * SLOT_GAP;
// The most of the screen's height the cards take before they scroll, in percent.
const LIST_HEIGHT: f32 = 50.0;
// The longest search there is.
const MAX_SEARCH: usize = 24;
const SEARCH_LABEL_WIDTH: f32 = 64.0;
const TOGGLE_WIDTH: f32 = 128.0;
const TOGGLE_HEIGHT: f32 = 28.0;

/// The station the inventory screen is open at: by hand from E, or a crafting table or a
/// furnace right-clicked. It's back to hand once the screen closes.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CraftingStation(pub Station);

/// Whether the recipe book shows only what can be made now, or everything. It stays as it was
/// set from one opening to the next, as the search does.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MakeableOnly(pub bool);

/// A card that lays a recipe in the grid.
#[derive(Component, Debug)]
pub struct RecipeCard(pub Recipe);

// The shade over a card when there aren't the ingredients for it.
#[derive(Component)]
pub(super) struct Dimmed;

// The whole book, which a container's or a villager's panel takes the place of.
#[derive(Component)]
pub(super) struct RecipeList;

/// The field typed into to search the book.
#[derive(Component)]
pub struct SearchField;

// The button choosing between everything and what can be made now, and its words.
#[derive(Component)]
struct MakeableToggle;

#[derive(Component)]
struct ToggleText;

/// The recipe book, as a panel for the screen to put beside its slots.
pub(super) fn spawn_recipes(screen: &mut ChildSpawnerCommands, icons: &ItemIcons) {
    screen.spawn((RecipeList, panel())).with_children(|book| {
        book.spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            width: Val::Px(BOOK_WIDTH),
            ..default()
        })
        .with_children(|header| {
            header.spawn(title("Recipe book"));
            header
                .spawn((MakeableToggle, card(TOGGLE_WIDTH, TOGGLE_HEIGHT)))
                .with_child((
                    ToggleText,
                    Text::new(toggle_text(false)),
                    font(14.0),
                    TextColor(QUIET),
                    TEXT_SHADOW,
                    Pickable::IGNORE,
                ))
                .observe(toggle_makeable);
        });
        book.spawn(Node {
            align_items: AlignItems::Center,
            width: Val::Px(BOOK_WIDTH),
            ..default()
        })
        .with_children(|search| {
            search.spawn((
                Text::new("Search"),
                font(15.0),
                TextColor(QUIET),
                TEXT_SHADOW,
                Node {
                    width: Val::Px(SEARCH_LABEL_WIDTH),
                    ..default()
                },
                Pickable::IGNORE,
            ));
            search
                .spawn((
                    SearchField,
                    text_field("", MAX_SEARCH, BOOK_WIDTH - SEARCH_LABEL_WIDTH),
                ))
                .observe(focus_search);
        });
        // a crafting table makes more than fit on a short screen
        book.spawn(scroller(Val::Vh(LIST_HEIGHT)))
            .with_children(|scroller| {
                scroller
                    .spawn(Node {
                        display: Display::Grid,
                        grid_template_columns: RepeatedGridTrack::px(COLUMNS, CARD_SIZE),
                        row_gap: Val::Px(SLOT_GAP),
                        column_gap: Val::Px(SLOT_GAP),
                        flex_shrink: 0.0,
                        ..default()
                    })
                    .with_children(|grid| {
                        for &recipe in Recipe::ALL {
                            if recipe.station() != Station::Furnace {
                                spawn_card(grid, icons, recipe);
                            }
                        }
                    });
            });
    });
}

fn toggle_text(makeable_only: bool) -> &'static str {
    match makeable_only {
        true => "Can make now",
        false => "All recipes",
    }
}

// A card with what the recipe makes, and a shade over it while there isn't enough to make it.
fn spawn_card(grid: &mut ChildSpawnerCommands, icons: &ItemIcons, recipe: Recipe) {
    let output = recipe.output();
    grid.spawn((RecipeCard(recipe), card(CARD_SIZE, CARD_SIZE)))
        .with_children(|card| {
            card.spawn(counted_icon(icons, output.item, output.count));
            card.spawn((Dimmed, shade()));
        })
        .observe(lay);
}

// The server lays it, as it makes all changes to inventories (INV-4), checking the station. If
// there isn't enough for it, it's shown as a ghost, as the server clears the grid and lays none.
fn lay(
    press: On<PointerPress>,
    cards: Query<&RecipeCard>,
    keys: Res<ButtonInput<KeyCode>>,
    player: Query<&Inventory, With<LocalPlayer>>,
    mut ghost: ResMut<Ghost>,
    mut fills: MessageWriter<FillGrid>,
) {
    let (Ok(card), PointerButton::Primary) = (cards.get(press.entity), press.button) else {
        return;
    };
    let recipe = card.0;
    let makeable = player
        .single()
        .is_ok_and(|inventory| recipe.times_makeable(inventory) > 0);
    ghost.0 = (!makeable).then_some(recipe);
    fills.write(FillGrid {
        recipe,
        all: keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
    });
}

fn toggle_makeable(
    press: On<PointerPress>,
    mut makeable_only: ResMut<MakeableOnly>,
    mut text: Query<&mut Text, With<ToggleText>>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    makeable_only.0 = !makeable_only.0;
    for mut text in &mut text {
        text.0 = toggle_text(makeable_only.0).into();
    }
}

// The field takes the keys once it's clicked, until something else is or the screen closes.
fn focus_search(mut press: On<PointerPress>, mut focus: ResMut<InputFocus>) {
    // kept from the screen, which would take the focus away again
    press.propagate(false);
    if press.button == PointerButton::Primary && focus.get() != Some(press.entity) {
        focus.set(press.entity, FocusCause::Pressed);
    }
}

/// Whether the recipe book's search field has the keys, so they type rather than act.
pub fn typing(focus: &InputFocus, fields: &Query<(), With<SearchField>>) -> bool {
    focus.get().is_some_and(|focused| fields.contains(focused))
}

/// Whether `recipe` shows for a search of `search`, already lowercase: if what it makes is
/// called that.
pub fn found(recipe: Recipe, search: &str) -> bool {
    let name = recipe.output().item.display_name().to_lowercase();
    search.split_whitespace().all(|word| name.contains(word))
}

/// Shows the recipes made at the screen's station that the search finds, all or only those
/// that can be made now, dimming those there aren't the ingredients for; or none at a container
/// or a villager, which show their own instead. Changes only what's changed, and only when
/// what's carried, the station, the search or the toggle does.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn show_recipes(
    station: Res<CraftingStation>,
    view: Res<ContainerView>,
    trade: Res<super::trade::TradeView>,
    makeable_only: Res<MakeableOnly>,
    player: Query<Ref<Inventory>, With<LocalPlayer>>,
    search: Query<Ref<EditableText>, With<SearchField>>,
    mut list: Query<&mut Node, With<RecipeList>>,
    mut cards: Query<(&RecipeCard, &mut Node, &Children), Without<RecipeList>>,
    mut dims: Query<&mut Visibility, With<Dimmed>>,
    mut shown: Local<Option<(Station, String, bool)>>,
) {
    let Ok(inventory) = player.single() else {
        return;
    };
    let display = match (view.at, trade.villager) {
        (None, None) => Display::Flex,
        _ => Display::None,
    };
    for mut node in &mut list {
        if node.display != display {
            node.display = display;
        }
    }
    let query = search
        .single()
        .map(|field| typed(&field).to_lowercase())
        .unwrap_or_default();
    let now = (station.0, query, makeable_only.0);
    if shown.as_ref() == Some(&now) && !inventory.is_changed() {
        return;
    }
    for (card, mut node, children) in &mut cards {
        let recipe = card.0;
        let makeable = recipe.times_makeable(&inventory) > 0;
        let listed =
            station.0.makes(recipe.station()) && found(recipe, &now.1) && (makeable || !now.2);
        let display = if listed { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        let dim = children.last().and_then(|&dim| dims.get_mut(dim).ok());
        if let Some(mut visibility) = dim {
            visibility.set_if_neq(if makeable {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            });
        }
    }
    *shown = Some(now);
}

#[cfg(test)]
mod tests {
    use super::found;
    use crate::crafting::Recipe;

    #[test]
    fn test_a_search_finds_recipes_by_every_word_of_what_they_make() {
        assert!(found(Recipe::StonePickaxe, "pick"));
        assert!(found(Recipe::StonePickaxe, "stone pick"));
        assert!(found(Recipe::StonePickaxe, ""));
        assert!(!found(Recipe::StonePickaxe, "iron pick"));
        assert!(found(Recipe::CobblestoneStairs, "stairs"));
        assert!(!found(Recipe::Planks, "stairs"));
    }
}
