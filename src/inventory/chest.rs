//! A chest's slots, over the inventory's in the inventory screen when it's opened at a chest, as
//! the server last sent them (`ContainerChanged`). Clicks on them ask the server to move stacks
//! about, as the furnace's do.

use bevy::prelude::*;

use super::furnace::{click_slot, ContainerSlot, ContainerView};
use super::icons::{stack_view, ItemIcons, StackView};
use super::screen::{slot_button, slot_grid};
use crate::block::BlockType;
use crate::containers::Container;
use crate::menu::{font, TEXT_SHADOW};

// In pixels, between the chest's slots and the inventory's under them.
const CHEST_GAP: f32 = 24.0;

#[derive(Component)]
pub(super) struct ChestPanel;

/// The chest's slots, under its name, hidden until the screen's opened at a chest, for the screen
/// to put over the inventory's.
pub(super) fn spawn_chest(slots: &mut ChildSpawnerCommands, icons: &ItemIcons) {
    let chest = Container::new(BlockType::Chest).expect("a chest holds things");
    slots
        .spawn((
            ChestPanel,
            Node {
                display: Display::None,
                flex_direction: FlexDirection::Column,
                margin: UiRect::bottom(Val::Px(CHEST_GAP)),
                ..default()
            },
        ))
        .with_children(|panel| {
            panel.spawn((Text::new("Chest"), font(18.0), TEXT_SHADOW));
            panel.spawn(slot_grid(8.0)).with_children(|grid| {
                for slot in 0..chest.slots.len() {
                    grid.spawn((
                        ContainerSlot(slot),
                        slot_button(),
                        children![stack_view(icons, StackView::Container(slot))],
                    ))
                    .observe(click_slot);
                }
            });
        });
}

/// Shows the chest's slots when the screen's open at a chest.
pub(super) fn show_chest(view: Res<ContainerView>, mut panel: Query<&mut Node, With<ChestPanel>>) {
    let display = match view.block() {
        Some(BlockType::Chest) => Display::Flex,
        _ => Display::None,
    };
    for mut node in &mut panel {
        if node.display != display {
            node.display = display;
        }
    }
}
