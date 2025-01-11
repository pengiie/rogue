implementing lib;

namespace voxel {

public interface IVoxelModel {

}

public struct Terrain {
  public uint side_length;
  uint volume;
  ByteAddressBuffer data;

  // World-space AABB corresponding to the entire terrain's bounds.
  [ForceInline]
  public AABB aabb() {
    let meter_half_side_length = consts::TERRAIN_CHUNK_METER_LENGTH * (this.side_length << 1);
    return AABB.center_extents(float3(0.0), float3(meter_half_side_length));
  }

  public static RayTerrainTest trace(Ray ray) {
    let terrain = u_frame.voxel.terrain;

    let ray_terrain = ray.test_aabb(terrain.aabb());
    if (!ray_terrain.hit) {
      return RayTerrainTest.miss();
    }

    let vox_ptr = terrain.data.Load(0);
    if (vox_ptr == 0) {
      return RayTerrainTest.miss();
    }

    return RayTerrainTest.hit(color::srgb_to_lsrgb(float3(0.9, 0.35, 0.8)));
  }
}

public struct RayTerrainTest {
  public bool did_hit;
  public float3 color;

  static RayTerrainTest miss() {
    return RayTerrainTest(false, float3(0.0));
  }

  static RayTerrainTest hit(float3 color) {
    return RayTerrainTest(true, color);
  }
}

public struct RayModelTest {
  public bool did_hit;
  public float3 color;

  static RayModelTest miss() {
    return RayModelTest(false, float3(0.0));
  }

  static RayModelTest hit(float3 color) {
    return RayModelTest(true, color);
  }
}

public RayModelTest trace_model(Ray ray, uint voxel_model_ptr) {
  return RayModelTest.miss();
}

public struct ESVO {

}

} // namespace voxel
