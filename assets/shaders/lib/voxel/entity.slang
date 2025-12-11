implementing lib;

namespace voxel {

// Total size 80
public struct EntityInfo {
  float3 aabb_min; // offset = 0
  float3 aabb_max; // offset = 16
  float3 rotation_1; // offset = 32
  float3 rotation_2; // offset = 48
  float3 rotation_3; // offset = 64
  uint model_info_ptr; // offset = 76
}

public struct RayEntityTest {
  public bool did_hit;
  public float3 color;
  public float3 normal;
  public float depth_t;

  static RayEntityTest miss() {
    return RayEntityTest(false, float3(0.0), 0.0);
  }

  static RayEntityTest hit(float3 color, float3 normal, float depth_t) {
    return RayEntityTest(true, color, normal, depth_t);
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

      let entity_rotation = matrix3x3(entity_info.rotation_1,
                                                         entity_info.rotation_2,
                                                         entity_info.rotation_3);
      let rotated_ray_pos = mul(entity_rotation, ray.origin - entity_aabb.center) + entity_aabb.center;
      let rotated_ray_dir = mul(entity_rotation, ray.dir);
      let rotated_ray = Ray(rotated_ray_pos, rotated_ray_dir);
      let hit_info = rotated_ray.test_aabb(entity_aabb);

      if(hit_info.hit) {
        let model_test = trace_model(rotated_ray, entity_aabb, entity_info.model_info_ptr, entity_rotation);
        if (model_test.did_hit && model_test.depth_t < min_test.depth_t) {
          let world_normal = mul(transpose(entity_rotation), model_test.normal);
          min_test = RayEntityTest.hit(model_test.color, world_normal, model_test.depth_t);
        }
      }
    }

    return min_test;
  }
}

}  // namespace voxel
