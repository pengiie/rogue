module normal_calc;

import lib;

struct ShaderUniforms {
  uint voxel_model_ptr;
}

ParameterBlock<ShaderUniforms> u_shader;

[shader("compute")]
[numthreads(8, 8, 8)]
void main(uint3 id: SV_DispatchThreadID) {
  let schema = u_frame.voxel.model_info_data.Load<uint>(u_shader.voxel_model_ptr << 2);
  switch (schema) {
    case consts::VOXEL_MODEL_SCHEMA_THC:
    case consts::VOXEL_MODEL_SCHEMA_THC_COMPRESSED:
      normal_calc_thc(id);
      break;
    case consts::VOXEL_MODEL_SCHEMA_FLAT:
      normal_calc_flat(id);
      break;
  }
}

void normal_calc_thc(uint3 id) {
  let thc = voxel::THC(u_shader.voxel_model_ptr);
  uint3 local_voxel_pos = id;
  if (!thc.in_bounds((int3) local_voxel_pos)) {
    return;
  }

  let voxel_addr = thc.get_voxel_addr(local_voxel_pos);
  if (voxel_addr == none) {
    return;
  }

  var normal = float3(0.0, 0.0, 0.0);
  [unroll]
  for (int x = -2; x <= 2; x++) {
    [unroll]
    for (int y = -2; y <= 2; y++) {
      [unroll]
      for (int z = -2; z <= 2; z++) {
        int3 offset = int3(x,y,z);
        if (any(offset == int3(0,0,0))) {
          continue;
        }

        let sample_pos = (int3) local_voxel_pos + offset;
        var exists: bool;
        if (!thc.in_bounds(sample_pos)) {
          exists = false;
        } else {
          let voxel_addr = thc.get_voxel_addr(sample_pos);
          exists = voxel_addr.hasValue;
        }
        if (!exists) {
          normal += (float3) offset;
        }
      }
    }
  }

  thc.set_normal(voxel_addr.value, normalize(normal));
  //thc.set_ptmaterial(voxel_addr.value, (float3) local_voxel_pos / float3(thc.get_side_length()));
}

void normal_calc_flat(uint3 id) {
  let flat = voxel::Flat(u_shader.voxel_model_ptr);
  uint3 local_voxel_pos = id;
  let sl = flat.get_side_length();
  if (local_voxel_pos.x >= sl.x || local_voxel_pos.y >= sl.y || local_voxel_pos.z >= sl.z) {
    return;
  }

  let voxel_index = flat.get_voxel_index((uint3) local_voxel_pos);
  if (!flat.voxel_exists(voxel_index)) {
    return;
  }

  var normal = float3(0.0, 0.0, 0.0);
  [unroll]
  for (int x = -2; x <= 2; x++) {
    [unroll]
    for (int y = -2; y <= 2; y++) {
      [unroll]
      for (int z = -2; z <= 2; z++) {
        int3 offset = int3(x,y,z);
        if (any(offset == int3(0,0,0))) {
          continue;
        }

        let sample_pos = (int3) local_voxel_pos + offset;
        var exists: bool;
        if (sample_pos.x < 0 || sample_pos.y < 0 || sample_pos.z < 0 ||
            sample_pos.x >= sl.x || sample_pos.y >= sl.y || sample_pos.z >= sl.z) {
          exists = false;
        } else {
          let voxel_index = flat.get_voxel_index((uint3) sample_pos);
          exists = flat.voxel_exists(voxel_index);
        }
        if (!exists) {
          normal += (float3) offset;
        }
      }
    }
  }

  flat.set_normal(voxel_index, normalize(normal));
}
