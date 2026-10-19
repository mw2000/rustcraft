//! The furnace's section, over the inventory's slots in the inventory screen's panel when it's
//! opened at a furnace: the input over the flame over the fuel, and an arrow to the output, as
//! the server last sent them (`ContainerChanged`). The flame burns down as the fuel does and the
//! arrow fills as an item smelts. Clicks on its slots ask the server to move stacks about, as
//! the inventory's do.

use bevy::math::I64Vec3;
use bevy::prelude::*;

use super::icons::ItemIcons;
use super::look::title;
use super::recipes::CraftingStation;
use super::screen::spawn_slot;
use super::slots::Place;
use crate::block::BlockType;
use crate::chunks::material::block_atlas;
use crate::containers::{Container, FUEL, INPUT, OUTPUT};
use crate::crafting::Station;
use crate::items::ItemStack;
use crate::menu::GameState;
use crate::protocol::ContainerChanged;

// In pixels: the flame and the arrow, their 16-pixel tiles drawn three times the size, bigger
// than icons, so they read across the room the panel leaves them.
const ART_SIZE: f32 = 48.0;
// The tiles of `furnace.png`, drawn by the block-textures skill's `ui.py`.
const FLAME_OUT: usize = 0;
const FLAME_LIT: usize = 1;
const ARROW_EMPTY: usize = 2;
const ARROW_FULL: usize = 3;

/// The container the inventory screen's open at, if it's one, and what's in it as the server
/// last said, which is an empty one of its kind until it's said. Both go once the screen closes.
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct ContainerView {
    pub at: Option<I64Vec3>,
    pub container: Option<Container>,
}

impl ContainerView {
    /// Opening `block` at `at`, which is a container if it's a block that holds things.
    pub fn opening(block: BlockType, at: I64Vec3) -> Self {
        let container = Container::new(block);
        Self {
            at: container.as_ref().map(|_| at),
            container,
        }
    }

    /// The block of the container open, if there's one.
    pub fn block(&self) -> Option<BlockType> {
        self.container.as_ref().map(|container| container.block)
    }
}

#[derive(Component)]
pub(super) struct FurnacePanel;

// The lit part of the flame, as tall as the fuel has left to burn, or the full part of the
// arrow, as wide as the smelting's got.
#[derive(Component, Clone, Copy)]
pub(super) enum Gauge {
    Flame,
    Arrow,
}

/// The section, hidden until the screen's opened at a furnace, for the screen to put over the
/// inventory's slots.
pub(super) fn spawn_furnace(
    main: &mut ChildSpawnerCommands,
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
    let column = Node {
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        row_gap: Val::Px(2.0),
        ..default()
    };
    let slot = |slot: usize| Place::Container(slot as u8);
    main.spawn((
        FurnacePanel,
        Node {
            display: Display::None,
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(6.0),
            margin: UiRect::bottom(Val::Px(6.0)),
            ..default()
        },
    ))
    .with_children(|section| {
        section.spawn(title("Furnace"));
        section
            .spawn(Node {
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(28.0),
                ..default()
            })
            .with_children(|row| {
                row.spawn(column).with_children(|fire| {
                    spawn_slot(fire, icons, slot(INPUT));
                    fire.spawn(gauge(Gauge::Flame));
                    spawn_slot(fire, icons, slot(FUEL));
                });
                row.spawn(gauge(Gauge::Arrow));
                spawn_slot(row, icons, slot(OUTPUT));
            });
    });
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
