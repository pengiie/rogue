module normal_calc;

import lib;

struct ShaderUniforms {
  int3 world_chunk_pos;
}

ParameterBlock<ShaderUniforms> u_shader;

[shader("compute")]
[numthreads(8, 8, 8)]
void main(uint3 id: SV_DispatchThreadID) {
  uint3 local_voxel_pos = id;
  let sl = consts::TERRAIN_CHUNK_VOXEL_LENGTH;
  if (local_voxel_pos.x >= consts::TERRAIN_CHUNK_VOXEL_LENGTH || 
      local_voxel_pos.y >= consts::TERRAIN_CHUNK_VOXEL_LENGTH ||
      local_voxel_pos.z >= consts::TERRAIN_CHUNK_VOXEL_LENGTH) {
    return;
  }
  int3 world_chunk_pos = u_shader.world_chunk_pos;
  
  let terrain = u_frame.voxel.terrain;
  uint3 terrain_local_chunk_pos = (uint3) (world_chunk_pos - terrain.anchor);
  uint chunk_model_ptr = voxel::Terrain.get_chunk_ptr(terrain_local_chunk_pos); 
  if (chunk_model_ptr == voxel::INVALID_PTR) {
    return;
  }
  let schema = u_frame.voxel.model_info_data.Load<uint>(chunk_model_ptr << 2);
  switch (schema) {
    case consts::VOXEL_MODEL_SCHEMA_THC:
    case consts::VOXEL_MODEL_SCHEMA_THC_COMPRESSED:
      normal_calc_thc(chunk_model_ptr, local_voxel_pos);
      break;
    case consts::VOXEL_MODEL_SCHEMA_SFT:
    case consts::VOXEL_MODEL_SCHEMA_SFT_COMPRESSED:
      normal_calc_sft(chunk_model_ptr, local_voxel_pos);
      break;
    case consts::VOXEL_MODEL_SCHEMA_FLAT:
      normal_calc_flat(chunk_model_ptr, local_voxel_pos);
      break;
  }
}

void normal_calc_flat(uint chunk_model_ptr, uint3 local_voxel_pos) {
  let world_chunk_pos = u_shader.world_chunk_pos;
  let flat = voxel::Flat(chunk_model_ptr);
  let voxel_index = flat.get_voxel_index((uint3) local_voxel_pos);
  if (!flat.voxel_exists(voxel_index)) {
    return;
  }

  int3 world_voxel_pos = world_chunk_pos * (int) consts::TERRAIN_CHUNK_VOXEL_LENGTH + (int3) local_voxel_pos;

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

        let sample_pos = world_voxel_pos + offset;
        let sample_hit = world_voxel_exists(sample_pos);
        if (!sample_hit) {
          normal += (float3) offset;
        }
      }
    }
  }

  flat.set_normal(voxel_index, normalize(normal));
}

void normal_calc_thc(uint chunk_model_ptr, uint3 local_voxel_pos) {
  let thc = voxel::THC(chunk_model_ptr);
  if (!thc.in_bounds((int3) local_voxel_pos)) {
    return;
  }

  let voxel_addr = thc.get_voxel_addr(local_voxel_pos);
  if (voxel_addr == none) {
    return;
  }

  let world_chunk_pos = u_shader.world_chunk_pos;
  int3 world_voxel_pos = world_chunk_pos * (int) consts::TERRAIN_CHUNK_VOXEL_LENGTH + (int3) local_voxel_pos;

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

        let sample_pos = world_voxel_pos + offset;
        var sample_hit = world_voxel_exists(sample_pos);
        if (!sample_hit) {
          normal += (float3) offset;
        }
      }
    }
  }

  thc.set_normal(voxel_addr.value, normalize(normal));
}

void normal_calc_sft(uint chunk_model_ptr, uint3 local_voxel_pos) {
  let sft = voxel::SFT(chunk_model_ptr);
  if (!sft.in_bounds((int3) local_voxel_pos)) {
    return;
  }

  let leaf_info_opt = sft.get_leaf_info(local_voxel_pos);
  if (leaf_info_opt == none) {
    return;
  }
  let leaf_info = leaf_info_opt.value;

  let world_chunk_pos = u_shader.world_chunk_pos;
  var normal = float3(0.0, 0.0, 0.0);

  int3 world_voxel_pos = world_chunk_pos * (int) consts::TERRAIN_CHUNK_VOXEL_LENGTH + (int3) local_voxel_pos;
  if (leaf_info.leaf_side_length == 1) {
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

          let sample_pos = world_voxel_pos + offset;
          var sample_hit = world_voxel_exists(sample_pos);
          if (!sample_hit) {
            normal += (float3) offset;
          }
        }
      }
    }
  } else if (leaf_info.leaf_side_length == 4) {
    let base_voxel_pos = world_chunk_pos & 0xFFFFFFFC;
    for (int x = -2; x <= 5; x++) {
      for (int y = -2; y <= 5; y++) {
        for (int z = -2; z <= 5; z++) {
          int3 offset = int3(x,y,z);
          if (!any(offset == -1) && !any(offset == 4)) {
            continue;
          }

          let sample_pos = base_voxel_pos + offset;
          var sample_hit = world_voxel_exists(sample_pos);
          if (!sample_hit) {
            offset = select(offset > 0, offset - 3, offset);
            normal += (float3) offset;
          }
        }
      }
    }
  } else if (leaf_info.leaf_side_length == 16) {
    let base_voxel_pos = world_chunk_pos & 0xFFFFFFFC;
    for (int x = -2; x <= 17; x++) {
      for (int y = -2; y <= 17; y++) {
        for (int z = -2; z <= 17; z++) {
          int3 offset = int3(x,y,z);
          if (!any(offset == -1) && !any(offset == 16)) {
            continue;
          }

          let sample_pos = base_voxel_pos + offset;
          var sample_hit = world_voxel_exists(sample_pos);
          if (!sample_hit) {
            offset = select(offset > 0, offset - 15, offset);
            normal += (float3) offset;
          }
        }
      }
    }
  } else {
    // Cube time.

  }

  sft.set_normal(leaf_info.addr, normalize(normal));
}

bool world_voxel_exists(int3 world_voxel_pos) {
  let terrain = u_frame.voxel.terrain;
  let world_chunk_pos = (int3) floor((float3) world_voxel_pos / (float) consts::TERRAIN_CHUNK_VOXEL_LENGTH);
  uint3 terrain_local_chunk_pos = (uint3) (world_chunk_pos - terrain.anchor);

  uint chunk_model_ptr = voxel::Terrain.get_chunk_ptr(terrain_local_chunk_pos); 
  if (chunk_model_ptr == voxel::INVALID_PTR) {
    return false;
  }

  let world_chunk_voxel_pos = world_chunk_pos * consts::TERRAIN_CHUNK_VOXEL_LENGTH;
  let local_voxel_pos = (uint3) (world_voxel_pos - world_chunk_voxel_pos);

  let schema = u_frame.voxel.model_info_data.Load<uint>(chunk_model_ptr << 2);
  if (schema == consts::VOXEL_MODEL_SCHEMA_FLAT) {
    let flat = voxel::Flat(chunk_model_ptr);
    return flat.voxel_exists(flat.get_voxel_index(local_voxel_pos));
  } else if (schema == consts::VOXEL_MODEL_SCHEMA_THC) {
    let thc = voxel::THC(chunk_model_ptr);
    return thc.get_voxel_addr(local_voxel_pos).hasValue;
  } else if (schema == consts::VOXEL_MODEL_SCHEMA_SFT || schema == consts::VOXEL_MODEL_SCHEMA_SFT_COMPRESSED) {
    let sft = voxel::SFT(chunk_model_ptr);
    return sft.voxel_exists(local_voxel_pos);
  }

  return false;
}
