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

}  // namespace voxel
