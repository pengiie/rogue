module terrain_prepass;

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

  var out_color = sample_sky(ray);
  out_color = float3(0.0);
  let ray_terrain = voxel::Terrain.trace(ray);
  if (ray_terrain.did_hit) {
    let normalized_depth = ray_terrain.depth_t / u_frame.world_info.camera.far_plane;
    out_color = lerp(ray_terrain.color, out_color, ray_terrain.depth_t / 64.0);

    u_shader.backbuffer_depth.Store(coord.xy, normalized_depth);
  }

  u_shader.backbuffer.Store(coord.xy, float4(out_color, random::rand_f32()));
}
