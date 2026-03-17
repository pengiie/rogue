module voxel_preview;

import lib;

struct ShaderUniforms {
  [format("rgba32f")]
  RWTexture2D<float4> backbuffer;
  voxel::EntityInfo entity_info;
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

  let entity_test = u_shader.entity_info.trace_entity(ray);
  if (!entity_test.did_hit) {
    return;
  }

  let current_frag = u_shader.backbuffer.Load(coord.xy);
  let out_color = lerp(current_frag.rgb, entity_test.albedo, 0.3);
  u_shader.backbuffer.Store(coord.xy, float4(out_color, 1.0));
}
