module debug_shapes;

import lib;

struct Line {
  float3 start;
  uint flags;
  float3 end;
  float thickness;
  float4 color;
}

struct Uniforms {
  [format("rgba8")]
  RWTexture2D<float4> backbuffer;
  [format("r16f")]
  Texture2D<float> backbuffer_depth;
  StructuredBuffer<Line> lines;
  uint line_count;
}

extern ParameterBlock<Uniforms> u_shader;

[shader("compute")]
[numthreads(8, 8, 1)]
void main(uint3 id: SV_DispatchThreadID) {
  var dimensions: uint2;
  u_shader.backbuffer.GetDimensions(dimensions.x, dimensions.y);

  let coord = id.xy;
  if (coord.x >= dimensions.x || coord.y >= dimensions.y) {
    return;
  }

  let far_plane = u_frame.world_info.camera.far_plane;

  var advanced = 0.0;
  var ray = u_frame.world_info.camera.create_ray(coord, dimensions);
  var color = float4(0.0, 0.0, 0.0, 0.0);
  for (uint k = 0; k < 1000 && advanced < far_plane; k++) {
    var min_distance = far_plane;
    var is_xray = false;
    var normal = float3(0.0);
    var c = float4(0.0);
    for (uint i = 0; i < u_shader.line_count; i++) {
      var li = u_shader.lines.Load(i);

      var d = sdf_sphere(ray.origin, li.start, li.thickness);
      // if (d < min_distance) {
      //   min_distance = d;
      //   normal = normalize(ray.origin - li.start);
      //   c = li.color;
      // }

      // d = sdf_sphere(ray.origin, li.end, li.thickness);
      // if (d < min_distance) {
      //   min_distance = d;
      //   normal = normalize(ray.origin - li.end);
      //   c = li.color;
      // }

      d = sdf_line_segment(ray.origin, li.start, li.end, li.thickness);
      let is_line_xray = (li.flags & 1) > 0;
      if (!is_line_xray && d < min_distance) {
        min_distance = d;
        normal = sdf_line_segment_normal(ray.origin, li.start, li.end);
        c = li.color;
      }
    }
    if (min_distance < 0.01) {
      float l = pow(dot(normalize(float3(1)), normal) * 0.5 + 0.55, 1.53);
      //l = dot(normalize(float3(1)), normal);
      color = float4(l * c.rgb, c.a);
      break;
    }
    ray.advance(min_distance);
    advanced += min_distance;
  }
    
    // Screen space line and thickness approach.
    // let projected_start = mul(u_frame.world_info.camera.proj_view, float4(li.start, 1.0));
    // let projected_end = mul(u_frame.world_info.camera.proj_view, float4(li.end, 1.0));

    // var start_pixels = (float2) dimensions * ((projected_start.xy / projected_start.w) * 0.5 + 0.5);
    // start_pixels.y = (float) dimensions.y - start_pixels.y;
    // start_pixels += 0.5;
    // var end_pixels = (float2) dimensions * ((projected_end.xy / projected_end.w) * 0.5 + 0.5);
    // end_pixels.y = (float) dimensions.y - end_pixels.y;
    // end_pixels += 0.5;

    // let min_p = min(start_pixels.xy, end_pixels.xy);
    // let max_p = max(start_pixels.xy, end_pixels.xy);

    // //printf("projected_start is %f %f %f %f\n", projected_start.x, projected_start.y, projected_start.z, projected_start.w);
    // // printf("projected_end is %f %f %f %f\n", projected_end.x, projected_end.y, projected_end.z, projected_end.w);
    // //printf("start is %f %f, end is %f %f \n", start_pixels.x, start_pixels.y, end_pixels.x, end_pixels.y);
    // //
    // let coord_center = (float2) coord + 0.5;
    // let start_thickness = max((dimensions.y * 0.5 * li.thickness) / projected_start.w, 1.0);
    // let start_d = distance(coord_center, start_pixels);
    // let end_thickness = max((dimensions.y * 0.5 * li.thickness) / projected_end.w, 1.0);
    // let end_d = distance(coord_center, end_pixels);
    // if (start_d <= start_thickness && projected_start.z >= 0) {
    //   let d = start_d / start_thickness;
    //   let offset = sqrt(1.0 + d*d);
    //   closest_t = projected_start.z+ offset;
    //   color = float4(1.0,0.0,0.0,1.0);
    // }
    // if (end_d <= end_thickness && projected_end.z >= 0) {
    //   let d = end_d / end_thickness;
    //   let offset = sqrt(1.0 + d*d);
    //   if (projected_end.z + offset < closest_t) {
    //     color = float4(0.0,1.0,0.0,1.0);
    //   }
    // }

    //   let slope = (end_pixels.y - start_pixels.y) / (end_pixels.x - start_pixels.x);
    //   let x = (float) (coord_center.x - start_pixels.x);
    //   let expected_y = start_pixels.y + x * slope;
    //   let z_t = abs(x) / abs(end_pixels.x - start_pixels.x);
    //   let z = lerp(projected_start.w, projected_end.w, 0.0);
    //   if (z < u_frame.world_info.camera.near_plane) {
    //     continue;
    //   }

    //   let scaled_thickness = max((dimensions.y * 0.5 * li.thickness) / z, 1.0);
    //   if (abs(x) < abs(end_pixels.x - start_pixels.x)) {
    //     if (distance(coord_center, float2(coord_center.x, expected_y)) <= scaled_thickness) {
    //       let opacity = 1.0;
    //       //var opacity = li.color.a * min(scaled_thickness, 1.0);
    //       //let aa_t = abs(scaled_thickness - abs(coord_center.y - expected_y));
    //       //let aa_limit = 0.4;
    //       //if (aa_t < aa_limit) {
    //       //  opacity *= aa_t * 2.0;
    //       //}
    //       color = float4(li.color.rgb, li.color.a * opacity);
    //     }
    //   }
  //}

  var depth_dimensions: uint2;
  u_shader.backbuffer_depth.GetDimensions(depth_dimensions.x, depth_dimensions.y);
  let uv = (float2) coord.xy / (float2) dimensions;
  let depth_coord = (uint2) (uv * depth_dimensions);
  let normalized_depth = u_shader.backbuffer_depth.Load(uint3(depth_coord, 0));
  if (normalized_depth * far_plane < advanced) {
    return;
  }
  let original = u_shader.backbuffer.Load(coord.xy);
  let final = float4(lerp(original.xyz, color.xyz, color.a), 1.0);
  u_shader.backbuffer.Store(coord.xy, final);
}
