implementing lib;

namespace voxel {

public struct Flat {
  uint voxel_model_ptr;
  uint3 side_length; 
                        
  VoxelDataRange presence_data;

  public __init(uint in_voxel_model_ptr) {
    let info_data = u_frame.voxel.model_info_data;
    
    voxel_model_ptr = in_voxel_model_ptr;
    side_length = uint3(
      info_data.Load<uint>((voxel_model_ptr + 1) << 2),
      info_data.Load<uint>((voxel_model_ptr + 2) << 2),
      info_data.Load<uint>((voxel_model_ptr + 3) << 2)
    );
    presence_data = VoxelDataRange(info_data.Load<uint>((voxel_model_ptr + 4) << 2));
  }

  public uint3 get_side_length() {
    return this.side_length;
  }

  public uint get_voxel_index(uint3 pos) {
      return pos.x + this.side_length.x * pos.y + (this.side_length.x * this.side_length.y) * pos.z;
  }

  public bool voxel_exists(uint voxel_index) {
      let presence_u32 = this.presence_data.load(voxel_index / 32);
      let is_present = (presence_u32 & (1 << (voxel_index % 32))) > 0;
      return is_present;
  }

  public RayModelTest trace(Ray in_ray, AABB root_aabb, matrix3x3 rotation) {
    let root_hit_info = ray_aabb_test(in_ray, root_aabb);

    var dda = RayDDA(in_ray.advance_clone(root_hit_info.t_enter),
                     root_aabb,
                     this.side_length);
    var i = 0;
    var last_grid_pos = dda.curr_grid_pos();
    while (dda.in_bounds() && i < 4321) {
      let grid_pos = dda.curr_grid_pos();
      let voxel_index = this.get_voxel_index((uint3) grid_pos);
      if (this.voxel_exists(voxel_index)) {
        if (let pt_material = this.load_ptmaterial(voxel_index)) {
          var color = pt_material;
          if (let normal = this.load_normal(voxel_index)) {
            let rot_normal = mul(rotation, normal);
            let l = sqrt(dot(rot_normal, normalize(-float3(0.7,-1,-0.2))) * 0.5 + 0.5);
            color *= max(l, 0.2);
          }
          //printf("Color is %f %f %f\n", pt_material.x, pt_material.y, pt_material.z);
          let t_scaling = (root_aabb.max() - root_aabb.min()) / (float3) this.side_length;
          let next_point = root_aabb.min() + ((float3) last_grid_pos + ((float3) dda.unit_grid * 0.5 + 0.5)) * t_scaling;
          let hit_t = ray_point_test(in_ray, next_point);
          var depth = min3(hit_t.x, hit_t.y, hit_t.z);
          if (i == 0) {
            depth = root_hit_info.t_enter;
          }
          return RayModelTest.hit(color, depth);
        }
      }

      last_grid_pos = dda.curr_grid_pos();
      dda.step();
      i += 1;
    }

    return RayModelTest.miss();
  }

  private Optional<Tuple<VoxelDataRange, VoxelDataRange>> load_attachment_ptrs(uint attachment_index) {
    let info_data = u_frame.voxel.model_info_data;
    let attachment_presence_ptr = info_data.Load<uint>((this.voxel_model_ptr + 5 + attachment_index) << 2);
    if (attachment_presence_ptr == 0xFFFFFFFF) {
      return none;
    }

    let attachment_raw_ptr = info_data.Load<uint>((voxel_model_ptr + 5 + consts::VOXEL_ATTACHMENT_COUNT + attachment_index) << 2);
    if (attachment_raw_ptr == 0xFFFFFFFF) {
      return none;
    }

    return makeTuple(
        VoxelDataRange(attachment_presence_ptr), 
        VoxelDataRange(attachment_raw_ptr));
  }

  private Optional<VoxelDataRange> load_voxel_attachment(uint attachment_index, uint voxel_index) {
    let attachment_ptr_pair = load_attachment_ptrs(attachment_index);
    if (attachment_ptr_pair == none) {
      return none;
    }
    let attachment_presence_ptr = attachment_ptr_pair.value._0;
    let attachment_raw_ptr = attachment_ptr_pair.value._1;

    let attachment_mask = attachment_presence_ptr.load(voxel_index / 32);
    let is_attachment_present = (attachment_mask & (1 << (voxel_index % 32))) > 0;
    if (!is_attachment_present) {
      return none;
    }

    return attachment_raw_ptr.with_offset(voxel_index);
  }

  private Optional<float3> load_ptmaterial(uint voxel_index) {
    let ptmaterial_data_ptr_opt = this.load_voxel_attachment(0, voxel_index);
    if (ptmaterial_data_ptr_opt == none) {
      return none;
    }
    let ptmaterial_data_ptr = ptmaterial_data_ptr_opt.value;
    let compressed_ptmaterial = ptmaterial_data_ptr.load(0);

    return color::srgb_to_lsrgb(float3(
      (float) ((compressed_ptmaterial >> 16) & 0xFF) / 255.0,
      (float) ((compressed_ptmaterial >> 8) & 0xFF) / 255.0,
      (float) (compressed_ptmaterial & 0xFF) / 255.0,
    ));
  }

  private Optional<float3> load_normal(uint voxel_index) {
    // Attachment presence is not checked we just assume the normal exists.
    // TODO: Remove presence buffer.
    let normal_data_ptrs = load_attachment_ptrs(1);
    if (normal_data_ptrs == none) {
      return none;
    }
    let normal_data_ptr = normal_data_ptrs.value._1.with_offset(voxel_index);
    let compressed_normal = normal_data_ptr.load(0);

    return float3(
      ((float) ((compressed_normal >> 16) & 0xFF) / 255.0) * 2.0 - 1.0,
      ((float) ((compressed_normal >> 8) & 0xFF) / 255.0) * 2.0 - 1.0,
      ((float) (compressed_normal & 0xFF) / 255.0) * 2.0 - 1.0,
    );
  }

  public void set_normal(uint voxel_index, float3 normal) {
    let normal_data_ptrs = load_attachment_ptrs(1);
    if (normal_data_ptrs == none) {
      return;
    }
    let presence_ptr = normal_data_ptrs.value._0;
    let data_ptr = normal_data_ptrs.value._1;

    let compressed_normal = (((uint) floor((normal.x * 0.5 + 0.5) * 255.0)) << 16) |
                            (((uint) floor((normal.y * 0.5 + 0.5) * 255.0)) << 8) |
                            (uint) floor((normal.z * 0.5 + 0.5) * 255.0);
    data_ptr.store(voxel_index, compressed_normal);
  }
}

}  // namespace voxel
