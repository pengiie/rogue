implementing lib;

namespace voxel {

public const static uint INVALID_PTR = 0xFFFFFFFF;

public struct RayModelTest {
  public bool did_hit;
  public float4 albedo;
  // Normal in the reference frame of the ray and aabb. 
  public float3 normal;
  public float3 face_normal;
  public float ray_t;

  static RayModelTest miss() {
    return RayModelTest(false, float4(0.0), 0.0);
  }

  static RayModelTest hit(float4 color, float3 normal, float depth_t) {
    return RayModelTest(true, color, normal, depth_t);
  }

  static RayModelTest hit() {
    var s = RayModelTest(true);
    s.albedo = float4(1.0);
    return s;
  }
}

public RayModelTest trace_model(Ray ray, AABB aabb, uint voxel_model_ptr) {
  let info_data = u_frame.voxel.model_info_data;
  let model_schema = info_data.Load<uint>(voxel_model_ptr << 2);
  switch (model_schema) {
    case 0:
      let sft = SFT(voxel_model_ptr);
      return sft.trace(ray, aabb);
  }

  return RayModelTest.miss();
}

public bool model_is_voxel_present(int3 local_voxel_pos, uint voxel_model_ptr) {
  let info_data = u_frame.voxel.model_info_data;
  let model_schema = info_data.Load<uint>(voxel_model_ptr << 2);
  switch (model_schema) {
    case 0:
      let sft = SFT(voxel_model_ptr);
      return sft.is_voxel_present(local_voxel_pos);
  }

  return false;
}

public void model_set_voxel_material(uint3 local_voxel_pos, uint voxel_model_ptr, CompressedVoxelMaterial comp_mat) {
  let info_data = u_frame.voxel.model_info_data;
  let model_schema = info_data.Load<uint>(voxel_model_ptr << 2);
  switch (model_schema) {
    case 0:
      let sft = SFT(voxel_model_ptr);
      sft.set_voxel_material(local_voxel_pos, comp_mat);
      break;
   }
}

public Optional<CompressedVoxelMaterial> model_get_voxel_material(uint3 local_voxel_pos, uint voxel_model_ptr) {
  let info_data = u_frame.voxel.model_info_data;
  let model_schema = info_data.Load<uint>(voxel_model_ptr << 2);
  switch (model_schema) {
    case 0:
      let sft = SFT(voxel_model_ptr);
      return sft.get_voxel_material(local_voxel_pos);
   }
   return none;
}

} // namespace voxel
