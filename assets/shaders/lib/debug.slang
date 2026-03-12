implementing lib;

namespace debug {
  public struct MeshInstance {
    public float4x4 transform;
    public float3 color;
    public uint mesh_info_index;
  }

  public struct MeshInfo {
    public uint vertex_offset;
    public uint index_offset;
    public uint vertex_count;
    public uint index_count;
  }

  public struct DebugRenderer {
    public ByteAddressBuffer vertices_buffer;
    public ByteAddressBuffer indices_buffer;
    public StructuredBuffer<MeshInfo> mesh_info_buffer;
    public StructuredBuffer<MeshInstance> instance_info_buffer;

    public static DebugRenderer get() {
      return u_frame.debug;
    }
  }
}
