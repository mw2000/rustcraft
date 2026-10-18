//! What survival shows of the local player (§5.5): hearts and the food bar over the hotbar, with
//! bubbles over the food while they're short of breath, a red flash and a tilt of the view when
//! they're hurt, the screen fading as they sleep, and the death screen. And eating, by holding
//! right click with food selected, which the server times.

use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;

use crate::client::player::LocalPlayer;
use crate::game_mode::GameMode;
use crate::hotbar::{Hotbar, HOTBAR_TOP};
use crate::interaction::{is_used, Target};
use crate::inventory::Inventory;
use crate::menu::{button, font, free_cursor, label, GameState, TEXT_SHADOW};
use crate::player::input::follow_look;
use crate::protocol::{Eat, Respawn};
use crate::survival::{Asleep, Vitals, MAX_AIR};
use crate::world::World;

// `hud.png`'s 9-pixel icons, drawn twice the size, in its order: a heart, half of one and an
// empty one, the same for food, then a bubble.
const ICON: f32 = 18.0;
const HEART: usize = 0;
const FOOD: usize = 3;
const BUBBLE: usize = 6;
// Ten icons to a bar, each two points.
const ICONS: usize = 10;
// As wide as the hotbar, whose edges the bars line up with, and how far over it they sit.
const BARS_WIDTH: f32 = 380.0;
const BARS_GAP: f32 = 4.0;

// How long the red flash and tilt of being hurt last, in seconds, how red the flash goes and how
// far the view tilts, in radians.
const HURT_SECONDS: f32 = 0.4;
const HURT_RED: f32 = 0.25;
const HURT_TILT: f32 = 0.06;
// How long the screen takes to fade to black falling asleep, and back waking, in seconds.
const FALL_ASLEEP_SECONDS: f32 = 1.5;
const WAKE_SECONDS: f32 = 0.3;
const ASLEEP_DARKNESS: f32 = 0.9;

/// The survival HUD, the death screen and eating. Needs `HotbarPlugin`'s place on screen and
/// `MenuPlugin`'s states.
pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hud)
            .add_systems(
                Update,
                (
                    draw_bars,
                    die_and_live,
                    (flash_hurt, fade_asleep).after(follow_look),
                    eat.run_if(resource_exists::<World>),
                ),
            )
            .add_systems(OnEnter(GameState::Dead), show_death_screen)
            .add_systems(OnExit(GameState::Dead), hide_death_screen);
    }
}

#[derive(Component)]
struct Bars;

/// An icon in a bar, the `nth` from where it fills from.
#[derive(Component, Clone, Copy)]
struct Icon {
    bar: Bar,
    nth: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Bar {
    Health,
    Food,
    Air,
}

#[derive(Component)]
struct HurtFlash;

#[derive(Component)]
struct SleepFade;

#[derive(Component)]
struct DeathScreen;

fn spawn_hud(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let image = assets.load("textures/hud.png");
    let layout = layouts.add(TextureAtlasLayout::from_grid(
        UVec2::splat(9),
        7,
        1,
        None,
        None,
    ));
    let icon = |bar, nth| {
        (
            Icon { bar, nth },
            ImageNode::from_atlas_image(
                image.clone(),
                TextureAtlas {
                    layout: layout.clone(),
                    index: 0,
                },
            ),
            Node {
                width: Val::Px(ICON),
                height: Val::Px(ICON),
                ..default()
            },
        )
    };
    // a row of a bar's icons, filling from the left, or the right as the food bar does
    let row = |bar, from_right: bool| {
        let direction = if from_right {
            FlexDirection::RowReverse
        } else {
            FlexDirection::Row
        };
        let icons: Vec<_> = (0..ICONS).map(|nth| icon(bar, nth)).collect();
        (
            Node {
                flex_direction: direction,
                ..default()
            },
            Children::spawn(SpawnIter(icons.into_iter())),
        )
    };
    let full_screen = || Node {
        position_type: PositionType::Absolute,
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        row_gap: Val::Px(10.0),
        ..default()
    };

    commands
        .spawn((
            Bars,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(HOTBAR_TOP + BARS_GAP),
                width: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_children(|bars| {
            bars.spawn(Node {
                width: Val::Px(BARS_WIDTH),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::End,
                ..default()
            })
            .with_children(|bars| {
                bars.spawn(row(Bar::Health, false));
                bars.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    ..default()
                })
                .with_children(|right| {
                    right.spawn(row(Bar::Air, true));
                    right.spawn(row(Bar::Food, true));
                });
            });
        });
    // neither of which stops a click reaching the screens under them
    commands.spawn((
        HurtFlash,
        full_screen(),
        BackgroundColor(Color::NONE),
        Pickable::IGNORE,
    ));
    commands
        .spawn((
            SleepFade,
            full_screen(),
            BackgroundColor(Color::NONE),
            Visibility::Hidden,
            Pickable::IGNORE,
        ))
        .with_child((
            Text::new("Sleeping... move to get up"),
            font(18.0),
            TextColor(Color::WHITE),
            TEXT_SHADOW,
        ));
    commands
        .spawn((
            DeathScreen,
            Node {
                display: Display::None,
                ..full_screen()
            },
            BackgroundColor(Color::srgba(0.5, 0.0, 0.0, 0.5)),
            // over the hotbar and the bars, under the pause menu
            GlobalZIndex(3),
        ))
        .with_children(|screen| {
            screen.spawn((
                Text::new("You died!"),
                font(36.0),
                TextColor(Color::WHITE),
                TEXT_SHADOW,
                Node {
                    margin: UiRect::bottom(Val::Px(30.0)),
                    ..default()
                },
            ));
            screen.spawn(button()).with_child(label("Respawn")).observe(
                |_: On<Activate>, mut respawns: MessageWriter<Respawn>| {
                    respawns.write(Respawn);
                },
            );
        });
}

// Each icon as the local player's vitals have it, and the bars only in survival and while
// the inventory screen, which hides the hotbar, is closed.
fn draw_bars(
    player: Query<(Ref<Vitals>, Ref<GameMode>), With<LocalPlayer>>,
    state: Res<State<GameState>>,
    mut bars: Query<&mut Visibility, With<Bars>>,
    mut icons: Query<(&Icon, &mut ImageNode, &mut Visibility), Without<Bars>>,
) {
    let Ok((vitals, mode)) = player.single() else {
        return;
    };
    if !vitals.is_changed() && !mode.is_changed() && !state.is_changed() {
        return;
    }
    let shown = *mode == GameMode::Survival && *state.get() != GameState::Inventory;
    for mut visibility in &mut bars {
        *visibility = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for (icon, mut image, mut visibility) in &mut icons {
        let (index, seen) = match icon.bar {
            Bar::Health => (HEART + halves(vitals.health, icon.nth), true),
            Bar::Food => (FOOD + halves(vitals.food, icon.nth), true),
            // bubbles only while short of breath, one for each bubble's worth left
            Bar::Air => (
                BUBBLE,
                vitals.air < MAX_AIR && icon.nth < usize::from(vitals.air),
            ),
        };
        if let Some(atlas) = &mut image.texture_atlas {
            atlas.index = index;
        }
        *visibility = if seen {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

// Which of the full, half and empty icons the `nth` of a bar of `points` shows, two to an icon.
fn halves(points: u8, nth: usize) -> usize {
    match usize::from(points).saturating_sub(nth * 2) {
        0 => 2,
        1 => 1,
        _ => 0,
    }
}

// Flashes the screen red and tilts the view for a moment when the local player's hurt.
fn flash_hurt(
    time: Res<Time>,
    player: Query<&Vitals, With<LocalPlayer>>,
    mut flash: Query<&mut BackgroundColor, With<HurtFlash>>,
    mut cameras: Query<&mut Transform, With<Camera3d>>,
    mut last_health: Local<Option<u8>>,
    mut left: Local<f32>,
) {
    let Ok(vitals) = player.single() else {
        return;
    };
    if last_health.is_some_and(|was| vitals.health < was) {
        *left = HURT_SECONDS;
    }
    *last_health = Some(vitals.health);
    *left = (*left - time.delta_secs()).max(0.0);
    let hurt = *left / HURT_SECONDS;
    for mut background in &mut flash {
        background.0 = Color::srgba(0.8, 0.0, 0.0, HURT_RED * hurt);
    }
    if hurt > 0.0 {
        // after the camera's turned the way the player looks, which it is each frame afresh
        for mut camera in &mut cameras {
            camera.rotation *= Quat::from_rotation_z(HURT_TILT * hurt);
        }
    }
}

// Fades the screen to black as the local player falls asleep, and back as they wake.
fn fade_asleep(
    time: Res<Time>,
    player: Query<Has<Asleep>, With<LocalPlayer>>,
    mut fade: Query<(&mut BackgroundColor, &mut Visibility), With<SleepFade>>,
    mut darkness: Local<f32>,
) {
    let Ok(asleep) = player.single() else {
        return;
    };
    let dt = time.delta_secs();
    *darkness = if asleep {
        (*darkness + dt / FALL_ASLEEP_SECONDS).min(1.0)
    } else {
        (*darkness - dt / WAKE_SECONDS).max(0.0)
    };
    for (mut background, mut visibility) in &mut fade {
        background.0 = Color::srgba(0.0, 0.0, 0.0, ASLEEP_DARKNESS * *darkness);
        *visibility = if *darkness > 0.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

// Shows the death screen as the local player dies, and goes back to the game as they respawn.
fn die_and_live(
    player: Query<&Vitals, (With<LocalPlayer>, Changed<Vitals>)>,
    state: Res<State<GameState>>,
    mut next: ResMut<NextState<GameState>>,
) {
    let Ok(vitals) = player.single() else {
        return;
    };
    let dead = *state.get() == GameState::Dead;
    if vitals.is_dead() && !dead {
        next.set(GameState::Dead);
    } else if !vitals.is_dead() && dead {
        next.set(GameState::Playing);
    }
}

fn show_death_screen(
    mut screen: Query<&mut Node, With<DeathScreen>>,
    mut cursor: Query<&mut bevy::window::CursorOptions, With<bevy::window::PrimaryWindow>>,
) {
    for mut node in &mut screen {
        node.display = Display::Flex;
    }
    free_cursor(&mut cursor, true);
}

fn hide_death_screen(mut screen: Query<&mut Node, With<DeathScreen>>) {
    for mut node in &mut screen {
        node.display = Display::None;
    }
}

// Holding right click with food selected eats it, as the server's told as it starts and stops,
// unless it's on a block that's used instead, such as a bed.
#[allow(clippy::too_many_arguments)]
fn eat(
    world: Res<World>,
    state: Res<State<GameState>>,
    mouse: Res<ButtonInput<MouseButton>>,
    target: Res<Target>,
    player: Query<(&GameMode, &Hotbar, &Inventory), With<LocalPlayer>>,
    mut eating: Local<Option<u8>>,
    mut eats: MessageWriter<Eat>,
) {
    let Ok((&mode, hotbar, inventory)) = player.single() else {
        return;
    };
    let food = hotbar
        .held(inventory)
        .is_some_and(|s| s.item.fills().is_some());
    let using = target.0.is_some_and(|hit| is_used(&world, hit, mode));
    let wants =
        *state.get() == GameState::Playing && mouse.pressed(MouseButton::Right) && food && !using;
    let slot = wants.then_some(hotbar.selected as u8);
    if slot != *eating {
        *eating = slot;
        eats.write(Eat(slot));
    }
}
