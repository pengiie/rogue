module bake_model;

import lib;

struct ShaderUniforms {
  uint model_ptr;
  uint3 voxel_offset;
  uint3 voxel_size;
  // Volume of voxel_size.
  uint bake_volume;
}

ParameterBlock<ShaderUniforms> u_shader;

[shader("compute")]
[numthreads(512)]
void main(uint id: SV_DispatchThreadID) {
  if (id >= u_shader.bake_volume) {
    return;
  }

  let pos_offset = int3(id % u_shader.voxel_size.x,
                            (id / u_shader.voxel_size.x) % u_shader.voxel_size.y, 
                             id / (u_shader.voxel_size.x * u_shader.voxel_size.y));
  let local_voxel_pos = (uint3) (u_shader.voxel_offset + pos_offset);

  let model_ptr = u_shader.model_ptr;
  let mat_opt = voxel::model_get_voxel_material(local_voxel_pos, model_ptr);
  if (!mat_opt.hasValue) {
    return;
  }
  let mat_comp = mat_opt.value;
  if (!mat_comp.need_normal_baking()) {
    return;
  }

  random::init(uint3(local_voxel_pos));
  var normal = calculate_normal((int3) local_voxel_pos, model_ptr);
  // Dither since bands where the normals are very flat look bad.
  normal = normalize(dither(normal, exp2(-7.0)));
  var bmat_data: voxel::BakedVoxelMaterial;
  if (mat_comp.needs_material_baking()) {
    let mat = MaterialBank.get_material(mat_comp.b & 0xFFFF);
    bmat_data = mat.into_bmat((int3) local_voxel_pos, normal);
  } else {
    bmat_data = voxel::BakedVoxelMaterial.decode(mat_comp);
    bmat_data.normal = normal;
  }
  voxel::model_set_voxel_material(local_voxel_pos, model_ptr, bmat_data.encode());
}

float3 calculate_normal(int3 local_voxel_pos, uint model_ptr) {
  float3 normal = float3(0.0, 0.0, 0.0);
  const int RADIUS = 2;
  [unroll]
  for (int x = -RADIUS; x <= RADIUS; x++) {
    [unroll]
    for (int y = -RADIUS; y <= RADIUS; y++) {
      [unroll]
      for (int z = -RADIUS; z <= RADIUS; z++) {
        if (x == 0 && y == 0 && z == 0) {
          continue;
        }
        int3 offset = int3(x, y, z);
        int3 sample_pos = local_voxel_pos + offset;
        // TODO: for some reason the normals are cooked on some surfaces idk why.
        bool voxel_exists = voxel::model_is_voxel_present(sample_pos, model_ptr);
        if (!voxel_exists) {
          normal += (float3) offset;
        }
      }
    }
  }
  return normalize(normal);
}
