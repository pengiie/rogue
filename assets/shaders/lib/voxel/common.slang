implementing lib;

namespace voxel {

public const static uint INVALID_PTR = 0xFFFFFFFF;

public struct RayModelTest {
  public bool did_hit;
  public float3 color;
  public float depth_t;

  static RayModelTest miss() {
    return RayModelTest(false, float3(0.0), 0.0);
  }

  static RayModelTest hit(float3 color, float depth_t) {
    return RayModelTest(true, color, depth_t);
  }
}

public RayModelTest trace_model(Ray ray, AABB aabb, uint voxel_model_ptr, matrix3x3 rotation) {
  let info_data = u_frame.voxel.model_info_data;
  let model_schema = info_data.Load<uint>(voxel_model_ptr << 2);
  switch (model_schema) {
    //case consts::VOXEL_MODEL_SCHEMA_ESVO:
    //  let esvo = ESVO(aabb, voxel_model_ptr);
    //  return esvo.trace(ray);
    case consts::VOXEL_MODEL_SCHEMA_THC:
    case consts::VOXEL_MODEL_SCHEMA_THC_COMPRESSED:
      let thc = THC(voxel_model_ptr);
      return thc.trace(ray, aabb, rotation);
    case consts::VOXEL_MODEL_SCHEMA_SFT:
    case consts::VOXEL_MODEL_SCHEMA_SFT_COMPRESSED:
      let sft = SFT(voxel_model_ptr);
      return sft.trace(ray, aabb, rotation);
    case consts::VOXEL_MODEL_SCHEMA_FLAT:
      let flat = Flat(voxel_model_ptr);
      return flat.trace(ray, aabb, rotation);
  }

  return RayModelTest.miss();
}

} // namespace voxel
