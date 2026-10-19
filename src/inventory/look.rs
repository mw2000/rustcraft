//! How every container screen looks (M4.1): raised slate panels with a bevelled edge, slots set
//! into them, titles in a warm light, and cards for recipes and trades raised as the panels are.
//! It's our own look, dark and cool where Minecraft's is light grey (INV-8): the same idea of
//! a panel with sunken slots, none of its pictures.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;

use super::slots::Place;
use crate::menu::{font, TEXT_SHADOW};

/// A slot's size in pixels, its bevelled edge included, and the gap between slots.
pub const SLOT: f32 = 44.0;
pub const SLOT_GAP: f32 = 4.0;
/// The edge round a slot or a panel, in pixels.
pub const EDGE: f32 = 2.0;
/// Room inside a panel's edge, in pixels.
pub const PANEL_PADDING: f32 = 12.0;

const PANEL: Color = Color::srgb(0.2, 0.215, 0.25);
const PANEL_LIGHT: Color = Color::srgb(0.44, 0.47, 0.54);
const PANEL_DARK: Color = Color::srgb(0.07, 0.075, 0.09);
const WELL: Color = Color::srgb(0.11, 0.12, 0.14);
const WELL_HOVERED: Color = Color::srgb(0.27, 0.3, 0.38);
// a drag's slots, lit warmer than hovering so the share can be seen spreading
const WELL_DRAGGED: Color = Color::srgb(0.36, 0.33, 0.22);
const CARD: Color = Color::srgb(0.27, 0.285, 0.33);
const CARD_HOVERED: Color = Color::srgb(0.34, 0.37, 0.46);
/// Titles' colour, a warm light over the cool panels.
pub const TITLE: Color = Color::srgb(0.98, 0.88, 0.64);
/// Quieter text: hints, and what's sold out.
pub const QUIET: Color = Color::srgb(0.62, 0.64, 0.7);
/// Over what can't be made or paid for.
pub const DIMMED: Color = Color::srgba(0.05, 0.05, 0.07, 0.62);

/// A slot in the inventory screen, where it is, for clicks, drags and keys over it to name.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotAt(pub Place);

/// A raised card, a recipe's or a trade's, lit under the pointer.
#[derive(Component)]
pub struct Card;

// A bevelled edge: light along the top and left and dark along the bottom and right for
// something raised, the other way round for something sunk.
fn bevel(raised: bool) -> BorderColor {
    let (lit, shade) = if raised {
        (PANEL_LIGHT, PANEL_DARK)
    } else {
        (PANEL_DARK, PANEL_LIGHT)
    };
    BorderColor {
        top: lit,
        left: lit,
        bottom: shade,
        right: shade,
    }
}

/// A raised panel, laying out what's in it in a column.
pub fn panel() -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(PANEL_PADDING)),
            border: UiRect::all(Val::Px(EDGE)),
            row_gap: Val::Px(6.0),
            ..default()
        },
        BackgroundColor(PANEL),
        bevel(true),
    )
}

/// A panel's or a section's title.
pub fn title(text: &str) -> impl Bundle {
    (
        Text::new(text),
        font(17.0),
        TextColor(TITLE),
        TEXT_SHADOW,
        Pickable::IGNORE,
    )
}

/// A slot's sunken frame at `place`, for a stack view to go in, which clicks and drags on
/// name.
pub fn slot_frame(place: Place) -> impl Bundle {
    (
        SlotAt(place),
        bevy::ui_widgets::Button,
        Hovered::default(),
        Node {
            width: Val::Px(SLOT),
            height: Val::Px(SLOT),
            border: UiRect::all(Val::Px(EDGE)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(WELL),
        bevel(false),
    )
}

/// A raised card `width` by `height`, its contents in a row.
pub fn card(width: f32, height: f32) -> impl Bundle {
    (
        Card,
        bevy::ui_widgets::Button,
        Hovered::default(),
        Node {
            width: Val::Px(width),
            height: Val::Px(height),
            border: UiRect::all(Val::Px(EDGE)),
            padding: UiRect::horizontal(Val::Px(5.0)),
            column_gap: Val::Px(4.0),
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(CARD),
        bevel(true),
    )
}

/// A shade over the whole of what it's in, for a card that can't be used.
pub fn shade() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            ..default()
        },
        BackgroundColor(DIMMED),
        Pickable::IGNORE,
    )
}

/// A grid of slots `columns` wide, `gap` pixels under what's above it.
pub fn slot_grid(columns: u16) -> Node {
    Node {
        display: Display::Grid,
        grid_template_columns: RepeatedGridTrack::px(columns, SLOT),
        row_gap: Val::Px(SLOT_GAP),
        column_gap: Val::Px(SLOT_GAP),
        ..default()
    }
}

/// Lights the slot under the pointer, and those a drag's crossed, and the card under it.
pub(super) fn light_slots(
    dragged: Res<super::screen::Dragging>,
    mut slots: Query<(&SlotAt, &Hovered, &mut BackgroundColor), Without<Card>>,
    mut cards: Query<(&Hovered, &mut BackgroundColor), With<Card>>,
) {
    for (at, hovered, mut background) in &mut slots {
        let colour = if dragged.crosses(at.0) {
            WELL_DRAGGED
        } else if hovered.get() {
            WELL_HOVERED
        } else {
            WELL
        };
        background.set_if_neq(BackgroundColor(colour));
    }
    for (hovered, mut background) in &mut cards {
        let colour = if hovered.get() { CARD_HOVERED } else { CARD };
        background.set_if_neq(BackgroundColor(colour));
    }
}
