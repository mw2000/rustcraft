use crate::block::BlockType;
use crate::chunks::generate::biome::BiomeId;
use crate::chunks::generate::biome::Biomes;
use crate::chunks::generate::WATER_LEVEL;

pub(super) const SOLID_FLOOR: i64 = 16;

// The cell at the water's surface over ground below it is water, not ground: filled by its
// ground it would draw as a beach at sea level, where the water over it is mostly of the cell.
pub(super) fn underwater(
    block: BlockType,
    bottom: i64,
    cell: i64,
    column_height: i64,
) -> BlockType {
    if is_solid(block)
        && bottom < WATER_LEVEL
        && WATER_LEVEL <= bottom + cell
        && ground_top(column_height) < WATER_LEVEL
    {
        BlockType::Water
    } else {
        block
    }
}

pub(super) fn is_solid(block: BlockType) -> bool {
    block.occlusion_weight() == 1.0
}

// Height above the sea where snow starts to settle in a temperate climate (temperature 0.5).
pub(super) const SNOW_LINE: i64 = 220;

// How many blocks the snow line climbs per unit of temperature.
pub(super) const SNOW_LINE_PER_DEGREE: f64 = 800.0;

// At or above this temperature it's too hot for snow, and the heights are bare rock.
pub(super) const SNOW_MAX_TEMPERATURE: f32 = 0.62;

// Below this temperature even the grass is under a layer of snow.
pub const SNOWY_GRASS_TEMPERATURE: f32 = 0.25;

// The lowest the snow line gets, so even the coldest lowlands are snow-covered.
pub(super) const COLDEST_SNOW_LINE: i64 = 10;

// Below this temperature the top of the sea freezes, thicker the colder it is.
pub const FREEZE_TEMPERATURE: f32 = 0.12;

pub(super) const ICE_MAX_DEPTH: i64 = 3;

pub(super) const ICE_TEMPERATURE_PER_LAYER: f32 = 0.04;

// How many blocks of the sea's surface are ice at this temperature.
pub(super) fn ice_depth(temperature: f32) -> i64 {
    if temperature >= FREEZE_TEMPERATURE {
        return 0;
    }
    (1 + ((FREEZE_TEMPERATURE - temperature) / ICE_TEMPERATURE_PER_LAYER) as i64).min(ICE_MAX_DEPTH)
}

// A wider LOD cell's water, as ice when it overlaps the frozen top of the sea; its middle is sampled, which is usually below it.
pub(super) fn freeze_cell(
    block: BlockType,
    temperature: f32,
    cell_top: i64,
    cell: i64,
) -> BlockType {
    if cell > 1 && block == BlockType::Water && cell_top > WATER_LEVEL - ice_depth(temperature) {
        BlockType::Ice
    } else {
        block
    }
}

// Height above the sea where snow starts to settle, lower in the cold.
pub(super) fn snow_line(temperature: f32) -> i64 {
    (SNOW_LINE as f64 + (temperature as f64 - 0.5) * SNOW_LINE_PER_DEGREE)
        .max(COLDEST_SNOW_LINE as f64) as i64
}

// The steepest ground snow settles on, `above_sea` blocks up: only gentle ground at the snow
// line, steeper the higher it gets, until only cliffs are left bare.
pub(super) fn snow_holding_slope(above_sea: i64, snow_line: i64) -> f64 {
    let t = ((above_sea - snow_line) as f64 / 120.0).clamp(0.0, 1.0);
    1.5 + 3.0 * t
}

// The height, in blocks, the ground of a column `column_height` high is solid up to.
pub(in crate::chunks::generate) fn ground_top(column_height: i64) -> i64 {
    column_height.max(SOLID_FLOOR)
}

pub(super) fn terrain_block(
    world_y: i64,
    column_height: i64,
    combined_gradient: f64,
    temperature: f32,
) -> BlockType {
    if world_y < ground_top(column_height) {
        let above_sea = world_y - WATER_LEVEL;
        let line = snow_line(temperature);
        let snows = temperature < SNOW_MAX_TEMPERATURE;
        if snows && above_sea >= line && combined_gradient <= snow_holding_slope(above_sea, line) {
            BlockType::Snow
        } else if !snows && above_sea >= SNOW_LINE
            || above_sea >= 120 && combined_gradient >= 2.0
            || (above_sea >= 2 && combined_gradient >= 3.5)
        {
            BlockType::Stone
        } else if above_sea >= 2 {
            if temperature < SNOWY_GRASS_TEMPERATURE && world_y == column_height - 1 {
                BlockType::Snow
            } else {
                BlockType::Grass
            }
        } else {
            BlockType::Sand
        }
    } else if world_y < WATER_LEVEL {
        if world_y >= WATER_LEVEL - ice_depth(temperature) {
            BlockType::Ice
        } else {
            BlockType::Water
        }
    } else {
        BlockType::Air
    }
}

// How deep each surface goes before the rock under it, counting the top layer: grass over dirt,
// sand over sandstone, gravel, and snow, or the ice of a glacier over snow, and a volcano's
// basalt, with stone at its heart for embers to grow in, and its magma over basalt.
const DIRT_DEPTH: i64 = 4;
const SAND_DEPTH: i64 = 4;
const SANDSTONE_DEPTH: i64 = 8;
const GRAVEL_DEPTH: i64 = 4;
const SNOW_DEPTH: i64 = 8;
const BASALT_DEPTH: i64 = 16;
const MAGMA_DEPTH: i64 = 2;

// The ground at `world_y`, which the terrain has as `block` all the way down: the top layer is
// the surface itself, then soil a few blocks deep, then stone. Only for full detail, since wider
// LOD cells are sampled mid-cell, which would turn their tops to soil or stone.
pub(super) fn under_surface(
    block: BlockType,
    world_y: i64,
    column_height: i64,
    cell: i64,
) -> BlockType {
    let depth = column_height - 1 - world_y;
    if cell != 1 || depth <= 0 {
        return block;
    }
    let soil = |soil, deepest| {
        if depth < deepest {
            soil
        } else {
            BlockType::Stone
        }
    };
    match block {
        BlockType::Grass => soil(BlockType::Dirt, DIRT_DEPTH),
        BlockType::Sand if depth < SAND_DEPTH => BlockType::Sand,
        BlockType::Sand => soil(BlockType::Sandstone, SANDSTONE_DEPTH),
        BlockType::Gravel => soil(BlockType::Gravel, GRAVEL_DEPTH),
        BlockType::Snow | BlockType::Ice => soil(BlockType::Snow, SNOW_DEPTH),
        BlockType::Basalt => soil(BlockType::Basalt, BASALT_DEPTH),
        BlockType::Magma if depth < MAGMA_DEPTH => BlockType::Magma,
        BlockType::Magma => soil(BlockType::Basalt, BASALT_DEPTH),
        other => other,
    }
}

// `terrain_block`, as the biome owning the column has it.
pub(in crate::chunks::generate) fn column_block(
    biomes: &Biomes,
    biome: BiomeId,
    world_y: i64,
    column_height: i64,
    combined_gradient: f64,
    temperature: f32,
) -> BlockType {
    let base = terrain_block(world_y, column_height, combined_gradient, temperature);
    biomes.block(biome, base, world_y - WATER_LEVEL, combined_gradient)
}
