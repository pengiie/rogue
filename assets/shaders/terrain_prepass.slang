module terrain_prepass;

import lib;

public struct FrameInfo {
  // Time since the start of the application.
  public uint time_ms;
}

public struct FrameUniforms {
  public ConstantBuffer<FrameInfo> frame_info;
  public ConstantBuffer<FrameWorldInfo> world_info;
}

struct ShaderUniforms {
  [format("rgba32f")]
  WTexture2D<float4> backbuffer;
//  [format("r32ui")]
//  WTexture2D<uint> terrain_traversal_morton;
  ByteAddressBuffer terrain_acceleration_data;
  ByteAddressBuffer model_info_data;
  ByteAddressBuffer model_voxel_data;
}

extern ParameterBlock<FrameUniforms> u_frame;
extern ParameterBlock<ShaderUniforms> u_shader;

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

  let camera_ray = u_frame.world_info.camera.create_ray((float2) coord, (float2) dimensions);
  var out_color = sample_sky(camera_ray);

  let uv = (float2) coord / (float2) dimensions;
  u_shader.backbuffer.Store(coord.xy, float4(out_color, random::rand_f32()));
}
