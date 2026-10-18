//! The menus' widgets, one look for all of them (M-U1): buttons, choices that cycle through
//! their options, sliders, text fields, scrolling lists, tabs and headings. They're Bevy's
//! headless widgets styled the way the pause menu always was, Minecraft's grey stone buttons,
//! lighter with a white edge under the pointer or with the keyboard's focus. Tab and the arrow
//! keys move the focus, Enter and Space press, and the arrow keys move sliders and list rows.

use std::ops::RangeInclusive;

use bevy::input_focus::tab_navigation::{NavAction, TabGroup, TabIndex, TabNavigation};
use bevy::input_focus::{AutoFocus, FocusCause, InputFocus, InputFocusVisible};
use bevy::picking::hover::Hovered;
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::text::{EditableText, EditableTextFilter, DEFAULT_REM_SIZE_PX};
use bevy::ui::{InteractionDisabled, Pressed};
use bevy::ui_widgets::{
    listbox_update_selection, ActiveDescendant, Button, ListBox, ListItem, ReselectListRow,
    ScrollArea, SelectAllOnFocus, Slider, SliderRange, SliderStep, SliderThumb, SliderValue,
    TextInput, TrackClick, ValueChange,
};

pub(crate) const BUTTON_COLOR: Color = Color::srgb(0.42, 0.42, 0.42);
const BUTTON_HOVER_COLOR: Color = Color::srgb(0.5, 0.55, 0.7);
const BUTTON_DISABLED_COLOR: Color = Color::srgb(0.25, 0.25, 0.25);
pub(crate) const BUTTON_BORDER_COLOR: Color = Color::BLACK;
const BUTTON_HOVER_BORDER_COLOR: Color = Color::WHITE;
// fields, lists and the track sliders run along, darker than buttons, as Minecraft's are
const FIELD_COLOR: Color = Color::srgba(0.0, 0.0, 0.0, 0.6);
const FIELD_BORDER_COLOR: Color = Color::srgb(0.63, 0.63, 0.63);
const ROW_SELECTED_COLOR: Color = Color::srgba(1.0, 1.0, 1.0, 0.15);
pub(crate) const TEXT_SHADOW: TextShadow = TextShadow {
    offset: Vec2::splat(2.0),
    color: Color::srgb(0.24, 0.24, 0.24),
};
/// Text that says less, such as the details under a world's name.
pub const QUIET_TEXT: Color = Color::srgb(0.66, 0.66, 0.66);
/// Text saying something's wrong.
pub const ERROR_TEXT: Color = Color::srgb(1.0, 0.4, 0.4);

/// How wide a button is in the menus' column, in pixels, and a pair of them side by side.
pub const WIDE: f32 = 400.0;
pub const HALF: f32 = 196.0;
const HEIGHT: f32 = 40.0;
const THUMB_WIDTH: f32 = 10.0;
// A slider's track, and how far its thumb stands out over and under it, in pixels.
const TRACK_HEIGHT: f32 = 6.0;
const THUMB_OVERHANG: f32 = 4.0;
// How soon a second click on a list's row makes a double click, in seconds.
const DOUBLE_CLICK: f32 = 0.5;

/// The widgets' looks, focus moving with the arrow keys, and the lists' selection.
pub struct WidgetsPlugin;

impl Plugin for WidgetsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, use_the_game_font)
            .init_resource::<LastPicked>()
            .add_observer(listbox_update_selection)
            .add_observer(select_active_row)
            .add_observer(note_picked)
            .add_observer(choose_row)
            .add_systems(
                Update,
                (
                    move_focus_with_arrows,
                    scroll_with_keys,
                    show_sliders,
                    highlight,
                )
                    .chain(),
            );
    }
}

/// How a widget's drawn as it's hovered, focused, selected or pressed.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Look {
    Button,
    Field,
    Slider,
    Row,
}

// All of Fira Mono, built in, in place of the subset of it Bevy has as its default font, which
// lacks symbols the UI uses such as ° and ×. Every text that names no font is in it.
const GAME_FONT: &[u8] = include_bytes!("../../assets/fonts/FiraMono-Regular.ttf");

fn use_the_game_font(fonts: Option<ResMut<Assets<Font>>>) {
    if let Some(mut fonts) = fonts {
        let font = Font::from_bytes(GAME_FONT.to_vec());
        if let Err(error) = fonts.insert(AssetId::default(), font) {
            warn!("couldn't use the game's font: {error}");
        }
    }
}

/// Text sized relative to the root size, so the larger text setting makes it all bigger.
pub(crate) fn font(size: f32) -> TextFont {
    TextFont {
        font_size: bevy::text::FontSize::Rem(size / DEFAULT_REM_SIZE_PX),
        ..default()
    }
}

/// A button's or a row's text, which lets clicks through to it.
pub(crate) fn label(text: &str) -> impl Bundle {
    (Text::new(text), font(18.0), TEXT_SHADOW, Pickable::IGNORE)
}

/// Quieter text, wrapped to `width`, such as a note under a setting.
pub fn note(text: &str, width: f32) -> impl Bundle {
    (
        Text::new(text),
        font(15.0),
        TextColor(QUIET_TEXT),
        TextLayout {
            justify: Justify::Center,
            ..default()
        },
        Node {
            max_width: Val::Px(width),
            ..default()
        },
        Pickable::IGNORE,
    )
}

/// A page's title, over its widgets.
pub fn heading(text: &str) -> impl Bundle {
    (
        Text::new(text),
        font(24.0),
        TextColor(Color::WHITE),
        TEXT_SHADOW,
        Node {
            margin: UiRect::bottom(Val::Px(20.0)),
            ..default()
        },
    )
}

/// A page's widgets, centred down the screen, with Tab moving only among them.
pub fn column() -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(8.0),
            ..default()
        },
        TabGroup::modal(),
    )
}

/// Room between widgets down a page, `height` pixels more than the column leaves.
pub fn gap(height: f32) -> impl Bundle {
    Node {
        height: Val::Px(height),
        ..default()
    }
}

/// Widgets side by side, such as a pair of half-width buttons.
pub fn row() -> impl Bundle {
    Node {
        column_gap: Val::Px(8.0),
        align_items: AlignItems::Center,
        ..default()
    }
}

/// A button `width` pixels wide, for a label to go in.
pub(crate) fn button_of(width: f32) -> impl Bundle {
    (
        Button,
        Look::Button,
        Hovered::default(),
        TabIndex(0),
        Node {
            width: Val::Px(width),
            height: Val::Px(HEIGHT),
            border: UiRect::all(Val::Px(2.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(BUTTON_COLOR),
        BorderColor::all(BUTTON_BORDER_COLOR),
    )
}

/// A tab along the top of a page, as wide as its name: a button, shown pressed as it's
/// `Selected`.
pub fn tab(name: &str) -> impl Bundle {
    (
        Button,
        Look::Button,
        Hovered::default(),
        TabIndex(0),
        Node {
            height: Val::Px(HEIGHT),
            padding: UiRect::horizontal(Val::Px(16.0)),
            border: UiRect::all(Val::Px(2.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(BUTTON_COLOR),
        BorderColor::all(BUTTON_BORDER_COLOR),
        children![label(name)],
    )
}

/// A labelled button `width` pixels wide.
pub fn text_button(text: &str, width: f32) -> impl Bundle {
    (button_of(width), children![label(text)])
}

/// The focus starts on this when its page opens, so Enter presses it.
pub fn first() -> AutoFocus {
    AutoFocus
}

/// A choice between options shown on a button as "Name: Option", stepping to the next on
/// each press, for switches and short lists of options. The page's `Activate` observer changes
/// the setting and calls `choice_text` for the label.
pub fn choice(name: &str, option: &str, width: f32) -> impl Bundle {
    text_button(&choice_text(name, option), width)
}

/// What a choice shows.
pub fn choice_text(name: &str, option: &str) -> String {
    format!("{name}: {option}")
}

/// "On" or "Off", for a switch's option.
pub fn on_off(on: bool) -> &'static str {
    if on {
        "On"
    } else {
        "Off"
    }
}

/// Sets the label of the button `button` to `text`.
pub fn relabel(
    button: Entity,
    text: String,
    children: &Query<&Children>,
    texts: &mut Query<&mut Text>,
) {
    for child in children.iter_descendants(button) {
        if let Ok(mut label) = texts.get_mut(child) {
            label.0.clone_from(&text);
        }
    }
}

/// How a slider shows its value, beside its name.
#[derive(Component, Clone, Copy)]
pub struct SliderShows {
    name: &'static str,
    describe: fn(f32) -> String,
}

#[derive(Component)]
struct SliderText;

/// A slider across `range` in steps of `step`, at `value`, showing "Name: value" with the value
/// as `describe` puts it. The page's `ValueChange<f32>` observer changes the setting, and
/// inserting a `SliderValue` moves it.
pub fn slider(
    name: &'static str,
    range: RangeInclusive<f32>,
    step: f32,
    value: f32,
    describe: fn(f32) -> String,
    width: f32,
) -> impl Bundle {
    (
        Slider {
            track_click: TrackClick::Snap,
            ..default()
        },
        SliderShows { name, describe },
        SliderValue(value),
        SliderRange::from_range(range),
        SliderStep(step),
        Look::Slider,
        Hovered::default(),
        TabIndex(0),
        // its name and value over a track the thumb runs along, so the thumb never hides the text
        Node {
            width: Val::Px(width),
            height: Val::Px(HEIGHT),
            border: UiRect::all(Val::Px(2.0)),
            padding: UiRect::horizontal(Val::Px(4.0)),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceEvenly,
            ..default()
        },
        BackgroundColor(Color::NONE),
        BorderColor::all(Color::NONE),
        children![
            (
                SliderText,
                Text::default(),
                font(16.0),
                TEXT_SHADOW,
                Pickable::IGNORE
            ),
            (
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(TRACK_HEIGHT),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(FIELD_COLOR),
                BorderColor::all(FIELD_BORDER_COLOR),
                Pickable::IGNORE,
                // the thumb travels along the track but for its own width
                children![(
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        right: Val::Px(THUMB_WIDTH),
                        top: Val::Px(-THUMB_OVERHANG),
                        bottom: Val::Px(-THUMB_OVERHANG),
                        ..default()
                    },
                    Pickable::IGNORE,
                    children![(
                        SliderThumb,
                        Node {
                            position_type: PositionType::Absolute,
                            width: Val::Px(THUMB_WIDTH),
                            top: Val::Px(0.0),
                            bottom: Val::Px(0.0),
                            border: UiRect::all(Val::Px(2.0)),
                            ..default()
                        },
                        BackgroundColor(BUTTON_COLOR),
                        BorderColor::all(Color::WHITE),
                        Pickable::IGNORE,
                    )],
                )],
            ),
        ],
    )
}

/// The value a slider's moved to, rounded to its step, as Bevy's slider leaves values dragged to
/// unrounded.
pub fn stepped(change: &ValueChange<f32>, sliders: &Query<(&SliderRange, &SliderStep)>) -> f32 {
    let Ok((range, step)) = sliders.get(change.source) else {
        return change.value;
    };
    let steps = ((change.value - range.start()) / step.0).round();
    range.clamp(range.start() + steps * step.0)
}

// Puts each slider's thumb and text where its value is.
#[allow(clippy::type_complexity)]
fn show_sliders(
    sliders: Query<
        (Entity, &SliderValue, &SliderRange, &SliderShows),
        Or<(Changed<SliderValue>, Added<SliderShows>)>,
    >,
    children: Query<&Children>,
    mut thumbs: Query<&mut Node, With<SliderThumb>>,
    mut texts: Query<&mut Text, With<SliderText>>,
) {
    for (slider, value, range, shows) in &sliders {
        for part in children.iter_descendants(slider) {
            if let Ok(mut thumb) = thumbs.get_mut(part) {
                thumb.left = Val::Percent(range.thumb_position(value.0) * 100.0);
            }
            if let Ok(mut text) = texts.get_mut(part) {
                text.0 = choice_text(shows.name, &(shows.describe)(value.0));
            }
        }
    }
}

/// A one-line text field `width` pixels wide holding `text`, taking up to `max` characters,
/// which it all selects as it's focused, so typing replaces it.
pub fn text_field(text: &str, max: usize, width: f32) -> impl Bundle {
    (
        TextInput,
        SelectAllOnFocus,
        EditableText {
            max_characters: Some(max),
            ..EditableText::new(text)
        },
        font(18.0),
        TextColor(Color::WHITE),
        Look::Field,
        Hovered::default(),
        TabIndex(0),
        Node {
            width: Val::Px(width),
            height: Val::Px(HEIGHT),
            border: UiRect::all(Val::Px(2.0)),
            padding: UiRect::axes(Val::Px(8.0), Val::Px(8.0)),
            ..default()
        },
        BackgroundColor(FIELD_COLOR),
        BorderColor::all(FIELD_BORDER_COLOR),
    )
}

/// Limits a text field to digits, for a number such as a seed or a port.
pub fn digits_only() -> EditableTextFilter {
    EditableTextFilter::new(|c| c.is_ascii_digit())
}

/// What's typed in a text field, trimmed.
pub fn typed(field: &EditableText) -> String {
    field.value().to_string().trim().to_owned()
}

/// A scrolling list `width` by `height` pixels, whose rows a click or the arrow keys select,
/// and Enter or a double click chooses, as `RowChosen`.
pub fn list(width: f32, height: f32) -> impl Bundle {
    (
        ListBox,
        ScrollArea,
        TabIndex(0),
        Node {
            width: Val::Px(width),
            height: Val::Px(height),
            flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll_y(),
            border: UiRect::all(Val::Px(2.0)),
            padding: UiRect::all(Val::Px(4.0)),
            row_gap: Val::Px(4.0),
            ..default()
        },
        BackgroundColor(FIELD_COLOR),
        BorderColor::all(BUTTON_BORDER_COLOR),
    )
}

/// What scrolls in a `scroller`, with the wheel or with the arrow and Page keys while the pointer's
/// over it or it has the focus.
#[derive(Component)]
pub struct Scroller;

// How far the arrow keys scroll, in pixels.
const KEY_SCROLL: f32 = 40.0;

/// A column, for widgets that may not all fit, scrolling once it's taller than `max_height`.
pub fn scroller(max_height: Val) -> impl Bundle {
    (
        Scroller,
        ScrollArea,
        Hovered::default(),
        Node {
            flex_direction: FlexDirection::Column,
            max_height,
            overflow: Overflow::scroll_y(),
            ..default()
        },
    )
}

fn scroll_with_keys(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    mut areas: Query<(Entity, &Hovered, &ComputedNode, &mut ScrollPosition), With<Scroller>>,
) {
    for (area, hovered, node, mut scrolled) in &mut areas {
        if !hovered.get() && focus.get() != Some(area) {
            continue;
        }
        let visible = node.size().y * node.inverse_scale_factor;
        let content = node.content_size().y * node.inverse_scale_factor;
        let by = [
            (KeyCode::ArrowUp, -KEY_SCROLL),
            (KeyCode::ArrowDown, KEY_SCROLL),
            (KeyCode::PageUp, -visible),
            (KeyCode::PageDown, visible),
        ]
        .into_iter()
        .filter(|&(key, _)| keys.just_pressed(key))
        .map(|(_, by)| by)
        .sum::<f32>();
        if by != 0.0 {
            scrolled.y = (scrolled.y + by).clamp(0.0, (content - visible).max(0.0));
        }
    }
}

/// A row of a `list`, for its lines of text to go in.
pub fn list_row() -> impl Bundle {
    (
        ListItem,
        Look::Row,
        Hovered::default(),
        Node {
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        BorderColor::all(Color::NONE),
    )
}

/// The row of `list` that's selected, if one is.
pub fn selected_row(
    list: Entity,
    children: &Query<&Children>,
    rows: &Query<(), (With<ListItem>, With<bevy::ui::Selected>)>,
) -> Option<Entity> {
    children
        .iter_descendants(list)
        .find(|&child| rows.contains(child))
}

/// Selects `row`, as clicking it does.
pub fn select_row(commands: &mut Commands, list: Entity, row: Entity) {
    commands.trigger(ValueChange {
        source: list,
        value: row,
        is_final: true,
    });
}

/// A row of a `list` chosen, by Enter on it or a double click, for playing or joining it.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct RowChosen {
    #[event_target]
    pub list: Entity,
    pub row: Entity,
}

// The row last selected or clicked again, and when, in seconds, to tell a double click by.
#[derive(Resource, Default)]
struct LastPicked(Option<(Entity, f32)>);

fn note_picked(
    picked: On<ValueChange<Entity>>,
    time: Res<Time<Real>>,
    mut last: ResMut<LastPicked>,
) {
    last.0 = Some((picked.value, time.elapsed_secs()));
}

// Bevy's list reselects a row clicked or entered on as it's selected already: that chooses it
// from the keyboard, or as the second click of a double click.
fn choose_row(
    reselected: On<ReselectListRow>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    mut last: ResMut<LastPicked>,
    mut commands: Commands,
) {
    let now = time.elapsed_secs();
    let keyed = keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]);
    let double = last
        .0
        .is_some_and(|(row, at)| row == reselected.row && now - at < DOUBLE_CLICK);
    last.0 = Some((reselected.row, now));
    if keyed || double {
        commands.trigger(RowChosen {
            list: reselected.source,
            row: reselected.row,
        });
    }
}

// Moving among a list's rows selects the row moved to, rather than waiting for Enter, which
// then plays or joins it.
fn select_active_row(
    moved: On<Insert<ActiveDescendant>>,
    lists: Query<&ActiveDescendant>,
    mut commands: Commands,
) {
    if let Ok(ActiveDescendant(Some(row))) = lists.get(moved.entity) {
        select_row(&mut commands, moved.entity, *row);
    }
}

// Up and Down move the focus as Shift-Tab and Tab do, where what's focused doesn't use them
// itself, as text fields, sliders and lists do.
#[allow(clippy::type_complexity)]
fn move_focus_with_arrows(
    keys: Res<ButtonInput<KeyCode>>,
    nav: TabNavigation,
    mut focus: ResMut<InputFocus>,
    mut visible: ResMut<InputFocusVisible>,
    uses_arrows: Query<(), Or<(With<TextInput>, With<Slider>, With<ListBox>)>>,
) {
    let action = if keys.just_pressed(KeyCode::ArrowDown) {
        NavAction::Next
    } else if keys.just_pressed(KeyCode::ArrowUp) {
        NavAction::Previous
    } else {
        return;
    };
    if focus
        .get()
        .is_some_and(|focused| uses_arrows.contains(focused))
    {
        return;
    }
    if let Ok(next) = nav.navigate(&focus, action) {
        focus.set(next, FocusCause::Navigated);
        visible.0 = true;
    }
}

// Lights up what's under the pointer, what the keyboard's focused, and the selected row or tab,
// and greys out what can't be pressed.
#[allow(clippy::type_complexity)]
fn highlight(
    focus: Res<InputFocus>,
    visible: Res<InputFocusVisible>,
    mut widgets: Query<(
        Entity,
        &Look,
        &Hovered,
        Has<bevy::ui::Selected>,
        Has<Pressed>,
        Has<InteractionDisabled>,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    lists: Query<&ActiveDescendant>,
) {
    let active_rows: Vec<Entity> = lists.iter().filter_map(|active| active.0).collect();
    for (widget, look, hovered, selected, pressed, disabled, mut background, mut border) in
        &mut widgets
    {
        let focused = visible.0 && (focus.get() == Some(widget) || active_rows.contains(&widget));
        let lit = !disabled && (hovered.get() || focused || pressed);
        let (colour, edge) = match look {
            Look::Button if disabled => (BUTTON_DISABLED_COLOR, BUTTON_BORDER_COLOR),
            Look::Button if lit || selected => (BUTTON_HOVER_COLOR, BUTTON_HOVER_BORDER_COLOR),
            Look::Button => (BUTTON_COLOR, BUTTON_BORDER_COLOR),
            Look::Field if lit || focus.get() == Some(widget) => (FIELD_COLOR, Color::WHITE),
            Look::Field => (FIELD_COLOR, FIELD_BORDER_COLOR),
            Look::Slider if lit || focus.get() == Some(widget) => (Color::NONE, FIELD_BORDER_COLOR),
            Look::Slider => (Color::NONE, Color::NONE),
            Look::Row => (
                if selected {
                    ROW_SELECTED_COLOR
                } else {
                    Color::NONE
                },
                if lit || selected {
                    FIELD_BORDER_COLOR
                } else {
                    Color::NONE
                },
            ),
        };
        background.set_if_neq(BackgroundColor(colour));
        border.set_if_neq(BorderColor::all(edge));
    }
}

#[cfg(test)]
mod tests {
    use bevy::input::InputPlugin;
    use bevy::input_focus::InputFocusPlugin;

    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin, InputFocusPlugin, WidgetsPlugin));
        app
    }

    #[test]
    fn test_a_slider_shows_its_value_and_puts_its_thumb_there() {
        let mut app = app();
        let slider = app
            .world_mut()
            .spawn(slider(
                "Field of View",
                30.0..=110.0,
                1.0,
                70.0,
                |v| format!("{v}"),
                WIDE,
            ))
            .id();
        app.update();
        let texts: Vec<String> = app
            .world_mut()
            .query_filtered::<&Text, With<SliderText>>()
            .iter(app.world())
            .map(|t| t.0.clone())
            .collect();
        assert_eq!(vec!["Field of View: 70".to_owned()], texts);
        let left = app
            .world_mut()
            .query_filtered::<&Node, With<SliderThumb>>()
            .single(app.world())
            .unwrap()
            .left;
        assert_eq!(Val::Percent(50.0), left);
        app.world_mut()
            .entity_mut(slider)
            .insert(SliderValue(110.0));
        app.update();
        let left = app
            .world_mut()
            .query_filtered::<&Node, With<SliderThumb>>()
            .single(app.world())
            .unwrap()
            .left;
        assert_eq!(Val::Percent(100.0), left);
    }
}
