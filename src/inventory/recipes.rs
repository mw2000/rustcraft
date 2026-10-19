//! The crafting list beside the inventory screen's slots: every recipe that can be made at the
//! station it's open at, bright if what's carried makes it and dimmed if not. Clicking one asks
//! the server to make it once, and shift-clicking as many times as it can (`Craft`). It scrolls,
//! with the wheel or the arrow keys over it, once there are more than fit.

use bevy::picking::events::PointerPress;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;

use super::furnace::ContainerView;
use super::icons::{counted_icon, ItemIcons};
use super::look::{card, panel, shade, title, QUIET};
use super::Inventory;
use crate::client::player::LocalPlayer;
use crate::crafting::{Recipe, Station};
use crate::menu::widgets::scroller;
use crate::menu::{font, TEXT_SHADOW};
use crate::protocol::Craft;

// In pixels: four columns of cards, each the output and what it takes, so a crafting table's
// sixty-odd recipes fit beside the inventory, scrolling down it.
const CARD_WIDTH: f32 = 140.0;
const CARD_HEIGHT: f32 = 42.0;
const CARD_GAP: f32 = 4.0;
const COLUMNS: u16 = 4;
// The most of the screen's height the list takes before it scrolls, in percent.
const LIST_HEIGHT: f32 = 58.0;

/// The station the inventory screen is open at: by hand from E, or a crafting table or a
/// furnace right-clicked. It's back to hand once the screen closes.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CraftingStation(pub Station);

/// A button that makes a recipe.
#[derive(Component, Debug)]
pub struct RecipeCard(pub Recipe);

// The shade over a card when there aren't the ingredients for it.
#[derive(Component)]
pub(super) struct Dimmed;

#[derive(Component)]
pub(super) struct Heading;

// The whole list, which a container's or a villager's panel takes the place of.
#[derive(Component)]
pub(super) struct RecipeList;

/// The crafting list, headed by where it's made, as a panel for the screen to put beside its
/// slots. M4.2 puts a crafting grid in the inventory's panel and makes this the recipe book.
pub(super) fn spawn_recipes(screen: &mut ChildSpawnerCommands, icons: &ItemIcons) {
    screen.spawn((RecipeList, panel())).with_children(|list| {
        list.spawn((Heading, title("Crafting")));
        // a crafting table makes more than fit on a short screen
        list.spawn(scroller(Val::Vh(LIST_HEIGHT)))
            .with_children(|scroller| {
                scroller
                    .spawn(Node {
                        display: Display::Grid,
                        grid_template_columns: RepeatedGridTrack::px(COLUMNS, CARD_WIDTH),
                        row_gap: Val::Px(CARD_GAP),
                        column_gap: Val::Px(CARD_GAP),
                        flex_shrink: 0.0,
                        ..default()
                    })
                    .with_children(|grid| {
                        for &recipe in Recipe::ALL {
                            spawn_card(grid, icons, recipe);
                        }
                    });
            });
    });
}

// A card with what the recipe makes, an arrow back from what it takes, and a shade over it while
// there isn't enough to make it.
fn spawn_card(grid: &mut ChildSpawnerCommands, icons: &ItemIcons, recipe: Recipe) {
    let output = recipe.output();
    grid.spawn((RecipeCard(recipe), card(CARD_WIDTH, CARD_HEIGHT)))
        .with_children(|card| {
            card.spawn(counted_icon(icons, output.item, output.count));
            card.spawn((
                Text::new("←"),
                font(16.0),
                TextColor(QUIET),
                TEXT_SHADOW,
                Pickable::IGNORE,
            ));
            for &(item, count) in recipe.ingredients() {
                card.spawn(counted_icon(icons, item, count));
            }
            card.spawn((Dimmed, shade()));
        })
        .observe(craft);
}

// The server makes it, as it makes all changes to inventories (INV-4), and checks the station.
fn craft(
    press: On<PointerPress>,
    cards: Query<&RecipeCard>,
    keys: Res<ButtonInput<KeyCode>>,
    mut crafts: MessageWriter<Craft>,
) {
    if let (Ok(card), PointerButton::Primary) = (cards.get(press.entity), press.button) {
        crafts.write(Craft {
            recipe: card.0,
            all: keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        });
    }
}

/// Shows the recipes made at the screen's station, dimming those there aren't the ingredients
/// for, or none at a container or a villager, which show their own instead.
/// Run every frame the screen's open, changing only what's changed, as there are a handful.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn show_recipes(
    station: Res<CraftingStation>,
    view: Res<ContainerView>,
    trade: Res<super::trade::TradeView>,
    player: Query<&Inventory, With<LocalPlayer>>,
    mut list: Query<&mut Node, With<RecipeList>>,
    mut cards: Query<(&RecipeCard, &mut Node, &Children), Without<RecipeList>>,
    mut dims: Query<&mut Visibility, With<Dimmed>>,
    mut heading: Query<&mut Text, With<Heading>>,
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
    for (card, mut node, children) in &mut cards {
        let shown = station.0.makes(card.0.station());
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        let dim = children.last().and_then(|&dim| dims.get_mut(dim).ok());
        if let Some(mut visibility) = dim {
            let makeable = card.0.times_makeable(inventory) > 0;
            visibility.set_if_neq(if makeable {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            });
        }
    }
    let title = match station.0 {
        Station::CraftingTable => "Crafting table",
        _ => "Crafting",
    };
    for mut text in &mut heading {
        if text.0 != title {
            text.0 = title.into();
        }
    }
}
