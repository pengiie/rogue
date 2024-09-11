struct Uniform {
  screen_size: vec2f
};

@group(0) @binding(0) var u_texture_sampler: sampler;
@group(0) @binding(1) var u_texture: texture_2d<f32>;
@group(0) @binding(2) var<uniform> u_uniform: Uniform;

struct VertexOut {
  @builtin(position) clip_position: vec4f,
  @location(0) color: vec4f,
  @location(1) uv: vec2f,
};
@vertex fn vs_main(@location(0) position: vec2f,
           @location(1) uv: vec2f,
           @location(2) color: u32) -> VertexOut {
 var out: VertexOut; 
 out.clip_position = vec4f(
    ((2.0 * position.x) / u_uniform.screen_size.x) - 1.0,
    1.0 - ((2.0 * position.y) / u_uniform.screen_size.y),
    0.0,
    1.0);
 out.color = vec4f(
    f32(color & 255u),
    f32((color >> 8u) & 255u),
    f32((color >> 16u) & 255u),
    f32((color >> 24u) & 255u),
 ) / 255.0;
 out.uv = uv;
 return out;
}

// 0-1 linear  from  0-1 sRGB gamma
fn linear_from_gamma_rgb(srgb: vec3<f32>) -> vec3<f32> {
    let cutoff = srgb < vec3<f32>(0.04045);
    let lower = srgb / vec3<f32>(12.92);
    let higher = pow((srgb + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4));
    return select(higher, lower, cutoff);
}

// 0-1 sRGB gamma  from  0-1 linear
fn gamma_from_linear_rgb(rgb: vec3<f32>) -> vec3<f32> {
    let cutoff = rgb < vec3<f32>(0.0031308);
    let lower = rgb * vec3<f32>(12.92);
    let higher = vec3<f32>(1.055) * pow(rgb, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(higher, lower, cutoff);
}

// 0-1 sRGBA gamma  from  0-1 linear
fn gamma_from_linear_rgba(linear_rgba: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(gamma_from_linear_rgb(linear_rgba.rgb), linear_rgba.a);
}

@fragment fn fs_main(in: VertexOut) -> @location(0) vec4f {
    let tex = textureSample(u_texture, u_texture_sampler, in.uv);

    return in.color * tex;
}
