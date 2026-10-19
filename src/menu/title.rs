//! The title screen: the game's name and the ways into it, over a slow pan along the horizon of
//! a world, which the game's own terrain renderer draws (P1), nearer than in the game to keep the
//! menus cheap.

use crate::client::player::WorldCamera;
use bevy::prelude::*;
use bevy::ui_widgets::{observe, Activate};

use super::widgets::{column, first, font, note, text_button, TEXT_SHADOW, WIDE};
use super::{GameState, Menu, Page, SettingsTab};
use crate::chunks::material::TerrainMaterials;
use crate::chunks::redraw_terrain;
use crate::settings::{RendererSettings, Settings};
use crate::world::{LeaveWorld, LeaveWorldSystems, World};

// A world with mountains and a lake in view of its spawn.
const SCENERY_SEED: u32 = 7;
// How far the scenery's drawn, in chunks, and at full detail, at most: far enough for the
// horizon, near enough to load in a few seconds and draw cheaply.
const SCENERY_DISTANCE: u32 = 384;
const SCENERY_FULL_DETAIL: u32 = 4;
// Where it's seen from, in blocks above the spawn, and how the view turns, in radians a second,
// looking a little down.
const VIEW_HEIGHT: f32 = 90.0;
const PAN_SPEED: f32 = 0.015;
const VIEW_PITCH: f32 = -0.07;

/// What the camera hangs from on the title screen, turning slowly.
#[derive(Component)]
struct Viewpoint;

pub(super) fn build(app: &mut App) {
    app.add_systems(OnExit(GameState::Title), leave_scenery)
        .add_systems(
            Update,
            (
                // once there's terrain to draw it with, which the game sets up as it starts
                show_scenery
                    .run_if(not(any_with_component::<Viewpoint>))
                    .run_if(resource_exists::<TerrainMaterials>),
                pan,
            )
                .chain()
                .run_if(in_state(GameState::Title)),
        )
        .add_systems(
            LeaveWorld,
            forget_viewpoint.in_set(LeaveWorldSystems::Clear),
        );
}

pub(super) fn spawn(In(root): In<Entity>, mut commands: Commands) {
    commands.entity(root).with_child((
        column(),
        children![
            (
                Text::new("rustcraft"),
                font(72.0),
                TextColor(Color::WHITE),
                TextShadow {
                    offset: Vec2::splat(4.0),
                    ..TEXT_SHADOW
                },
                Node {
                    margin: UiRect::bottom(Val::Px(48.0)),
                    ..default()
                },
            ),
            (
                text_button("Singleplayer", WIDE),
                first(),
                observe(open(Page::Worlds)),
            ),
            (
                text_button("Multiplayer", WIDE),
                observe(open(Page::Servers))
            ),
            (
                text_button("Settings", WIDE),
                observe(open(Page::Settings(SettingsTab::Video))),
            ),
            (
                text_button("Quit Game", WIDE),
                observe(|_: On<Activate>, mut exit: MessageWriter<AppExit>| {
                    exit.write(AppExit::Success);
                }),
            ),
        ],
    ));
    commands.entity(root).with_child((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(8.0),
            bottom: Val::Px(6.0),
            ..default()
        },
        children![note(concat!("rustcraft ", env!("CARGO_PKG_VERSION")), WIDE)],
    ));
}

/// An observer opening `page` over the one shown.
pub(super) fn open(page: Page) -> impl FnMut(On<Activate>, ResMut<Menu>) {
    move |_, mut menu| menu.open(page.clone())
}

// Draws the scenery's world and hangs the camera over it.
fn show_scenery(
    settings: Res<Settings>,
    cameras: Query<Entity, WorldCamera>,
    mut commands: Commands,
) {
    let renderer = RendererSettings {
        render_distance: settings.renderer.render_distance.min(SCENERY_DISTANCE),
        full_detail_distance: settings
            .renderer
            .full_detail_distance
            .min(SCENERY_FULL_DETAIL),
        ..settings.renderer
    };
    let world = World::joined(SCENERY_SEED, renderer.lod_layout().region_reach() as u32);
    let at = world.spawn() + Vec3::Y * VIEW_HEIGHT;
    commands.insert_resource(world);
    commands.run_system_cached_with(redraw_terrain, renderer);
    let viewpoint = commands
        .spawn((
            Viewpoint,
            Transform::from_translation(at).with_rotation(Quat::from_rotation_x(VIEW_PITCH)),
            Visibility::default(),
        ))
        .id();
    for camera in &cameras {
        commands
            .entity(camera)
            .insert((ChildOf(viewpoint), Transform::IDENTITY));
    }
}

fn pan(time: Res<Time<Real>>, mut viewpoints: Query<&mut Transform, With<Viewpoint>>) {
    for mut view in &mut viewpoints {
        let yaw = time.elapsed_secs() * PAN_SPEED;
        view.rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(VIEW_PITCH);
    }
}

// Puts the scenery away as a world's opened: it's a world of its own, left as any other is.
fn leave_scenery(world: &mut bevy::ecs::world::World) {
    world.run_schedule(LeaveWorld);
}

fn forget_viewpoint(viewpoints: Query<Entity, With<Viewpoint>>, mut commands: Commands) {
    for viewpoint in &viewpoints {
        commands.entity(viewpoint).despawn();
    }
}
