module post_process;

import lib;

struct Uniforms {
  [format("rgba32f")]
  Texture2D<float4> rt_final;

  [format("rgba8")]
  WTexture2D<float4> backbuffer;
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

  let pixel = u_shader.rt_final.Load(int3(500, 0, 0));

  u_shader.backbuffer.Store(coord.xy, pixel);
}
