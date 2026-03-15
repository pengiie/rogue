module debug_raster;

import lib;

struct VertexOut {
  float3 normal;
  float4 color;
}

struct VertexStageOutput {
  VertexOut vertex;
  float4 position : SV_POSITION;
}

[shader("vertex")]
VertexStageOutput main_vs(uint vertex_id : SV_VertexID, uint instance_id : SV_VulkanInstanceID) {
  let debug = debug::DebugRenderer::get();
  let instance_info = debug.instance_info_buffer.Load(instance_id);
  let mesh_info = debug.mesh_info_buffer.Load(instance_info.mesh_info_index);

  let index_offset = mesh_info.index_offset + vertex_id;
  let index = debug.indices_buffer.Load<uint>(index_offset << 2);

  let vertex_offset = mesh_info.vertex_offset + index * 6;
  let vx = debug.vertices_buffer.Load<float>(vertex_offset << 2);
  let vy = debug.vertices_buffer.Load<float>((vertex_offset + 1) << 2);
  let vz = debug.vertices_buffer.Load<float>((vertex_offset + 2) << 2);
  let nx = debug.vertices_buffer.Load<float>((vertex_offset + 3) << 2);
  let ny = debug.vertices_buffer.Load<float>((vertex_offset + 4) << 2);
  let nz = debug.vertices_buffer.Load<float>((vertex_offset + 5) << 2);
  let vertex_pos = float4(vx, vy, vz, 1.0);
  let vertex_normal = float3(nx, ny, nz);

  let proj_view = u_frame.world_info.camera.proj_view;
  let world_pos = mul(instance_info.transform, vertex_pos);
  var vs_pos = mul(proj_view, world_pos);
  // Vulkan has up as -y.
  vs_pos.y = -vs_pos.y;

  var vs_out = VertexStageOutput();
  vs_out.vertex = VertexOut();
  vs_out.vertex.color = float4(color::srgb_to_lsrgb(instance_info.color), 1.0);
  vs_out.vertex.normal = vertex_normal;;
  vs_out.position = vs_pos;
  return vs_out;
}

[shader("fragment")]
float4 main_fs(VertexOut input) {
  let l = max(pow(dot(input.normal, float3(1.0, 1.0, 1.0)) * 0.3 + 0.7, 2.0), 0.05);
  let color_rgb = input.color.rgb * l;
  return float4(color::lsrgb_to_srgb(color_rgb), input.color.a);
}
