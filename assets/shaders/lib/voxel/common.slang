implementing lib;

namespace voxel {

public interface IVoxelModel {

}

const static uint INVALID_PTR = 0xFFFFFFFF;
public struct Terrain {
  public uint side_length;
  uint volume;
  ByteAddressBuffer data;

  // World-space AABB corresponding to the entire terrain's bounds.
  [ForceInline]
  public AABB aabb() {
    let meter_side_length = consts::TERRAIN_CHUNK_METER_LENGTH * this.side_length * 0.5;
    return AABB.new_center_extents(float3(0.0), float3(meter_side_length));
  }

  public static RayTerrainTest trace(Ray ray) {
    let terrain = u_frame.voxel.terrain;
    let terrain_aabb = terrain.aabb();

    let ray_terrain = ray.test_aabb(terrain_aabb);
    //if (!ray_terrain.hit) {
    //  return RayTerrainTest.miss();
    //}

    var dda = RayDDA(ray.advance_clone(ray_terrain.t_enter),
                     terrain_aabb,
                     terrain.side_length);
    var i = 0;
    while (dda.in_bounds() && i < 1024 /* just in case */) {
      let curr_morton = dda.curr_morton();
      let chunk_model_ptr = terrain.data.Load<uint>(curr_morton << 2);
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

    return RayTerrainTest.miss();
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
    default:
  }

  return RayModelTest.miss();
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

} // namespace voxel
