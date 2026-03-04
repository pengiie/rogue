module rt_prepass;

import lib;

struct ShaderUniforms {
  [format("rgba32f")]
  WTexture2D<float4> backbuffer;
  [format("r16f")]
  WTexture2D<float> backbuffer_depth;
}

ParameterBlock<ShaderUniforms> u_shader;

[shader("compute")]
[numthreads(8, 8, 1)]
void main(uint3 id: SV_DispatchThreadID) {
  var dimensions: uint2;
  u_shader.backbuffer.GetDimensions(dimensions.x, dimensions.y);

  let coord = id.xy;
  if (coord.x >= dimensions.x || coord.y >= dimensions.y) {
    return;
  }
  test_pixel = coord;
  random::init(uint3(coord.xy, u_frame.frame_info.time_ms));

  let ray = u_frame.world_info.camera.create_ray((float2) coord, (float2) dimensions);
  let sky_color = Sky.get().sample_sky(ray);
  var depth = u_frame.world_info.camera.far_plane;

  var normal = float3(0);
  var out_color = sky_color;

  let ray_terrain = voxel::Terrain.get().trace(ray);
  if (ray_terrain.did_hit && ray_terrain.depth_t < depth) {
    depth = ray_terrain.depth_t;
    out_color = ray_terrain.albedo;
    normal = normalize(ray_terrain.normal);
  }

  //out_color = float3(depth / 100.0);

  // If the hit entity is closer than the hit terrain, use the
  // entities hit.
  //let ray_entity = voxel::EntityData.trace(ray);
  //if (ray_entity.did_hit && ray_entity.depth_t < depth) {
  //  depth = ray_entity.depth_t;
  //  out_color = ray_entity.color;
  //  normal = normalize(ray_entity.normal);
  //}

  if (any(normal != float3(0))) {
    let sun_dir = Sky.get().sun_dir;
    let l = clamp(pow(dot(normal, sun_dir) + 0.3, 2.0), 0.035, 1.0);
    out_color = out_color * l;
  }

  let normalized_depth = depth / u_frame.world_info.camera.far_plane;
  u_shader.backbuffer_depth.Store(coord.xy, normalized_depth);
  u_shader.backbuffer.Store(coord.xy, float4(out_color, 1.0));
}
