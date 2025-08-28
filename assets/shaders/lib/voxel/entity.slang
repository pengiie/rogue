implementing lib;

namespace voxel {

public struct EntityInfo {
  float3 aabb_min; // offset = 0
  float3 aabb_max; // offset = 16
  matrix3x3 rotation; // offset = 32
  uint model_info_ptr; // offset = 76
}

public struct RayEntityTest {
  public bool did_hit;
  public float3 color;
  public float depth_t;

  static RayEntityTest miss() {
    return RayEntityTest(false, float3(0.0), 0.0);
  }

  static RayEntityTest hit(float3 color, float depth_t) {
    return RayEntityTest(true, color, depth_t);
  }
}

public struct EntityData {
  public uint entity_count;
  public StructuredBuffer<EntityInfo> accel_buf;


  public static RayEntityTest trace(Ray ray) {
    let entity_data = u_frame.voxel.entity_data;

    RayEntityTest min_test = RayEntityTest.miss();
    min_test.depth_t = 100000.0;
    for (uint i = 0; i < entity_data.entity_count; i++) {
      let entity_info = entity_data.accel_buf.Load(i);
      let entity_aabb = AABB.new_min_max(entity_info.aabb_min, entity_info.aabb_max);

      let rotated_ray_pos = mul(entity_info.rotation, ray.origin - entity_aabb.center) + entity_aabb.center;
      let rotated_ray_dir = mul(entity_info.rotation, ray.dir);
      let rotated_ray = Ray(rotated_ray_pos, rotated_ray_dir);
      let hit_info = rotated_ray.test_aabb(entity_aabb);

      if(hit_info.hit) {
        let model_test = trace_model(rotated_ray, entity_aabb, entity_info.model_info_ptr, entity_info.rotation);
        if (model_test.did_hit && model_test.depth_t < min_test.depth_t) {
          min_test = RayEntityTest.hit(model_test.color, model_test.depth_t);
        }
      }
    }

    return min_test;
  }
}

}  // namespace voxel
