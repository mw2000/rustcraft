// ChunkMaterial's bindings and atlas lookup, shared by its main and shadow passes.

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> material_color: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var material_color_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var material_color_sampler: sampler;

@group(constants::MATERIAL_BIND_GROUP) @binding(3) var<uniform> material_fade: vec4<f32>;

// textures in blocks.png, see `Tile` in block.rs
const ATLAS_CELLS: f32 = 82.0;
const TILE_INSET: f32 = 0.002;

// Just the alpha of `block_sample`, at full detail: without screen-space derivatives, so
// it's fine where only some fragments sample, as the shadow pass does for leaves.
fn block_alpha(uv: vec2<f32>, vertex_alpha: f32) -> f32 {
    let atlas_cell = floor(vertex_alpha + 0.5);
    let tile_uv = clamp(fract(uv), vec2<f32>(TILE_INSET), vec2<f32>(1.0 - TILE_INSET));
    let atlas_uv = vec2<f32>((atlas_cell + tile_uv.x) / ATLAS_CELLS, tile_uv.y);
    return textureSampleLevel(material_color_texture, material_color_sampler, atlas_uv, 0.0).a * material_color.a;
}

// How a block face's uv covers the screen, for `block_sample`. It takes derivatives, so needs
// uniform control flow, while `block_sample` doesn't: work it out once, then sample any tiles.
struct BlockFootprint {
    dx: vec2<f32>,
    dy: vec2<f32>,
    inset: f32,
}

fn block_footprint(uv: vec2<f32>) -> BlockFootprint {
    // Blurry mip levels' texels are wider, and bilinear filtering would blend the edge ones
    // with the next tile's, so the lookup stays half a texel of the coarsest level in use inside.
    let tile_texels = f32(textureDimensions(material_color_texture).y);
    let footprint = max(length(vec2<f32>(dpdx(uv.x), dpdx(uv.y))), length(vec2<f32>(dpdy(uv.x), dpdy(uv.y)))) * tile_texels;
    let coarsest = exp2(clamp(ceil(log2(max(footprint, 1.0))), 0.0, log2(tile_texels)));
    return BlockFootprint(
        vec2<f32>(dpdx(uv.x) / ATLAS_CELLS, dpdx(uv.y)),
        vec2<f32>(dpdy(uv.x) / ATLAS_CELLS, dpdy(uv.y)),
        max(TILE_INSET, 0.5 * coarsest / tile_texels),
    );
}

// Block meshes put tile counts in uv and the atlas cell in vertex alpha, so merged faces
// repeat their texture.
fn block_sample(uv: vec2<f32>, vertex_alpha: f32, footprint: BlockFootprint) -> vec4<f32> {
    let atlas_cell = floor(vertex_alpha + 0.5);
    let tile_uv = clamp(fract(uv), vec2<f32>(footprint.inset), vec2<f32>(1.0 - footprint.inset));
    let atlas_uv = vec2<f32>((atlas_cell + tile_uv.x) / ATLAS_CELLS, tile_uv.y);
    let texel = textureSampleGrad(material_color_texture, material_color_sampler, atlas_uv, footprint.dx, footprint.dy);
    return material_color * texel;
}

// What a tile averages to: the mean linear colour of each atlas cell's opaque texels, in atlas
// order. `gen_atlas` prints these and the block-textures skill's `pack.py`
// rewrites them, and `material.rs`'s tests check them against blocks.png.
fn block_mean(vertex_alpha: f32) -> vec4<f32> {
    var means = array<vec3<f32>, 82>(
        vec3<f32>(0.1182, 0.1186, 0.1186), // stone
        vec3<f32>(0.5003, 0.5003, 0.5003), // grass top
        vec3<f32>(0.4776, 0.3611, 0.1822), // sand
        vec3<f32>(0.0665, 0.1023, 0.9149), // water
        vec3<f32>(0.9308, 0.9726, 0.9921), // snow
        vec3<f32>(0.0776, 0.0449, 0.0211), // log bark
        vec3<f32>(0.2433, 0.2433, 0.2433), // leaves
        vec3<f32>(0.5106, 0.5106, 0.5106), // grass side
        vec3<f32>(0.2611, 0.1228, 0.0581), // dirt
        vec3<f32>(0.4083, 0.2484, 0.0983), // log end
        vec3<f32>(0.2623, 0.2423, 0.2180), // gravel
        vec3<f32>(0.3090, 0.6154, 0.8315), // ice
        vec3<f32>(0.6178, 0.4855, 0.2738), // sandstone side
        vec3<f32>(0.6696, 0.5261, 0.2962), // sandstone top
        vec3<f32>(0.4256, 0.3341, 0.2906), // snow side
        vec3<f32>(0.3118, 0.1779, 0.0644), // planks
        vec3<f32>(0.0935, 0.0932, 0.0932), // cobblestone
        vec3<f32>(0.1054, 0.1064, 0.1064), // stone bricks
        vec3<f32>(0.2736, 0.1348, 0.1069), // bricks
        vec3<f32>(0.1115, 0.1119, 0.1121), // coal ore
        vec3<f32>(0.1429, 0.1223, 0.1154), // copper ore
        vec3<f32>(0.1485, 0.1359, 0.1296), // iron ore
        vec3<f32>(0.2778, 0.1663, 0.0667), // crafting table top
        vec3<f32>(0.2776, 0.1755, 0.0874), // crafting table side
        vec3<f32>(0.0981, 0.0981, 0.0981), // furnace front
        vec3<f32>(0.4960, 0.4960, 0.4960), // tall grass
        vec3<f32>(0.3511, 0.0999, 0.0184), // poppy
        vec3<f32>(0.4152, 0.3750, 0.0280), // dandelion
        vec3<f32>(0.0808, 0.1821, 0.3736), // cornflower
        vec3<f32>(0.4400, 0.2406, 0.0737), // torch
        vec3<f32>(0.4146, 0.1724, 0.1339), // bed top
        vec3<f32>(0.3625, 0.1686, 0.1236), // bed side
        vec3<f32>(0.1558, 0.0746, 0.0371), // farmland top
        vec3<f32>(0.0959, 0.2408, 0.0274), // wheat 0
        vec3<f32>(0.0884, 0.2269, 0.0254), // wheat 1
        vec3<f32>(0.0958, 0.2427, 0.0274), // wheat 2
        vec3<f32>(0.0903, 0.2330, 0.0259), // wheat 3
        vec3<f32>(0.1887, 0.2844, 0.0409), // wheat 4
        vec3<f32>(0.3343, 0.3523, 0.0643), // wheat 5
        vec3<f32>(0.5131, 0.3994, 0.0809), // wheat 6
        vec3<f32>(0.6091, 0.4307, 0.0923), // ripe wheat
        vec3<f32>(0.2579, 0.1482, 0.0546), // chest top
        vec3<f32>(0.2456, 0.1474, 0.0674), // chest side
        vec3<f32>(0.1878, 0.6445, 1.0000), // structure void
        vec3<f32>(0.6570, 0.4062, 0.0420), // loot marker
        vec3<f32>(0.4955, 0.4955, 0.4955), // fern
        vec3<f32>(0.1791, 0.1559, 0.1158), // gold ore
        vec3<f32>(0.1391, 0.1787, 0.2149), // frost crystal ore
        vec3<f32>(0.1200, 0.1453, 0.1263), // jade ore
        vec3<f32>(0.1037, 0.0676, 0.0474), // ember ore
        vec3<f32>(0.7385, 0.4981, 0.0937), // gold block
        vec3<f32>(0.4980, 0.7352, 0.9018), // frost crystal block
        vec3<f32>(0.0782, 0.3459, 0.1625), // polished jade
        vec3<f32>(0.5330, 0.5788, 0.6221), // bright steel block
        vec3<f32>(0.6481, 0.6424, 0.5752), // ember lamp
        vec3<f32>(0.1088, 0.1098, 0.1098), // furnace side
        vec3<f32>(0.6831, 0.7870, 0.8147), // glass
        vec3<f32>(0.4673, 0.4673, 0.4673), // vines
        vec3<f32>(0.2550, 0.1495, 0.0621), // door bottom
        vec3<f32>(0.2036, 0.1183, 0.0475), // door top
        vec3<f32>(0.2224, 0.1310, 0.0518), // trapdoor
        vec3<f32>(0.3244, 0.1875, 0.0686), // ladder
        vec3<f32>(0.2115, 0.1453, 0.0898), // lantern
        vec3<f32>(0.1097, 0.1369, 0.0860), // mossy cobblestone
        vec3<f32>(0.0967, 0.0972, 0.0972), // cracked stone bricks
        vec3<f32>(0.1082, 0.1189, 0.1006), // mossy stone bricks
        vec3<f32>(0.1945, 0.1953, 0.1953), // polished stone
        vec3<f32>(0.3128, 0.1112, 0.0556), // terracotta
        vec3<f32>(0.2736, 0.0467, 0.0285), // red terracotta
        vec3<f32>(0.4915, 0.2348, 0.0168), // yellow terracotta
        vec3<f32>(0.0607, 0.0482, 0.1112), // blue terracotta
        vec3<f32>(0.0723, 0.0866, 0.0228), // green terracotta
        vec3<f32>(0.0732, 0.0343, 0.0139), // dark planks
        vec3<f32>(0.5076, 0.3990, 0.2001), // light planks
        vec3<f32>(0.1658, 0.1021, 0.0519), // bookshelf
        vec3<f32>(0.1401, 0.0877, 0.0445), // barrel top
        vec3<f32>(0.1550, 0.0950, 0.0480), // barrel side
        vec3<f32>(0.1468, 0.0905, 0.0441), // barrel bottom
        vec3<f32>(0.6964, 0.6707, 0.5964), // wool
        vec3<f32>(0.0712, 0.0676, 0.0783), // basalt side
        vec3<f32>(0.0470, 0.0444, 0.0527), // basalt top
        vec3<f32>(0.6308, 0.2929, 0.0596), // magma
    );
    let atlas_cell = u32(floor(vertex_alpha + 0.5));
    return material_color * vec4<f32>(means[atlas_cell], 1.0);
}
