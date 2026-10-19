//! A chest's slots, over the inventory's in the inventory screen's panel when it's opened at a
//! chest or a barrel, as the server last sent them (`ContainerChanged`). Clicks and drags on them
//! ask the server to move stacks about, as on the inventory's own.

use bevy::prelude::*;

use super::furnace::ContainerView;
use super::icons::ItemIcons;
use super::look::{slot_grid, title};
use super::screen::spawn_slot;
use super::slots::Place;
use crate::block::BlockType;
use crate::containers::Container;
use crate::inventory::HOTBAR_SLOTS;

#[derive(Component)]
pub(super) struct ChestPanel;

// The section's name, of the chest or barrel it's open at.
#[derive(Component)]
pub(super) struct ChestName;

/// The chest's slots, under its name, hidden until the screen's opened at a chest, for the screen
/// to put over the inventory's.
pub(super) fn spawn_chest(main: &mut ChildSpawnerCommands, icons: &ItemIcons) {
    let chest = Container::new(BlockType::Chest).expect("a chest holds things");
    main.spawn((
        ChestPanel,
        Node {
            display: Display::None,
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(6.0),
            margin: UiRect::bottom(Val::Px(6.0)),
            ..default()
        },
    ))
    .with_children(|section| {
        section.spawn((ChestName, title("Chest")));
        section
            .spawn(slot_grid(HOTBAR_SLOTS as u16))
            .with_children(|grid| {
                for slot in 0..chest.slots.len() {
                    spawn_slot(grid, icons, Place::Container(slot as u8));
                }
            });
    });
}

/// Shows the chest's slots, and its name, when the screen's open at a chest or a barrel, which
/// hold as much.
pub(super) fn show_chest(
    view: Res<ContainerView>,
    mut panel: Query<&mut Node, With<ChestPanel>>,
    mut name: Query<&mut Text, With<ChestName>>,
) {
    let (display, title) = match view.block() {
        Some(BlockType::Chest) => (Display::Flex, "Chest"),
        Some(BlockType::Barrel) => (Display::Flex, "Barrel"),
        _ => (Display::None, ""),
    };
    for mut node in &mut panel {
        if node.display != display {
            node.display = display;
        }
    }
    for mut text in &mut name {
        if !title.is_empty() && text.0 != title {
            text.0 = title.into();
        }
    }
}
