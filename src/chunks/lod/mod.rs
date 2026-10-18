mod edits;
mod key;
mod layout;

pub use edits::{CellEdits, LodEdits, NodeEdits, EDITED_LEVELS};
pub use key::LodKey;
pub use layout::LodLayout;

// Drawing them, which a dedicated server doesn't, though it generates chunks by `LodKey`.
#[cfg(feature = "client")]
mod apply;
#[cfg(feature = "client")]
mod loader;
#[cfg(feature = "client")]
mod rebuild;
#[cfg(feature = "client")]
mod select;
#[cfg(feature = "client")]
mod selection;
#[cfg(feature = "client")]
mod spawn;
#[cfg(feature = "client")]
mod state;
#[cfg(all(test, feature = "client"))]
mod tests;

#[cfg(feature = "client")]
pub use apply::{load_lods, refresh_lod_visibility, retire_lods, SOLID_WATER_LEVEL};
#[cfg(feature = "client")]
pub use loader::{GenerateLodMesh, LodLoader, LodNode};
#[cfg(feature = "client")]
pub use rebuild::{load_rebuilt_lods, rebuild_lods, RebuildLodMesh};
#[cfg(feature = "client")]
pub use select::{select_lods, select_lods_and_stand_ins, LodBand};
#[cfg(feature = "client")]
pub use selection::{update_lod_selection, LodSelection};
#[cfg(feature = "client")]
pub use spawn::spawn_lods;
#[cfg(feature = "client")]
pub use state::{ChunkState, ChunkStates, LodState};
