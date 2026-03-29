module voxel_preview;

import lib;

struct ShaderUniforms {
  [format("rgba32f")]
  RWTexture2D<float4> backbuffer;
  [format("r16f")]
  Texture2D<float> backbuffer_depth;
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

  let current_depth = u_shader.backbuffer_depth.Load(int3(coord.xy, 0));
  let fn = abs(entity_test.face_normal);
  let l = max3(1.0 * fn.x, 0.99 * fn.y, 0.98 * fn.z);
  let albedo = entity_test.albedo.rgb * l;
  const float BACKBUFFER_DEPTH_BIAS = 0.001;
  if (entity_test.depth_t > current_depth - BACKBUFFER_DEPTH_BIAS) {
    //albedo.a = 1.0;
  }

  let current_frag = u_shader.backbuffer.Load(coord.xy);
  var out_color = lerp(current_frag.rgb, albedo, entity_test.albedo.a);
  if (abs(entity_test.depth_t - current_depth) < 0.0025) {
  }

  // TODO: Run this after post process maybe? need to split blit then.
  out_color = color::srgb_to_lsrgb(out_color);
  u_shader.backbuffer.Store(coord.xy, float4(out_color, 1.0));
}
