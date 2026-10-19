// Clouds' shadows on the land: where the sun's rays cross the cloud layer, looked up in
// its pattern, so they drift with the clouds however far out. See `clouds.rs`.

import bevy_pbr::render::mesh_view_bindings as view_bindings;
import super::sky::SUN_LIGHT;

@group(constants::MATERIAL_BIND_GROUP) @binding(4) var cloud_pattern: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var cloud_sampler: sampler;
// the middle of the layer's height, the pattern's width in blocks, and how far the wind has drifted the clouds;
// then how overcast a storm has made the sky, 0..1
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var<storage, read> cloud_layers: array<vec4<f32>, 2>;

// How much of the sun a cloud keeps off the ground, and the overcast of the heaviest storm.
const CLOUD_SHADE: f32 = 0.7;
const OVERCAST_SHADE: f32 = 0.85;

// The cloud layer's place and drift, see `cloud_layers`.
fn cloud_layer() -> vec4<f32> {
    return cloud_layers[0];
}

// How much sunlight reaches `world_position` past the clouds, 0..1.
fn cloud_shadow(world_position: vec3<f32>) -> f32 {
    let to_sun = view_bindings::lights.directional_lights[SUN_LIGHT].direction_to_light;
    let layer = cloud_layer().x;
    if to_sun.y <= 0.0 || world_position.y >= layer {
        return 1.0;
    }
    // under a storm's overcast, the sun's kept off everywhere, so shadows all but go
    let overcast = 1.0 - OVERCAST_SHADE * cloud_layers[1].x;
    let under = world_position.xz + to_sun.xz * ((layer - world_position.y) / to_sun.y);
    // the drift follows the shared time of day, see `clouds::wind_drift`
    let drift = cloud_layer().zw;
    let cover = textureSampleLevel(cloud_pattern, cloud_sampler, (under - drift) / cloud_layer().y, 0.0).r;
    return (1.0 - CLOUD_SHADE * cover) * overcast;
}
