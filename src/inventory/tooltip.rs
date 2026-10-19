//! The description beside the pointer in the inventory screen (M4.1): what the stack under it is,
//! with a tool's or armour's durability left and how much food fills; what a recipe makes and
//! from what, with a small picture of its pattern (M4.2); and what a trade takes and gives. It's
//! hidden while a stack's carried, as it would cover where it's going.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::furnace::{container_stack, ContainerView};
use super::icons::ItemIcons;
use super::look::{slot_grid, SlotAt, QUIET, TITLE, WELL};
use super::recipes::{CraftingStation, RecipeCard};
use super::screen::Dragging;
use super::slots::Place;
use super::trade::{TradeCard, TradeView};
use super::{Inventory, SLOTS};
use crate::client::player::LocalPlayer;
use crate::crafting::{Pattern, Recipe, Station, GRID, GRID_WIDTH};
use crate::items::{Item, ItemStack};
use crate::menu::{font, TEXT_SHADOW};

// What each armour slot's for, shown as it's hovered empty.
const ARMOUR_NAMES: [&str; 4] = ["Helmet", "Chestplate", "Leggings", "Boots"];
// In pixels: how far from the pointer it's drawn, and room inside its edge.
const OFFSET: Vec2 = Vec2::new(18.0, -6.0);
const PADDING: f32 = 6.0;
const BACKGROUND: Color = Color::srgba(0.06, 0.06, 0.09, 0.94);
const EDGE: Color = Color::srgb(0.42, 0.36, 0.6);
// In pixels: a cell of a recipe's picture, and the icon in it.
const PREVIEW_CELL: f32 = 28.0;
const PREVIEW_ICON: f32 = 22.0;

#[derive(Component)]
pub(super) struct Tooltip;

#[derive(Component)]
pub(super) struct TooltipText;

// The small picture of a recipe's pattern under its description, and its cells.
#[derive(Component)]
pub(super) struct Preview;

#[derive(Component)]
pub(super) struct PreviewCell(usize);

/// The tooltip, hidden until something's hovered.
pub(super) fn spawn_tooltip(screen: &mut ChildSpawnerCommands) {
    screen
        .spawn((
            Tooltip,
            Node {
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(PADDING),
                padding: UiRect::axes(Val::Px(PADDING + 2.0), Val::Px(PADDING)),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(BACKGROUND),
            BorderColor::all(EDGE),
            Visibility::Hidden,
            Pickable::IGNORE,
            GlobalZIndex(3),
        ))
        .with_children(|tooltip| {
            tooltip.spawn((
                TooltipText,
                Text::new(""),
                font(16.0),
                TEXT_SHADOW,
                Pickable::IGNORE,
            ));
            let grid = Node {
                display: Display::None,
                row_gap: Val::Px(2.0),
                column_gap: Val::Px(2.0),
                ..slot_grid(GRID_WIDTH as u16)
            };
            tooltip
                .spawn((Preview, grid, Pickable::IGNORE))
                .with_children(|grid| {
                    for cell in 0..GRID {
                        grid.spawn((
                            PreviewCell(cell),
                            Node {
                                width: Val::Px(PREVIEW_CELL),
                                height: Val::Px(PREVIEW_CELL),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            BackgroundColor(WELL),
                            Pickable::IGNORE,
                            children![(
                                ImageNode::default(),
                                Node {
                                    width: Val::Px(PREVIEW_ICON),
                                    height: Val::Px(PREVIEW_ICON),
                                    ..default()
                                },
                                Pickable::IGNORE,
                            )],
                        ));
                    }
                });
        });
}

/// The lines describing `stack`: its name, and its durability left, armour or food.
pub fn describe_stack(stack: ItemStack) -> Vec<(String, Color)> {
    let item = stack.item;
    let mut lines = vec![(item.display_name(), Color::WHITE)];
    if let Some(durability) = item.durability() {
        let left = durability - stack.wear;
        lines.push((format!("Durability {left} / {durability}"), QUIET));
    }
    if item.tool().is_some() {
        lines.push((format!("Hits for {}", hearts(item.damage())), QUIET));
    }
    if let Some(armour) = item.armour() {
        lines.push((format!("Armour +{}", armour.points), QUIET));
    }
    if let Some(fills) = item.fills() {
        lines.push((format!("Restores {} hunger", hearts(fills)), QUIET));
    }
    lines
}

// Half points as whole and halves, as the HUD draws hearts and food: 5 is "2½".
fn hearts(halves: u8) -> String {
    match (halves / 2, halves % 2) {
        (0, _) => "½".into(),
        (whole, 0) => whole.to_string(),
        (whole, _) => format!("{whole}½"),
    }
}

fn named(item: Item, count: u8) -> String {
    match count {
        1 => item.display_name(),
        _ => format!("{} ×{count}", item.display_name()),
    }
}

/// What `recipe` makes and from what, and how the recipe book lays it.
pub(super) fn describe_recipe(recipe: Recipe) -> Vec<(String, Color)> {
    let output = recipe.output();
    let ingredients: Vec<_> = recipe
        .ingredients()
        .iter()
        .map(|&(item, count)| named(item, count))
        .collect();
    let mut lines = vec![
        (named(output.item, output.count), Color::WHITE),
        (format!("from {}", ingredients.join(" and ")), QUIET),
    ];
    if let Pattern::Shapeless(_) = recipe.pattern() {
        lines.push(("in any arrangement".into(), QUIET));
    }
    lines.push((
        "Click lays it in the grid, shift-click all you can".into(),
        QUIET,
    ));
    lines
}

// How many columns of the 3×3 a recipe's pattern takes, as a table lays it, and what's in each
// cell, or none for what isn't laid in a grid.
fn pattern_cells(recipe: Recipe) -> Option<(usize, [Option<Item>; GRID])> {
    let layout = recipe.layout(Station::CraftingTable)?;
    let filled = (0..GRID).filter(|&cell| layout[cell].is_some());
    let width = filled.map(|cell| cell % GRID_WIDTH + 1).max()?;
    Some((width, layout))
}

type PreviewNodes<'w, 's> =
    Query<'w, 's, &'static mut Node, (With<Preview>, Without<RecipeCard>, Without<PreviewCell>)>;
type PreviewCells<'w, 's> = Query<
    'w,
    's,
    (&'static PreviewCell, &'static mut Node, &'static Children),
    (Without<Tooltip>, Without<RecipeCard>, Without<Preview>),
>;

/// Finds what's hovered and describes it beside the pointer, rebuilding the text only when
/// that changes.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn show_tooltip(
    window: Query<&Window, With<PrimaryWindow>>,
    player: Query<&Inventory, With<LocalPlayer>>,
    view: Res<ContainerView>,
    trade: Res<TradeView>,
    station: Res<CraftingStation>,
    icons: Res<ItemIcons>,
    dragging: Res<Dragging>,
    slots: Query<(&SlotAt, &Hovered)>,
    recipes: Query<(&RecipeCard, &Hovered, &Node)>,
    trades: Query<(&TradeCard, &Hovered)>,
    mut tooltip: Query<
        (&mut Node, &mut Visibility, &ComputedNode),
        (
            With<Tooltip>,
            Without<RecipeCard>,
            Without<Preview>,
            Without<PreviewCell>,
        ),
    >,
    // the pattern's picture: its grid, its cells and their icons
    (mut preview, mut cells, mut images): (PreviewNodes, PreviewCells, Query<&mut ImageNode>),
    text: Query<Entity, With<TooltipText>>,
    mut shown: Local<Vec<(String, Color)>>,
    mut commands: Commands,
) {
    let (Ok(inventory), Ok(window)) = (player.single(), window.single()) else {
        return;
    };
    let Ok((mut node, mut visibility, size)) = tooltip.single_mut() else {
        return;
    };
    let mut previewed = None;
    let lines = if inventory.cursor.is_some() || dragging.active() {
        Vec::new()
    } else if let Some((at, _)) = slots.iter().find(|(_, h)| h.get()) {
        let stack = match at.0 {
            Place::Own(slot) => inventory.get(usize::from(slot)),
            Place::Container(slot) => container_stack(&view, usize::from(slot)),
            Place::Grid(cell) => inventory.grid[usize::from(cell)],
            Place::Output => Recipe::matching(&inventory.grid, station.0).map(Recipe::output),
        };
        match (stack, at.0) {
            (Some(stack), Place::Output) => {
                let mut lines = describe_stack(stack);
                lines.push(("Shift-click makes all you can".into(), QUIET));
                lines
            }
            (Some(stack), _) => describe_stack(stack),
            (None, Place::Own(slot)) if usize::from(slot) >= SLOTS => {
                let piece = ARMOUR_NAMES[usize::from(slot) - SLOTS];
                vec![(piece.into(), QUIET)]
            }
            _ => Vec::new(),
        }
    } else if let Some((card, ..)) = recipes
        .iter()
        .find(|(_, h, node)| h.get() && node.display != Display::None)
    {
        previewed = Some(card.0);
        describe_recipe(card.0)
    } else if let Some((card, _)) = trades.iter().find(|(_, h)| h.get()) {
        trade
            .offers
            .get(card.0)
            .map(|&(offer, left)| {
                let stock = match left {
                    0 => "Sold out for today".to_string(),
                    left => format!("{left} more today"),
                };
                vec![
                    (named(offer.gives.0, offer.gives.1), Color::WHITE),
                    (
                        format!("for {}", named(offer.takes.0, offer.takes.1)),
                        QUIET,
                    ),
                    (stock, QUIET),
                    ("Shift-click trades all you can".into(), QUIET),
                ]
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    if lines.is_empty() {
        visibility.set_if_neq(Visibility::Hidden);
        shown.clear();
        return;
    }
    if *shown != lines {
        show_pattern(previewed, &icons, &mut preview, &mut cells, &mut images);
        if let Ok(text) = text.single() {
            let mut text = commands.entity(text);
            text.despawn_related::<Children>();
            text.insert((Text::new(""), TextColor(TITLE)));
            text.with_children(|spans| {
                for (i, (line, colour)) in lines.iter().enumerate() {
                    let line = if i == 0 {
                        line.clone()
                    } else {
                        format!("\n{line}")
                    };
                    spans.spawn((TextSpan::new(line), font(16.0), TextColor(*colour)));
                }
            });
        }
        *shown = lines;
    }
    visibility.set_if_neq(Visibility::Inherited);
    // beside the pointer, or on its left if it would run off the screen's right edge
    if let Some(at) = window.cursor_position() {
        let width = size.size().x * size.inverse_scale_factor();
        let height = size.size().y * size.inverse_scale_factor();
        let mut x = at.x + OFFSET.x;
        if x + width > window.width() {
            x = at.x - OFFSET.x - width;
        }
        let y = (at.y + OFFSET.y).min(window.height() - height).max(0.0);
        let (left, top) = (Val::Px(x), Val::Px(y));
        if node.left != left || node.top != top {
            node.left = left;
            node.top = top;
        }
    }
}

// Draws `recipe`'s pattern in the tooltip's picture, only the rows and columns it takes, or
// hides the picture for anything else.
fn show_pattern(
    recipe: Option<Recipe>,
    icons: &ItemIcons,
    preview: &mut PreviewNodes,
    cells: &mut PreviewCells,
    images: &mut Query<&mut ImageNode>,
) {
    let pattern = recipe.and_then(pattern_cells);
    for mut node in preview.iter_mut() {
        match pattern {
            Some((width, _)) => {
                node.grid_template_columns = RepeatedGridTrack::px(width as u16, PREVIEW_CELL);
                node.display = Display::Grid;
            }
            None => node.display = Display::None,
        }
    }
    let Some((width, layout)) = pattern else {
        return;
    };
    let height = (0..GRID)
        .filter(|&cell| layout[cell].is_some())
        .map(|cell| cell / GRID_WIDTH + 1)
        .max()
        .unwrap_or(0);
    for (cell, mut node, children) in cells.iter_mut() {
        let (row, column) = (cell.0 / GRID_WIDTH, cell.0 % GRID_WIDTH);
        node.display = match row < height && column < width {
            true => Display::Flex,
            false => Display::None,
        };
        let Ok(mut image) = images.get_mut(children[0]) else {
            continue;
        };
        match layout[cell.0] {
            Some(item) => icons.paint(&mut image, item),
            None => image.color = Color::NONE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{describe_recipe, describe_stack, hearts, pattern_cells};
    use crate::crafting::Recipe;
    use crate::items::{Item, ItemStack};

    fn text(lines: Vec<(String, bevy::color::Color)>) -> Vec<String> {
        lines.into_iter().map(|(line, _)| line).collect()
    }

    #[test]
    fn test_tooltips_give_the_name_durability_and_food() {
        let pick = ItemStack {
            wear: 14,
            ..ItemStack::new(Item::WoodenPickaxe, 1)
        };
        let lines = text(describe_stack(pick));
        assert_eq!("Wooden pickaxe", lines[0]);
        assert_eq!("Durability 50 / 64", lines[1]);
        let bread = text(describe_stack(ItemStack::new(Item::Bread, 3)));
        assert_eq!(vec!["Bread", "Restores 2½ hunger"], bread);
        assert_eq!(
            vec!["Stick"],
            text(describe_stack(ItemStack::new(Item::Stick, 1)))
        );
        assert_eq!(("½", "2", "2½"), (&*hearts(1), &*hearts(4), &*hearts(5)));
    }

    #[test]
    fn test_recipes_are_described_by_what_they_make_and_take() {
        let lines = text(describe_recipe(Recipe::StonePickaxe));
        assert_eq!("Stone pickaxe", lines[0]);
        assert_eq!("from Cobblestone ×3 and Stick ×2", lines[1]);
        let planks = text(describe_recipe(Recipe::Planks));
        assert_eq!("Planks ×4", planks[0]);
        assert_eq!("in any arrangement", planks[2]);
    }

    #[test]
    fn test_a_recipes_picture_is_as_wide_as_its_pattern() {
        let (width, layout) = pattern_cells(Recipe::WoodenPickaxe).unwrap();
        assert_eq!(3, width);
        assert_eq!(Some(Item::Stick), layout[7]);
        assert_eq!(1, pattern_cells(Recipe::Sticks).unwrap().0);
        assert_eq!(2, pattern_cells(Recipe::MossyCobblestone).unwrap().0);
        assert_eq!(None, pattern_cells(Recipe::IronIngot));
    }
}
