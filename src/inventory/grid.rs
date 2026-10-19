//! The crafting grid in the inventory screen's panel (M4.2, D-11): by hand the inventory's 2×2
//! beside the armour worn, and at a crafting table a 3×3 in the armour's place, with an arrow to
//! the output, what the grid makes. Its cells are slots like any other (`Place::Grid`), and what's
//! in them is the server's, sent with the inventory (INV-4). A recipe chosen in the recipe book
//! without the ingredients for it shows as a ghost in the grid's empty cells, the client's alone,
//! so the player sees what's needed.

use bevy::prelude::*;

use super::furnace::{FurnaceArt, ARROW_FULL, ART_SIZE};
use super::icons::ItemIcons;
use super::look::{slot_grid, title, SlotAt};
use super::recipes::CraftingStation;
use super::screen::spawn_slot;
use super::slots::Place;
use super::Inventory;
use crate::client::player::LocalPlayer;
use crate::crafting::{Recipe, Station, GRID, GRID_WIDTH};

// How strongly a ghost's pictures show, so they read as what's wanted, not what's there.
const GHOST_ALPHA: f32 = 0.45;
// In pixels: from a slot's edge to the icon in it, a slot's size less an icon's, halved, less
// its edge.
const GHOST_INSET: f32 = 4.0;

/// The recipe the recipe book last showed as a ghost in the grid, as there weren't the
/// ingredients to lay it. It goes once the grid makes it, another recipe's chosen, or the
/// screen closes.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ghost(pub Option<Recipe>);

/// The crafting grid's section: its title, the grid, the arrow and the output.
#[derive(Component)]
pub(super) struct CraftingSection;

#[derive(Component)]
pub(super) struct CraftingTitle;

// The grid of cells, as many columns as the station's grid is wide.
#[derive(Component)]
pub(super) struct GridCells;

// The faded picture of what a ghost wants in a slot, under its stack.
#[derive(Component)]
pub(super) struct GhostIcon(pub(super) Place);

/// The section, for the screen to put beside the armour.
pub(super) fn spawn_crafting(
    parent: &mut ChildSpawnerCommands,
    icons: &ItemIcons,
    art: &FurnaceArt,
) {
    parent
        .spawn((
            CraftingSection,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                ..default()
            },
        ))
        .with_children(|section| {
            section.spawn((CraftingTitle, title("Crafting")));
            section
                .spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn((GridCells, slot_grid(GRID_WIDTH as u16)))
                        .with_children(|grid| {
                            for cell in 0..GRID {
                                spawn_ghosted(grid, icons, Place::Grid(cell as u8));
                            }
                        });
                    row.spawn((
                        art.image(ARROW_FULL),
                        Node {
                            width: Val::Px(ART_SIZE),
                            height: Val::Px(ART_SIZE),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                    spawn_ghosted(row, icons, Place::Output);
                });
        });
}

// A slot with a ghost's picture under its stack.
fn spawn_ghosted(parent: &mut ChildSpawnerCommands, icons: &ItemIcons, place: Place) {
    spawn_slot(parent, icons, place).with_child((
        GhostIcon(place),
        // painted once there's a ghost to show
        ImageNode::default(),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(GHOST_INSET),
            top: Val::Px(GHOST_INSET),
            width: Val::Px(32.0),
            height: Val::Px(32.0),
            ..default()
        },
        Visibility::Hidden,
        Pickable::IGNORE,
    ));
}

/// Shapes the grid to the screen's station, 2×2 by hand and 3×3 at a table, and titles it.
#[allow(clippy::type_complexity)]
pub(super) fn show_grid(
    station: Res<CraftingStation>,
    mut grid: Query<&mut Node, With<GridCells>>,
    mut cells: Query<(&SlotAt, &mut Node), Without<GridCells>>,
    mut heading: Query<&mut Text, With<CraftingTitle>>,
) {
    if !station.is_changed() {
        return;
    }
    let used = station.0.grid();
    let columns = station.0.grid_width().max(1) as u16;
    for mut node in &mut grid {
        node.grid_template_columns = slot_grid(columns).grid_template_columns;
    }
    for (at, mut node) in &mut cells {
        if let Place::Grid(cell) = at.0 {
            let display = match used.contains(&usize::from(cell)) {
                true => Display::Flex,
                false => Display::None,
            };
            if node.display != display {
                node.display = display;
            }
        }
    }
    let text = match station.0 {
        Station::CraftingTable => "Crafting table",
        _ => "Crafting",
    };
    for mut heading in &mut heading {
        if heading.0 != text {
            heading.0 = text.into();
        }
    }
}

/// Shows the ghost's pattern faded in the grid's empty cells, and what it makes in the output
/// while the grid makes nothing, and lets it go once the grid makes it.
pub(super) fn show_ghost(
    icons: Res<ItemIcons>,
    station: Res<CraftingStation>,
    mut ghost: ResMut<Ghost>,
    player: Query<Ref<Inventory>, With<LocalPlayer>>,
    mut pictures: Query<(&GhostIcon, &mut ImageNode, &mut Visibility)>,
) {
    let Ok(inventory) = player.single() else {
        return;
    };
    if !ghost.is_changed() && !inventory.is_changed() && !station.is_changed() {
        return;
    }
    let made = Recipe::matching(&inventory.grid, station.0);
    if ghost.0.is_some() && made == ghost.0 {
        ghost.0 = None;
    }
    let layout = ghost.0.and_then(|recipe| recipe.layout(station.0));
    for (picture, mut image, mut visibility) in &mut pictures {
        let wanted = match (picture.0, layout) {
            (Place::Grid(cell), Some(layout)) => {
                let cell = usize::from(cell);
                layout[cell].filter(|_| inventory.grid[cell].is_none())
            }
            (Place::Output, Some(_)) if made.is_none() => ghost.0.map(|r| r.output().item),
            _ => None,
        };
        match wanted {
            Some(item) => {
                icons.paint(&mut image, item);
                image.color = image.color.with_alpha(GHOST_ALPHA);
                visibility.set_if_neq(Visibility::Inherited);
            }
            None => {
                visibility.set_if_neq(Visibility::Hidden);
            }
        }
    }
}
