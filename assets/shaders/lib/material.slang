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
    if(this.color_texture_index == 0xFFFFFFFF) {
      return null_color(uv);
    }

    let texture = MaterialBank.get().textures_float4[this.color_texture_index];
    let sampler = MaterialBank.get().samplers[this.color_sampler_index];
    let sampled_color = texture.Sample(sampler, uv);
    return sampled_color;
  }
}
