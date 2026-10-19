// ModelMaterial: creatures, players, what they hold and dropped items, lit as the land is where
// they are (`land_light.wesl`), from the sky light and block light the client looks up for each
// part and passes in its mesh tag, see `client::model_light`. Their colours are their meshes',
// so the parts of every kind share a few materials and are drawn batched, the light being the
// instance's own. Bends with the land and fogs as it does.
//
// Those drawn in view (`IN_VIEW`), the first-person hand and the skin's preview, are drawn by a
// camera of their own that has none of the world's lights, fog or curve, so they're lit by the
// same sky light and block light from their tag against the open sky's light the client gives
// them, `model_daylight`, with each face shaded as though lit from over the shoulder.

import bevy_pbr::render::{
    forward_io::{Vertex, decompress_vertex},
    mesh_functions,
    view_transformations::position_world_to_clip,
};
import super::land_light::land_light;
import super::land_light::{block_ambient, CAVE_AMBIENT, SKY_LIGHT_CURVE};
import super::fog::{fogged_by, land_haze};
import super::curvature::{curved, uncurved};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> model_tint: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var model_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var model_sampler: sampler;
// a colour it's turned towards, and how far
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var<uniform> model_flash: vec4<f32>;
// the open sky's light, for those drawn in view
@group(constants::MATERIAL_BIND_GROUP) @binding(9) var<uniform> model_daylight: vec4<f32>;

// Matches `model_light::LIT`: a tag with this bit carries light, and one without, a part never
// looked up, is drawn as if under the open sky.
const LIT: u32 = 0x10000u;

struct ModelOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    // sky light and block light, 0..1
    @location(4) light: vec2<f32>,
}

// The sky light and block light a part's tag carries, see `model_light::tag`.
fn tag_light(tag: u32) -> vec2<f32> {
    if (tag & LIT) == 0u {
        return vec2<f32>(1.0, 0.0);
    }
    return vec2<f32>(f32(tag & 0xffu), f32((tag >> 8u) & 0xffu)) / 255.0;
}

@vertex
fn vertex(vertex_in: Vertex) -> ModelOutput {
    let vertex = decompress_vertex(vertex_in, vertex_in.instance_index);
    var out: ModelOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
    out.world_position = vec4<f32>(curved(world.xyz), 1.0);
@if(IN_VIEW) {
    out.world_position = world;
}
    out.position = position_world_to_clip(out.world_position.xyz);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
    out.uv = vec2<f32>(0.0);
@if(VERTEX_UVS_A) {
    out.uv = vertex.uv;
}
    out.color = vec4<f32>(1.0);
@if(VERTEX_COLORS) {
    out.color = vertex.color;
}
    out.light = tag_light(mesh_functions::get_tag(vertex.instance_index));
    return out;
}

@fragment
fn fragment(in: ModelOutput, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    // the mesh's own colour, which is what tells most parts apart, see `model_light::coloured`
    var color = vec4<f32>(mix(in.color.rgb, model_flash.rgb, model_flash.a), in.color.a) * model_tint;
@if(TEXTURED) {
    color *= textureSample(model_texture, model_sampler, in.uv);
}
@if(CUT_OUT) {
    if color.a < 0.5 {
        discard;
    }
}
    // eyes and the like glow as brightly as they are whatever the light
    // a picture seen from behind as it spins is lit from that side
    var normal = normalize(in.world_normal);
    if !is_front {
        normal = -normal;
    }
@if(IN_VIEW) {
    return vec4<f32>(color.rgb * in_view_light(normal, in.light.x, in.light.y), color.a);
}
@if(!GLOWS && !IN_VIEW) {
    color = vec4<f32>(color.rgb * land_light(in.world_position, normal, in.light.x, in.light.y, in.position.xy), color.a);
}
    let world_position = in.world_position.xyz;
    return fogged_by(color, world_position, in.position.xy, land_haze(uncurved(world_position).y));
}

// Faces turned up and towards the light over the shoulder, up and to the left, are lit most.
const IN_VIEW_FROM: vec3<f32> = vec3<f32>(-0.35, 0.85, 0.4);

// The light on a face of something drawn in view turned to `normal`, with sky light `sky` and
// block light `block`, 0..1, as `land_light` has them but for the shading.
fn in_view_light(normal: vec3<f32>, sky: f32, block: f32) -> vec3<f32> {
    let shade = 0.55 + 0.45 * max(dot(normal, normalize(IN_VIEW_FROM)), 0.0);
    let sky_light = model_daylight.rgb * pow(clamp(sky, 0.0, 1.0), SKY_LIGHT_CURVE);
    return (CAVE_AMBIENT + max(sky_light, block_ambient(block))) * shade;
}
