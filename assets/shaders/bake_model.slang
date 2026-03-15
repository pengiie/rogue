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
  let mat_comp = voxel::model_get_voxel_material(local_voxel_pos, model_ptr);
  if (!mat_comp.hasValue) {
    return;
  }
  voxel::model_set_voxel_material(local_voxel_pos, model_ptr, 0x800000FF);
  let mat_val = mat_comp.value;
  if (voxel::BMaterial.is_bmaterial(mat_val)) {
    return;
  }

  let normal = calculate_normal((int3) local_voxel_pos, model_ptr);
  var bmat: voxel::BMaterial;
  if (voxel::BMaterial.needs_normal_update(mat_val)) {
    bmat = voxel::BMaterial.decode(mat_val);
    bmat.normal = normal;
  } else {
    let mat = MaterialBank.get_material(mat_val);
    bmat = mat.into_bmat((int3) local_voxel_pos, normal);
  }
  voxel::model_set_voxel_material(local_voxel_pos, model_ptr, bmat.encode());
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
