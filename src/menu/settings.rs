//! Settings, in tabs: Video, Controls (with a key for every action), Audio and Accessibility.
//! Each widget shows one of the player's `Preferences`, changes it as it's used, and follows it
//! as anything else changes it, such as a preset.

use bevy::prelude::*;
use bevy::ui::Selected;
use bevy::ui_widgets::{observe, Activate, ValueChange};
use bevy::ui_widgets::{SliderRange, SliderStep, SliderValue};

use super::widgets::{
    choice, choice_text, column, first, gap, heading, label, list, note, on_off, relabel, slider,
    stepped, text_button, ERROR_TEXT, WIDE,
};
use super::{back, Menu, Page};
use crate::controls::{Action, Binding, Bindings};
use crate::preferences::{redraw_for_settings, Crosshair, Preferences, Preset};

// Two columns of settings, each this wide.
const COLUMN: f32 = 380.0;
const BINDINGS_HEIGHT: f32 = 300.0;

/// The tabs of the Settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Video,
    Controls,
    Audio,
    Accessibility,
}

impl SettingsTab {
    const ALL: [SettingsTab; 4] = [
        SettingsTab::Video,
        SettingsTab::Controls,
        SettingsTab::Audio,
        SettingsTab::Accessibility,
    ];

    fn name(self) -> &'static str {
        match self {
            SettingsTab::Video => "Video",
            SettingsTab::Controls => "Controls",
            SettingsTab::Audio => "Audio",
            SettingsTab::Accessibility => "Accessibility",
        }
    }
}

/// The action a key's being bound to, waiting for the key or button to be pressed, which
/// Escape cancels.
#[derive(Resource)]
pub(super) struct Rebinding {
    action: Action,
    // pressed already in the frame it started, so the key that pressed the button isn't taken
    armed: bool,
}

/// Which setting a widget shows and changes.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum Setting {
    Preset,
    RenderDistance,
    FullDetail,
    FieldOfView,
    UiScale,
    FrameCap,
    Fullscreen,
    Vsync,
    Shadows,
    Clouds,
    Sensitivity,
    InvertY,
    Master,
    Effects,
    Ambience,
    Music,
    HurtTilt,
    LargerText,
    Crosshair,
}

/// A slider's range, step and how it shows its value.
struct Range {
    name: &'static str,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    describe: fn(f32) -> String,
}

impl Setting {
    fn range(self) -> Option<Range> {
        let range = |name, range, step, describe| {
            Some(Range {
                name,
                range,
                step,
                describe,
            })
        };
        match self {
            Setting::RenderDistance => range("Render Distance", 64.0..=2048.0, 64.0, |v| {
                format!("{v} ({:.1} km)", v * 16.0 / 1000.0)
            }),
            Setting::FullDetail => range("Full Detail", 4.0..=32.0, 1.0, |v| format!("{v} chunks")),
            Setting::FieldOfView => range("Field of View", 30.0..=110.0, 1.0, |v| {
                format!("{v} degrees")
            }),
            Setting::UiScale => range("UI Scale", 0.5..=2.0, 0.05, |v| format!("{v:.2}x")),
            Setting::FrameCap => range("Frame Cap", 0.0..=240.0, 10.0, |v| {
                if v == 0.0 {
                    "Unlimited".into()
                } else {
                    format!("{v} a second")
                }
            }),
            Setting::Sensitivity => {
                range("Mouse Sensitivity", 0.1..=3.0, 0.05, |v| format!("{v:.2}x"))
            }
            Setting::Master => range("Volume", 0.0..=1.0, 0.05, percent),
            Setting::Effects => range("Effects", 0.0..=1.0, 0.05, percent),
            Setting::Ambience => range("Ambience", 0.0..=1.0, 0.05, percent),
            Setting::Music => range("Music", 0.0..=1.0, 0.05, percent),
            _ => None,
        }
    }

    fn value(self, p: &Preferences) -> f32 {
        match self {
            Setting::RenderDistance => p.video.render_distance as f32,
            Setting::FullDetail => p.video.full_detail_distance as f32,
            Setting::FieldOfView => p.video.field_of_view,
            Setting::UiScale => p.video.ui_scale,
            Setting::FrameCap => p.video.frame_cap as f32,
            Setting::Sensitivity => p.controls.sensitivity,
            Setting::Master => p.audio.master,
            Setting::Effects => p.audio.effects,
            Setting::Ambience => p.audio.ambience,
            Setting::Music => p.audio.music,
            _ => 0.0,
        }
    }

    fn set(self, p: &mut Preferences, value: f32) {
        match self {
            Setting::RenderDistance => p.video.render_distance = value as u32,
            Setting::FullDetail => p.video.full_detail_distance = value as u32,
            Setting::FieldOfView => p.video.field_of_view = value,
            Setting::UiScale => p.video.ui_scale = value,
            Setting::FrameCap => p.video.frame_cap = value as u32,
            Setting::Sensitivity => p.controls.sensitivity = value,
            Setting::Master => p.audio.master = value,
            Setting::Effects => p.audio.effects = value,
            Setting::Ambience => p.audio.ambience = value,
            Setting::Music => p.audio.music = value,
            _ => {}
        }
    }

    /// A choice's name and the option chosen.
    fn choice(self, p: &Preferences) -> (&'static str, &'static str) {
        match self {
            Setting::Preset => (
                "Preset",
                Preset::of(p.video).map_or("Custom", Preset::label),
            ),
            Setting::Fullscreen => ("Fullscreen", on_off(p.video.fullscreen)),
            Setting::Vsync => ("VSync", on_off(p.video.vsync)),
            Setting::Shadows => ("Shadows", on_off(p.video.shadows)),
            Setting::Clouds => ("Clouds", on_off(p.video.clouds)),
            Setting::InvertY => ("Invert Mouse", on_off(p.controls.invert_y)),
            Setting::HurtTilt => ("Hurt Tilt", on_off(p.accessibility.hurt_tilt)),
            Setting::LargerText => ("Larger Text", on_off(p.accessibility.larger_text)),
            Setting::Crosshair => ("Crosshair", p.accessibility.crosshair.label()),
            _ => ("", ""),
        }
    }

    /// Steps a choice on to its next option.
    fn choose_next(self, p: &mut Preferences) {
        let toggle = |on: &mut bool| *on = !*on;
        match self {
            Setting::Preset => {
                let next = match Preset::of(p.video) {
                    Some(Preset::Low) => Preset::Medium,
                    Some(Preset::Medium) => Preset::High,
                    Some(Preset::High) | None => Preset::Low,
                };
                p.video = next.apply(p.video);
            }
            Setting::Fullscreen => toggle(&mut p.video.fullscreen),
            Setting::Vsync => toggle(&mut p.video.vsync),
            Setting::Shadows => toggle(&mut p.video.shadows),
            Setting::Clouds => toggle(&mut p.video.clouds),
            Setting::InvertY => toggle(&mut p.controls.invert_y),
            Setting::HurtTilt => toggle(&mut p.accessibility.hurt_tilt),
            Setting::LargerText => toggle(&mut p.accessibility.larger_text),
            Setting::Crosshair => {
                let at = Crosshair::ALL
                    .iter()
                    .position(|&c| c == p.accessibility.crosshair)
                    .unwrap_or(0);
                p.accessibility.crosshair = Crosshair::ALL[(at + 1) % Crosshair::ALL.len()];
            }
            _ => {}
        }
    }
}

/// A button binding `action`, showing what it's bound to.
#[derive(Component)]
struct BindingButton(Action);

pub(super) fn build(app: &mut App) {
    app.add_observer(slide).add_observer(choose).add_systems(
        Update,
        (
            take_binding.run_if(resource_exists::<Rebinding>),
            show_settings.run_if(resource_changed::<Preferences>),
            show_bindings,
            apply_on_close,
        )
            .chain(),
    );
}

pub(super) fn spawn(
    In(root): In<Entity>,
    menu: Res<Menu>,
    preferences: Res<Preferences>,
    mut commands: Commands,
) {
    let Some(&Page::Settings(shown)) = menu.top() else {
        return;
    };
    commands.entity(root).with_children(|root| {
        root.spawn(column()).with_children(|page| {
            page.spawn(heading("Settings"));
            page.spawn(Node {
                margin: UiRect::bottom(Val::Px(12.0)),
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|tabs| {
                for tab_shown in SettingsTab::ALL {
                    let selected = tab_shown == shown;
                    tabs.spawn((
                        text_button(tab_shown.name(), 180.0),
                        observe(move |_: On<Activate>, mut menu: ResMut<Menu>| {
                            if menu.top() != Some(&Page::Settings(tab_shown)) {
                                menu.replace(Page::Settings(tab_shown));
                            }
                        }),
                    ))
                    .insert_if((Selected, first()), || selected);
                }
            });
            match shown {
                SettingsTab::Video => video(page, &preferences),
                SettingsTab::Controls => controls(page, &preferences),
                SettingsTab::Audio => audio(page, &preferences),
                SettingsTab::Accessibility => accessibility(page, &preferences),
            }
            page.spawn(gap(8.0));
            page.spawn((
                text_button("Done", WIDE),
                observe(|_: On<Activate>, mut commands: Commands| {
                    commands.run_system_cached(back);
                }),
            ));
        });
    });
}

// Two columns of the settings given, the first filled first.
fn columns(page: &mut ChildSpawnerCommands, p: &Preferences, settings: &[&[Setting]]) {
    page.spawn(Node {
        column_gap: Val::Px(16.0),
        align_items: AlignItems::Start,
        ..default()
    })
    .with_children(|columns| {
        for settings in settings {
            columns
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|column| {
                    for &setting in *settings {
                        widget(column, p, setting);
                    }
                });
        }
    });
}

fn widget(parent: &mut ChildSpawnerCommands, p: &Preferences, setting: Setting) {
    match setting.range() {
        Some(Range {
            name,
            range,
            step,
            describe,
        }) => {
            parent.spawn((
                setting,
                slider(name, range, step, setting.value(p), describe, COLUMN),
            ));
        }
        None => {
            let (name, option) = setting.choice(p);
            parent.spawn((setting, choice(name, option, COLUMN)));
        }
    }
}

fn video(page: &mut ChildSpawnerCommands, p: &Preferences) {
    use Setting::*;
    columns(
        page,
        p,
        &[
            &[Preset, RenderDistance, FullDetail, FieldOfView, UiScale],
            &[FrameCap, Fullscreen, Vsync, Shadows, Clouds],
        ],
    );
    page.spawn(note(
        "Changes to how far the terrain's drawn take effect as Settings closes.",
        COLUMN * 2.0,
    ));
}

fn controls(page: &mut ChildSpawnerCommands, p: &Preferences) {
    columns(page, p, &[&[Setting::Sensitivity], &[Setting::InvertY]]);
    page.spawn(list(COLUMN * 2.0 + 16.0, BINDINGS_HEIGHT))
        .with_children(|list| {
            for &action in Action::ALL {
                list.spawn(Node {
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    flex_shrink: 0.0,
                    padding: UiRect::left(Val::Px(8.0)),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(label(action.label()));
                    row.spawn((
                        BindingButton(action),
                        text_button(&p.controls.bindings.get(action).describe(), 180.0),
                        observe(start_binding),
                    ));
                });
            }
        });
    page.spawn((
        text_button("Reset Keys", COLUMN),
        observe(|_: On<Activate>, mut preferences: ResMut<Preferences>| {
            preferences.controls.bindings = Bindings::default();
        }),
    ));
}

fn audio(page: &mut ChildSpawnerCommands, p: &Preferences) {
    use Setting::*;
    columns(page, p, &[&[Master, Effects], &[Ambience, Music]]);
}

fn percent(volume: f32) -> String {
    format!("{:.0}%", volume * 100.0)
}

fn accessibility(page: &mut ChildSpawnerCommands, p: &Preferences) {
    use Setting::*;
    columns(page, p, &[&[HurtTilt, LargerText], &[Crosshair]]);
}

fn slide(
    change: On<ValueChange<f32>>,
    settings: Query<&Setting>,
    sliders: Query<(&SliderRange, &SliderStep)>,
    mut preferences: ResMut<Preferences>,
) {
    if let Ok(&setting) = settings.get(change.source) {
        let value = stepped(&change, &sliders);
        if setting.value(&preferences) != value {
            setting.set(&mut preferences, value);
        }
    }
}

fn choose(activate: On<Activate>, settings: Query<&Setting>, mut preferences: ResMut<Preferences>) {
    if let Ok(&setting) = settings.get(activate.entity) {
        setting.choose_next(&mut preferences);
    }
}

// Every setting's widget as the settings have it now.
fn show_settings(
    preferences: Res<Preferences>,
    widgets: Query<(Entity, &Setting, Option<&SliderValue>)>,
    children: Query<&Children>,
    mut texts: Query<&mut Text>,
    mut commands: Commands,
) {
    for (widget, setting, value) in &widgets {
        match value {
            Some(&SliderValue(shown)) => {
                let wanted = setting.value(&preferences);
                if shown != wanted {
                    commands.entity(widget).insert(SliderValue(wanted));
                }
            }
            None => {
                let (name, option) = setting.choice(&preferences);
                relabel(widget, choice_text(name, option), &children, &mut texts);
            }
        }
    }
}

fn start_binding(activate: On<Activate>, buttons: Query<&BindingButton>, mut commands: Commands) {
    if let Ok(&BindingButton(action)) = buttons.get(activate.entity) {
        commands.insert_resource(Rebinding {
            action,
            armed: false,
        });
    }
}

// Binds the key or button pressed next to the action waiting for one, or cancels with Escape.
fn take_binding(
    mut rebinding: ResMut<Rebinding>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut preferences: ResMut<Preferences>,
    mut commands: Commands,
) {
    if !rebinding.armed {
        rebinding.armed = true;
        return;
    }
    let pressed = keys
        .get_just_pressed()
        .next()
        .map(|&key| Binding::Key(key))
        .or_else(|| mouse.get_just_pressed().next().map(|&b| Binding::Mouse(b)));
    let Some(pressed) = pressed else {
        return;
    };
    if pressed != Binding::Key(KeyCode::Escape) {
        preferences.controls.bindings.set(rebinding.action, pressed);
    }
    commands.remove_resource::<Rebinding>();
}

// Each binding's button with what it's bound to, red if another action's bound to the same, or
// asking for a key while it waits for one.
fn show_bindings(
    preferences: Res<Preferences>,
    rebinding: Option<Res<Rebinding>>,
    buttons: Query<(Entity, &BindingButton)>,
    children: Query<&Children>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let bindings = &preferences.controls.bindings;
    for (button, &BindingButton(action)) in &buttons {
        let waiting = rebinding.as_ref().is_some_and(|r| r.action == action);
        let (text, colour) = if waiting {
            ("> Press a key <".to_owned(), Color::WHITE)
        } else if bindings.clashes(action).next().is_some() {
            (bindings.get(action).describe(), ERROR_TEXT)
        } else {
            (bindings.get(action).describe(), Color::WHITE)
        };
        for child in children.iter_descendants(button) {
            if let Ok((mut shown, mut shown_colour)) = texts.get_mut(child) {
                if shown.0 != text {
                    shown.0.clone_from(&text);
                }
                shown_colour.set_if_neq(TextColor(colour));
            }
        }
    }
}

// Draws the terrain at the distances chosen once Settings has closed, as doing it while they're
// being chosen would draw it again at every step.
fn apply_on_close(menu: Res<Menu>, mut was_open: Local<bool>, mut commands: Commands) {
    let open = menu.0.iter().any(|page| matches!(page, Page::Settings(_)));
    if *was_open && !open {
        commands.run_system_cached(redraw_for_settings);
    }
    *was_open = open;
}
