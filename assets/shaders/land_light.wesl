// How what's on the land is lit: the world's own faces (`world.wesl`) and the models standing
// on it, creatures, players and dropped items (`model.wesl`), so both are lit alike: sunlight
// and moonlight, kept out of caves by sky light, the open sky's light, torchlight from block
// light and fireflies' glow. Each material using it carries the clouds at bindings 4 to 6 and the curvature buffer
// at 8.

import bevy_pbr::render::{
    mesh_view_bindings as view_bindings,
    mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT,
    shadows::fetch_directional_shadow,
    view_transformations::position_world_to_view,
};
import super::lighting::{direct_moon, direct_sun, open_sky};
import super::sky::SUN_LIGHT;
import super::cloud_shadow::cloud_shadow;
import super::curvature::uncurved;
import super::point_lights::point_light_glow;

// All that's left with no sky light at all, so caves aren't pitch black.
const CAVE_AMBIENT: vec3<f32> = vec3<f32>(0.02, 0.02, 0.025);
// Above 1, light falls away quickly at first under cover, then more slowly.
const SKY_LIGHT_CURVE: f32 = 2.0;
// Shadow maps only reach so far, so beyond them sunlight is kept out of caves by sky
// light instead: none gets in below this much of it.
const SUN_SKY_GATE: f32 = 0.3;
// Shadows fade out over this last part of their reach, instead of stopping at a line.
const SHADOW_FADE_START: f32 = 0.8;

// Torchlight at full strength, warm against the sky's light, and how it falls away with block
// light, as sky light does.
const TORCH_LIGHT: vec3<f32> = vec3<f32>(0.9, 0.5, 0.22);
const BLOCK_LIGHT_CURVE: f32 = 2.0;
// The most fireflies' light adds to a face, so a swarm near a torch doesn't wash it out.
const POINT_GLOW_MAX: f32 = 0.3;

// Light from the sky for sky light 0..1, see `light.rs`.
fn sky_ambient(sky: f32, normal: vec3<f32>) -> vec3<f32> {
    return CAVE_AMBIENT + open_sky(normal) * pow(clamp(sky, 0.0, 1.0), SKY_LIGHT_CURVE);
}

// Light from torches and other blocks for block light 0..1, the same by day or night.
fn block_ambient(block: f32) -> vec3<f32> {
    return TORCH_LIGHT * pow(clamp(block, 0.0, 1.0), BLOCK_LIGHT_CURVE);
}

fn sun_shadow(world_position: vec4<f32>, normal: vec3<f32>, frag_coord: vec2<f32>) -> f32 {
    let sun = &view_bindings::lights.directional_lights[SUN_LIGHT];
    if ((*sun).flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) == 0u {
        return 1.0;
    }
    let view_z = position_world_to_view(world_position.xyz).z;
    let shadow = fetch_directional_shadow(SUN_LIGHT, world_position, normal, view_z, frag_coord);
    let reach = (*sun).cascades[(*sun).num_cascades - 1u].far_bound;
    return mix(shadow, 1.0, smoothstep(reach * SHADOW_FADE_START, reach, -view_z));
}

// Sunlight reaching a face, shadowed by the land and the clouds, and moonlight, kept out of caves
// by sky light but not shadowed.
fn sunlight(world_position: vec4<f32>, normal: vec3<f32>, shaded: vec3<f32>, sky: f32, frag_coord: vec2<f32>) -> vec3<f32> {
    let gate = smoothstep(0.0, SUN_SKY_GATE, sky);
    let moon = direct_moon(shaded) * gate;
    let light = direct_sun(shaded, uncurved(world_position.xyz)) * gate;
    // faces it can't reach need no shadow lookups
    if all(light == vec3<f32>(0.0)) {
        return moon;
    }
    // the clouds are flat, so their shadows fall where the land really is
    return light * sun_shadow(world_position, normal, frag_coord) * cloud_shadow(uncurved(world_position.xyz)) + moon;
}

// Faces turn toward up between these distances in blocks, to this much of the way: past
// where a step is under a pixel, lit tops and dark sides shimmer into each other. LOD cells are
// a few blocks wide well before then, so it starts where shadows end, the default
// `RendererSettings::shadow_distance`, and the land is lit block by block as far as it's shadowed.
const FLATTEN_START: f32 = 1024.0;
const FLATTEN_END: f32 = 4096.0;
const FLATTEN_MAX: f32 = 0.85;

// The normal to light by: turned toward up with distance, which evens out the sun's and sky's contrast between faces.
fn lit_normal(normal: vec3<f32>, world_position: vec3<f32>) -> vec3<f32> {
    let flatten = FLATTEN_MAX * smoothstep(FLATTEN_START, FLATTEN_END, distance(world_position, view_bindings::view.world_position));
    return normalize(mix(normal, vec3<f32>(0.0, 1.0, 0.0), flatten));
}

// The light on a face at `world_position`, as it's drawn, turned to `normal`, with sky light
// `sky` and block light `block`, 0..1: the brighter, colour by colour, of the sky's light,
// which follows the day, and the torches', which doesn't, and on top of it fireflies' glow,
// which moves with them, so can't be block light. Any other light on the land belongs here
// too, so it reaches models as well.
fn land_light(world_position: vec4<f32>, normal: vec3<f32>, sky: f32, block: f32, frag_coord: vec2<f32>) -> vec3<f32> {
    let shaded = lit_normal(normal, world_position.xyz);
    let sky_light = sunlight(world_position, normal, shaded, sky, frag_coord) + sky_ambient(sky, shaded);
    let glow = min(point_light_glow(world_position.xyz, normal, frag_coord), vec3<f32>(POINT_GLOW_MAX));
    return max(sky_light, block_ambient(block)) + glow;
}
