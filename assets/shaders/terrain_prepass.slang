module terrain_prepass;

import lib;

struct WorldInfo {
  Camera camera;
}

struct Uniforms {
  [format("rgba32f")]
  WTexture2D<float4> backbuffer;
//  [format("r32ui")]
//  WTexture2D<uint> terrain_traversal_morton;
//  ConstantBuffer<WorldInfo> world_info;
//  ByteAddressBuffer terrain_acceleration_data;
//  ByteAddressBuffer model_info_data;
//  ByteAddressBuffer model_voxel_data;
}

extern ParameterBlock<Uniforms> u_shader;

[shader("compute")]
[numthreads(8, 8, 1)]
void main(uint3 id: SV_DispatchThreadID) {
  var dimensions: uint2;
  u_shader.backbuffer.GetDimensions(dimensions.x, dimensions.y);

  let coord = id.xy;
  if (coord.x >= dimensions.x || coord.y >= dimensions.y) {
    return;
  }
//  init_rng(uint3(coord.xy, 1234));
//
//  let camera_ray = uniforms.world_info.camera.create_ray((float2) coord, (float2) dimensions);
//
//  var out_color = sample_sky(uniforms.model_voxel_data, camera_ray) * sample_sky(uniforms.model_info_data, camera_ray);
//
  u_shader.backbuffer.Store(coord.xy, float4(0.55, 0.33, 0.77, 1.0));
}
