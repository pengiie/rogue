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
    if (!ray_terrain.hit) {
      return RayTerrainTest.miss();
    }

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
          return RayTerrainTest.hit(model_test.color);
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

  static RayTerrainTest miss() {
    return RayTerrainTest(false, float3(0.0));
  }

  static RayTerrainTest hit(float3 color) {
    return RayTerrainTest(true, color);
  }
}

public struct RayModelTest {
  public bool did_hit;
  public float3 color;

  static RayModelTest miss() {
    return RayModelTest(false, float3(0.0));
  }

  static RayModelTest hit(float3 color) {
    return RayModelTest(true, color);
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

  public RayModelTest trace(Ray ray) {
    let root_hit_info = ray_aabb_test(ray, this.root_aabb);

    var curr_octant = ESVO.next_octant_morton(ray, this.root_aabb.center, root_hit_info.t_enter);
    var curr_aabb = ESVO.next_octant_aabb(this.root_aabb, curr_octant);
    var curr_node_index = 1u; // 0u is our page header.
    var curr_node_data = this.load_node(curr_node_index);

    for (uint i = 0; i < 6; i++) {
      let value_mask = (curr_node_data >> 8);
      let is_octant_present = (value_mask & (0x1u << curr_octant)) > 0;
      if (is_octant_present) {
        let is_leaf = (curr_node_data & (0x1u << curr_octant)) > 0;
        if (is_leaf) {
          return RayModelTest.hit(float3(0.3));
        }

        let octant_hit_info = ray_aabb_test(ray, curr_aabb);
        let children_offset = curr_node_data >> 17;
        let child_offset = countbits(((1 << curr_octant) - 1) & value_mask);

        curr_octant = ESVO.next_octant_morton(ray, curr_aabb.center, octant_hit_info.t_enter);
        curr_aabb = ESVO.next_octant_aabb(curr_aabb, curr_octant);
        curr_node_index = curr_node_index + children_offset + child_offset;
        curr_node_data = this.load_node(curr_node_index);
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

    return RayModelTest.hit(color);
  }

  private uint load_node(uint node_index) {
    return u_frame.voxel.model_voxel_data.Load<uint>(
      (this.node_data_ptr + node_index) << 2
    );
  }
}

} // namespace voxel
