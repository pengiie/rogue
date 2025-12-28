implementing lib;

namespace voxel {

public struct RayTerrainTest {
  public bool did_hit;
  public float3 color;
  public float3 normal;
  public float depth_t;

  static RayTerrainTest miss() {
    return RayTerrainTest(false, float3(0.0), float3(0.0), 0.0);
  }

  static RayTerrainTest hit() {
    return RayTerrainTest(true, float3(0.0), float3(0.0), 0.0);
  }
}

public static const uint AIR_PTR = 0xFFFFFFFE;

public struct Terrain {
  // Allocated region trees.
  ByteAddressBuffer region_data;
  // 3D array of pointers to ptrs in region_data.
  ByteAddressBuffer region_ptrs_window;
  // -XYZ corner anchor of the region.
  int3 region_anchor;
  // Side length in regions.
  uint3 side_length;
  // Offset of the sliding window to determine the local-space (0,0,0) relative
  // to the data.
  uint3 region_offset;

  public static Terrain get() { return u_frame.voxel.terrain; }

  // Traverses the region tree and returns the voxel model info ptr
  // that was hit, if any.
  public Optional<uint> traverse_region_tree(Ray ray, uint region_ptr) {

  }

  // World-space AABB corresponding to the entire renderable terrain's bounds.
  public AABB aabb() {
    let meter_side_length = consts::TERRAIN_REGION_METER_LENGTH * this.side_length;
    let min = (float3) this.region_anchor * consts::TERRAIN_REGION_METER_LENGTH;
    let max = min + meter_side_length;
    return AABB.new_min_max(min, max);
  }
  
  public uint calc_region_index(uint3 local_region_pos) {
    return local_region_pos.x + 
           local_region_pos.y * this.side_length.x + 
           local_region_pos.z * (this.side_length.x * this.side_length.y);
  }

  public RayTerrainTest trace(Ray ray) {
    let aabb = this.aabb();
    var dda = RayDDA(ray, aabb, this.side_length);
    while (dda.in_bounds()) {
      // The region position we are taking into account the current sliding window.
      let curr_region_pos = (dda.curr_grid_pos() + this.region_offset) % this.side_length;
      let region_ptr_index = this.calc_region_index(curr_region_pos);
      let region_ptr = this.region_ptrs_window.Load<uint>(region_ptr_index << 2);

      dda.step();
    }

    return RayTerrainTest.miss();
  }
}

//public struct Terrain {
//  public uint side_length;
//  uint volume;
//  public int3 anchor;
//  uint3 window_offset;
//  ByteAddressBuffer data;
//
//  // World-space AABB corresponding to the entire terrain's bounds.
//  [ForceInline]
//  public AABB aabb() {
//    let meter_side_length = consts::TERRAIN_CHUNK_METER_LENGTH * this.side_length;
//    let min = (float3) this.anchor * consts::TERRAIN_CHUNK_METER_LENGTH;
//    let max = min + meter_side_length;
//    return AABB.new_min_max(min, max);
//  }
//
//  public static uint get_chunk_index(uint3 pos) {
//      let side_length = u_frame.voxel.terrain.side_length;
//      return pos.x + 
//             pos.y * side_length + 
//             pos.z * (side_length * side_length);
//  }
//
//  public static uint get_chunk_ptr(uint3 pos) {
//      let terrain = u_frame.voxel.terrain;
//      let window_pos = (uint3) ((pos + terrain.window_offset) % terrain.side_length);
//      let index = Terrain.get_chunk_index(window_pos);
//      let chunk_model_ptr = terrain.data.Load<uint>(index << 2);
//      return chunk_model_ptr;
//  }
//
//  public static RayTerrainTest trace(Ray ray) {
//    let terrain = u_frame.voxel.terrain;
//    let terrain_aabb = terrain.aabb();
//
//    let ray_terrain = ray.test_aabb(terrain_aabb);
//    if (!ray_terrain.hit) {
//      return RayTerrainTest.miss();
//    }
//
//    var dda = RayDDA(ray.advance_clone(ray_terrain.t_enter),
//                     terrain_aabb,
//                     terrain.side_length);
//    var i = 0;
//    while (dda.in_bounds() && i < 1024 /* just in case */) {
//      let curr_grid_pos = (uint3) ((dda.curr_grid_pos() + terrain.window_offset) % terrain.side_length);
//      var curr_index = curr_grid_pos.x + 
//                       curr_grid_pos.y * terrain.side_length + 
//                       curr_grid_pos.z * (terrain.side_length * terrain.side_length);
//
//      let chunk_model_ptr = terrain.data.Load<uint>(curr_index << 2);
//      if (chunk_model_ptr != AIR_PTR /*Treat as air for now*/ && chunk_model_ptr != INVALID_PTR) {
//        if (chunk_model_ptr == INVALID_PTR) {
//          // TODO: Request chunk from cpu.
//        }
//        let chunk_offset = consts::TERRAIN_CHUNK_METER_LENGTH * ((float3) dda.curr_grid_pos() + 0.5);
//        let chunk_center = terrain_aabb.min() + chunk_offset; 
//        let chunk_aabb = AABB.new_center_extents(chunk_center, 
//                                                 float3(consts::TERRAIN_CHUNK_METER_LENGTH) * 0.5);
//
//        let model_test = trace_model(ray, chunk_aabb, chunk_model_ptr, matrix3x3(1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0));
//        if (model_test.did_hit) {
//          return RayTerrainTest.hit(model_test.color, model_test.normal, model_test.depth_t);
//        }
//      }
//      dda.step();
//      i += 1;
//    }
//
//    // Return for chunk grid background
//    // return RayTerrainTest.hit(float3(i / 512.0), 100000.0);
//    return RayTerrainTest.miss();
//  }
//}


}  // namespace voxel
