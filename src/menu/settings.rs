//! Settings, in tabs: Video, Controls (with a key for every action), Audio, Accessibility and
//! Skin. Each widget shows one of the player's `Preferences`, changes it as it's used, and
//! follows it as anything else changes it, such as a preset.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::text::{EditableText, TextEdit};
use bevy::ui::Selected;
use bevy::ui_widgets::{observe, Activate, ValueChange};
use bevy::ui_widgets::{SliderRange, SliderStep, SliderValue};

use super::widgets::{
    choice, choice_text, column, first, gap, heading, label, list, list_row, note, on_off, relabel,
    row, slider, stepped, tab, text_button, text_field, typed, ERROR_TEXT, QUIET_TEXT, WIDE,
};
use super::{back, Menu, Page};
use crate::client::models::SkinPreview;
use crate::client::player::OwnSkin;
use crate::controls::{Action, Binding, Bindings};
use crate::preferences::{
    redraw_for_settings, skins_dir, Crosshair, Particles, Preferences, Preset, SkinSettings,
};
use crate::skin::{default_skin, read_skin, ArmStyle, DEFAULT_SKINS};

// Two columns of settings, each this wide.
const COLUMN: f32 = 380.0;
const BINDINGS_HEIGHT: f32 = 300.0;
// The skin's preview, in pixels, and the list of skins kept beside it.
const PREVIEW: UVec2 = UVec2::new(200, 320);
const SKINS_HEIGHT: f32 = 132.0;
// The longest path a skin's file field takes, in characters.
const MAX_PATH: usize = 1024;

/// The tabs of the Settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Video,
    Controls,
    Audio,
    Accessibility,
    Skin,
}

impl SettingsTab {
    const ALL: [SettingsTab; 5] = [
        SettingsTab::Video,
        SettingsTab::Controls,
        SettingsTab::Audio,
        SettingsTab::Accessibility,
        SettingsTab::Skin,
    ];

    fn name(self) -> &'static str {
        match self {
            SettingsTab::Video => "Video",
            SettingsTab::Controls => "Controls",
            SettingsTab::Audio => "Audio",
            SettingsTab::Accessibility => "Accessibility",
            SettingsTab::Skin => "Skin",
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
    AmbientLife,
    RainAndSnow,
    Sensitivity,
    InvertY,
    Master,
    Effects,
    Ambience,
    Music,
    HurtTilt,
    LargerText,
    Crosshair,
    Arms,
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
                format!("{v} ({:.0} km)", v * 16.0 / 1000.0)
            }),
            Setting::FullDetail => range("Full Detail", 4.0..=32.0, 1.0, |v| format!("{v} chunks")),
            Setting::FieldOfView => range("Field of View", 30.0..=110.0, 1.0, |v| format!("{v}°")),
            Setting::UiScale => range("UI Scale", 0.5..=2.0, 0.05, |v| format!("{v:.2}×")),
            Setting::FrameCap => range("Frame Cap", 0.0..=240.0, 10.0, |v| {
                if v == 0.0 {
                    "Unlimited".into()
                } else {
                    format!("{v} a second")
                }
            }),
            Setting::Sensitivity => {
                range("Mouse Sensitivity", 0.1..=3.0, 0.05, |v| format!("{v:.2}×"))
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
            Setting::AmbientLife => ("Ambient Life", on_off(p.video.ambient_life)),
            Setting::RainAndSnow => ("Rain and Snow", p.video.particles.label()),
            Setting::InvertY => ("Invert Mouse", on_off(p.controls.invert_y)),
            Setting::HurtTilt => ("Hurt Tilt", on_off(p.accessibility.hurt_tilt)),
            Setting::LargerText => ("Larger Text", on_off(p.accessibility.larger_text)),
            Setting::Crosshair => ("Crosshair", p.accessibility.crosshair.label()),
            Setting::Arms => (
                "Arms",
                match p.skin.arms {
                    ArmStyle::Classic => "Classic",
                    ArmStyle::Slim => "Slim",
                },
            ),
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
            Setting::AmbientLife => toggle(&mut p.video.ambient_life),
            Setting::RainAndSnow => {
                let at = Particles::ALL
                    .iter()
                    .position(|&c| c == p.video.particles)
                    .unwrap_or(0);
                p.video.particles = Particles::ALL[(at + 1) % Particles::ALL.len()];
            }
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
            Setting::Arms => {
                p.skin.arms = match p.skin.arms {
                    ArmStyle::Classic => ArmStyle::Slim,
                    ArmStyle::Slim => ArmStyle::Classic,
                }
            }
            _ => {}
        }
    }
}

/// A button binding `action`, showing what it's bound to.
#[derive(Component)]
struct BindingButton(Action);

// The field the skin's file is typed in, a row of the skins kept in the skins directory, and
// what's said of the skin worn.
#[derive(Component)]
struct SkinField;

#[derive(Component)]
struct SkinRow(PathBuf);

#[derive(Component)]
struct SkinStatus;

pub(super) fn build(app: &mut App) {
    app.add_observer(slide).add_observer(choose).add_systems(
        Update,
        (
            take_binding.run_if(resource_exists::<Rebinding>),
            show_settings.run_if(resource_changed::<Preferences>),
            show_bindings,
            show_skin_status,
            apply_on_close,
        )
            .chain(),
    );
}

pub(super) fn spawn(
    In(root): In<Entity>,
    menu: Res<Menu>,
    preferences: Res<Preferences>,
    // none without a renderer, as in tests
    mut images: Option<ResMut<Assets<Image>>>,
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
                        tab(tab_shown.name()),
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
                SettingsTab::Skin => skin(page, &preferences, images.as_deref_mut()),
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
            &[
                Preset,
                RenderDistance,
                FullDetail,
                FieldOfView,
                UiScale,
                RainAndSnow,
            ],
            &[FrameCap, Fullscreen, Vsync, Shadows, Clouds, AmbientLife],
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

// The preview of the skin worn, beside the file it's from, the skins kept in the skins directory,
// the arms it's drawn with and what's said of it.
fn skin(page: &mut ChildSpawnerCommands, p: &Preferences, images: Option<&mut Assets<Image>>) {
    let preview_image = images.map(|images| {
        images.add(Image::new_target_texture(
            PREVIEW.x,
            PREVIEW.y,
            TextureFormat::Rgba8Unorm,
            Some(TextureFormat::Rgba8UnormSrgb),
        ))
    });
    let dir = skins_dir();
    // made for the player to find, as it's where they're told to keep their skins
    if let Err(error) = std::fs::create_dir_all(&dir) {
        warn!("couldn't make {}: {error}", dir.display());
    }
    let kept = kept_skins(&dir);
    page.spawn(Node {
        column_gap: Val::Px(16.0),
        align_items: AlignItems::Start,
        ..default()
    })
    .with_children(|columns| {
        let mut preview = columns.spawn((
            Node {
                width: Val::Px(PREVIEW.x as f32),
                height: Val::Px(PREVIEW.y as f32),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BorderColor::all(Color::BLACK),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
        ));
        if let Some(image) = preview_image {
            preview.insert((ImageNode::new(image), SkinPreview));
        }
        columns
            .spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|column| {
                column.spawn((label("PNG file"), TextColor(QUIET_TEXT)));
                column.spawn(row()).with_children(|row| {
                    row.spawn((
                        SkinField,
                        text_field(&p.skin.file, MAX_PATH, COLUMN - 108.0),
                    ));
                    row.spawn((text_button("Use", 100.0), observe(use_typed)));
                });
                column
                    .spawn(list(COLUMN, SKINS_HEIGHT))
                    .observe(use_kept)
                    .with_children(|list| {
                        if kept.is_empty() {
                            list.spawn(note("No skins kept here yet.", COLUMN - 16.0));
                        }
                        for path in kept {
                            let name = path.file_stem().unwrap_or_default().to_string_lossy();
                            list.spawn((SkinRow(path.clone()), list_row()))
                                .insert_if(Selected, || Path::new(&p.skin.file) == path)
                                .with_child(label(&name));
                        }
                    });
                widget(column, p, Setting::Arms);
                column.spawn((
                    text_button("Default Skin", COLUMN),
                    observe(|_: On<Activate>, mut preferences: ResMut<Preferences>| {
                        preferences.skin = SkinSettings::default();
                    }),
                ));
                column.spawn((SkinStatus, note("", COLUMN)));
            });
    });
    page.spawn(note(
        &format!(
            "A skin's a 64×64 or 64×32 PNG of up to 32 KB, in Minecraft's layout. Keep yours in \
             {} to choose them here.",
            dir.display()
        ),
        COLUMN * 2.0,
    ));
}

// The PNG files in `dir`, by name.
fn kept_skins(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut kept: Vec<PathBuf> = entries
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
        })
        .collect();
    kept.sort();
    kept
}

// Wears the skin in the file at `path`, with the arms it says it's drawn for, or says why it
// can't, wearing what was worn before.
fn wear_file(path: &Path, preferences: &mut Preferences, own: &mut OwnSkin) {
    match read_skin(path) {
        Ok((_, arms)) => {
            preferences.skin = SkinSettings {
                file: path.to_string_lossy().into_owned(),
                arms,
            };
        }
        Err(error) => own.problem = Some(error),
    }
}

fn use_typed(
    _: On<Activate>,
    field: Query<&EditableText, With<SkinField>>,
    mut preferences: ResMut<Preferences>,
    mut own: ResMut<OwnSkin>,
) {
    let path = field.single().map(typed).unwrap_or_default();
    if path.is_empty() {
        preferences.skin = SkinSettings::default();
    } else {
        wear_file(Path::new(&path), &mut preferences, &mut own);
    }
}

// Selecting a skin kept wears it, so the arrow keys go through them on the preview.
fn use_kept(
    selected: On<ValueChange<Entity>>,
    rows: Query<&SkinRow>,
    mut preferences: ResMut<Preferences>,
    mut own: ResMut<OwnSkin>,
    mut fields: Query<&mut EditableText, With<SkinField>>,
) {
    if let Ok(SkinRow(path)) = rows.get(selected.value) {
        wear_file(path, &mut preferences, &mut own);
        for mut field in &mut fields {
            field.clear();
            field.queue_edit(TextEdit::Insert(path.to_string_lossy().as_ref().into()));
        }
    }
}

// What's said of the skin worn: whose it is, or why the one chosen can't be worn.
fn show_skin_status(
    own: Res<OwnSkin>,
    preferences: Res<Preferences>,
    mut status: Query<(&mut Text, &mut TextColor), With<SkinStatus>>,
) {
    if !own.is_changed() && !preferences.is_changed() && status.iter().all(|(t, _)| !t.0.is_empty())
    {
        return;
    }
    let (text, colour) = match (&own.problem, &own.skin) {
        (Some(problem), _) => (format!("Couldn't wear that skin: {problem}."), ERROR_TEXT),
        (None, Some(_)) => ("Wearing your own skin.".to_owned(), QUIET_TEXT),
        (None, None) => {
            let (name, _) = DEFAULT_SKINS[default_skin(&preferences.player_name())];
            (
                format!("Wearing the {name}, the default skin for your name."),
                QUIET_TEXT,
            )
        }
    };
    for (mut shown, mut shown_colour) in &mut status {
        if shown.0 != text {
            shown.0.clone_from(&text);
        }
        shown_colour.set_if_neq(TextColor(colour));
    }
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
