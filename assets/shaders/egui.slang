module egui;

import lib;

struct ShaderUniforms {
  // Logical screen size since egui performs scaling.
  float2 screen_size;

  [format("rgba8")]
  Texture2D<float4> texture; 
  SamplerState sampler;
}
ParameterBlock<ShaderUniforms> u_shader;

struct AssembledVertex {
  [[vk::location(0)]]
  float2 position;
  [[vk::location(1)]]
  float2 uv;
  [[vk::location(2)]]
  uint color;
}

struct VertexOut {
  float4 color;
  float2 uv;
}

struct VertexStageOutput {
  VertexOut vertex;
  float4 position : SV_POSITION;
}

[shader("vertex")]
VertexStageOutput main_vs(AssembledVertex in_vertex) {
  let col = float4(
    (float) (in_vertex.color & 0xFF),
    (float) ((in_vertex.color >> 8) & 0xFF),
    (float) ((in_vertex.color >> 16) & 0xFF),
    (float) (in_vertex.color >> 24)) / 255.0;
  let position = float4(
    ((2.0 * in_vertex.position.x) / u_shader.screen_size.x) - 1.0,
    ((2.0 * in_vertex.position.y) / u_shader.screen_size.y) - 1.0,
    0.0, 1.0);
  let linear_color = float4(color::srgb_to_lsrgb(col.rgb), col.a);
  return VertexStageOutput(VertexOut(linear_color, in_vertex.uv), position);
}

[shader("fragment")]
float4 main_fs(VertexOut input) {
  // Our textures our stored in a SRGB format so sampling gives us LSRGB.
  let texture_color = u_shader.texture.Sample(u_shader.sampler, input.uv);
  return float4(color::lsrgb_to_srgb(input.color.rgb * texture_color.rgb),
                input.color.a * texture_color.a);
}
