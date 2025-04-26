implementing lib;

static const float EPSILON = 0.01;

public typealias matrix2x2 = matrix<float, 2, 2>;
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

  public Ray clone() {
    return Ray(this.origin, this.dir, this.inv_dir);
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

public struct RayDDA {
  public int3 curr_grid;
  public int3 unit_grid;
  public float3 curr_t;
  public float3 unit_t;
  public uint3 bounds;

  // Expects `in_ray` to be advanced to the aabb with an intersection
  // test prior to this in world-space, no epsilon is needed since it 
  // will be clamped to the grid bounds.
  public __init(Ray in_ray, AABB in_aabb, uint3 in_dda_bounds) {
    // Local-space position with the origin being the AABB minimum.
    let pos = in_ray.origin - in_aabb.min();
    // Normalize the local-space to [0, 1), mapping from the AABB side length.
    let norm_pos = clamp(pos / in_aabb.side_length(), 0.0, 0.99999);
    // Turn the local-space position from [0, 1) to [0, in_dda_bounds).
    let norm_grid_pos = norm_pos * (float3) in_dda_bounds;
    let norm_grid_ray = Ray(norm_grid_pos, in_ray.dir);

    // Clamp the grid pos since `norm_grid_pos` can be `in_dda_bounds`
    // long though since we are indexing from 0 we must adjust this.
    curr_grid = (int3) floor(norm_grid_pos);
    unit_grid = (int3) sign(in_ray.dir);
    // Selects are for fixing floating point divide by zero stuff.
    unit_grid = select(unit_grid == 0, int3(1), unit_grid); 

    let next_point = (float3) curr_grid + (/*Bit in parenthesis just maps [-1, 1] to [0, 1]*/
                                           (float3) unit_grid * 0.5 + 0.5);
    curr_t = ray_point_test(norm_grid_ray, next_point);
    curr_t = select(in_ray.dir == 0.0, float3(100000000.0), curr_t);
    unit_t = abs(in_ray.inv_dir);
    // TODO: Figure out why i need this, if the curr_t is correct then i dont need 
    // to worry about the unit_t, but it seems i still do, fix later i guess?
    unit_t = select(isinf(unit_t), float3(0.0), unit_t);
    bounds = in_dda_bounds;
  }

  public uint curr_morton() {
    return morton::encode(abs(this.curr_grid % this.bounds));
  }

  public bool in_bounds() {
    return this.curr_grid.x >= 0 && this.curr_grid.y >= 0 && 
           this.curr_grid.z >= 0 && this.curr_grid.x < this.bounds.x && 
           this.curr_grid.y < this.bounds.y && this.curr_grid.z < this.bounds.z;
  }

  public uint3 curr_grid_pos() {
    return this.curr_grid;
  }

  [mutating]
  public void step() {
    let mask = this.curr_t <= min(curr_t.zxy, curr_t.yzx);
    this.curr_grid += int3(mask) * this.unit_grid;
    this.curr_t += float3(mask) * this.unit_t;
  }
}

public struct OBB {
  public AABB aabb;
  public matrix3x3 rotation;
}

public struct AABB {
  public float3 center;
  public float3 half_side_length;

  private __init(float3 in_center, float3 in_half_side_length) {
    center = in_center;
    half_side_length = in_half_side_length;
  }

  static AABB new_center_extents(float3 in_center, float3 in_half_side_length) {
    return AABB(in_center, in_half_side_length);
  }

  static AABB new_min_max(float3 min, float3 max) {
    let half_side_length = (max - min) / 2.0;
    let center = min + half_side_length;
    return AABB(center, half_side_length);
  }

  public float3 side_length() {
    return this.half_side_length * 2;
  }

  public float3 min() {
    return this.center - this.half_side_length;
  }

  public float3 max() {
    return this.center + this.half_side_length;
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
  // If the ray starts inside the AABB, a negative t_enter solution
  // will still count as a hit, for that reason, this t-value is
  // adjust to always have the range [0, infinity).
  public float t_enter;
  public float t_exit;
}

public RayAABBTest ray_aabb_test(Ray ray, AABB aabb) {
  let t0 = ray_point_test(ray, aabb.min());
  let t1 = ray_point_test(ray, aabb.max());
  let t_min = min(t0, t1);
  let t_max = max(t0, t1);

  var temp = max(t_min.xx, t_min.yz);
  let t_enter = max(temp.x, temp.y);
  temp = min(t_max.xx, t_max.yz);
  let t_exit = min(temp.x, temp.y);

  // Since we can get a negative solution for the t-value
  // that would intersect this AABB, we bound to 0 so we
  // only register intersections with a positive t.
  let bound_t_enter = max(t_enter, 0.0);
  let hit = t_exit > bound_t_enter;

  return RayAABBTest(hit, t_min, t_max, bound_t_enter, t_exit);
}

public float3 dither(float3 v) {
  let n = random::rand_f32() + random::rand_f32() - 1.0;  // triangular noise
  return v + n * exp2(-8.0);
}

namespace morton {

uint split_by_2(uint x) {
  var y = x & 0x000003ff; //      00000000000000000000001111111111
  y = (y | (y << 16)) & 0x030000ff; // 00000011000000000000000011111111
  y = (y | (y << 8)) & 0x0300f00f; //  00000011000000001111000000001111
  y = (y | (y << 4)) & 0x030c30c3; //  00000011000011000011000011000011
  y = (y | (y << 2)) & 0x09249249; //  00001001001001001001001001001001
  return y;
}

public uint encode(uint3 val) {
  return split_by_2(val.x) |
         (split_by_2(val.y) << 1) | 
         (split_by_2(val.z) << 2);
}

uint compact_by_2(uint x) {
  var y = x & 0x09249249; //      00001001001001001001001001001001
  y = (y | (y >> 2)) & 0x030c30c3; //  00000011000011000011000011000011
  y = (y | (y >> 4)) & 0x0300f00f; //  00000011000000001111000000001111
  y = (y | (y >> 8)) & 0x030000ff; //  00000011000000000000000011111111
  y = (y | (y >> 16)) & 0x000003ff; // 00000000000000000000001111111111
  return y;
}

public uint3 decode(uint val) {
  return uint3(
    compact_by_2(val),
    compact_by_2(val >> 1),
    compact_by_2(val >> 2)
  );
}

} // namespace morton

public uint morton_encode(uint3 val) {
  return morton::encode(val);
}
