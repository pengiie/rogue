implementing lib;

namespace voxel {

// Size in uints.
static const uint SFT_NODE_SIZE = 5;

public struct SFTVoxelAddr {
  public uint node_index;
  public uint morton_half;
  public uint child_bit;
}

// Traces the same way as the THC, better as existing early on solid chunks.
public struct SFT {
  // Pointer to the voxel_model_info region.
  uint voxel_model_ptr; 
  // Side length in voxels.
  uint side_length;
  uint quarter_side_length;
  // Height of the tree starting from 0, therefore a 16 side_length 
  // would have a height of 1. Conventionally it would have a 
  // height of 2 but this height represents the jumps of nodes
  // we must traverse before we reach the pre-leaf nodes.
  uint height;
  VoxelDataRange node_data_ptr;

  public __init(uint in_voxel_model_ptr) {
    let info_data = u_frame.voxel.model_info_data;

    voxel_model_ptr = in_voxel_model_ptr;
    side_length = info_data.Load<uint>((voxel_model_ptr + 1) << 2);
    quarter_side_length = side_length >> 2;
    // `firstbitlow()` is equivalent to rust `u32::trailing_zeros()`.
    height = (firstbitlow(side_length) / 2) - 1;
    node_data_ptr = VoxelDataRange(info_data.Load<uint>((voxel_model_ptr + 2) << 2));
  }

  public bool in_bounds(int3 global_grid) {
    return global_grid.x >= 0 && global_grid.y >= 0 && 
           global_grid.z >= 0 && global_grid.x < this.side_length && 
           global_grid.y < this.side_length && global_grid.z < this.side_length;
  }

  public RayModelTest trace(Ray in_ray, AABB root_aabb, matrix3x3 rotation) {
    let root_hit_info = ray_aabb_test(in_ray, root_aabb);
    if (!root_hit_info.hit) {
      return RayModelTest.miss();
    }
    let ray = in_ray.advance_clone(root_hit_info.t_enter);
    
    // Local-space position with the origin being the AABB minimum.
    let relative_model_pos = ray.origin - root_aabb.min();
    // Normalize the local-space to [0, 1), mapping from the AABB side length.
    let norm_model_pos = clamp(relative_model_pos / root_aabb.side_length(),
                               float3(0.0),
                               float3(0.99999));
    // Turn the local-space position from [0, 1) to [0, this.side_length).
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

    var i = 0;
    while(this.in_bounds((int3) floor(curr_ray.origin)) && i < 200) {
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
        curr_anchor_grid = (curr_anchor_grid >> ((this.height - curr_height + 1) * 2)) 
                             << ((this.height - curr_height + 1) * 2);
      } else {
        uint children[2] = {this.load_node(curr_node_idx + 1), this.load_node(curr_node_idx + 2)};
        let morton = morton_encode((uint3) curr_local_grid);
        let morton_half = morton / 32;
        let child_bit = 1 << (morton % 32);
        let children_half = children[morton_half];
        let is_child_present = (children_half & child_bit) > 0;
        if (is_child_present) {
          let child_ptr_raw = this.load_node(curr_node_idx);

          var child_offset = countbits(children_half & (child_bit - 1));
          child_offset += morton_half == 0 ? 0 : countbits(children[0]);

          uint leaves[2] = {this.load_node(curr_node_idx + 3), this.load_node(curr_node_idx + 4)};
          let leaf_half = leaves[morton_half];
          let is_leaf_present = (leaf_half & child_bit) > 0;
          if (is_leaf_present) {
            let voxel_addr = SFTVoxelAddr(curr_node_idx, morton_half, child_bit);
            if (let pt_material = this.load_ptmaterial(voxel_addr)) {
              var color = pt_material;
              if (let normal = this.load_normal(voxel_addr)) {
                let rot_normal = mul(rotation, normal);
                let l = sqrt(dot(rot_normal, normalize(-float3(0.7,-1,-0.2))) * 0.5 + 0.5);
                color *= max(l, 0.2);
              }
              let next_point = curr_anchor_grid + curr_local_grid + max(unit_grid, 0);
              let next_t = ray_point_test(curr_ray, next_point);
              let min_t = min3(next_t.x, next_t.y, next_t.z);
              let voxel_scaling = (root_aabb.max() - root_aabb.min()) / (float3) this.side_length;
              let d = distance(in_ray.origin, root_aabb.min() + curr_ray.origin * voxel_scaling);
              return RayModelTest.hit(color, d);
            }
            return RayModelTest.hit(float3(0.5), 0.0);
          }

          let curr_node_size = this.quarter_side_length >> (curr_height * 2);
          curr_node_stack[curr_height] = curr_node_idx;
          curr_node_idx = (child_ptr_raw + child_offset) * SFT_NODE_SIZE;
          curr_height += 1;

          curr_anchor_grid += curr_local_grid * curr_node_size;

          int3 curr_global_grid = (int3) curr_ray.origin;
          curr_global_grid = clamp(curr_global_grid, curr_anchor_grid, curr_anchor_grid + curr_node_size - 1);

          curr_local_grid = curr_global_grid >> ((this.height - curr_height) * 2); 
          curr_local_grid &= 3;
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
    }

    return RayModelTest.miss();
  }

  public Optional<SFTVoxelAddr> get_voxel_addr(uint3 global_voxel_pos) {
    var curr_node_idx = 0;
    var morton_half = 0;
    var child_bit = 0;
    var curr_height = 0;
    var curr_local_grid = global_voxel_pos >> ((this.height) * 2);
    while (curr_height <= this.height) {
      uint children[2] = {this.load_node(curr_node_idx + 1), this.load_node(curr_node_idx + 2)};
      let morton = morton_encode((uint3) curr_local_grid);
      morton_half = morton / 32;
      child_bit = 1 << (morton % 32);
      let children_half = children[morton_half];
      let is_child_present = (children_half & child_bit) > 0;
      if (!is_child_present) {
        return none;
      }

      let child_ptr_raw = this.load_node(curr_node_idx);
      let is_leaf_node = (child_ptr_raw >> 31) > 0;
      if (is_leaf_node) {
        break;
      }

      var child_offset = countbits(children_half & (child_bit - 1));
      child_offset += morton_half == 0 ? 0 : countbits(children[0]);
      curr_node_idx = ((child_ptr_raw & 0x7FFFFFFF) + child_offset) * 3;
      curr_height += 1;
      curr_local_grid = global_voxel_pos >> ((this.height - curr_height) * 2);
      curr_local_grid &= 3;
    }

    return SFTVoxelAddr(curr_node_idx, morton_half, child_bit);
  }

  public uint get_side_length() {
    return this.side_length;
  }

  private uint load_node(uint node_index) {
    return this.node_data_ptr.load(node_index);
  }

  private Optional<Tuple<VoxelDataRange, VoxelDataRange>> load_attachment_ptrs(uint attachment_index) {
    let info_data = u_frame.voxel.model_info_data;
    let attachment_presence_ptr = info_data.Load<uint>((this.voxel_model_ptr + 3 + attachment_index) << 2);
    if (attachment_presence_ptr == 0xFFFFFFFF) {
      return none;
    }

    let attachment_raw_ptr = info_data.Load<uint>(
      (voxel_model_ptr + 3 + consts::VOXEL_ATTACHMENT_COUNT + attachment_index) << 2
    );
    if (attachment_raw_ptr == 0xFFFFFFFF) {
      return none;
    }

    return makeTuple(
        VoxelDataRange(attachment_presence_ptr), 
        VoxelDataRange(attachment_raw_ptr));
  }

  private Optional<VoxelDataRange> load_voxel_attachment(uint attachment_index, SFTVoxelAddr voxel_addr) {
    let attachment_ptr_pair = load_attachment_ptrs(attachment_index);
    if (attachment_ptr_pair == none) {
      return none;
    }
    let attachment_presence_ptr = attachment_ptr_pair.value._0;
    let attachment_raw_ptr = attachment_ptr_pair.value._1;

    uint attachment_mask[2] = {
      load_attachment_lookup_node(attachment_presence_ptr, voxel_addr.node_index + 1),
      load_attachment_lookup_node(attachment_presence_ptr, voxel_addr.node_index + 2)
    };
    let is_attachment_present = (attachment_mask[voxel_addr.morton_half] & voxel_addr.child_bit) > 0;
    if (!is_attachment_present) {
      return none;
    }

    // Offset into the raw attachment data where the current 
    // nodes children attachment data starts.
    let node_attachment_data_ptr = 
      load_attachment_lookup_node(attachment_presence_ptr, voxel_addr.node_index);
    var child_offset = countbits(attachment_mask[voxel_addr.morton_half] & (voxel_addr.child_bit - 1));
    child_offset += voxel_addr.morton_half == 0 ? 0 : countbits(attachment_mask[0]);
    return attachment_raw_ptr.with_offset(node_attachment_data_ptr + child_offset);
  }

  private VoxelDataRange load_voxel_attachment_data(uint presence_attachment_index, 
                                                      uint data_attachment_index, 
                                                      SFTVoxelAddr voxel_addr) {
    let info_data = u_frame.voxel.model_info_data;
    let attachment_presence_data = VoxelDataRange(info_data.Load<uint>((
        this.voxel_model_ptr + 3 + presence_attachment_index) << 2));
    let attachment_raw_data = VoxelDataRange(info_data.Load<uint>(
        (voxel_model_ptr + 3 + consts::VOXEL_ATTACHMENT_COUNT + data_attachment_index) << 2));

    uint attachment_mask[2] = {
      load_attachment_lookup_node(attachment_presence_data, voxel_addr.node_index + 1),
      load_attachment_lookup_node(attachment_presence_data, voxel_addr.node_index + 2)
    };

    // Offset into the raw attachment data where the current 
    // nodes children attachment data starts.
    let node_attachment_data_ptr = 
      load_attachment_lookup_node(attachment_presence_data, voxel_addr.node_index);
    var child_offset = countbits(attachment_mask[voxel_addr.morton_half] & (voxel_addr.child_bit - 1));
    child_offset += voxel_addr.morton_half == 0 ? 0 : countbits(attachment_mask[0]);
    return VoxelDataRange(
        attachment_raw_data.buffer_index, 
        attachment_raw_data.offset + node_attachment_data_ptr + child_offset);
  }

  private uint load_attachment_lookup_node(VoxelDataRange attachment_lookup_data, uint node_index) {
    return attachment_lookup_data.load(node_index);
  }

  private Optional<float3> load_ptmaterial(SFTVoxelAddr voxel_addr) {
    let ptmaterial_data_ptr_opt = this.load_voxel_attachment(0, voxel_addr);
    if (ptmaterial_data_ptr_opt == none) {
      return none;
    }
    let ptmaterial_data_ptr = ptmaterial_data_ptr_opt.value;
    let compressed_ptmaterial = ptmaterial_data_ptr.load(0);
    return ptmaterial_decode(compressed_ptmaterial);
  }

  private Optional<float3> load_normal(SFTVoxelAddr voxel_addr) {
    let normal_data = this.load_voxel_attachment_data(0, 1, voxel_addr);
    let compressed_normal = normal_data.load(0);
    return normal_decode(compressed_normal);
  }

  public void set_normal(SFTVoxelAddr voxel_addr, float3 normal) {
    let normal_data = this.load_voxel_attachment_data(0, 1, voxel_addr);
    let compressed_normal = normal_encode(normal);
    normal_data.store(0, compressed_normal);
  }

  public void set_ptmaterial(SFTVoxelAddr voxel_addr, float3 color) {
    let ptmaterial_data = this.load_voxel_attachment_data(0, 0, voxel_addr);
    let compressed_ptmaterial = ptmaterial_encode(color);
    ptmaterial_data.store(0, compressed_ptmaterial);
  }
}

}  // namespace voxel
