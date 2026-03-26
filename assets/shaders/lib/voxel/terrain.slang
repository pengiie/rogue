implementing lib;

namespace voxel {

public struct RayTerrainTest {
  public bool did_hit;
  public float3 albedo;
  public float3 normal;
  public float3 face_normal;
  public float depth_t;

  static RayTerrainTest miss() {
    return RayTerrainTest(false, float3(0.0), float3(0.0), 0.0);
  }

  static RayTerrainTest hit() {
    return RayTerrainTest(true, float3(1.0), float3(0.0), 0.0);
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
    return none;
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
    let ray_aabb = ray.test_aabb(aabb);
    if (!ray_aabb.hit) {
      return RayTerrainTest.miss();
    }

    var dda = RayDDA(ray, aabb, this.side_length);
    while (dda.in_bounds()) {
      let curr_grid_pos = dda.curr_grid_pos();
      // The region position we are taking into account the current sliding window.
      let curr_region_pos = (curr_grid_pos + this.region_offset) % this.side_length;
      let region_ptr_index = this.calc_region_index(curr_region_pos);
      let region_ptr = this.region_ptrs_window.Load<uint>(region_ptr_index << 2);
      if (region_ptr != 0xFFFFFFFF) {
        let region = Region.new(region_ptr);
        let region_aabb = AABB.new_min_max(
                              aabb.min() + ((float3) curr_grid_pos * consts::TERRAIN_REGION_METER_LENGTH),
                              aabb.min() + ((float3) (curr_grid_pos + 1) * consts::TERRAIN_REGION_METER_LENGTH));
        let region_result = region.trace(ray, region_aabb);
        if (region_result.did_hit) {
          return region_result;
        }
      }

      dda.step();
    }

    return RayTerrainTest.miss();
  }

  public static int3 chunk_to_region_pos(int3 chunk_pos) {
    return (int3) floor((float3) chunk_pos / (float) consts::TERRAIN_REGION_CHUNK_LENGTH);
  }

  public static int3 voxel_to_region_pos(int3 voxel_pos) {
    return (int3) floor((float3) voxel_pos / (float) consts::TERRAIN_REGION_VOXEL_LENGTH);
  }

  public static int3 voxel_to_chunk_pos(int3 voxel_pos) {
    return (int3) floor((float3) voxel_pos / (float) consts::TERRAIN_CHUNK_VOXEL_LENGTH);
  }

  bool in_bounds(int3 local_chunk_pos) {
    return local_chunk_pos.x >= 0 && local_chunk_pos.y >= 0 && 
           local_chunk_pos.z >= 0 && local_chunk_pos.x < this.side_length.x && 
           local_chunk_pos.y < this.side_length.y && local_chunk_pos.z < this.side_length.z;
  }

  public bool is_voxel_present(int3 voxel_pos) {
    let terrain = u_frame.voxel.terrain;
    let region_pos = this.voxel_to_region_pos(voxel_pos);
    let local_region_pos = region_pos - terrain.region_anchor;
    if (!terrain.in_bounds(local_region_pos)) {
      return false;
    }
    let mem_region_pos = (local_region_pos + terrain.region_offset) % terrain.side_length;
    let region_ptr_index = this.calc_region_index(mem_region_pos);
    let region_ptr = terrain.region_ptrs_window.Load<uint>(region_ptr_index << 2);
    if (region_ptr == 0xFFFFFFFF) {
      return false;
    }
    
    let region = Region.new(region_ptr);
    return region.is_voxel_present(voxel_pos);
  }

  public Optional<CompressedVoxelMaterial> get_voxel_material(int3 chunk_pos, int chunk_height, uint3 local_voxel_pos) {
    let terrain = u_frame.voxel.terrain;
    let region_pos = this.chunk_to_region_pos(chunk_pos);
    let local_region_pos = region_pos - terrain.region_anchor;
    if (!terrain.in_bounds(local_region_pos)) {
      return none;
    }
    let mem_region_pos = (local_region_pos + terrain.region_offset) % terrain.side_length;
    let region_ptr_index = this.calc_region_index(mem_region_pos);
    let region_ptr = terrain.region_ptrs_window.Load<uint>(region_ptr_index << 2);
    if (region_ptr == 0xFFFFFFFF) {
      return none;
    }
    
    let region = Region.new(region_ptr);
    let local_chunk_pos = (uint3) (chunk_pos % consts::TERRAIN_REGION_CHUNK_LENGTH);
    return region.get_voxel_material(local_chunk_pos, chunk_height, local_voxel_pos);
  }

  public void set_voxel_material(int3 chunk_pos, int chunk_height, uint3 local_voxel_pos, CompressedVoxelMaterial comp_mat) {
    let terrain = u_frame.voxel.terrain;
    let region_pos = this.chunk_to_region_pos(chunk_pos);
    let local_region_pos = region_pos - terrain.region_anchor;
    if (!terrain.in_bounds(local_region_pos)) {
      return;
    }
    let mem_region_pos = (local_region_pos + terrain.region_offset) % terrain.side_length;
    let region_ptr_index = this.calc_region_index(mem_region_pos);
    let region_ptr = terrain.region_ptrs_window.Load<uint>(region_ptr_index << 2);
    if (region_ptr == 0xFFFFFFFF) {
      return;
    }
    
    let region = Region.new(region_ptr);
    let local_chunk_pos = (uint3) (chunk_pos % consts::TERRAIN_REGION_CHUNK_LENGTH);
    region.set_voxel_material(local_chunk_pos, chunk_height, local_voxel_pos, comp_mat);
  }
}

public struct Region {
  uint root_node_ptr;
  uint height;
  uint side_length;
  uint quarter_side_length;
  uint lod;

  static Region new(uint in_region_ptr) {
    Region region;
    region.root_node_ptr = in_region_ptr;
    let lod = u_frame.voxel.terrain.region_data.Load<uint>(in_region_ptr << 2);
    region.lod = lod;
    region.side_length = consts::TERRAIN_REGION_CHUNK_LENGTH >> (region.lod * 2);
    region.height = (firstbitlow(region.side_length) / 2) - 1;
    region.quarter_side_length = region.side_length >> 2;
    return region;
  }

  bool in_bounds(int3 global_grid) {
    return global_grid.x >= 0 && global_grid.y >= 0 && 
           global_grid.z >= 0 && global_grid.x < this.side_length && 
           global_grid.y < this.side_length && global_grid.z < this.side_length;
  }

  uint load_node(uint node_idx) {
    return u_frame.voxel.terrain.region_data.Load<uint>((this.root_node_ptr + node_idx + /*header_offset*/1) << 2);
  }

  RayTerrainTest trace(Ray in_ray, AABB root_aabb) {
    let root_hit_info = ray_aabb_test(in_ray, root_aabb);
    if (!root_hit_info.hit) {
      return RayTerrainTest.miss();
    }
    let ray = in_ray.advance_clone(root_hit_info.t_enter);
    
    // Local-space position with the origin being the AABB minimum.
    let relative_model_pos = ray.origin - root_aabb.min();
    // Normalize the local-space to [0, 1), mapping from the AABB side length.
    let norm_model_pos = clamp(relative_model_pos / root_aabb.side_length(),
                               float3(0.0),
                               float3(0.99999));
    // Turn the local-space position from [1, 1) to [0, this.side_length).
    // Where side length is dependent on the LOD of the region.
    // TODO: Figure out the best way to clamp right before this.side_length 
    // without floating point errors.
    let norm_grid_pos = norm_model_pos * (float3) this.side_length;
    let norm_ray = Ray(norm_grid_pos, ray.dir, ray.inv_dir);

    var curr_node_idx = 0;
    uint curr_node_stack[8];
    var curr_height = 0;
    var curr_ray = norm_ray;
    // The local grid position in an octant with the range [0, 3].
    var curr_local_grid = ((int3) floor(curr_ray.origin)) >> (this.height * 2);
    var curr_anchor_grid = int3(0);

    let unit_grid = (int3) sign(ray.dir);
    var last_mask = root_hit_info.t_enter == root_hit_info.t_min;
    var was_last_leaf = false;
    // If we traced a model ptr at some point in this
    // nodes children so we don't test a higher lod.
    var did_test_node = false;
    var was_last_model_leaf = false;

    var i = 0;
    while(i < 100) {
      i += 1;
      let should_pop = curr_local_grid.x < 0 || curr_local_grid.y < 0 ||
                       curr_local_grid.z < 0 || curr_local_grid.x > 3 ||
                       curr_local_grid.y > 3 || curr_local_grid.z > 3;
      if (should_pop || was_last_leaf) {
        let model_ptr = this.load_node(curr_node_idx);
        if (model_ptr != 0xFFFFFFFF && !was_last_leaf) {
          let chunk_meter_length = consts::TERRAIN_CHUNK_METER_LENGTH * pow(4, 3 - curr_height);
          let model_min = root_aabb.min() + ((float3) curr_anchor_grid * (chunk_meter_length / 4));
          let model_max = model_min + (float3) chunk_meter_length;
          let model_aabb = AABB.new_min_max(model_min, model_max);
          let model_test = trace_model(ray, model_aabb, model_ptr);
          // TODO: Test depth against higher lod? no because never see higher lod and then lower lod.
          if (model_test.did_hit) {
            var res = RayTerrainTest.hit();
            let global_grid_pos = (int3) floor(curr_ray.origin);
            res.albedo = model_test.albedo;
            res.normal = model_test.normal;
            res.face_normal = model_test.face_normal;
            return res;
          }
          did_test_node = true;
        }

        if (curr_height == 0) {
          break;
        }

        if (was_last_leaf) {
            // compare this and if outside of bounds then break ;
        }

        if (did_test_node || was_last_leaf) {
            if (!this.in_bounds((int3) floor(curr_ray.origin))) {
              break;
            }
        }

        curr_height -= 1;
        curr_node_idx = curr_node_stack[curr_height];
        // The - 1 is important here because
        curr_local_grid = curr_anchor_grid >> ((this.height - curr_height) * 2); 
        curr_local_grid &= 3;
        curr_anchor_grid = (curr_anchor_grid >> ((this.height - curr_height + 1) * 2)) 
                             << ((this.height - curr_height + 1) * 2);
      } else {
        uint model_ptr = this.load_node(curr_node_idx);
        if (curr_height != this.height + 1) {
          uint child_ptr = this.load_node(curr_node_idx + 1);
          if (child_ptr != 0xFFFFFFFF) {
            // Check for child node and push if that child exists.
            uint children[2] = {this.load_node(curr_node_idx + 2), this.load_node(curr_node_idx + 3)};
            let morton = morton_encode((uint3) curr_local_grid);
            let morton_half = morton / 32;
            let child_bit = 1 << (morton % 32);
            let children_half = children[morton_half];
            let is_child_present = (children_half & child_bit) > 0;
            if (is_child_present) {
              let curr_node_size = this.quarter_side_length >> (curr_height * 2);
              curr_node_stack[curr_height] = curr_node_idx;
              curr_node_idx = (child_ptr + morton) * /*node struct size for regions*/ 4;
              curr_height += 1;

              curr_anchor_grid += curr_local_grid * curr_node_size;

              int3 curr_global_grid = (int3) curr_ray.origin;
              curr_global_grid = clamp(curr_global_grid, curr_anchor_grid, curr_anchor_grid + curr_node_size - 1);

              curr_local_grid = curr_global_grid >> ((this.height - curr_height) * 2); 
              curr_local_grid &= 3;
              continue;
            } else {
              was_last_model_leaf = false;
            }
          }
        } else {
          if (model_ptr != 0xFFFFFFFF) {
            //if (curr_height == 2) {
            //  var hit = RayTerrainTest.hit();
            //  hit.color = float3(0.2, 1.0, 0.2); 
            //  return hit;
            //}
            let chunk_meter_length = consts::TERRAIN_CHUNK_METER_LENGTH * pow(4, this.lod);
            let model_aabb = AABB.new_min_max(
                                  root_aabb.min() + ((float3) curr_anchor_grid  * chunk_meter_length),
                                  root_aabb.min() + ((float3) (curr_anchor_grid + 1) * chunk_meter_length));
            let model_test = trace_model(ray, model_aabb, model_ptr);
            if (model_test.did_hit) {
              var res = RayTerrainTest.hit();
              let global_grid_pos = (int3) floor(curr_ray.origin);
              res.albedo = model_test.albedo;
              res.normal = model_test.normal;
              res.face_normal = model_test.face_normal;
              res.depth_t = model_test.ray_t + root_hit_info.t_enter;
              return res;
            }
            did_test_node = true;
          }
            was_last_leaf = true;
            let next_point = curr_anchor_grid + max(unit_grid, 0);
            let curr_t = ray_point_test(curr_ray, next_point);
            let next_t = min3(curr_t.x, curr_t.y, curr_t.z);
            curr_ray.advance(next_t + 0.00001);
          continue;
        }
      }

      let curr_node_size = this.quarter_side_length >> (curr_height * 2);
      let next_point = curr_anchor_grid + curr_local_grid * curr_node_size + max(unit_grid, 0) * curr_node_size;
      let curr_t = ray_point_test(curr_ray, next_point);
      let next_t = min3(curr_t.x, curr_t.y, curr_t.z);
      let mask = curr_t == next_t;
      last_mask = mask;
      curr_local_grid += int3(mask) * unit_grid; 
      curr_ray.advance(next_t + 0.0001);
      was_last_leaf = false;
    }

    return RayTerrainTest.miss();
  }

  public void set_voxel_material(uint3 local_chunk_pos, int chunk_height, uint3 local_voxel_pos, CompressedVoxelMaterial comp_mat) {
    if (chunk_height > this.height + 1) {
      return;
    }

    var curr_node_idx = 0;
    for (int curr_height = 0; curr_height <= chunk_height; curr_height++) {
      if (curr_height == chunk_height) {
        uint model_ptr = this.load_node(curr_node_idx);
        if (model_ptr == 0xFFFFFFFF) {
          return;
        }
        model_set_voxel_material(local_voxel_pos, model_ptr, comp_mat);
        return;
      }

      uint child_ptr = this.load_node(curr_node_idx + 1);
      if (child_ptr == 0xFFFFFFFF) {
        return;
      }

      
      var child_pos = local_chunk_pos >> ((consts::TERRAIN_REGION_TREE_HEIGHT - curr_height - 1) * 2);
      child_pos &= 3;
      let child_index = morton_encode(child_pos);
      // Check for child node and push if that child exists.
      uint children[2] = {this.load_node(curr_node_idx + 2), this.load_node(curr_node_idx + 3)};
      let morton = morton_encode(child_pos);
      let morton_half = morton / 32;
      let child_bit = 1 << (morton % 32);
      let children_half = children[morton_half];
      let is_child_present = (children_half & child_bit) > 0;
      if (!is_child_present) {
        return;
      }

      curr_node_idx = (child_ptr + child_index) * 4;
    }
  }

  public Optional<CompressedVoxelMaterial> get_voxel_material(uint3 local_chunk_pos, 
                                                              int chunk_height, 
                                                              uint3 local_voxel_pos) {
    if (chunk_height > this.height + 1) {
      return none;
    }

    var curr_node_idx = 0;
    for (int curr_height = 0; curr_height <= chunk_height; curr_height++) {
      if (curr_height == chunk_height) {
        uint model_ptr = this.load_node(curr_node_idx);
        if (model_ptr == 0xFFFFFFFF) {
          return none;
        }
        return model_get_voxel_material(local_voxel_pos, model_ptr);
      }

      uint child_ptr = this.load_node(curr_node_idx + 1);
      if (child_ptr == 0xFFFFFFFF) {
        return none;
      }

      
      var child_pos = local_chunk_pos >> ((consts::TERRAIN_REGION_TREE_HEIGHT - curr_height - 1) * 2);
      child_pos &= 3;
      let child_index = morton_encode(child_pos);
      // Check for child node and push if that child exists.
      uint children[2] = {this.load_node(curr_node_idx + 2), this.load_node(curr_node_idx + 3)};
      let morton_half = child_index / 32;
      let child_bit = 1 << (child_index % 32);
      let children_half = children[morton_half];
      let is_child_present = (children_half & child_bit) > 0;
      if (!is_child_present) {
        return none;
      }

      curr_node_idx = (child_ptr + child_index) * 4;
    }

    return none;
  }

  public bool is_voxel_present(int3 voxel_pos) {
    let local_chunk_pos = Terrain.voxel_to_chunk_pos(voxel_pos) % consts::TERRAIN_REGION_CHUNK_LENGTH;
    var prev_node_idx = 0;
    var curr_node_idx = 0;
    var curr_height = 0;
    var hit_leaf = false;
    while (true) {
      if (curr_height < consts::TERRAIN_REGION_TREE_HEIGHT && !hit_leaf) {
        uint child_ptr = this.load_node(curr_node_idx + 1);
        if (child_ptr != 0xFFFFFFFF) {
          var child_pos = (uint3) (local_chunk_pos >> ((consts::TERRAIN_REGION_TREE_HEIGHT - curr_height - 1) * 2));
          child_pos &= 3;
          let child_index = morton_encode(child_pos);
          // Check for child node and push if that child exists.
          uint children[2] = {this.load_node(curr_node_idx + 2), this.load_node(curr_node_idx + 3)};
          let morton_half = child_index / 32;
          let child_bit = 1 << (child_index % 32);
          let children_half = children[morton_half];
          let is_child_present = (children_half & child_bit) > 0;
          if (is_child_present) {
            curr_node_idx = (child_ptr + child_index) * 4;
            curr_height += 1;
            continue;
          }
        }
      }

      let model_ptr = this.load_node(curr_node_idx);
      if (model_ptr != 0xFFFFFFFF) {
        let voxel_scaling = 1 << ((consts::TERRAIN_REGION_TREE_HEIGHT - curr_height) * 2);
        let scaled_pos = (voxel_pos / voxel_scaling);
        let voxel_local_pos = (uint3) (scaled_pos % consts::TERRAIN_CHUNK_VOXEL_LENGTH);
        return model_is_voxel_present(voxel_local_pos, model_ptr);
      }

      if (curr_height == 0) {
        return false;
      }
      curr_height -= 1;
      hit_leaf = true;
      return false;
    }
    return false;
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
