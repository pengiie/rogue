module debug_raster;

import lib;

struct VertexOut {
  float3 normal;
  float4 color;
  bool shading;
}

struct VertexStageOutput {
  VertexOut vertex;
  float4 position : SV_POSITION;
}

[shader("vertex")]
VertexStageOutput main_vs(uint vertex_id : SV_VertexID, uint instance_id : SV_VulkanInstanceID) {
  let debug_info = debug::DebugRenderer::get();
  let instance_info = debug_info.instance_info_buffer.Load(instance_id);
  let mesh_info = debug_info.mesh_info_buffer.Load(instance_info.mesh_info_index);

  let index_offset = mesh_info.index_offset + vertex_id;
  let index = debug_info.indices_buffer.Load<uint>(index_offset << 2);

  let vertex_offset = mesh_info.vertex_offset + index * 6;
  let vx = debug_info.vertices_buffer.Load<float>(vertex_offset << 2);
  let vy = debug_info.vertices_buffer.Load<float>((vertex_offset + 1) << 2);
  let vz = debug_info.vertices_buffer.Load<float>((vertex_offset + 2) << 2);
  let nx = debug_info.vertices_buffer.Load<float>((vertex_offset + 3) << 2);
  let ny = debug_info.vertices_buffer.Load<float>((vertex_offset + 4) << 2);
  let nz = debug_info.vertices_buffer.Load<float>((vertex_offset + 5) << 2);
  let vertex_pos = float4(vx, vy, vz, 1.0);
  let vertex_normal = float3(nx, ny, nz);

  let proj_view = u_frame.world_info.camera.proj_view;
  let world_pos = mul(instance_info.transform, vertex_pos);
  var vs_pos = mul(proj_view, world_pos);
  // Vulkan has up as -y.
  vs_pos.y = -vs_pos.y;

  let depth_test = (instance_info.flags & debug::MeshInstance.FLAG_DEPTH_TEST) > 0;
  if (!depth_test) {
    // Compress all the debug vertices to 0.0-0.1 since we still want to depth test
    // between debug meshes while still always having the rendered voxels behind.
    // Could use a separate depth buffer and do fragment dicarding in the fragment shader
    // based on the rendered world depth but this is easier just less precision for further geometry.
    vs_pos.z *= 0.1;
  }

  var vs_out = VertexStageOutput();
  vs_out.vertex = VertexOut();
  vs_out.vertex.color = float4(color::srgb_to_lsrgb(instance_info.color.rgb), instance_info.color.a);
  vs_out.vertex.normal = vertex_normal;
  vs_out.vertex.shading = (instance_info.flags & debug::MeshInstance.FLAG_SHADING) > 0;
  vs_out.position = vs_pos;
  return vs_out;
}

[shader("fragment")]
float4 main_fs(VertexOut input) {
  var col = input.color.rgb;
  if (input.shading) {
    let l = max(pow(dot(input.normal, float3(1.0, 1.0, 1.0)) * 0.3 + 0.7, 2.0), 0.05);
    col *= l;
  }
  
  return float4(col, input.color.a);
}
