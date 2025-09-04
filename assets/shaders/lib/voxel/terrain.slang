implementing lib;

namespace voxel {

public struct RayTerrainTest {
  public bool did_hit;
  public float3 color;
  public float depth_t;

  static RayTerrainTest miss() {
    return RayTerrainTest(false, float3(0.0), 0.0);
  }

  static RayTerrainTest hit(float3 color, float depth_t) {
    return RayTerrainTest(true, color, depth_t);
  }
}

static const uint AIR_PTR = 0xFFFFFFFE;

public struct Terrain {
  public uint side_length;
  uint volume;
  public int3 anchor;
  uint3 window_offset;
  ByteAddressBuffer data;

  // World-space AABB corresponding to the entire terrain's bounds.
  [ForceInline]
  public AABB aabb() {
    let meter_side_length = consts::TERRAIN_CHUNK_METER_LENGTH * this.side_length;
    let min = (float3) this.anchor * consts::TERRAIN_CHUNK_METER_LENGTH;
    let max = min + meter_side_length;
    return AABB.new_min_max(min, max);
  }

  public static uint get_chunk_index(uint3 pos) {
      let side_length = u_frame.voxel.terrain.side_length;
      return pos.x + 
             pos.y * side_length + 
             pos.z * (side_length * side_length);
  }

  public static uint get_chunk_ptr(uint3 pos) {
      let terrain = u_frame.voxel.terrain;
      let window_pos = (uint3) ((pos + terrain.window_offset) % terrain.side_length);
      let index = Terrain.get_chunk_index(window_pos);
      let chunk_model_ptr = terrain.data.Load<uint>(index << 2);
      return chunk_model_ptr;
  }

  public static RayTerrainTest trace(Ray ray) {
    let terrain = u_frame.voxel.terrain;
    let terrain_aabb = terrain.aabb();

    let ray_terrain = ray.test_aabb(terrain_aabb);
    if (!ray_terrain.hit) {
      return RayTerrainTest.miss();
    }

    var dda = RayDDA(ray.advance_clone(ray_terrain.t_enter),
                     terrain_aabb,
                     terrain.side_length);
    var i = 0;
    while (dda.in_bounds() && i < 1024 /* just in case */) {
      let curr_grid_pos = (uint3) ((dda.curr_grid_pos() + terrain.window_offset) % terrain.side_length);
      var curr_index = curr_grid_pos.x + 
                       curr_grid_pos.y * terrain.side_length + 
                       curr_grid_pos.z * (terrain.side_length * terrain.side_length);

      let chunk_model_ptr = terrain.data.Load<uint>(curr_index << 2);
      if (chunk_model_ptr != AIR_PTR /*Treat as air for now*/ && chunk_model_ptr != INVALID_PTR) {
        if (chunk_model_ptr == INVALID_PTR) {
          // TODO: Request chunk from cpu.
        }
        let chunk_offset = consts::TERRAIN_CHUNK_METER_LENGTH * ((float3) dda.curr_grid_pos() + 0.5);
        let chunk_center = terrain_aabb.min() + chunk_offset; 
        let chunk_aabb = AABB.new_center_extents(chunk_center, 
                                                 float3(consts::TERRAIN_CHUNK_METER_LENGTH) * 0.5);

        let model_test = trace_model(ray, chunk_aabb, chunk_model_ptr, matrix3x3(1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0));
        if (model_test.did_hit) {
          return RayTerrainTest.hit(model_test.color, model_test.depth_t);
        }
      }
      dda.step();
      i += 1;
    }

    // Return for chunk grid background
    // return RayTerrainTest.hit(float3(i / 512.0), 100000.0);
    return RayTerrainTest.miss();
  }
}


}  // namespace voxel
