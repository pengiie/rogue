module terrain_prepass;

import lib;

struct ShaderUniforms {
  [format("rgba32f")]
  WTexture2D<float4> backbuffer;
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
  let ray_terrain = voxel::Terrain.trace(ray);
  if (ray_terrain.did_hit) {
    out_color = ray_terrain.color;
  }

  u_shader.backbuffer.Store(coord.xy, float4(out_color, random::rand_f32()));
}
