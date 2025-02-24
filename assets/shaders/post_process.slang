module post_process;

import lib;

struct Uniforms {
  [format("rgba32f")]
  Texture2D<float4> rt_final;

  [format("rgba8")]
  WTexture2D<float4> backbuffer;
}

extern ParameterBlock<ConstantBuffer<Uniforms>> u_shader;

[shader("compute")]
[numthreads(8, 8, 1)]
void main(uint3 id: SV_DispatchThreadID) {
  var dimensions: uint2;
  u_shader.backbuffer.GetDimensions(dimensions.x, dimensions.y);

  let coord = id.xy;
  if (coord.x >= dimensions.x || coord.y >= dimensions.y) {
    return;
  }

  let uv = float2(coord) / float2(dimensions);

  // Nearest neighbor interpolation.
  var rt_dim: uint2;
  u_shader.rt_final.GetDimensions(rt_dim.x, rt_dim.y);
  let sample_pixel = uint2(uv * float2(rt_dim));
  let sample_color = u_shader.rt_final.Load(uint3(sample_pixel.xy, 0));

  let color_srgb = color::lsrgb_to_srgb(sample_color.rgb);

  u_shader.backbuffer.Store(coord.xy, float4(color_srgb, 1.0));
}
