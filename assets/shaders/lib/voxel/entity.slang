implementing lib;

namespace voxel {

// Total size 80 bytes
public struct EntityInfo {
  float3 aabb_min; // offset = 0
  float3 aabb_max; // offset = 16
  // Rows of the rotation matrix.
  float3 rotation_1; // offset = 32
  float3 rotation_2; // offset = 48
  float3 rotation_3; // offset = 64
  uint model_info_ptr; // offset = 76

  public RayEntityTest trace_entity(Ray ray) {
    let entity_aabb = AABB.new_min_max(this.aabb_min, this.aabb_max);

    let entity_rotation = matrix3x3(this.rotation_1,
                                    this.rotation_2,
                                    this.rotation_3);
    let rotated_ray_pos = mul(entity_rotation, ray.origin - entity_aabb.center) + entity_aabb.center;
    let rotated_ray_dir = mul(entity_rotation, ray.dir);
    let rotated_ray = Ray(rotated_ray_pos, rotated_ray_dir);
    let hit_info = rotated_ray.test_aabb(entity_aabb);

    if(hit_info.hit) {
      var model_test = trace_model(rotated_ray, entity_aabb, this.model_info_ptr);
      if (model_test.did_hit) {
        let world_normal = mul(transpose(entity_rotation), model_test.normal);
        RayEntityTest test;
        test.did_hit = true;
        test.albedo = model_test.albedo;
        test.normal = mul(entity_rotation, model_test.normal);
        test.face_normal = model_test.face_normal;
        test.depth_t = model_test.ray_t;
        return test;
      }
    }
    return RayEntityTest.miss();
  }
}


public struct RayEntityTest {
  public bool did_hit;
  public float3 albedo;
  public float3 normal;
  public float3 face_normal;
  public float depth_t;

  static RayEntityTest miss() {
    return RayEntityTest(false, float3(0.0), 0.0);
  }

  static RayEntityTest hit(float3 albedo, float3 normal, float depth_t) {
    return RayEntityTest(true, albedo, normal, depth_t);
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
      let entity_test = entity_info.trace_entity(ray);
      if(entity_test.did_hit && entity_test.depth_t.x < min_test.depth_t) {
         min_test = entity_test;
      }
    }

    return min_test;
  }
}

}  // namespace voxel
