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

  let center = (float2) dimensions / 2.0;
  var over = false;
  if (abs(coord.x - center.x) <= 3 && abs(coord.y - center.y) <= 10) {
    //voxel::testglobal = true;
    over = true;
  }
  if (abs(coord.x - center.x) <= 10 && abs(coord.y - center.y) <= 3) {
    over = true;
  }

  let ray = u_frame.world_info.camera.create_ray((float2) coord, (float2) dimensions);

  var out_color = sample_sky(ray);
  let ray_terrain = voxel::Terrain.trace(ray);
  if (ray_terrain.did_hit) {
    let normalized_depth = ray_terrain.depth_t / u_frame.world_info.camera.far_plane;
    if (!over) {
      out_color = ray_terrain.color;
    } else {
      out_color = float3(0.2, 0.2, 0.2);
    }

    u_shader.backbuffer_depth.Store(coord.xy, normalized_depth);
  }

  u_shader.backbuffer.Store(coord.xy, float4(out_color, random::rand_f32()));
}
