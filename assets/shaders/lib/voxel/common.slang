implementing lib;

namespace voxel {

public interface IVoxelModel {

}

const static uint INVALID_PTR = 0xFFFFFFFF;
public struct Terrain {
  public uint side_length;
  uint volume;
  int3 anchor;
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
      if (chunk_model_ptr != INVALID_PTR) {
        let chunk_offset = consts::TERRAIN_CHUNK_METER_LENGTH * ((float3) dda.curr_grid_pos() + 0.5);
        let chunk_center = terrain_aabb.min() + chunk_offset; 
        let chunk_aabb = AABB.new_center_extents(chunk_center, 
                                                 float3(consts::TERRAIN_CHUNK_METER_LENGTH) * 0.5);

        let model_test = trace_model(ray, chunk_aabb, chunk_model_ptr);
        if (model_test.did_hit) {

          return RayTerrainTest.hit(model_test.color, model_test.depth_t);
        }
      }
      dda.step();
      i += 1;
    }

     return RayTerrainTest.hit(float3(i / 512.0), 0.0);
  }
}

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

public struct RayModelTest {
  public bool did_hit;
  public float3 color;
  public float depth_t;

  static RayModelTest miss() {
    return RayModelTest(false, float3(0.0), 0.0);
  }

  static RayModelTest hit(float3 color, float depth_t) {
    return RayModelTest(true, color, depth_t);
  }
}

public RayModelTest trace_model(Ray ray, AABB aabb, uint voxel_model_ptr) {
  let info_data = u_frame.voxel.model_info_data;
  let model_schema = info_data.Load<uint>(voxel_model_ptr << 2);
  switch (model_schema) {
    case consts::VOXEL_MODEL_SCHEMA_ESVO:
      let esvo = ESVO(aabb, voxel_model_ptr);
      return esvo.trace(ray);
    case consts::VOXEL_MODEL_SCHEMA_THC:
      let thc = THC(aabb, voxel_model_ptr);
      return thc.trace(ray);
    case consts::VOXEL_MODEL_SCHEMA_FLAT:
      let flat = Flat(aabb, voxel_model_ptr);
      return flat.trace(ray);
  }

  return RayModelTest.miss();
}

public struct Flat {
  AABB root_aabb;
  uint voxel_model_ptr;
  uint3 side_length; 
                        
  uint presence_data_ptr;

  public __init(AABB in_aabb, uint in_voxel_model_ptr) {
    let info_data = u_frame.voxel.model_info_data;
    
    root_aabb = in_aabb;
    voxel_model_ptr = in_voxel_model_ptr;
    side_length = uint3(
      info_data.Load<uint>((voxel_model_ptr + 1) << 2),
      info_data.Load<uint>((voxel_model_ptr + 2) << 2),
      info_data.Load<uint>((voxel_model_ptr + 3) << 2)
    );
    presence_data_ptr = info_data.Load<uint>((voxel_model_ptr + 4) << 2);
  }

  public RayModelTest trace(Ray in_ray) {
    let root_hit_info = ray_aabb_test(in_ray, this.root_aabb);

    var dda = RayDDA(in_ray.advance_clone(root_hit_info.t_enter),
                     this.root_aabb,
                     this.side_length);
    var i = 0;
    while (dda.in_bounds() && i < 4321) {
      let grid_pos = dda.curr_grid_pos();
      let voxel_index = grid_pos.x + this.side_length.x * grid_pos.y + (this.side_length.x * this.side_length.y) * grid_pos.z;
      let presence_u32 = u_frame.voxel.model_voxel_data.Load<uint>((this.presence_data_ptr + (voxel_index / 32)) << 2);
      let is_present = (presence_u32 & (1 << (voxel_index % 32))) > 0;
      if (is_present) {
        if (let pt_material = this.load_ptmaterial(voxel_index)) {
          //printf("Color is %f %f %f\n", pt_material.x, pt_material.y, pt_material.z);
          return RayModelTest.hit(pt_material, 0.0);
        }
        return RayModelTest.hit(float3(0.5), 0.0);
      }

      dda.step();
      i += 1;
    }

    return RayModelTest.miss();
  }

  private Optional<uint> load_attachment_ptr(uint attachment_index, uint voxel_index) {
    let info_data = u_frame.voxel.model_info_data;
    let attachment_presence_ptr = info_data.Load<uint>((this.voxel_model_ptr + 5 + attachment_index) << 2);
    if (attachment_presence_ptr == 0xFFFFFFFF) {
      return none;
    }

    let attachment_raw_ptr = info_data.Load<uint>((voxel_model_ptr + 5 + consts::VOXEL_ATTACHMENT_COUNT + attachment_index) << 2);
    if (attachment_raw_ptr == 0xFFFFFFFF) {
      return none;
    }

    let attachment_mask = u_frame.voxel.model_voxel_data.Load<uint>((attachment_presence_ptr + (voxel_index / 32)) << 2);
    let is_attachment_present = (attachment_mask & (1 << (voxel_index % 32))) > 0;
    if (!is_attachment_present) {
      return none;
    }

    return attachment_raw_ptr + voxel_index;
  }

  private Optional<float3> load_ptmaterial(uint voxel_index) {
    let ptmaterial_data_ptr_opt = this.load_attachment_ptr(0, voxel_index);
    if (ptmaterial_data_ptr_opt == none) {
      return none;
    }
    let ptmaterial_data_ptr = ptmaterial_data_ptr_opt.value;
    let compressed_ptmaterial = u_frame.voxel.model_voxel_data.Load<uint>(ptmaterial_data_ptr << 2);
    if (testglobal) {
      //printf("mat is %u\n", compressed_ptmaterial);
    }

    return color::srgb_to_lsrgb(float3(
      (float) ((compressed_ptmaterial >> 16) & 0xFF) / 255.0,
      (float) ((compressed_ptmaterial >> 8) & 0xFF) / 255.0,
      (float) (compressed_ptmaterial & 0xFF) / 255.0,
    ));
  }
}

struct ESVOStackItem {
  uint node_index; 
  uint octant;
}

public struct ESVO {
  AABB root_aabb;
  uint voxel_model_ptr;
  uint node_data_ptr;

  public __init(AABB in_aabb, uint in_voxel_model_ptr) {
    let info_data = u_frame.voxel.model_info_data;

    root_aabb = in_aabb;
    voxel_model_ptr = in_voxel_model_ptr;
    node_data_ptr = info_data.Load<uint>((voxel_model_ptr + 1) << 2);
  }

  private static uint next_octant_morton(Ray ray, float3 octant_center, float t_enter) {
    let t_center = ray_point_test(ray, octant_center);

    var octant = 0u;
    if (t_center.x <= t_enter) {
      if (ray.dir.x >= 0.0) {
        octant |= 1u;
      }
    } else {
      if (ray.dir.x < 0.0) {
        octant |= 1u;
      }
    }

    if (t_center.y <= t_enter) {
      if (ray.dir.y >= 0.0) {
        octant |= 2u;
      }
    } else {
      if (ray.dir.y < 0.0) {
        octant |= 2u;
      }
    }

    if (t_center.z <= t_enter) {
      if (ray.dir.z >= 0.0) {
        octant |= 4u;
      }
    } else {
      if (ray.dir.z < 0.0) {
        octant |= 4u;
      }
    }

    return octant;
  }

  private static AABB next_octant_aabb(AABB parent_aabb, uint octant_morton) {
    let signs = float3(
      ((float) (octant_morton & 1)) * 2.0 - 1.0,
      ((float) (octant_morton & 2)) - 1.0,
      ((float) (octant_morton & 4)) * 0.5 - 1.0,
    );
    let octant_half_side_length = parent_aabb.half_side_length * 0.5;
    return AABB.new_center_extents(parent_aabb.center + octant_half_side_length * signs,
                                   octant_half_side_length);
  }

  private static AABB next_octant_parent_aabb(AABB octant_aabb, uint octant_morton) {
    let signs = float3(
      ((float) (octant_morton & 1)) * -2.0 + 1.0,
      ((float) (octant_morton & 2)) * -1.0 + 1.0,
      ((float) (octant_morton & 4)) * -0.5 + 1.0,
    );
    let parent_half_side_length = octant_aabb.half_side_length * 2;
    return AABB.new_center_extents(octant_aabb.center + octant_aabb.half_side_length * signs,
                                   parent_half_side_length);
  }

  private static bool should_pop(Ray ray, uint octant_morton, uint advance_morton) {
    if (ray.dir.x < 0 && ((advance_morton & 1) > (octant_morton & 1))) {
      return (true);
    }
    if (ray.dir.y < 0 && ((advance_morton & 2) > (octant_morton & 2))) {
      return true;
    }
    if (ray.dir.z < 0 && ((advance_morton & 4) > (octant_morton & 4))) {
      return true;
    }

    if (ray.dir.x > 0 && ((advance_morton & 1) < (octant_morton & 1))) {
      return true;
    }
    if (ray.dir.y > 0 && ((advance_morton & 2) < (octant_morton & 2))) {
      return true;
    }
    if (ray.dir.z > 0 && ((advance_morton & 4) < (octant_morton & 4))) {
      return true;
    }

    return false;
  }

  public RayModelTest trace(Ray ray) {
    let root_hit_info = ray_aabb_test(ray, this.root_aabb);
 
                        
    var curr_octant = ESVO.next_octant_morton(ray, this.root_aabb.center, root_hit_info.t_enter);
    var curr_aabb = ESVO.next_octant_aabb(this.root_aabb, curr_octant);
    var curr_node_index = 1u; // 0u is our page header.
    var curr_node_data = this.load_node(curr_node_index);
    var curr_height = 0;
    var should_push = true;
    ESVOStackItem stack[8];

    for (uint i = 0; i < 1028; i++) {
      if (curr_height >= 8) {
        // Break early so we don't overwrite the bounds of our stack.
        return RayModelTest.hit(float3(1.0, 0.0, 0.0), 1.0);
      }
      let octant_hit_info = ray_aabb_test(ray, curr_aabb);

      let value_mask = (curr_node_data >> 8) & 0xFF;
      let is_octant_present = (value_mask & (0x1u << curr_octant)) > 0;
      if (should_push && is_octant_present) {
        let is_leaf = (curr_node_data & (0x1u << curr_octant)) > 0;
        if (is_leaf) { 
                        
          let depth_t = octant_hit_info.t_enter;
          // More vibrant.
          return RayModelTest.hit(color::srgb_to_lsrgb(float3((i * 12.0) / 1028.0, log((i * 16.0) / 1028.0), (i * 8.0) / 1028.0)), depth_t);
          //return RayModelTest.hit(float3(i / 1028.0), depth_t);
        }

        let children_offset = curr_node_data >> 17;
        let child_offset = countbits(((1 << curr_octant) - 1) & value_mask);

        stack[curr_height] = ESVOStackItem(curr_node_index, curr_octant);

        curr_octant = ESVO.next_octant_morton(ray, curr_aabb.center, octant_hit_info.t_enter);
        curr_aabb = ESVO.next_octant_aabb(curr_aabb, curr_octant);
        curr_node_index = curr_node_index + children_offset + child_offset;
        curr_node_data = this.load_node(curr_node_index);
        curr_height += 1;

        continue;
      }

      let exit_axis: bool3 = octant_hit_info.t_exit == octant_hit_info.t_max;
      let exit_morton = (uint) exit_axis.x | (((uint) exit_axis.y) << 1)
                                           | (((uint) exit_axis.z) << 2);
      let advance_morton = curr_octant ^ exit_morton;
      should_push = !ESVO.should_pop(ray, curr_octant, advance_morton);
      if (!should_push) {
        if (curr_height == 0) {
          break;
        }

        curr_height -= 1;
        let item = stack[curr_height];

        curr_aabb = ESVO.next_octant_parent_aabb(curr_aabb, curr_octant);
        curr_octant = item.octant;
        curr_node_index = item.node_index;
        curr_node_data = this.load_node(curr_node_index);
      } else {
        let advance_sign = (float3) exit_axis * sign(ray.dir);
        curr_aabb.center = curr_aabb.center + curr_aabb.side_length() * advance_sign;
        curr_octant = advance_morton;
      }
    }

    var color = float3(0.0);
    if ((curr_octant & 1) > 0) {
      color.r = 1.0;
    }
    if ((curr_octant & 2) > 0) {
      color.g = 1.0;
    }
    if ((curr_octant & 4) > 0) {
      color.b = 1.0;
    }

    return RayModelTest.miss();
  }

  private uint load_node(uint node_index) {
    return u_frame.voxel.model_voxel_data.Load<uint>(
      (this.node_data_ptr + node_index) << 2
    );
  }
}

public static bool testglobal = false;

public struct THC {
  AABB root_aabb;
  uint voxel_model_ptr;
  // Side length in voxels.
  uint side_length;
  uint quarter_side_length;
  // Height of the tree starting from 0.
  uint height;
  uint node_data_ptr;

  public __init(AABB in_aabb, uint in_voxel_model_ptr) {
    let info_data = u_frame.voxel.model_info_data;

    root_aabb = in_aabb;
    voxel_model_ptr = in_voxel_model_ptr;
    side_length = info_data.Load<uint>((voxel_model_ptr + 1) << 2);
    quarter_side_length = side_length >> 2;
    // `firstbitlow()` is equivalent to rust `u32::trailing_zeros()`.
    height = (firstbitlow(side_length) / 2) - 1;
    node_data_ptr = info_data.Load<uint>((voxel_model_ptr + 2) << 2);
  }

  private bool in_bounds(int3 global_grid) {
    return global_grid.x >= 0 && global_grid.y >= 0 && 
           global_grid.z >= 0 && global_grid.x < this.side_length && 
           global_grid.y < this.side_length && global_grid.z < this.side_length;
  }

  public RayModelTest trace(Ray in_ray) {
    let root_hit_info = ray_aabb_test(in_ray, this.root_aabb);
    if (!root_hit_info.hit) {
      return RayModelTest.miss();
    }
    let ray = in_ray.advance_clone(root_hit_info.t_enter);
    
    // Local-space position with the origin being the AABB minimum.
    let relative_model_pos = ray.origin - this.root_aabb.min();
    // Normalize the local-space to [0, 1), mapping from the AABB side length.
    let norm_model_pos = relative_model_pos / this.root_aabb.side_length();
    // Turn the local-space position from [0, 1) to [0, this.side_length).
    let norm_grid_pos = clamp(norm_model_pos * (float3) this.side_length, float3(0.0), (float3) (this.side_length - 1));
    let norm_ray = Ray(norm_grid_pos, ray.dir, ray.inv_dir);

    var curr_node_idx = 0;
    uint curr_node_stack[8];
    var curr_height = 0;
    // The local grid position in an octant with the range [0, 3].
    var curr_ray = norm_ray;
    var curr_local_grid = ((int3) floor(curr_ray.origin)) >> (this.height * 2);
    var curr_anchor_grid = int3(0);

    let unit_grid = (int3) sign(ray.dir);

    var i = 0;
    if (testglobal) {
      printf("INIT THE VOXEL!!!!!\n");
    }
    while(this.in_bounds((int3) floor(curr_ray.origin)) && i < 100) {
      if (testglobal) {
        //printf("next iter\n");
      }
      i += 1;
      let should_pop = curr_local_grid.x < 0 || curr_local_grid.y < 0 ||
                       curr_local_grid.z < 0 || curr_local_grid.x > 3 ||
                       curr_local_grid.y > 3 || curr_local_grid.z > 3;
      if (should_pop) {
        if (curr_height == 0) {
          //break;
          return RayModelTest.hit(float3(1.0, 0.0, 0.0), 0.0);
        }

        curr_height -= 1;
        curr_node_idx = curr_node_stack[curr_height];
        curr_local_grid = curr_anchor_grid >> ((this.height - curr_height) * 2); 
        curr_local_grid &= 3;
        let old_anchor = curr_anchor_grid;
        curr_anchor_grid = (curr_anchor_grid >> ((this.height - curr_height + 1) * 2)) << ((this.height - curr_height + 1) * 2);
        if (testglobal) {
          //printf("Popping height %u, prev anchor %d %d %d, new anchor %d %d %d, new local size %d %d %d\n", curr_height + 1, old_anchor.x, old_anchor.y, old_anchor.z,
          //curr_anchor_grid.x, curr_anchor_grid.y, curr_anchor_grid.z,
          //curr_local_grid.x, curr_local_grid.y, curr_local_grid.z);
        }
        // so we don't re-enter the same node we pop out of.
        //return RayModelTest.hit(float3(0.0, 0.0, 1.0), 0.0);
    //return RayModelTest.hit((float) curr_height / float3(3.0), 0.0);
      } else {
        uint children[2] = {this.load_node(curr_node_idx + 1), this.load_node(curr_node_idx + 2)};
        let morton = morton_encode((uint3) curr_local_grid);
        let morton_half = morton / 32;
        let child_bit = 1 << (morton % 32);
        let children_half = children[morton_half];
        let is_child_present = (children_half & child_bit) > 0;
        if (testglobal) {
          // printf("Curr node idx is %u\n", curr_node_idx);
          // printf("Curr local pos is %d %d %d\n", curr_local_grid.x, curr_local_grid.y, curr_local_grid.z);
        }
        if (is_child_present) {
          let child_ptr_raw = this.load_node(curr_node_idx);

          var child_offset = countbits(children_half & (child_bit - 1));
          child_offset += morton_half == 0 ? 0 : countbits(children[0]);

          let is_leaf_node = (child_ptr_raw >> 31) > 0;
          if (is_leaf_node) {
            if (let pt_material = this.load_ptmaterial(curr_node_idx, morton_half, child_bit, child_offset)) {
              //printf("Color is %f %f %f\n", pt_material.x, pt_material.y, pt_material.z);
              return RayModelTest.hit(pt_material, 0.0);
            }
            return RayModelTest.hit(float3(0.5), 0.0);
          }

          let curr_node_size = this.quarter_side_length >> (curr_height * 2);
          curr_node_stack[curr_height] = curr_node_idx;
          curr_node_idx = ((child_ptr_raw & 0x7FFFFFFF) + child_offset) * 3;
          curr_height += 1;

          curr_anchor_grid += curr_local_grid * curr_node_size;

          int3 curr_global_grid = (int3) curr_ray.origin;
          curr_global_grid = clamp(curr_global_grid, curr_anchor_grid, curr_anchor_grid + curr_node_size - 1);

          curr_local_grid = curr_global_grid >> ((this.height - curr_height) * 2); 
          curr_local_grid &= 3;
          continue;
        }
      }

      //var curr_grid_anchor = c >> ((this.height - curr_height) * 2);
      //curr_grid_anchor = (curr_grid_anchor >> 2) << 2;
      //curr_grid_anchor = curr_grid_anchor << ((this.height - curr_height) * 2);
      let curr_node_size = this.quarter_side_length >> (curr_height * 2);
      let next_point = curr_anchor_grid + curr_local_grid * curr_node_size + max(unit_grid, 0) * curr_node_size;
      let curr_t = ray_point_test(curr_ray, next_point);
      let next_t = min3(curr_t.x, curr_t.y, curr_t.z);
      let mask = curr_t == next_t;
      curr_local_grid += int3(mask) * unit_grid; 
      if (testglobal) {
        printf("OLD global: %f %f %f\n", curr_ray.origin.x, curr_ray.origin.y, curr_ray.origin.z);
      }
      curr_ray.advance(next_t + 0.001);
      if (testglobal) {
        printf("NEW global: %f %f %f\n", curr_ray.origin.x, curr_ray.origin.y, curr_ray.origin.z);
      }
    }

if (curr_height == 0) {
    return RayModelTest.hit(float3(0.0, 1.0, 0.0), 0.0);
}
    return RayModelTest.hit(float3((float) curr_height / 2.0), 0.0);
    //return RayModelTest.miss();
  }

  private uint load_node(uint node_index) {
    return u_frame.voxel.model_voxel_data.Load<uint>(
      (this.node_data_ptr + node_index) << 2
    );
  }

  private Optional<uint> load_attachment_ptr(uint attachment_index, uint node_index, uint morton_half, 
                                             uint child_bit, uint child_offset) {
    let info_data = u_frame.voxel.model_info_data;
    let attachment_lookup_ptr = info_data.Load<uint>((voxel_model_ptr + 3 + attachment_index) << 2);
    if (attachment_lookup_ptr == 0xFFFFFFFF) {
      return none;
    }

    let attachment_raw_ptr = info_data.Load<uint>((voxel_model_ptr + 3 + consts::VOXEL_ATTACHMENT_COUNT + attachment_index) << 2);
    if (attachment_raw_ptr == 0xFFFFFFFF) {
      return none;
    }

    let attachment_mask_half = u_frame.voxel.model_voxel_data.Load<uint>((attachment_lookup_ptr + node_index + 1 + morton_half) << 2);
    let is_attachment_present = (attachment_mask_half & child_bit) > 0;
    if (!is_attachment_present) {
      return none;
    }

    let node_attachment_data_ptr = u_frame.voxel.model_voxel_data.Load<uint>((attachment_lookup_ptr + node_index) << 2);
    if (testglobal) {
      // printf("node index is %u\n", node_index);
      // printf("pointer is %u\n", node_attachment_data_ptr);
    }
    return attachment_raw_ptr + node_attachment_data_ptr + child_offset;
  }

  private Optional<float3> load_ptmaterial(uint node_index, uint morton_half, uint child_bit, uint child_offset) {
    let ptmaterial_data_ptr_opt = this.load_attachment_ptr(0, node_index, morton_half, child_bit, child_offset);
    if (ptmaterial_data_ptr_opt == none) {
      return none;
    }
    let ptmaterial_data_ptr = ptmaterial_data_ptr_opt.value;
    let compressed_ptmaterial = u_frame.voxel.model_voxel_data.Load<uint>(ptmaterial_data_ptr << 2);
    if (testglobal) {
      //printf("mat is %u\n", compressed_ptmaterial);
    }

    return color::srgb_to_lsrgb(float3(
      (float) ((compressed_ptmaterial >> 16) & 0xFF) / 255.0,
      (float) ((compressed_ptmaterial >> 8) & 0xFF) / 255.0,
      (float) (compressed_ptmaterial & 0xFF) / 255.0,
    ));
  }
}

} // namespace voxel
