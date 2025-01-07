implementing lib;

namespace voxel {

public interface IVoxelModel {

}

public struct Terrain {
  ByteAddressBuffer data;

  public __init(ByteAddressBuffer in_data) {
    data = in_data;
  }
}

extension Terrain: IVoxelModel {

}

} // namespace voxel
