pub mod biome;
pub mod caves;
pub mod checksum;
pub mod columns;
pub mod fractal;
pub mod generator;
pub mod light;
pub mod noise;
pub mod ores;
pub mod plants;
pub mod region;
pub mod region_store;
pub mod structures;
pub mod trees;

/// Height of the world in blocks.
pub const WORLD_HEIGHT: u64 = 1024;
/// Height of the sea's surface in blocks.
pub const WATER_LEVEL: i64 = 448;
