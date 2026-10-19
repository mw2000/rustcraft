#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rustcraft::settings::Settings;

use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::core_pipeline::prepass::{DepthPrepass, MotionVectorPrepass};
use bevy::post_process::bloom::Bloom;
use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};
use bevy::prelude::*;
use bevy_replicon::RepliconPlugins;
use bevy_replicon_renet2::RepliconRenetPlugins;
use rustcraft::chunks::{
    fade::FULLY_VISIBLE,
    generate::WORLD_HEIGHT,
    insert_terrain_resources,
    material::{block_atlas, see_through_reach, ChunkMaterial, TerrainMaterials, WaterMaterial},
    task_pool_plugin, TerrainPlugin,
};
use rustcraft::client::authoring::AuthoringPlugin;
use rustcraft::client::clock::{follow_clock, wind_clock};
use rustcraft::client::command_bar::{command_bar_closed, CommandPlugin};
use rustcraft::client::cracks::DrawCracksPlugin;
use rustcraft::client::creatures::DrawCreaturesPlugin;
use rustcraft::client::critters::AmbientLifePlugin;
use rustcraft::client::drops::DrawDropsPlugin;
use rustcraft::client::hand::HandPlugin;
use rustcraft::client::model_light::ModelLightPlugin;
use rustcraft::client::models::DrawPlayersPlugin;
use rustcraft::client::servers::{ServerList, SERVERS_FILE};
use rustcraft::client::sound::SoundPlugin;
use rustcraft::client::ClientPlugin;
use rustcraft::clouds::{spawn_clouds, CloudLayer, CloudPlugin};
use rustcraft::curvature::{Curvature, CurvaturePlugin};
use rustcraft::debug::DebugOverlayPlugin;
use rustcraft::game_mode::GameModePlugin;
use rustcraft::horizon::update_horizon;
use rustcraft::hotbar::HotbarPlugin;
use rustcraft::hud::HudPlugin;
use rustcraft::interaction::InteractionPlugin;
use rustcraft::inventory::screen::InventoryScreenPlugin;
use rustcraft::map::MapPlugin;
use rustcraft::menu::{GameState, MenuPlugin};
use rustcraft::picker::PickerPlugin;
use rustcraft::player::input::{follow_look, player_look, player_move};
use rustcraft::player::underwater::{murk, update_underwater_effects, MurkMaterial};
use rustcraft::preferences::{PreferencesFile, PreferencesPlugin, PREFERENCES_FILE};
use rustcraft::protocol::ProtocolPlugin;
use rustcraft::save::worlds::WorldsDir;
use rustcraft::save::{data_dir, worlds_dir, SavePlugin};
use rustcraft::server::{run_clock, ServerPlugin};
use rustcraft::session::{Session, SessionPlugin};
use rustcraft::sky::{
    atmosphere_camera, match_sky_horizon, moon, move_sky, spawn_atmosphere, sun, FOG_MAX_OPACITY,
};
use rustcraft::sky_dome::{NightSkyPlugin, SunGlarePlugin};
use rustcraft::tonemap::{dry_color_grading, TONEMAPPING};
use rustcraft::weather::look::WeatherLookPlugin;
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
    settings: Res<Settings>,
) {
    let renderer = settings.renderer;
    let layout = renderer.lod_layout();
    let render_distance = layout.render_distance() as u32;
    info!(
        "drawing terrain {render_distance} chunks out with {} LOD levels",
        layout.max_level
    );

    let fog_distance = render_distance as f32 * 16.0;
    // hung from the local player's eyes once it's spawned, see `ClientPlugin`
    let camera = commands
        .spawn((
            Camera3d { ..default() },
            atmosphere_camera(),
            Projection::Perspective(PerspectiveProjection {
                far: fog_distance + WORLD_HEIGHT as f32,
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
        cloud_layer: clouds.layer.clone(),
        curvature: curvature.buffer.clone(),
        debug: settings.debug,
    });
    let water_material_handle = water_materials.add(WaterMaterial {
        color: LinearRgba::rgb(0.0, 0.85, 0.85),
        texture: Some(atlas),
        fade: FULLY_VISIBLE,
        cloud_pattern: clouds.pattern.clone(),
        cloud_layer: clouds.layer.clone(),
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
        WORLD_HEIGHT,
        TerrainMaterials {
            chunk: chunk_material_handle,
            water: water_material_handle,
            solid_water: solid_water_material_handle,
        },
        &mut chunk_materials,
        &mut water_materials,
    );
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

fn main() {
    let settings = Settings::read("assets/settings.toml").expect("Failed to read settings.toml");
    let mut app = App::new();
    // before the menus, which read the player's settings
    app.insert_resource(settings)
        .insert_resource(PreferencesFile(data_dir().join(PREFERENCES_FILE)))
        .add_plugins(PreferencesPlugin);
    app.add_plugins((
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
        WeatherLookPlugin,
        CurvaturePlugin,
        SunGlarePlugin,
        NightSkyPlugin,
        DebugOverlayPlugin,
        MenuPlugin,
        CommandPlugin,
        InteractionPlugin,
    ))
    .add_plugins((
        GameModePlugin,
        HotbarPlugin,
        HudPlugin,
        PickerPlugin,
        InventoryScreenPlugin,
        ModelLightPlugin,
        DrawDropsPlugin,
        DrawPlayersPlugin,
        HandPlugin,
        DrawCreaturesPlugin,
        DrawCracksPlugin,
        MapPlugin,
        AuthoringPlugin,
        SoundPlugin,
        AmbientLifePlugin,
    ))
    // the client and the server: in singleplayer they're in one App with no transport running,
    // so replicon passes their messages straight across, and joining a server elsewhere the
    // server here does nothing (§4.4)
    .add_plugins((
        RepliconPlugins,
        RepliconRenetPlugins,
        ProtocolPlugin,
        ClientPlugin,
        ServerPlugin,
        SavePlugin,
        SessionPlugin,
    ));
    app.insert_resource(WorldsDir(worlds_dir()))
        .insert_resource(ServerList::read(data_dir().join(SERVERS_FILE)));
    let var = |name: &str| std::env::var(name).ok();
    if let Some(session) = Session::from_env(var, &worlds_dir()) {
        app.insert_resource(session)
            .insert_state(GameState::Loading);
    }
    app.insert_resource(winit_settings())
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
                // with the camera on the player, not over the title screen's scenery
                update_underwater_effects.run_if(|state: Res<State<GameState>>| state.in_world()),
            )
                .chain()
                // a client joining a server has no world or player until it's welcomed
                .run_if(resource_exists::<World>),
        )
        .add_systems(
            Update,
            (
                wind_clock.run_if(in_state(GameState::Playing)),
                move_sky.after(run_clock).after(follow_clock),
            ),
        )
        .run();
}
