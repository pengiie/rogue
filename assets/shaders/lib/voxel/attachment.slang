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

public struct BMaterial {
  public uint index;
  public float3 normal;

  public static BMaterial decode(uint comp) {
    let octahedral_coords = uint2((comp >> 16) & 0xFF, (comp >> 24) & 0xFF);
    let normal = octahedral_normal_decode(octahedral_coords, /*bits_per_component*/8);
    let id = comp & 0xFFFF;
    return BMaterial(id, normal);
  }

  public static uint update_normals(uint compressed, float3 normal) {
    let compressed_normal = octahedral_normal_encode(normal, /*bits_per_component*/8);
    compressed &= 0x0000FFFF;
    compressed |= ((compressed_normal.x << 16) & 0x00FF0000) | ((compressed_normal.y << 24) & 0xFF000000);
    return compressed;
  }

  public float3 calculate_color(int3 world_voxel_position) {
    switch (index) {
      case consts::bmat::GRASS:
        return calc_grass_color(world_voxel_position);
      case consts::bmat::DIRT:
        return calc_dirt_color(world_voxel_position);
      default:
        return calc_unknown_color(world_voxel_position);
    }
  }

  float checker_pattern_t(int3 world_voxel_position) {
    world_voxel_position /= 16;
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
