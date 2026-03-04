module bake_chunk;

import lib;

struct ShaderUniforms {
  int3 chunk_pos;
  uint chunk_height;
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
  let chunk_voxel_pos = (uint3) (u_shader.chunk_pos * consts::TERRAIN_CHUNK_VOXEL_LENGTH);
  let chunk_voxel_length = 1 << ((consts::TERRAIN_REGION_TREE_HEIGHT - u_shader.chunk_height)) * 2;
  let world_voxel_pos = chunk_voxel_pos + local_voxel_pos * chunk_voxel_length;

  let mat_comp = voxel::Terrain.get().get_voxel_material(u_shader.chunk_pos, u_shader.chunk_height, local_voxel_pos);
  if (!mat_comp.hasValue) {
    return;
  }
  let mat_val = mat_comp.value;
  if (voxel::BMaterial.is_bmaterial(mat_val)) {
    return;
  }

  let normal = calculate_normal(world_voxel_pos, chunk_voxel_length);
  let mat = MaterialBank.get_material(mat_val);
  var bmat = mat.into_bmat(world_voxel_pos, normal);
  voxel::Terrain.get().set_voxel_material(u_shader.chunk_pos, u_shader.chunk_height, local_voxel_pos, bmat.encode());
}

float3 calculate_normal(int3 world_voxel_pos, int voxel_length) {
  float3 normal = float3(0.0, 0.0, 0.0);
  [unroll]
  for (int x = -2; x <= 2; x++) {
    [unroll]
    for (int y = -2; y <= 2; y++) {
      [unroll]
      for (int z = -2; z <= 2; z++) {
        if (x == 0 && y == 0 && z == 0) {
          continue;
        }
        int3 offset = int3(x, y, z) * voxel_length;
        int3 sample_pos = world_voxel_pos + offset;
        bool voxel_exists = voxel::Terrain.get().is_voxel_present(sample_pos);
        if (!voxel_exists) {
          normal += (float3) offset;
        }
      }
    }
  }
  return normal;
}
