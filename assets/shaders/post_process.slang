module post_process;

import lib;

struct Uniforms {
  [format("rgba32f")]
  Texture2D<float4> rt_final;
  [format("rgba8")]
  WTexture2D<float4> backbuffer;
  uint2 blit_offset;
}

ParameterBlock<Uniforms> u_shader;

[shader("compute")]
[numthreads(8, 8, 1)]
void main(uint3 id: SV_DispatchThreadID) {
  var rt_dim: uint2;
  u_shader.rt_final.GetDimensions(rt_dim.x, rt_dim.y);

  let coord = id.xy;
  if (coord.x >= rt_dim.x || coord.y >= rt_dim.y) {
    return;
  }

  let sample_color = u_shader.rt_final.Load(uint3(coord.xy, 0));
  let out_color = color::lsrgb_to_srgb(sample_color.rgb);

  let target_coord = u_shader.blit_offset + coord;
  u_shader.backbuffer.Store(target_coord, float4(out_color, 1.0));
}
