use crate::client::player::WorldCamera;
use bevy::prelude::*;

use super::chunk_loader::{
    gather_chunks, generate_chunks, load_chunks, mark_chunks, mesh_nearing_sealed_chunks,
    unload_chunks, Chunk, ChunkLoader,
};
use super::fade::{apply_fades, FadeMaterials};
use super::generate::WORLD_HEIGHT;
use super::group::{build_groups, load_groups, rebuild_groups, ChunkGroup};
use super::inner_leaves::show_inner_leaves;
use super::lod::{
    load_lods, load_rebuilt_lods, rebuild_lods, refresh_lod_visibility, retire_lods, spawn_lods,
    update_lod_selection, LodLoader, LodNode,
};
use super::material::{ChunkMaterial, TerrainMaterials, WaterMaterial};
use crate::settings::{DebugSettings, RendererSettings, Settings};
use crate::world::{LeaveWorld, LeaveWorldSystems, World};

/// Loads, meshes and unloads full-detail chunks and LOD terrain around the camera, and
/// rebuilds the meshes edits change.
pub struct TerrainPlugin;

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                // ahead of merging groups as they load, which would only merge what an edit
                // has made out of date
                (mesh_nearing_sealed_chunks, rebuild_groups)
                    .chain()
                    .before(build_groups),
                (
                    gather_chunks,
                    generate_chunks,
                    mark_chunks,
                    load_chunks,
                    build_groups,
                    load_groups,
                )
                    .before(unload_chunks),
                (
                    update_lod_selection,
                    // ahead of spawning, as a node spawned takes in the edits made so far
                    rebuild_lods,
                    spawn_lods,
                    load_lods,
                    load_rebuilt_lods,
                    retire_lods,
                    refresh_lod_visibility,
                    apply_fades,
                )
                    .chain()
                    .after(gather_chunks)
                    .before(unload_chunks),
                unload_chunks,
                // once the groups and nodes loaded this frame have their faces between leaves
                show_inner_leaves
                    .after(load_groups)
                    .after(load_rebuilt_lods),
            )
                // a client joining a server has nothing to draw until it's welcomed, and
                // nothing's drawn until the camera's over the world, hung from the player
                .run_if(resource_exists::<World>)
                .run_if(camera_placed),
        )
        .add_systems(LeaveWorld, clear_terrain.in_set(LeaveWorldSystems::Clear));
    }
}

fn camera_placed(cameras: Query<(), (WorldCamera, With<ChildOf>)>) -> bool {
    !cameras.is_empty()
}

// Puts the terrain away as the world's left, ready to draw the next as the settings have it.
fn clear_terrain(world: &mut bevy::ecs::world::World) {
    let renderer = world.resource::<Settings>().renderer;
    world.run_system_cached_with(redraw_terrain, renderer).ok();
}

/// Puts away every chunk and LOD node drawn and starts drawing afresh with `renderer`'s
/// layout, letting go of the chunks the world held for them: as the world's left, and as the
/// render distance changes.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn redraw_terrain(
    In(renderer): In<RendererSettings>,
    drawn: Query<Entity, Or<(With<Chunk>, With<ChunkGroup>, With<LodNode>)>>,
    loader: Option<Res<ChunkLoader>>,
    world: Option<ResMut<World>>,
    settings: Res<Settings>,
    materials: Res<TerrainMaterials>,
    mut chunk_materials: ResMut<Assets<ChunkMaterial>>,
    mut water_materials: ResMut<Assets<WaterMaterial>>,
    mut commands: Commands,
) {
    if let (Some(loader), Some(mut world)) = (loader, world) {
        loader.release_all(&mut world);
    }
    for entity in &drawn {
        commands.entity(entity).try_despawn();
    }
    insert_terrain_resources(
        &mut commands,
        &renderer,
        &settings.debug,
        WORLD_HEIGHT,
        materials.clone(),
        &mut chunk_materials,
        &mut water_materials,
    );
}

/// Inserts the resources `TerrainPlugin`'s systems run on, drawing with the given materials.
pub fn insert_terrain_resources(
    commands: &mut Commands,
    renderer: &RendererSettings,
    debug: &DebugSettings,
    world_height: u64,
    materials: TerrainMaterials,
    chunk_materials: &mut Assets<ChunkMaterial>,
    water_materials: &mut Assets<WaterMaterial>,
) {
    let layout = renderer.lod_layout();
    commands.insert_resource(materials.clone());
    commands.insert_resource(FadeMaterials::new(
        renderer.lod_fade_seconds,
        &materials,
        chunk_materials,
        water_materials,
    ));
    commands.insert_resource(LodLoader::new(
        layout,
        world_height,
        materials.chunk.clone(),
        materials.water.clone(),
        materials.solid_water,
    ));
    commands.insert_resource(ChunkLoader::new(
        layout,
        world_height,
        debug.solid_leaves,
        materials.chunk,
        materials.water,
    ));
}
