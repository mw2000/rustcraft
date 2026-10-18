#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rustcraft::settings::Settings;

use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::core_pipeline::prepass::{DepthPrepass, MotionVectorPrepass};
use bevy::post_process::bloom::Bloom;
use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};
use bevy::prelude::*;
use rustcraft::chunks::{
    fade::FULLY_VISIBLE,
    insert_terrain_resources,
    material::{block_atlas, see_through_reach, ChunkMaterial, TerrainMaterials, WaterMaterial},
    task_pool_plugin, TerrainPlugin,
};
use rustcraft::clock::{run_clock, wind_clock, ClockPlugin, TimeOfDay};
use rustcraft::clouds::{spawn_clouds, CloudLayer, CloudPlugin};
use rustcraft::commands::{command_bar_closed, CommandPlugin};
use rustcraft::curvature::{Curvature, CurvaturePlugin};
use rustcraft::debug::DebugOverlayPlugin;
use rustcraft::game_mode::GameModePlugin;
use rustcraft::horizon::update_horizon;
use rustcraft::hotbar::HotbarPlugin;
use rustcraft::interaction::InteractionPlugin;
use rustcraft::menu::{GameState, MenuPlugin};
use rustcraft::picker::PickerPlugin;
use rustcraft::player::input::{follow_look, player_look, player_move};
use rustcraft::player::underwater::{murk, update_underwater_effects, MurkMaterial};
use rustcraft::player::{Player, EYE_HEIGHT};
use rustcraft::save::{world_name, worlds_dir, PlayerState, SavePlugin, WorldSave};
use rustcraft::sky::{
    atmosphere_camera, match_sky_horizon, moon, move_sky, spawn_atmosphere, sun, FOG_MAX_OPACITY,
};
use rustcraft::sky_dome::{NightSkyPlugin, SunGlarePlugin};
use rustcraft::tonemap::{dry_color_grading, TONEMAPPING};
use rustcraft::world::World;

#[allow(clippy::too_many_arguments)]
fn setup_scene(
    mut commands: Commands,
    mut chunk_materials: ResMut<Assets<ChunkMaterial>>,
    mut water_materials: ResMut<Assets<WaterMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut murk_materials: ResMut<Assets<MurkMaterial>>,
    clouds: Res<CloudLayer>,
    curvature: Res<Curvature>,
) {
    let settings = Settings::read("assets/settings.toml").expect("Failed to read settings.toml");
    let renderer = settings.renderer;
    let layout = renderer.lod_layout();
    let render_distance = layout.render_distance() as u32;
    info!(
        "drawing terrain {render_distance} chunks out with {} LOD levels",
        layout.max_level
    );

    let dir = worlds_dir().join(world_name());
    let (save, saved, edits) = WorldSave::open(dir.clone(), World::requested_seed())
        .unwrap_or_else(|error| panic!("couldn't open the world in {}: {error}", dir.display()));
    let game_world = World::from_save(saved.seed, edits, layout.region_reach() as u32);
    let world_height = game_world.height;
    info!(
        "playing the world in {}, with seed {}",
        dir.display(),
        saved.seed
    );
    let arrival = saved
        .player
        .unwrap_or_else(|| PlayerState::arriving_at(game_world.spawn()));
    commands.insert_resource(game_world);
    commands.insert_resource(save);
    commands.insert_resource(TimeOfDay(start_time().unwrap_or(saved.time_of_day)));

    let [x, y, z] = arrival.position;
    info!("spawned at {x:?}, {y:?}, {z:?}");

    let mut body = Player::default();
    body.movement = arrival.movement;
    let player = commands
        .spawn((
            body,
            arrival.look(),
            arrival.game_mode,
            arrival.hotbar,
            arrival.body(),
        ))
        .id();

    let fog_distance = render_distance as f32 * 16.0;
    let camera = commands
        .spawn((
            Transform::from_xyz(0.0, EYE_HEIGHT, 0.0),
            Camera3d { ..default() },
            atmosphere_camera(),
            Projection::Perspective(PerspectiveProjection {
                far: fog_distance + world_height as f32,
                ..default()
            }),
            // Depth of field, bloom and sunlight brighter than white all need HDR.
            Camera::default(),
            bevy::camera::Hdr,
            TONEMAPPING,
            // Just a soft glow around the brightest sunlit faces and sky.
            Bloom {
                intensity: 0.05,
                ..Bloom::NATURAL
            },
            // TAA for the thin faces and one-block steps that shimmer at a distance
            Msaa::Off,
            DepthPrepass,
            MotionVectorPrepass,
            TemporalAntiAliasing::default(),
            DistanceFog {
                color: Color::NONE.with_alpha(FOG_MAX_OPACITY),
                falloff: FogFalloff::Linear {
                    // half fogged where the farthest LOD level starts, hiding the swap to it,
                    // see `fog.wesl`
                    start: layout.last_level_start() * 16.0,
                    end: fog_distance,
                },
                ..default()
            },
            // In focus 8 blocks out, blurred ~1px at 16 blocks and ~2px from 150 on, so LOD seams soften. Gaussian is cheaper than Bokeh.
            DepthOfField {
                mode: DepthOfFieldMode::Gaussian,
                focal_distance: 8.0,
                aperture_f_stops: 2.2,
                ..default()
            },
            dry_color_grading(),
        ))
        .id();
    commands.entity(player).add_children(&[camera]);
    let (murk, murk_handle) = murk(&mut meshes, &mut murk_materials);
    commands.spawn((murk, ChildOf(camera)));
    commands.insert_resource(murk_handle);
    let shadow_distance = (renderer.shadow_distance as f32 * 16.0).min(fog_distance);
    commands.spawn(sun(shadow_distance, renderer.shadow_cascades));
    commands.spawn(moon());

    let atlas = images.add(block_atlas("assets/textures/blocks.png"));
    let chunk_material_handle = chunk_materials.add(ChunkMaterial {
        color: LinearRgba::WHITE,
        texture: Some(atlas.clone()),
        fade: FULLY_VISIBLE,
        cloud_pattern: clouds.pattern.clone(),
        cloud_layer: clouds.layer,
        curvature: curvature.buffer.clone(),
        debug: settings.debug,
    });
    let water_material_handle = water_materials.add(WaterMaterial {
        color: LinearRgba::rgb(0.0, 0.85, 0.85),
        texture: Some(atlas),
        fade: FULLY_VISIBLE,
        cloud_pattern: clouds.pattern.clone(),
        cloud_layer: clouds.layer,
        see_through: see_through_reach(layout),
        curvature: curvature.buffer.clone(),
        solid: false,
    });
    let solid_water = WaterMaterial {
        solid: true,
        ..water_materials.get(&water_material_handle).unwrap().clone()
    };
    let solid_water_material_handle = water_materials.add(solid_water);
    insert_terrain_resources(
        &mut commands,
        &renderer,
        &settings.debug,
        world_height,
        TerrainMaterials {
            chunk: chunk_material_handle,
            water: water_material_handle,
            solid_water: solid_water_material_handle,
        },
        &mut chunk_materials,
        &mut water_materials,
    );
    commands.spawn(settings);
}

// the stats runs are unfocused, which Bevy otherwise throttles to 60 fps
fn winit_settings() -> bevy::winit::WinitSettings {
    use bevy::winit::{UpdateMode, WinitSettings};
    if std::env::var("RC_STATS").is_ok() {
        WinitSettings {
            focused_mode: UpdateMode::Continuous,
            unfocused_mode: UpdateMode::Continuous,
        }
    } else {
        WinitSettings::game()
    }
}

// RC_TIME starts the clock at that time of day, in days, for looking at the sky at any hour
fn start_time() -> Option<f32> {
    std::env::var("RC_TIME")
        .ok()
        .and_then(|time| time.parse().ok())
}

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(ImagePlugin::default_nearest())
                .set(task_pool_plugin())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        present_mode: bevy::window::PresentMode::AutoNoVsync,
                        ..default()
                    }),
                    ..default()
                }),
            MaterialPlugin::<ChunkMaterial>::default(),
            MaterialPlugin::<WaterMaterial>::default(),
            MaterialPlugin::<MurkMaterial>::default(),
            TerrainPlugin,
            CloudPlugin,
            CurvaturePlugin,
            SunGlarePlugin,
            NightSkyPlugin,
            DebugOverlayPlugin,
            MenuPlugin,
            CommandPlugin,
            InteractionPlugin,
            SavePlugin,
        ))
        .add_plugins((GameModePlugin, ClockPlugin, HotbarPlugin, PickerPlugin))
        .insert_resource(winit_settings())
        .insert_resource(ClearColor(Color::BLACK))
        .add_systems(Startup, (spawn_atmosphere, setup_scene.after(spawn_clouds)))
        // once the camera's moved, see `TransformSystems::Propagate`
        .add_systems(
            PostUpdate,
            match_sky_horizon.after(bevy::transform::TransformSystems::Propagate),
        )
        .add_systems(
            Update,
            (
                (player_move, player_look)
                    .run_if(in_state(GameState::Playing))
                    .run_if(command_bar_closed),
                follow_look,
                update_horizon,
                update_underwater_effects,
            )
                .chain(),
        )
        .add_systems(Update, (wind_clock, move_sky.after(run_clock)).chain())
        .run();
}
