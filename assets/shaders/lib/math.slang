implementing lib;

static const float EPSILON = 0.001;

public typealias matrix3x3 = matrix<float, 3, 3>;
public typealias matrix4x4 = matrix<float, 4, 4>;

public struct Ray {
  public float3 origin;
  public float3 dir;
  public float3 inv_dir;

  public __init(float3 in_origin, float3 in_dir) {
    origin = in_origin;
    dir = in_dir;
    inv_dir = 1.0 / in_dir;
  }

  public __init(float3 in_origin, float3 in_dir, float3 in_inv_dir) {
    origin = in_origin;
    dir = in_dir;
    inv_dir = in_inv_dir;
  }

  [mutating]
  public void advance(float t) {
    this.origin += dir * t;
  }

  public Ray advance_clone(float t) {
    return Ray(this.origin + dir * t, dir, inv_dir);
  }

  public RayAABBTest test_aabb(AABB aabb) {
    return ray_aabb_test(this, aabb);
  }
}

public struct AABB {
  public float3 center;
  public float3 half_side_length;

  private __init(float3 in_center, float3 in_half_side_length) {
    center = in_center;
    half_side_length = in_half_side_length;
  }

  static AABB center_extents(float3 in_center, float3 in_half_side_length) {
    return AABB(in_center, in_half_side_length);
  }
}

[ForceInline]
public float3 ray_point_test(Ray ray, float3 p) {
  return ray.inv_dir * (p - ray.origin);
}

public struct RayAABBTest {
  public bool hit;
  public float3 t_min;
  public float3 t_max;
  public float t_enter;
  public float t_exit;
}

public RayAABBTest ray_aabb_test(Ray ray, AABB aabb) {
  let t0 = ray_point_test(ray, aabb.center - aabb.half_side_length);
  let t1 = ray_point_test(ray, aabb.center + aabb.half_side_length);
  let t_min = min(t0, t1);
  let t_max = max(t0, t1);

  var temp = max(t_min.xx, t_min.yz);
  let t_enter = max(temp.x, temp.y);
  temp = min(t_max.xx, t_max.yz);
  let t_exit = min(temp.x, temp.y);

  let bound_t_enter = max(t_enter, 0.0);
  let hit = t_exit > bound_t_enter;

  return RayAABBTest(hit, t_min, t_max, t_enter, t_exit);
}

public float3 dither(float3 v) {
  let n = random::rand_f32() + random::rand_f32() - 1.0;  // triangular noise
  return v + n * exp2(-8.0);
}
