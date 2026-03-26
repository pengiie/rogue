implementing lib;

public struct MaterialBank {
  Texture2D<float4> textures_float4[1000];
  SamplerState samplers[1000];
  ByteAddressBuffer materials;

  public static MaterialBank get() { return u_frame.material_bank; }
  public static Material get_material(uint material_index) { 
    let offset = material_index * 8;
    uint color_texture = MaterialBank.get().materials.Load<uint>(offset);
    uint color_sampler = MaterialBank.get().materials.Load<uint>(offset + 4);
    return Material(uint2(color_texture, color_sampler));
  }
}

public struct Material {
  // Most signifigant bit determines presence.
  uint color_texture_index;
  uint color_sampler_index;

  __init(uint2 color_packed) {
    this.color_texture_index = color_packed.x;
    this.color_sampler_index = color_packed.y;
  }

  float4 null_color(float2 uv) {
    uv = frac(uv * 16.0);
    let clamped = uv >= 0.5;
    if(clamped.x == clamped.y) {
      return float4(0, 0, 0, 1);
    }
    return float4(1, 0, 1, 1);
  }

  /// Returns srgb color.
  public float4 get_color(float2 uv) {
    if(this.color_texture_index == 0xFFFFFFFF || this.color_sampler_index == 0xFFFFFFFF) {
      return null_color(uv);
    }

    let texture = MaterialBank.get().textures_float4[this.color_texture_index];
    // TODO: Figure out why the using Texture2D.Sample with a sampler causes the texture color to just be black or idk 
    // maybe the thread crashed gotta figure that out. It seems relative to the sft being painted with the banding of black
    // which is extra weird. For now we nearest neighbor sample.
    //let sampler = MaterialBank.get().samplers[this.color_sampler_index];
    var dimensions: uint2;
    texture.GetDimensions(dimensions.x, dimensions.y);
    var sampled_color = texture.Load(int3((int2) ((float2) dimensions * uv), 0));
    return sampled_color;
  }

  public float4 calculate_triplanar_color(int3 world_voxel_pos, float3 normal) {
    let tri = fract((float3) world_voxel_pos / 64);

    var weights = abs(normal);
    weights /= (weights.x + weights.y + weights.z);
  
    let uv_x = this.get_color(tri.zy) * weights.x;
    let uv_y = this.get_color(tri.xz) * weights.y;
    let uv_z = this.get_color(tri.xy) * weights.z;
    return uv_x + uv_y + uv_z;
  }

  public voxel::BakedVoxelMaterial into_bmat(int3 world_voxel_pos, float3 normal) {
    voxel::BakedVoxelMaterial bmat;
    let rgba = calculate_triplanar_color(world_voxel_pos, normalize(normal));
    bmat.albedo = rgba.rgb;
    bmat.alpha = rgba.a;
    bmat.normal = normal;
    return bmat;
  }
}
