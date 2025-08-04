implementing lib;

namespace voxel {
  
public struct VoxelDataBuffer {
  public ByteAddressBuffer model_voxel_data[32];
  public RWByteAddressBuffer model_voxel_data_rw[32];
}

public struct VoxelDataRange {
  public uint buffer_index;
  public uint offset;

  public __init(uint model_pointer) {
    buffer_index = model_pointer >> 27;
    offset = model_pointer & 0x07FFFFFF;
  }

  public __init(uint buffer_index, uint offset) {
    this.buffer_index = buffer_index;
    this.offset = offset;
  }

  // Offsets from the current offset in the same buffer
  public VoxelDataRange with_offset(uint offset) {
    return VoxelDataRange(this.buffer_index, this.offset + offset);
  }

  public uint load(uint index) {
    return u_frame
      .voxel
      .model_voxel_data
      .model_voxel_data[buffer_index]
      .Load<uint>((this.offset + index) << 2);
  }

  public void store(uint index, uint data) {
    u_frame
      .voxel
      .model_voxel_data
      .model_voxel_data_rw[buffer_index]
      .Store<uint>((this.offset + index) << 2, data);
  }
}

} // namespace voxel
