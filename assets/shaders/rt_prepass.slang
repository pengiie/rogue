module rt_prepass;

import lib;

struct ShaderUniforms {
  [format("rgba32f")]
  WTexture2D<float4> backbuffer;
  [format("r16f")]
  WTexture2D<float> backbuffer_depth;
}

ParameterBlock<ShaderUniforms> u_shader;

[shader("compute")]
[numthreads(8, 8, 1)]
void main(uint3 id: SV_DispatchThreadID) {
  var dimensions: uint2;
  u_shader.backbuffer.GetDimensions(dimensions.x, dimensions.y);

  let coord = id.xy;
  if (coord.x >= dimensions.x || coord.y >= dimensions.y) {
    return;
  }
  random::init(uint3(coord.xy, u_frame.frame_info.time_ms));

  let ray = u_frame.world_info.camera.create_ray((float2) coord, (float2) dimensions);
  let sky_color = sample_sky(ray);
  var depth = u_frame.world_info.camera.far_plane;
  var out_color = sky_color;

  var light = sky_color;
  let ray_terrain = voxel::Terrain.trace(ray);
  if (ray_terrain.did_hit) {
    depth = ray_terrain.depth_t;
    out_color = ray_terrain.color;
  }

  // If the hit entity is closer than the hit terrain, use the
  // entities hit.
  let ray_entity = voxel::EntityData.trace(ray);
  if (ray_entity.did_hit) {
    if (ray_entity.depth_t < depth) {
      depth = ray_entity.depth_t;
      out_color = ray_entity.color;
    }
  }

  let should_cast_shadow_ray = ray_terrain.did_hit || ray_entity.did_hit;
  if (should_cast_shadow_ray) {
    var shadow_ray = ray.advance_clone(depth - 0.001);
    shadow_ray.dir = normalize(-float3(0.7,-1,0.5));
    shadow_ray.inv_dir = 1.0 / shadow_ray.dir;
    let shadow_ray_terrain = voxel::Terrain.trace(shadow_ray);
    if (shadow_ray_terrain.did_hit) {
      out_color *= 0.1;
    } else {
      // If the hit entity is closer than the hit terrain, use the
      // entities hit.
      let shadow_ray_entity = voxel::EntityData.trace(shadow_ray);
      if (shadow_ray_entity.did_hit) {
        out_color *= 0.1;
      } else {
        out_color *= sample_sky(shadow_ray);
      }
    }
  }

  let normalized_depth = depth / u_frame.world_info.camera.far_plane;
  u_shader.backbuffer_depth.Store(coord.xy, normalized_depth);
  u_shader.backbuffer.Store(coord.xy, float4(out_color, 1.0));
}
