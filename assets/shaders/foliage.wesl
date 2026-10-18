// Leaves and plants swaying in the wind, shared by ChunkMaterial's main and shadow passes so
// they keep to their shadows.

// How far, in blocks, leaves sway: little enough that they never pull far from their twigs.
const SWAY: f32 = 0.08;
// How far plants' tops sway, a little less, as they're small and rooted.
const PLANT_SWAY: f32 = 0.06;

// Whether a block mesh's vertex alpha, its atlas cell, is leaves'. `LEAF_CELL` comes from
// `ChunkMaterial::specialize`.
fn is_leaf(vertex_alpha: f32) -> bool {
    return abs(vertex_alpha - f32(constants::LEAF_CELL)) < 0.5;
}

// Whether a block mesh's face is a plant's, from how it draws, its second uv's y: `generator::PLANT`.
fn is_plant(draws: f32) -> bool {
    return abs(draws - 0.25) < 0.125;
}

// How far a block mesh's vertex sways, from its atlas cell, how its face draws and its uv's v,
// which runs down a plant from its top: leaves all over, plants from their roots, which stay
// put, up to their tops. Coarse LOD cells, whose faces span many blocks, stay still: their
// corners would drift apart and tear.
fn sway(vertex_alpha: f32, draws: f32, v: f32) -> f32 {
    if is_plant(draws) {
        return PLANT_SWAY * (1.0 - v);
    }
    if draws > 0.5 || !is_leaf(vertex_alpha) {
        return 0.0;
    }
    return SWAY;
}

// Where a vertex at `world` swaying by `amount` blocks is by `time`. It depends on nothing else,
// so corners shared by neighbouring leaves move together and leave no cracks.
fn swayed(world: vec3<f32>, amount: f32, time: f32) -> vec3<f32> {
    // `DebugSettings::still_leaves`
@if(STILL_LEAVES) {
    return world;
}
    if amount == 0.0 {
        return world;
    }
    // slow gusts roll across the canopy, with a quicker flutter on top
    let gust = sin(time * 1.3 + dot(world.xz, vec2<f32>(0.11, 0.07)));
    let flutter = sin(time * 4.1 + dot(world, vec3<f32>(0.9, 0.7, 1.3)));
    let push = gust * 0.7 + flutter * 0.3;
    return world + vec3<f32>(push, flutter * 0.4, push * 0.6) * amount;
}
