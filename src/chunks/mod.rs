pub mod chunk;
#[cfg(feature = "client")]
pub mod chunk_loader;
#[cfg(feature = "client")]
pub mod fade;
pub mod generate;
#[cfg(feature = "client")]
pub mod group;
#[cfg(feature = "client")]
pub mod inner_leaves;
pub mod lod;
#[cfg(feature = "client")]
pub mod material;
#[cfg(feature = "client")]
mod terrain;

#[cfg(feature = "client")]
pub use terrain::{insert_terrain_resources, redraw_terrain, TerrainPlugin};

use bevy::app::{TaskPoolOptions, TaskPoolPlugin, TaskPoolThreadAssignmentPolicy};

/// Terrain generation runs on the async compute pool, so it gets most of the cores.
pub fn task_pool_plugin() -> TaskPoolPlugin {
    TaskPoolPlugin {
        task_pool_options: TaskPoolOptions {
            io: TaskPoolThreadAssignmentPolicy {
                min_threads: 1,
                max_threads: 1,
                percent: 0.0,
                on_thread_spawn: None,
                on_thread_destroy: None,
            },
            async_compute: TaskPoolThreadAssignmentPolicy {
                min_threads: 1,
                max_threads: usize::MAX,
                percent: 0.6,
                on_thread_spawn: None,
                on_thread_destroy: None,
            },
            ..Default::default()
        },
    }
}
