implementing lib;

public struct FrameInfo {
  // Time since the start of the application.
  public uint time_ms;
}

public ParameterBlock<uniforms::FrameUniforms> u_frame;

namespace uniforms {

  public struct FrameUniforms {
    public FrameInfo frame_info;
    public FrameWorldInfo world_info;
    public VoxelUniforms voxel;
  }

  public struct VoxelUniforms {
    public voxel::Terrain terrain;
    public voxel::EntityData entity_data;
    public ByteAddressBuffer model_info_data;
    public ByteAddressBuffer model_voxel_data;
    public RWByteAddressBuffer rw_model_voxel_data;
  };


} // namespace uniforms
