implementing lib;

namespace voxel {

// Returns a linear gamma space srgb color.
public float3 ptmaterial_decode(uint comp) {
    return color::srgb_to_lsrgb(float3(
      (float) ((comp >> 16) & 0xFF) / 255.0,
      (float) ((comp >> 8) & 0xFF) / 255.0,
      (float) (comp & 0xFF) / 255.0,
    ));
}

public uint ptmaterial_encode(float3 color) {
    return (((uint) floor(color.x * 255.0)) << 16) |
           (((uint) floor(color.y * 255.0)) << 8) |
           (uint) floor(color.z * 255.0);
}

public uint normal_encode(float3 normal) {
    return (((uint) floor((normal.x * 0.5 + 0.5) * 255.0)) << 16) |
           (((uint) floor((normal.y * 0.5 + 0.5) * 255.0)) << 8) |
           (uint) floor((normal.z * 0.5 + 0.5) * 255.0);
}

public float3 normal_decode(uint comp) {
    return float3(
      ((float) ((comp >> 16) & 0xFF) / 255.0) * 2.0 - 1.0,
      ((float) ((comp >> 8) & 0xFF) / 255.0) * 2.0 - 1.0,
      ((float) (comp & 0xFF) / 255.0) * 2.0 - 1.0,
    );
}

float2 octahedral_wrap(float2 c) {
  return (1.0 - abs(c.yx)) * sign(c.xy);
}

public uint2 octahedral_normal_encode(float3 normal, uint bits_per_component) {
  normal /= (abs(normal.x) + abs(normal.y) + abs(normal.z));
  normal.xy = normal.z >= 0.0 ? normal.xy : octahedral_wrap(normal.xy);
  normal.xy = normal.xy * 0.5 + 0.5;
  return (uint2) (normal.xy * 255.0);
}

public float3 octahedral_normal_decode(uint2 compressed, uint bits_per_component) {
  var c = (float2) compressed;
  //c /= (float) ((1 << bits_per_component) - 1);
  c /= 255.0;
  c = c * 2.0 - 1.0;
  
  float3 n = float3(c.x, c.y, 1.0 - abs(c.x) - abs(c.y));
  float t = max(-n.z, 0.0);
  n.xy += select(n.xy > 0.0, -t, t);
  return normalize(n);
}

public struct BakedVoxelMaterial {
  // In srgb.
  public float3 albedo = float3(0.0);
  public float alpha = 1.0;
  public float3 normal = float3(0.0);

  public CompressedVoxelMaterial encode() {
    CompressedVoxelMaterial comp;
    let r = (uint) (this.albedo.r * 255.0);
    let g = (uint) (this.albedo.g * 255.0);
    let b = (uint) (this.albedo.b * 255.0);
    let a = (uint) (this.alpha * 255.0);
    comp.b = (r << 24) | (g << 16) | (b << 8) | a;
    let normal_oct = octahedral_normal_encode(this.normal, /*bits_per_component*/8);
    comp.a = ((normal_oct.x & 0xFF) << 8) | (normal_oct.y & 0xFF);
    return comp;
  }

  public static BakedVoxelMaterial decode(CompressedVoxelMaterial comp) {
    BakedVoxelMaterial mat;
    let rgba = comp.b;
    mat.albedo = float3(
      ((rgba >> 24) & 0xFF) / 255.0,
      ((rgba >> 16) & 0xFF) / 255.0,
      ((rgba >> 8) & 0xFF) / 255.0,
    );
    mat.alpha = (rgba & 0xFF) / 255.0;
    let normal_oct = uint2((comp.a >> 8) & 0xFF, comp.a & 0xFF);
    mat.normal = octahedral_normal_decode(normal_oct, /*bits_per_component*/8);
    return mat;
  }
}

public struct CompressedVoxelMaterial {
  // A = most signifigant, b is least in terms of u64.
  public uint a = 0; public uint b = 0;

  public bool needs_material_baking() {
    return (this.a & 0x80000000) > 0;
  }

  public bool need_normal_baking() {
    return (this.a & 0x40000000) > 0 || this.needs_material_baking();
  }
}

public struct BMaterial {
  // Albedo in srgb.
  public float3 albedo;
  public float3 normal;

  public static bool is_bmaterial(uint comp) {
    return (comp & 0x80000000) > 0;
  }

  public static bool needs_normal_update(uint comp) {
    return (comp & 0x40000000) > 0;
  }

  public static BMaterial decode(uint comp) {
    // 16 bits for normal.
    let octahedral_coords = uint2((comp >> 15) & 0xFF, (comp >> 23) & 0xFF);
    let normal = octahedral_normal_decode(octahedral_coords, /*bits_per_component*/8);
    // 565 encoding gives us 16 bits for color.
    let r = (float) ((comp >> 10) & 0b11111) / 0b11111;
    let g = (float) ((comp >> 5) & 0b11111) / 0b11111;
    let b = (float) (comp & 0b11111) / 0b11111;
    let color = color::srgb_to_lsrgb(float3(r,g,b));
    
    return BMaterial(color, normal);
  }

  public uint encode() {
    let octahedral_coords = octahedral_normal_encode(this.normal, /*bits_per_component*/8);
    let normal_comp = ((octahedral_coords.x & 0xFF) << 15) | ((octahedral_coords.y & 0xFF) << 23);
    let r = (uint) floor(this.albedo.x * 0b11111) & 0b11111;
    let g = (uint) floor(this.albedo.y * 0b11111) & 0b11111;
    let b = (uint) floor(this.albedo.z * 0b11111) & 0b11111;
    let color_comp = (r << 10) | (g << 5) | b;
    return normal_comp | color_comp | 0x80000000;
  }

  public static uint update_normals(uint compressed, float3 normal) {
    let compressed_normal = octahedral_normal_encode(normal, /*bits_per_component*/8);
    compressed &= 0x0000FFFF;
    compressed |= ((compressed_normal.x << 16) & 0x00FF0000) | ((compressed_normal.y << 24) & 0xFF000000);
    return compressed;
  }

  //public float3 calculate_color(int3 world_voxel_position) {
  //  Material material = MaterialBank.get_material(this.index);
  //  let tri = fract((float3) world_voxel_position * (consts::METERS_PER_VOXEL / 64));

  //  var weights = abs(normal);
  //  weights /= (weights.x + weights.y + weights.z);
  //
  //  let uv_x = material.get_color(tri.zy) * weights.x;
  //  let uv_y = material.get_color(tri.xz) * weights.y;
  //  let uv_z = material.get_color(tri.xy) * weights.z;
  //  return (uv_x + uv_y + uv_z).xyz;
  //}

  float checker_pattern_t(int3 world_voxel_position) {
    world_voxel_position /= 4;
    let checker_zero = (world_voxel_position.z % 2);
    return (float) ((world_voxel_position.x % 2) == ((world_voxel_position.y + checker_zero) % 2));
  }

  float3 calc_grass_color(int3 world_voxel_position) {
    let t = checker_pattern_t(world_voxel_position);
    static const float3 GREEN = float3(0.27, 0.65, 0.35);
    static const float3 LIMEY_GREEN = float3(0.53, 0.83, 0.24);
    return lerp(GREEN, LIMEY_GREEN, t);
  }

  float3 calc_dirt_color(int3 world_voxel_position) {
    let t = checker_pattern_t(world_voxel_position);
    static const float3 REALLY_DARK_BROWN = float3(0.41, 0.365, 0.248);
    static const float3 LIGHTER_BROWN = float3(0.67, 0.478, 0.106);
    return lerp(REALLY_DARK_BROWN, LIGHTER_BROWN, t);
  }

  float3 calc_unknown_color(int3 world_voxel_position) {
    let t = checker_pattern_t(world_voxel_position);
    static const float3 BLACK = float3(0.0);
    static const float3 PURPLE = float3(1.0, 1.0, 0.0);
    return lerp(BLACK, PURPLE, t);
  }
}

}  // namespace voxel
