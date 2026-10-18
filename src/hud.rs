//! What survival shows of the local player (§5.5): hearts and the food bar over the hotbar, with
//! bubbles over the food while they're short of breath and the armour they wear over the
//! hearts, a red flash and a tilt of the view when they're hurt, shaking as a blow lands and a
//! mark round the crosshair pointing where it came from (M3.3), the screen fading as they
//! sleep, and the death screen. And eating, by holding
//! right click with food selected, which the server times. And in any mode, the crosshair.

use std::f32::consts::{PI, TAU};

use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate};

use crate::client::player::LocalPlayer;
use crate::controls::{Action, Bindings, Controls};
use crate::game_mode::GameMode;
use crate::hotbar::HOTBAR_TOP;
use crate::interaction::{is_used, Target, TargetCreature};
use crate::inventory::Hotbar;
use crate::inventory::Inventory;
use crate::menu::widgets::{column, first, text_button, WIDE};
use crate::menu::{font, free_cursor, GameState, InWorld, TEXT_SHADOW};
use crate::player::input::follow_look;
use crate::player::PlayerLook;
use crate::preferences::{Crosshair, Preferences};
use crate::protocol::{Eat, Respawn};
use crate::session;
use crate::survival::{Asleep, Bruise, Vitals, MAX_AIR};
use crate::world::World;

// `hud.png`'s 9-pixel icons, drawn twice the size, in its order: a heart, half of one and an
// empty one, the same for food, then a bubble, then armour, whole, half and none.
const ICON: f32 = 18.0;
const HEART: usize = 0;
const FOOD: usize = 3;
const BUBBLE: usize = 6;
const ARMOUR: usize = 7;
const ICON_COUNT: u32 = 10;
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
// A blow shakes the view this far, in radians, this many times a second, as it tilts.
const BLOW_SHAKE: f32 = 0.015;
const SHAKE_RATE: f32 = 45.0;
// The mark pointing where a blow came from: how long it shows, fading, and its size and how
// far it is from the middle of the screen, in pixels.
const BLOW_MARK_SECONDS: f32 = 1.2;
const BLOW_MARK: Vec2 = Vec2::new(36.0, 8.0);
const BLOW_MARK_RADIUS: f32 = 70.0;
// How long the screen takes to fade to black falling asleep, and back waking, in seconds.
const FALL_ASLEEP_SECONDS: f32 = 1.5;
const WAKE_SECONDS: f32 = 0.3;
const ASLEEP_DARKNESS: f32 = 0.9;
// The crosshair's arms, and its dot, in pixels.
const CROSSHAIR_LENGTH: f32 = 18.0;
const CROSSHAIR_THICKNESS: f32 = 2.0;
const CROSSHAIR_DOT: f32 = 4.0;
const CROSSHAIR_COLOUR: Color = Color::srgba(1.0, 1.0, 1.0, 0.8);

/// The survival HUD, the death screen and eating. Needs `HotbarPlugin`'s place on screen and
/// `MenuPlugin`'s states.
pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Bindings>()
            .add_systems(Startup, spawn_hud)
            .add_systems(
                Update,
                (
                    draw_bars,
                    die_and_live,
                    (flash_hurt, fade_asleep).after(follow_look),
                    eat.run_if(resource_exists::<World>),
                    show_crosshair,
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
    Armour,
}

#[derive(Component)]
struct HurtFlash;

/// What turns to point the mark round the crosshair where a blow came from.
#[derive(Component)]
struct BlowMark;

#[derive(Component)]
struct SleepFade;

#[derive(Component)]
struct DeathScreen;

/// A part of the crosshair, drawn for some of its styles.
#[derive(Component)]
struct CrosshairPart(&'static [Crosshair]);

fn spawn_hud(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let image = assets.load("textures/hud.png");
    let layout = layouts.add(TextureAtlasLayout::from_grid(
        UVec2::splat(9),
        ICON_COUNT,
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
            InWorld,
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
                bars.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    ..default()
                })
                .with_children(|left| {
                    left.spawn(row(Bar::Armour, false));
                    left.spawn(row(Bar::Health, false));
                });
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
    // a point at the middle of the screen, turned to the blow, with the mark above it
    commands.spawn((
        InWorld,
        full_screen(),
        Pickable::IGNORE,
        children![(
            BlowMark,
            Node::default(),
            UiTransform::default(),
            Visibility::Hidden,
            children![(
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(-BLOW_MARK.x / 2.0),
                    top: Val::Px(-BLOW_MARK_RADIUS - BLOW_MARK.y / 2.0),
                    width: Val::Px(BLOW_MARK.x),
                    height: Val::Px(BLOW_MARK.y),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            )],
        )],
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
    commands.spawn((
        DeathScreen,
        Node {
            display: Display::None,
            ..full_screen()
        },
        BackgroundColor(Color::srgba(0.5, 0.0, 0.0, 0.5)),
        // over the hotbar and the bars, under the pause menu
        GlobalZIndex(3),
        children![(
            column(),
            children![
                (
                    Text::new("You died!"),
                    font(36.0),
                    TextColor(Color::WHITE),
                    TEXT_SHADOW,
                    Node {
                        margin: UiRect::bottom(Val::Px(30.0)),
                        ..default()
                    },
                ),
                (
                    text_button("Respawn", WIDE),
                    first(),
                    observe(|_: On<Activate>, mut respawns: MessageWriter<Respawn>| {
                        respawns.write(Respawn);
                    }),
                ),
                (
                    text_button("Quit to Title", WIDE),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.run_system_cached(session::leave);
                    }),
                ),
            ],
        )],
    ));
    let bar = |width, height, styles| {
        (
            CrosshairPart(styles),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Px(width),
                height: Val::Px(height),
                ..default()
            },
            BackgroundColor(CROSSHAIR_COLOUR),
        )
    };
    commands.spawn((
        InWorld,
        Pickable::IGNORE,
        full_screen(),
        children![
            bar(CROSSHAIR_LENGTH, CROSSHAIR_THICKNESS, &[Crosshair::Cross]),
            bar(CROSSHAIR_THICKNESS, CROSSHAIR_LENGTH, &[Crosshair::Cross]),
            bar(CROSSHAIR_DOT, CROSSHAIR_DOT, &[Crosshair::Dot]),
        ],
    ));
}

// The crosshair in the style chosen, only while playing, as the menus and screens over the
// game have the cursor instead.
fn show_crosshair(
    preferences: Res<Preferences>,
    state: Res<State<GameState>>,
    mut parts: Query<(&CrosshairPart, &mut Visibility)>,
) {
    if !preferences.is_changed() && !state.is_changed() {
        return;
    }
    let style = preferences.accessibility.crosshair;
    for (part, mut visibility) in &mut parts {
        *visibility = if *state.get() == GameState::Playing && part.0.contains(&style) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

// Each icon as the local player's vitals and armour have it, and the bars only in survival
// and while the inventory screen, which hides the hotbar, is closed.
#[allow(clippy::type_complexity)]
fn draw_bars(
    player: Query<(Ref<Vitals>, Ref<GameMode>, Ref<Inventory>), With<LocalPlayer>>,
    state: Res<State<GameState>>,
    mut bars: Query<&mut Visibility, With<Bars>>,
    mut icons: Query<(&Icon, &mut ImageNode, &mut Visibility), Without<Bars>>,
) {
    let Ok((vitals, mode, inventory)) = player.single() else {
        return;
    };
    let changed = vitals.is_changed() || mode.is_changed() || inventory.is_changed();
    if !changed && !state.is_changed() {
        return;
    }
    let armour = inventory.armour_points();
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
            // only while some's worn
            Bar::Armour => (ARMOUR + halves(armour, icon.nth), armour > 0),
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

/// How the local player's last hurt is being shown: how much longer the flash and tilt last,
/// which way the view tilts, and how much longer the mark of a blow shows and which way it
/// points, as an angle from straight ahead, clockwise.
#[derive(Default)]
struct Hurting {
    left: f32,
    tilt: f32,
    shaking: bool,
    mark_left: f32,
    mark: f32,
}

// Flashes the screen red and tilts the view for a moment when the local player's hurt, shaking
// it if it was a blow and tilting away from it, unless the tilt's turned off, and points the
// mark round the crosshair where a blow came from.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn flash_hurt(
    time: Res<Time>,
    preferences: Res<Preferences>,
    player: Query<(&Vitals, Ref<Bruise>, &Transform, &PlayerLook), With<LocalPlayer>>,
    mut flash: Query<&mut BackgroundColor, With<HurtFlash>>,
    mut cameras: Query<&mut Transform, (With<Camera3d>, Without<LocalPlayer>)>,
    mut marks: Query<(&mut UiTransform, &mut Visibility, &Children), With<BlowMark>>,
    mut colours: Query<&mut BackgroundColor, Without<HurtFlash>>,
    mut last_health: Local<Option<u8>>,
    mut hurting: Local<Hurting>,
) {
    let Ok((vitals, bruise, body, look)) = player.single() else {
        return;
    };
    let dt = time.delta_secs();
    if last_health.is_some_and(|was| vitals.health < was) {
        *hurting = Hurting {
            left: HURT_SECONDS,
            tilt: 1.0,
            shaking: false,
            ..*hurting
        };
    }
    *last_health = Some(vitals.health);
    if bruise.is_changed() && !bruise.is_added() {
        let towards = bruise.from - body.translation;
        let yaw = (-towards.x).atan2(-towards.z);
        let aside = (yaw - look.yaw + PI).rem_euclid(TAU) - PI;
        hurting.mark = -aside;
        hurting.mark_left = BLOW_MARK_SECONDS;
        // away from the side it came from, and shaken
        hurting.tilt = if aside > 0.0 { -1.0 } else { 1.0 };
        hurting.shaking = true;
    }
    hurting.left = (hurting.left - dt).max(0.0);
    hurting.mark_left = (hurting.mark_left - dt).max(0.0);
    let hurt = hurting.left / HURT_SECONDS;
    for mut background in &mut flash {
        background.0 = Color::srgba(0.8, 0.0, 0.0, HURT_RED * hurt);
    }
    if hurt > 0.0 && preferences.accessibility.hurt_tilt {
        let shake = if hurting.shaking {
            BLOW_SHAKE * hurt * (time.elapsed_secs() * SHAKE_RATE).sin()
        } else {
            0.0
        };
        // after the camera's turned the way the player looks, which it is each frame afresh
        for mut camera in &mut cameras {
            camera.rotation *= Quat::from_rotation_z(HURT_TILT * hurt * hurting.tilt)
                * Quat::from_rotation_x(shake);
        }
    }
    let shown = hurting.mark_left / BLOW_MARK_SECONDS;
    for (mut turned, mut visibility, children) in &mut marks {
        visibility.set_if_neq(if shown > 0.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        turned.rotation = Rot2::radians(hurting.mark);
        for &child in children {
            if let Ok(mut colour) = colours.get_mut(child) {
                colour.0 = Color::srgba(0.85, 0.08, 0.05, 0.85 * shown);
            }
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
// unless it's on a block that's used instead, such as a bed, or on a creature.
#[allow(clippy::too_many_arguments)]
fn eat(
    world: Res<World>,
    state: Res<State<GameState>>,
    controls: Controls,
    target: Res<Target>,
    creature: Res<TargetCreature>,
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
    // and feeds a creature looked at instead
    let using = target.0.is_some_and(|hit| is_used(&world, hit, mode)) || creature.0.is_some();
    let wants =
        *state.get() == GameState::Playing && controls.pressed(Action::Place) && food && !using;
    let slot = wants.then_some(hotbar.selected as u8);
    if slot != *eating {
        *eating = slot;
        eats.write(Eat(slot));
    }
}
