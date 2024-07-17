@group(0) @binding(0) var u_backbuffer_img: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
  let dimensions = textureDimensions(u_backbuffer_img);
  let coords = id.xy;
  if (coords.x >= dimensions.x || coords.y >= dimensions.y) {
    return;
  }

  textureStore(u_backbuffer_img, coords, vec4<f32>(vec2f(coords.xy) / vec2f(dimensions), 0, 1));  
}
