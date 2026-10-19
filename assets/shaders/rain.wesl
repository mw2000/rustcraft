// Rain and snow falling round the camera, see `weather::look::RainMaterial`. The mesh is a box
// of streaks that each carry a fixed spot in their position and which corner they are in their
// uv; they fall here, wrapped round the camera so the box goes wherever it does. Some of them
// are flakes of snow, as many as is snow of what falls, and none falls under a roof.

import bevy_pbr::render::{
    forward_io::{Vertex, VertexOutput},
    mesh_view_bindings as view_bindings,
    view_transformations::position_world_to_clip,
};

// x: how much is falling, 0..1; y: how bright the light on it is; zw: the wind, in blocks
// a second across the ground.
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> rain: vec4<f32>;
// x: how much of what falls is snow, 0..1; yz: the roof map's first column.
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var<uniform> look: vec4<f32>;
// Where what falls stops over each column round the camera, x then z, four to an element,
// see `RoofMap`.
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var<uniform> roofs: array<vec4<f32>, 576>;

// Matches `RAIN_BOX` in `weather/look.rs`.
const BOX: vec3<f32> = vec3<f32>(48.0, 32.0, 48.0);
// Matches `ROOF_SPAN` in `weather/look.rs`.
const ROOFS: i32 = 48;
// How fast drops fall, in blocks a second, and how long and wide a streak is.
const FALL: f32 = 22.0;
const LENGTH: f32 = 0.9;
const WIDTH: f32 = 0.012;
// How fast flakes fall, how far they sway, in blocks, and how big one is.
const SNOW_FALL: f32 = 2.2;
const SWAY: f32 = 0.5;
const FLAKE: f32 = 0.045;

// A number 0..1 that's the same for every corner of a streak, from where it starts.
fn hash(p: vec3<f32>) -> f32 {
    return fract(sin(dot(p, vec3<f32>(12.9898, 78.233, 37.719))) * 43758.5453);
}

// Where what falls stops over the column at `at`, or nowhere outside the map.
fn roof(at: vec3<f32>) -> f32 {
    let column = vec2<i32>(floor(at.xz)) - vec2<i32>(look.yz);
    if any(column < vec2<i32>(0)) || any(column >= vec2<i32>(ROOFS)) {
        return -1.0e9;
    }
    let index = column.x * ROOFS + column.y;
    return roofs[index / 4][index % 4];
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let eye = view_bindings::view.world_position;
    let time = view_bindings::globals.time;
    let seed = hash(vertex.position);
    let flake = hash(vertex.position * 1.37 + vec3<f32>(3.1)) < look.x;
    // heavier drops fall faster; flakes drift slowly with the wind
    var velocity = vec3<f32>(rain.z, -FALL * (0.8 + 0.4 * seed), rain.w);
    if flake {
        velocity = vec3<f32>(rain.z * 0.4, -SNOW_FALL * (0.7 + 0.6 * seed), rain.w * 0.4);
    }
    let fallen = vertex.position + velocity * time - eye;
    let offset = (fract(fallen / BOX + 0.5) - 0.5) * BOX;
    var centre = eye + offset;
    var world: vec3<f32>;
    if flake {
        // swaying as it falls, a little square turned to the camera
        let phase = time * (0.8 + seed) + seed * 40.0;
        centre += vec3<f32>(sin(phase), 0.0, cos(phase * 0.8)) * SWAY;
        let towards = normalize(eye - centre);
        let right = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), towards));
        let up = cross(towards, right);
        world = centre + (right * vertex.uv.x + up * (vertex.uv.y * 2.0 - 1.0)) * FLAKE;
    } else {
        // a streak along its fall, turned to face the camera across it
        let along = normalize(velocity);
        let across = normalize(cross(along, normalize(eye - centre)));
        world = centre + across * vertex.uv.x * WIDTH * (1.0 + length(offset) * 0.05) + along * (vertex.uv.y - 0.5) * LENGTH;
    }
    out.world_position = vec4<f32>(world, 1.0);
    out.position = position_world_to_clip(world);
    out.world_normal = vec3<f32>(0.0, 1.0, 0.0);
    // how far through the box it is, to fade it out before it wraps round, and only some
    // of the streaks show in lighter rain
    let through = abs(offset) / (BOX * 0.5);
    let edge = max(through.x, max(through.y, through.z));
    // none under a roof, nor through the ground
    let shown = step(seed, rain.x) * step(roof(centre), centre.y);
    out.uv = vertex.uv;
    // and before it's so near it smears across the view, which flakes come closer than
    let near = select(smoothstep(1.5, 5.0, length(offset)), smoothstep(0.3, 1.0, length(offset)), flake);
    out.color = vec4<f32>((1.0 - smoothstep(0.7, 1.0, edge)) * near, shown, select(0.0, 1.0, flake), 0.0);
    // a streak that won't show is put outside the view, so it's never rasterised
    if shown * near == 0.0 {
        out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    }
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    var alpha: f32;
    var tint: vec3<f32>;
    if in.color.z > 0.5 {
        // a soft round flake
        let round = 1.0 - smoothstep(0.5, 1.0, length(vec2<f32>(in.uv.x, in.uv.y * 2.0 - 1.0)));
        alpha = round * in.color.x * in.color.y * 0.85;
        tint = vec3<f32>(0.95, 0.96, 1.0) * rain.y;
    } else {
        let across = 1.0 - abs(in.uv.x);
        let along = sin(in.uv.y * 3.14159);
        alpha = across * along * in.color.x * in.color.y * 0.35;
        tint = vec3<f32>(0.72, 0.78, 0.86) * rain.y;
    }
    if alpha < 0.002 {
        discard;
    }
    return vec4<f32>(tint * alpha, alpha);
}
