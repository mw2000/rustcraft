// Leaves and plants swaying in the wind, shared by ChunkMaterial's main and shadow passes so
// they keep to their shadows.

// How far, in blocks, leaves sway: little enough that they never pull far from their twigs.
const SWAY: f32 = 0.08;
// How far, in blocks, a plant's tip sways. Its roots stay put.
const PLANT_SWAY: f32 = 0.16;
// How far a plant leans with the wind on average, as a share of `PLANT_SWAY`, so a meadow
// bends one way in a gust rather than wobbling about upright.
const PLANT_LEAN: f32 = 0.35;

// Whether a block mesh's vertex alpha, its atlas cell, is leaves'. `LEAF_CELL` comes from
// `ChunkMaterial::specialize`.
fn is_leaf(vertex_alpha: f32) -> bool {
    return abs(vertex_alpha - f32(constants::LEAF_CELL)) < 0.5;
}

// Whether a block mesh's face of leaves at `world_position`, drawn as `draws` (its second uv's y),
// has its holes filled with the shade of the leaves behind, at the dither `threshold`: past
// `INNER_LEAVES_REACH` blocks of the camera at `eye` the faces between leaves aren't drawn, see
// `chunks::inner_leaves`, and the holes fill in over the band before it. Only faces drawn with
// their own tile, `generator::TEXTURED`, have holes to fill.
fn leaf_holes_filled(world_position: vec3<f32>, eye: vec3<f32>, vertex_alpha: f32, draws: f32, threshold: f32) -> bool {
    if !is_leaf(vertex_alpha) || abs(draws) >= 0.125 {
        return false;
    }
    let reach = f32(constants::INNER_LEAVES_REACH);
    let band = f32(constants::INNER_LEAVES_BAND);
    return threshold < smoothstep(reach - band, reach, distance(world_position, eye));
}

// Whether a block mesh's face is a plant's, from how it draws, its second uv's y: `generator::PLANT`.
fn is_plant(draws: f32) -> bool {
    return abs(draws - 0.25) < 0.125;
}

// Whether a block mesh's face is a shape's, a stair's or a glass pane's, whose tile's holes are
// cut out of its shadow: `generator::CUTOUT`.
fn is_cutout(draws: f32) -> bool {
    return abs(draws - 0.5) < 0.125;
}

// Whether a block mesh's vertex alpha, its atlas cell, is glass's, which has holes as leaves do.
fn is_glass(vertex_alpha: f32) -> bool {
    return abs(vertex_alpha - f32(constants::GLASS_CELL)) < 0.5;
}

// Whether a block mesh's face has holes cut in it, from its atlas cell and how it draws: leaves,
// glass, plants and shapes. Only these pay for a texture sample in the shadow pass, and the main
// pass cuts them at full detail as it does, see `world.wesl`.
fn has_holes(vertex_alpha: f32, draws: f32) -> bool {
    return is_leaf(vertex_alpha) || is_glass(vertex_alpha) || is_plant(draws) || is_cutout(draws);
}

// Where a block mesh's vertex at `world` sways to by `time`, from its atlas cell, how its face
// draws and its uv's v, which runs down a plant from its tip: leaves all over, plants bending
// from their roots, which stay put, most at their tips. It depends on nothing but where the
// vertex is, so corners shared by neighbouring leaves move together and leave no cracks. Coarse
// LOD cells, whose faces span many blocks, stay still: their corners would drift apart and tear.
fn swayed(world: vec3<f32>, vertex_alpha: f32, draws: f32, v: f32, time: f32) -> vec3<f32> {
    // `DebugSettings::still_leaves`
@if(STILL_LEAVES) {
    return world;
}
    let plant = is_plant(draws);
    if !plant && (draws > 0.5 || !is_leaf(vertex_alpha)) {
        return world;
    }
    // slow gusts roll across the land, the same for leaves and plants, so one crosses a meadow
    // and the wood behind it alike
    let gust = sin(time * 1.3 + dot(world.xz, vec2<f32>(0.11, 0.07)));
    if plant {
        // bending most at the tip, as a stem does, and quicker than the boughs
        let bend = 1.0 - clamp(v, 0.0, 1.0);
        let flutter = sin(time * 3.3 + dot(world.xz, vec2<f32>(1.7, 1.1)));
        let push = (PLANT_LEAN + gust * 0.45 + flutter * 0.25) * bend * bend * PLANT_SWAY;
        // a leaning tip dips a little, as its stem doesn't stretch
        return world + vec3<f32>(push, -abs(push) * 0.3, push * 0.6);
    }
    // with a quicker flutter on top
    let flutter = sin(time * 4.1 + dot(world, vec3<f32>(0.9, 0.7, 1.3)));
    let push = gust * 0.7 + flutter * 0.3;
    return world + vec3<f32>(push, flutter * 0.4, push * 0.6) * SWAY;
}
