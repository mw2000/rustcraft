use bevy::{
    app::{App, Plugin, Update},
    asset::Assets,
    ecs::{schedule::IntoScheduleConfigs, system::Commands},
    prelude::resource_exists,
};

use super::chunk_loader::{
    gather_chunks, generate_chunks, load_chunks, mark_chunks, mesh_nearing_sealed_chunks,
    unload_chunks, ChunkLoader,
};
use super::fade::{apply_fades, FadeMaterials};
use super::group::{build_groups, load_groups, rebuild_groups};
use super::lod::{
    load_lods, refresh_lod_visibility, retire_lods, spawn_lods, update_lod_selection, LodLoader,
};
use super::material::{ChunkMaterial, TerrainMaterials, WaterMaterial};
use crate::settings::{DebugSettings, RendererSettings};
use crate::world::World;

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
                    spawn_lods,
                    load_lods,
                    retire_lods,
                    refresh_lod_visibility,
                    apply_fades,
                )
                    .chain()
                    .after(gather_chunks)
                    .before(unload_chunks),
                unload_chunks,
            )
                // a client joining a server has nothing to draw until it's welcomed
                .run_if(resource_exists::<World>),
        );
    }
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
