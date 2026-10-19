//! The description beside the pointer in the inventory screen (M4.1): what the stack under it is,
//! with a tool's or armour's durability left and how much food fills; what a recipe makes and
//! from what; and what a trade takes and gives. It's hidden while a stack's carried, as it
//! would cover where it's going.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use super::furnace::{container_stack, ContainerView};
use super::look::{SlotAt, QUIET, TITLE};
use super::recipes::RecipeCard;
use super::screen::Dragging;
use super::slots::Place;
use super::trade::{TradeCard, TradeView};
use super::{Inventory, SLOTS};
use crate::client::player::LocalPlayer;
use crate::crafting::Recipe;
use crate::items::{Item, ItemStack};
use crate::menu::{font, TEXT_SHADOW};

// What each armour slot's for, shown as it's hovered empty.
const ARMOUR_NAMES: [&str; 4] = ["Helmet", "Chestplate", "Leggings", "Boots"];
// In pixels: how far from the pointer it's drawn, and room inside its edge.
const OFFSET: Vec2 = Vec2::new(18.0, -6.0);
const PADDING: f32 = 6.0;
const BACKGROUND: Color = Color::srgba(0.06, 0.06, 0.09, 0.94);
const EDGE: Color = Color::srgb(0.42, 0.36, 0.6);

#[derive(Component)]
pub(super) struct Tooltip;

#[derive(Component)]
pub(super) struct TooltipText;

/// The tooltip, hidden until something's hovered.
pub(super) fn spawn_tooltip(screen: &mut ChildSpawnerCommands) {
    screen.spawn((
        Tooltip,
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(Val::Px(PADDING + 2.0), Val::Px(PADDING)),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(BACKGROUND),
        BorderColor::all(EDGE),
        Visibility::Hidden,
        Pickable::IGNORE,
        GlobalZIndex(3),
        children![(
            TooltipText,
            Text::new(""),
            font(16.0),
            TEXT_SHADOW,
            Pickable::IGNORE
        )],
    ));
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

/// What `recipe` makes and from what.
pub(super) fn describe_recipe(recipe: Recipe) -> Vec<(String, Color)> {
    let output = recipe.output();
    let ingredients: Vec<_> = recipe
        .ingredients()
        .iter()
        .map(|&(item, count)| named(item, count))
        .collect();
    vec![
        (named(output.item, output.count), Color::WHITE),
        (format!("from {}", ingredients.join(" and ")), QUIET),
        ("Shift-click makes all you can".into(), QUIET),
    ]
}

/// Finds what's hovered and describes it beside the pointer, rebuilding the text only when
/// that changes.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn show_tooltip(
    window: Query<&Window, With<PrimaryWindow>>,
    player: Query<&Inventory, With<LocalPlayer>>,
    view: Res<ContainerView>,
    trade: Res<TradeView>,
    dragging: Res<Dragging>,
    slots: Query<(&SlotAt, &Hovered)>,
    recipes: Query<(&RecipeCard, &Hovered, &Node)>,
    trades: Query<(&TradeCard, &Hovered)>,
    mut tooltip: Query<
        (&mut Node, &mut Visibility, &ComputedNode),
        (With<Tooltip>, Without<RecipeCard>),
    >,
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
    let lines = if inventory.cursor.is_some() || dragging.active() {
        Vec::new()
    } else if let Some((at, _)) = slots.iter().find(|(_, h)| h.get()) {
        let stack = match at.0 {
            Place::Own(slot) => inventory.get(usize::from(slot)),
            Place::Container(slot) => container_stack(&view, usize::from(slot)),
        };
        match (stack, at.0) {
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

#[cfg(test)]
mod tests {
    use super::{describe_recipe, describe_stack, hearts};
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
        assert_eq!("Planks ×4", text(describe_recipe(Recipe::Planks))[0]);
    }
}
