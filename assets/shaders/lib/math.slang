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
  void advance(float t) {
    this.origin += dir * t;
  }

  Ray advance_clone(float t) {
    return Ray(this.origin + dir * t, dir, inv_dir);
  }
}

public struct AABB {
  float3 center;
  float3 half_side_length;

  private __init(float3 in_center, float3 in_half_side_length) {
    center = in_center;
    half_side_length = in_half_side_length;
  }


  static AABB center_extents(float3 in_center, float3 in_half_side_length) {
    return AABB(in_center, in_half_side_length);
  }
}

public float3 dither(float3 v) {
  let n = rand_f32() + rand_f32() - 1.0;  // triangular noise
  return v + n * exp2(-8.0);
}
