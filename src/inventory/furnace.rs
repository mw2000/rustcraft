//! The furnace's panel, beside the inventory screen's slots in place of the crafting list when
//! it's opened at a furnace: the input over the flame over the fuel, and an arrow to the output,
//! as the server last sent them (`ContainerChanged`). The flame burns down as the fuel does and
//! the arrow fills as an item smelts. Clicks on its slots ask the server to move stacks about,
//! as the inventory's do.

use bevy::math::I64Vec3;
use bevy::picking::events::PointerPress;
use bevy::prelude::*;

use super::icons::{stack_view, ItemIcons, StackView};
use super::recipes::CraftingStation;
use super::screen::{click_of, slot_button};
use crate::block::BlockType;
use crate::chunks::material::block_atlas;
use crate::containers::{Container, FUEL, INPUT, OUTPUT};
use crate::crafting::Station;
use crate::items::ItemStack;
use crate::menu::{font, GameState, TEXT_SHADOW};
use crate::protocol::{ContainerChanged, InventoryClick};

// In pixels: the flame and the arrow, their 16-pixel tiles drawn twice the size, as icons are.
const ART_SIZE: f32 = 32.0;
// The tiles of `furnace.png`, drawn by the block-textures skill's `ui.py`.
const FLAME_OUT: usize = 0;
const FLAME_LIT: usize = 1;
const ARROW_EMPTY: usize = 2;
const ARROW_FULL: usize = 3;

/// The container the inventory screen's open at, if it's one, and what's in it as the server
/// last said, which is nothing until it's said. Both go once the screen closes.
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct ContainerView {
    pub at: Option<I64Vec3>,
    pub container: Option<Container>,
}

impl ContainerView {
    /// Opening `block` at `at`, which is a container if it's a block that holds things.
    pub fn opening(block: BlockType, at: I64Vec3) -> Self {
        Self {
            at: Container::new(block).map(|_| at),
            container: None,
        }
    }
}

/// A slot of the open container, in the inventory screen.
#[derive(Component)]
pub(super) struct ContainerSlot(pub(super) usize);

#[derive(Component)]
pub(super) struct FurnacePanel;

// The lit part of the flame, as tall as the fuel has left to burn, or the full part of the
// arrow, as wide as the smelting's got.
#[derive(Component, Clone, Copy)]
pub(super) enum Gauge {
    Flame,
    Arrow,
}

/// The panel, hidden until the screen's opened at a furnace, for the screen to put beside its
/// slots.
pub(super) fn spawn_furnace(
    row: &mut ChildSpawnerCommands,
    icons: &ItemIcons,
    images: &mut Assets<Image>,
    layouts: &mut Assets<TextureAtlasLayout>,
) {
    let art = block_atlas("assets/textures/furnace.png");
    let tile = art.height();
    let layout = layouts.add(TextureAtlasLayout::from_grid(
        UVec2::splat(tile),
        art.width() / tile,
        1,
        None,
        None,
    ));
    let art = images.add(art);
    let tile = |index| {
        (
            ImageNode::from_atlas_image(
                art.clone(),
                TextureAtlas {
                    layout: layout.clone(),
                    index,
                },
            ),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                bottom: Val::Px(0.0),
                width: Val::Px(ART_SIZE),
                height: Val::Px(ART_SIZE),
                ..default()
            },
            Pickable::IGNORE,
        )
    };
    // the lit tile over the unlit one, showing as much of it as is filled, from nothing
    let gauge = |gauge| {
        let (empty, full, fill) = match gauge {
            Gauge::Flame => (
                FLAME_OUT,
                FLAME_LIT,
                Node {
                    width: Val::Px(ART_SIZE),
                    ..default()
                },
            ),
            Gauge::Arrow => (
                ARROW_EMPTY,
                ARROW_FULL,
                Node {
                    height: Val::Px(ART_SIZE),
                    ..default()
                },
            ),
        };
        (
            Node {
                width: Val::Px(ART_SIZE),
                height: Val::Px(ART_SIZE),
                ..default()
            },
            Pickable::IGNORE,
            children![
                tile(empty),
                (
                    gauge,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        bottom: Val::Px(0.0),
                        overflow: Overflow::clip(),
                        ..fill
                    },
                    Pickable::IGNORE,
                    children![tile(full)],
                ),
            ],
        )
    };
    let slot = |slot| {
        (
            ContainerSlot(slot),
            slot_button(),
            children![stack_view(icons, StackView::Container(slot))],
        )
    };
    let column = Node {
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        row_gap: Val::Px(8.0),
        ..default()
    };
    row.spawn((
        FurnacePanel,
        Node {
            display: Display::None,
            ..column.clone()
        },
    ))
    .with_children(|panel| {
        panel.spawn((Text::new("Furnace"), font(18.0), TEXT_SHADOW));
        panel
            .spawn(Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(16.0),
                ..default()
            })
            .with_children(|row| {
                row.spawn(column).with_children(|fire| {
                    fire.spawn(slot(INPUT)).observe(click_slot);
                    fire.spawn(gauge(Gauge::Flame));
                    fire.spawn(slot(FUEL)).observe(click_slot);
                });
                row.spawn(gauge(Gauge::Arrow));
                row.spawn(slot(OUTPUT)).observe(click_slot);
            });
    });
}

// The server makes the change, as it does all changes to containers (INV-4), naming the
// container so a click meant for one that's since closed changes nothing.
fn click_slot(
    press: On<PointerPress>,
    slots: Query<&ContainerSlot>,
    keys: Res<ButtonInput<KeyCode>>,
    view: Res<ContainerView>,
    mut clicks: MessageWriter<InventoryClick>,
) {
    let (Ok(slot), Some(at)) = (slots.get(press.entity), view.at) else {
        return;
    };
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if let Some(click) = click_of(press.button, shift) {
        clicks.write(InventoryClick {
            container: Some(at),
            slot: slot.0 as u8,
            click,
        });
    }
}

/// Keeps what the server says of the container open, and closes the screen once it says it's
/// gone or out of reach.
pub(super) fn follow_container(
    mut changes: MessageReader<ContainerChanged>,
    mut view: ResMut<ContainerView>,
    mut state: ResMut<NextState<GameState>>,
) {
    for ContainerChanged { at, container } in changes.read() {
        if view.at != Some(*at) {
            continue;
        }
        match container {
            Some(container) => view.container = Some(container.clone()),
            None => state.set(GameState::Playing),
        }
    }
}

/// Shows the panel at a furnace, with its flame and arrow as far on as the server last said.
pub(super) fn show_furnace(
    station: Res<CraftingStation>,
    view: Res<ContainerView>,
    mut panel: Query<&mut Node, With<FurnacePanel>>,
    mut gauges: Query<(&Gauge, &mut Node), Without<FurnacePanel>>,
) {
    let display = match station.0 {
        Station::Furnace => Display::Flex,
        _ => Display::None,
    };
    for mut node in &mut panel {
        if node.display != display {
            node.display = display;
        }
    }
    let fire = view.container.as_ref().map(|c| c.fire).unwrap_or_default();
    for (gauge, mut node) in &mut gauges {
        let (filled, full) = match gauge {
            Gauge::Flame => (&mut node.height, fire.flame()),
            Gauge::Arrow => (&mut node.width, fire.progress()),
        };
        let size = Val::Px((ART_SIZE * full).round());
        if *filled != size {
            *filled = size;
        }
    }
}

/// The stack in the open container's slot `slot`, as last sent.
pub(super) fn container_stack(view: &ContainerView, slot: usize) -> Option<ItemStack> {
    let container = view.container.as_ref()?;
    container.slots.get(slot).copied().flatten()
}
