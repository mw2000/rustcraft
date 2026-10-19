import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    mesh_functions,
    mesh_view_bindings as view_bindings,
    view_transformations::position_world_to_clip,
};
import super::block_texture::{block_alpha, block_footprint, block_mean, block_sample, material_fade};
import super::foliage::{has_holes, is_leaf, is_plant, leaf_holes_filled, swayed};
import super::fog::{fogged_by, land_haze};
import super::land_light::land_light;
import super::lod_patch::patch_tint;
import super::dither::dither_threshold;
import super::curvature::{curved, eye, uncurved};
// How bright the leaves seen through a solid tree box's holes are, to its surface's.
const HOLE_SHADE: f32 = 0.55;
// The same for far leaves, see `leaf_holes_filled`. Their holes are a texel or so across, and
// show the lit face of the next leaf in.
const FAR_HOLE_SHADE: f32 = 0.8;
// A torch's flame, the top of its tile down to here, see `generator::TORCH`, which glows this
// brightly whatever the light.
const FLAME_BOTTOM: f32 = 9.0 / 16.0;
const FLAME_GLOW: f32 = 1.6;
// Magma glows at least this brightly, near or far, so a volcano's crater shows at night from
// kilometres away, where LODs draw no block light.
const MAGMA_GLOW: f32 = 1.4;

// A lantern's glass, which glows as a flame does, where its tile has it, see
// `generator::shapes`'s `LANTERN_BODY`.
const LANTERN_GLASS_MIN: vec2<f32> = vec2<f32>(1.0, 10.0) / 16.0;
const LANTERN_GLASS_MAX: vec2<f32> = vec2<f32>(5.0, 15.0) / 16.0;

// Whether a fragment of a block face is a torch's flame or a lantern's glass, from its atlas
// cell and uv.
fn is_flame(vertex_alpha: f32, uv: vec2<f32>) -> bool {
    let tile = fract(uv);
    let lantern = abs(vertex_alpha - f32(constants::LANTERN_CELL)) < 0.5
        && all(tile >= LANTERN_GLASS_MIN) && all(tile < LANTERN_GLASS_MAX);
    return lantern || (abs(vertex_alpha - f32(constants::TORCH_CELL)) < 0.5 && tile.y < FLAME_BOTTOM);
}

// Whether a fragment is of magma, from its atlas cell.
fn is_magma(vertex_alpha: f32) -> bool {
    return abs(vertex_alpha - f32(constants::MAGMA_CELL)) < 0.5;
}

// Blocks per pixel at which grass sides start turning to the grass on top, and are wholly it:
// four pixels tall, as the tile's green fringe is about a quarter of it, to under two.
const GRASS_SIDE_FADE_START: f32 = 0.25;
const GRASS_SIDE_FADE_END: f32 = 0.6;

// Bevy's mesh vertex shader, for meshes that are never skinned or morphed, with leaves and
// plants swaying in the wind and the land curving away. Block meshes always carry vertex colours,
// see `MeshBuffers`.
@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
    var draws = 0.0;
@if(VERTEX_UVS_B) {
    draws = vertex.uv_b.y;
}
    let swaying = swayed(out.world_position.xyz, vertex.color.a, draws, vertex.uv.y, view_bindings::globals.time);
    // bent where it's drawn, so shadows and fog are looked up where it's seen
    out.world_position = vec4<f32>(curved(swaying), 1.0);
    out.color = vertex.color;
    out.position = position_world_to_clip(out.world_position.xyz);
    // a mesh fading out sits a little farther than it is, so what replaces it wins where they coincide
    out.position.z *= 1.0 - material_fade.z;
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
    out.uv = vertex.uv;
@if(VERTEX_UVS_B) {
    out.uv_b = vertex.uv_b;
}
    // block light, where there's any, see `MeshBuffers::build_mesh`
@if(VERTEX_TANGENTS) {
    out.world_tangent = vertex.tangent;
}
@if(VERTEX_OUTPUT_INSTANCE_INDEX) {
    out.instance_index = vertex.instance_index;
}
@if(VISIBILITY_RANGE_DITHER) {
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(vertex.instance_index, world_from_local[3]);
}
    return out;
}

struct FragmentOutput {
  @location(0) color: vec4<f32>
}

@fragment
fn fragment(
    in: VertexOutput,
) ->  FragmentOutput {

    let world_position = in.world_position.xyz;
    let normal = normalize(in.world_normal);

    let footprint = block_footprint(in.uv);
    var color = block_sample(in.uv, in.color.a, footprint);
    // blocks per pixel up the face, worked out here as derivatives need uniform control flow
    let block_rate = length(vec2<f32>(dpdx(in.uv.y), dpdy(in.uv.y)));
    // fades and far leaves dither with the depth prepass, see `world_prepass.wesl`
    let threshold = dither_threshold(in.position.xy);
    // coarse LOD cells, where a stretched texture would read as one giant block, and wide
    // tree boxes, which keep it: leaves come out solid, and their shadows match, see
    // `world_prepass.wesl`. How a face draws is `generator::TEXTURED` and so on.
@if(VERTEX_UVS_B) {
    if in.uv_b.y > 0.875 {
        let mean = block_mean(in.color.a);
        color = vec4<f32>(mean.rgb * patch_tint(world_position), mean.a);
    } else if in.uv_b.y > 0.5 {
        // the holes show deeper leaves, in their shade
        let deeper = block_mean(in.color.a).rgb * HOLE_SHADE;
        color = vec4<f32>(mix(deeper, color.rgb, smoothstep(0.3, 0.7, color.a)), 1.0);
    } else if leaf_holes_filled(world_position, eye(), in.color.a, in.uv_b.y, threshold) {
        // the leaves behind aren't drawn this far out, so the holes show them in their shade,
        // cut where they'd be cut out so the rest is as bright as nearer leaves
        if color.a < 0.5 {
            color = vec4<f32>(block_mean(in.color.a).rgb * FAR_HOLE_SHADE, 1.0);
        }
        color.a = 1.0;
    } else if has_holes(in.color.a, in.uv_b.y) {
        // cut where the depth prepass and shadows do, at full detail: the blurred alpha of a
        // far face agrees with them only up close, and where it doesn't the holes flicker
        color.a = block_alpha(in.uv, in.color.a);
    }
}
    // `DebugSettings::solid_leaves`: leaves without their holes, as coarse LODs draw them
@if(SOLID_LEAVES) {
    if is_leaf(in.color.a) {
        color = block_mean(in.color.a);
    }
}
    // ambient occlusion and grass/leaf tint
    var tinted = color.rgb * in.color.rgb;
@if(VERTEX_UVS_B) {
    // grass sides are a greyscale grass fringe, transparent below, layered over dirt: the grass
    // carries the grass's tint and the dirt its shade alone, see `MeshBuffers::tint_grass_sides`.
    // Once blocks are a few pixels tall the tile averages out to mostly dirt, while the grass
    // on top of them would fill the pixel, so they turn to it
    if in.uv_b.y < 0.0 {
        let dirt = block_sample(in.uv, f32(constants::DIRT_CELL), footprint).rgb * -in.uv_b.y;
        let grass_top = block_mean(f32(constants::GRASS_TOP_CELL)).rgb * in.color.rgb;
        let near = mix(dirt, tinted, color.a);
        tinted = mix(near, grass_top, smoothstep(GRASS_SIDE_FADE_START, GRASS_SIDE_FADE_END, block_rate));
        color.a = 1.0;
    }
}
    color = vec4<f32>(tinted, color.a);

    // block meshes carry smoothed sky light, all but those with no faces at all
    var sky = 1.0;
@if(VERTEX_UVS_B) {
    sky = in.uv_b.x;
}

    var block = 0.0;
@if(VERTEX_TANGENTS) {
    block = in.world_tangent.x;
}

    var light = land_light(in.world_position, normal, sky, block, in.position.xy);
    if is_flame(in.color.a, in.uv) {
        light = vec3<f32>(FLAME_GLOW);
    }
    if is_magma(in.color.a) {
        light = max(light, vec3<f32>(MAGMA_GLOW));
    }
    color = vec4<f32>(color.rgb * light, color.a);

    // so distant chunks fade into the sky instead of popping in and out, high ground the least
    color = fogged_by(color, world_position, in.position.xy, land_haze(uncurved(world_position).y));

    // after the texture samples above, which need uniform control flow
    if threshold < material_fade.x || threshold >= material_fade.y {
        discard;
    }
    // the holes in leaves and round plants: this shader replaces Bevy's, so must cut out
    // alpha-masked texels itself
@if(!SOLID_LEAVES) {
    if color.a < 0.5 {
        discard;
    }
}
    // drawn solid, leaves still keep their shape, but a plant would be a square
@if(SOLID_LEAVES) {
@if(VERTEX_UVS_B) {
    if is_plant(in.uv_b.y) && color.a < 0.5 {
        discard;
    }
}
}

    var output: FragmentOutput;
    output.color = color;
    return output;
}
