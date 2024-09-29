@group(0) @binding(0) var u_backbuffer_sampler: sampler;
@group(0) @binding(1) var u_backbuffer_texture: texture_2d<f32>;

alias TriangleVertices = array<vec4f, 6>;
var<private> vertices: TriangleVertices = TriangleVertices(
  vec4f(-1.0,  1.0, 0.0, 0.0),
  vec4f(-1.0, -1.0, 0.0, 1.0),
  vec4f( 1.0,  1.0, 1.0, 0.0),
  vec4f( 1.0,  1.0, 1.0, 0.0),
  vec4f(-1.0, -1.0, 0.0, 1.0),
  vec4f( 1.0, -1.0, 1.0, 1.0),
);

struct VertexOutput {
  @builtin(position) clip_position: vec4f,
  @location(0) tex_coord: vec2f
};

@vertex fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOutput {
  var out: VertexOutput; 
  let vertex = vertices[idx];
  out.clip_position = vec4f(vertex.xy, 0.0, 1.0);
  out.tex_coord = vertex.zw;
  return out;
}

@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4f {
  let color = textureSample(u_backbuffer_texture, u_backbuffer_sampler, in.tex_coord).xyz;

  return vec4f(color, 1.0);
}
