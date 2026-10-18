// ModelMaterial's depth-only passes, the sun's shadow maps among them: models bend with the land
// as in the main pass, and dropped plants' and items' pictures cut their holes out.

import bevy_pbr::{
    prepass::{bindings as prepass_bindings, io as prepass_io},
    render::{mesh_functions, mesh_view_bindings::view, view_transformations::position_world_to_clip},
};
import super::curvature::{curved, curved_last};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> model_tint: vec4<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var model_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var model_sampler: sampler;

@vertex
fn vertex(vertex_in: prepass_io::Vertex) -> prepass_io::VertexOutput {
    let vertex = prepass_io::decompress_vertex(vertex_in, vertex_in.instance_index);
    var out: prepass_io::VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let world = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0));
    out.world_position = vec4<f32>(curved(world.xyz), 1.0);
    out.position = position_world_to_clip(out.world_position.xyz);
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
}
@if(VERTEX_UVS_A) {
    out.uv = vertex.uv;
}
@if(VERTEX_UVS_B) {
    out.uv_b = vertex.uv_b;
}
@if(VERTEX_COLORS) {
    out.color = vertex.color;
}
@if(NORMAL_PREPASS_OR_DEFERRED_PREPASS) {
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
}
@if(MOTION_VECTOR_PREPASS) {
    let previous_world_from_local = mesh_functions::get_previous_world_from_local(vertex.instance_index);
    let previous = mesh_functions::mesh_position_local_to_world(previous_world_from_local, vec4<f32>(vertex.position, 1.0));
    out.previous_world_position = vec4<f32>(curved_last(previous.xyz), 1.0);
}
@if(VERTEX_OUTPUT_INSTANCE_INDEX) {
    out.instance_index = vertex.instance_index;
}
    return out;
}

// Only cut-out pictures have holes, so only they pay for a texture sample.
fn cut_holes(in: prepass_io::VertexOutput) {
@if(CUT_OUT && VERTEX_UVS_A) {
    if textureSampleLevel(model_texture, model_sampler, in.uv, 0.0).a * model_tint.a < 0.5 {
        discard;
    }
}
}

@if(PREPASS_FRAGMENT)
@fragment
fn fragment(in: prepass_io::VertexOutput) -> prepass_io::FragmentOutput {
    cut_holes(in);

    var out: prepass_io::FragmentOutput;
@if(NORMAL_PREPASS) {
    out.normal = vec4<f32>(normalize(in.world_normal) * 0.5 + vec3<f32>(0.5), 1.0);
}
    // the sun's shadow maps clamp depth so casters behind their near plane still count
@if(UNCLIPPED_DEPTH_ORTHO_EMULATION) {
    out.frag_depth = in.unclipped_depth;
}
@if(MOTION_VECTOR_PREPASS) {
    let clip = view.unjittered_clip_from_world * in.world_position;
    let previous_clip = prepass_bindings::previous_view_uniforms.unjittered_clip_from_world * in.previous_world_position;
    out.motion_vector = (clip.xy / clip.w - previous_clip.xy / previous_clip.w) * vec2(0.5, -0.5);
}
    return out;
}
@else
@fragment
fn fragment(in: prepass_io::VertexOutput) {
    cut_holes(in);
}
