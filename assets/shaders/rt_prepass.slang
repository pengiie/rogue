module rt_prepass;

import lib;

struct ShaderUniforms {
  [format("rgba32f")]
  WTexture2D<float4> backbuffer;
  [format("r16f")]
  WTexture2D<float> backbuffer_depth;
  uint shading_mode;
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
  var face_normal = float3(0);
  var out_color = float4(sky_color, 1.0);

  let ray_terrain = voxel::Terrain.get().trace(ray);
  if (ray_terrain.did_hit && ray_terrain.depth_t < depth) {
    depth = ray_terrain.depth_t;
    out_color = float4(color::srgb_to_lsrgb(ray_terrain.albedo.rgb), ray_terrain.albedo.a);
    normal = normalize(ray_terrain.normal);
    face_normal = ray_terrain.face_normal;
  }

  //out_color = float3(depth / 100.0);

  // If the hit entity is closer than the hit terrain, use the
  // entities hit.
  let ray_entity = voxel::EntityData.trace(ray);
  if (ray_entity.did_hit && ray_entity.depth_t < depth) {
    depth = ray_entity.depth_t;
    out_color = float4(color::srgb_to_lsrgb(ray_entity.albedo.rgb), ray_entity.albedo.a);
    normal = normalize(ray_entity.normal);
    face_normal = ray_entity.face_normal;
  }

  var sun_dir = Sky.get().sun_dir;
  bool is_night = sun_dir.y < 0.0;
  var sun_color = Sky.sun_color();
  if (is_night) {
    sun_dir = -sun_dir;
    sun_color = Sky.moon_color() * 0.3;
  }
  switch (u_shader.shading_mode) {
    case 1: 
      if (any(face_normal != float3(0))) {
        let fn = abs(face_normal);
        let l = max3(1.0 * fn.x, 0.9 * fn.y, 0.8 * fn.z);
        out_color.rgb = out_color.rgb * l * sun_color;
      }
      break;
    case 2: 
      if (any(normal != float3(0))) {
        let l = clamp(pow(dot(normal, sun_dir) * 0.7 + 0.3, 2.0), 0.035, 1.0);
        out_color.rgb = out_color.rgb * l * sun_color;
      }
      break;
    default: break;
  }

  let near = u_frame.world_info.camera.near_plane;
  let far = u_frame.world_info.camera.far_plane;
  let view_depth = depth;
  u_shader.backbuffer_depth.Store(coord.xy, view_depth);
  u_shader.backbuffer.Store(coord.xy, float4(out_color.rgb, 1.0));
}
