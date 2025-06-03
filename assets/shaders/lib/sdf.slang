implementing lib;

public float sdf_sphere(float3 ray_origin, float3 sphere_pos, float radius) {
  return length(ray_origin - sphere_pos) - radius;
}

public float sdf_ring(float3 ray_origin, float3 center, float3 normal, float2 stretch, float thickness) {
  let rd = ray_origin - center; 
  let p = (dot(rd, normal)) * normal;
  let dir = normalize(rd - p);
  let edge = center + dir * stretch.x;
  return distance(edge, ray_origin) - thickness;
}

public float3 sdf_ring_normal(float3 ray_origin, float3 center, float3 normal, float2 stretch, float thickness) {
  let rd = ray_origin - center; 
  let p = (dot(rd, normal)) * normal;
  let dir = normalize(rd - p);
  let edge = center + dir * stretch.x;
  return normalize(ray_origin - edge);
}

public float sdf_line_segment(float3 ray_origin, float3 start, float3 end, float radius) {
  var lv = end - start;
  let rd = ray_origin - start;
  // For projection onto the line itself.
  let p_t = clamp(dot(rd, lv) / dot(lv, lv), 0.0, 1.0);
  let d = length(rd - lv * p_t);
  return d - radius;

  // orthogonal complement then combine projection onto planes
  // var lvo1 = float3(1.0, 0.0, 0.0);
  // var lvo2 = float3(0.0, 1.0, 0.0);
  // if (lv.x != 0) {
  //   lvo1 = float3(-lv.y / lv.x, 1.0, 0.0);
  //   lvo2 = float3(-lv.z / lv.x, 0.0, 1.0);
  // } else if(lv.y != 0) {
  //   lvo1 = float3(1.0, 0.0, 0.0);
  //   lvo2 = float3(0.0, -lv.z / lv.y, 1.0);
  // }
  // let lpo1 = (dot(rd, lvo1) / dot(lvo1, lvo1)) * lvo1;
  // let lpo2 = (dot(rd, lvo2) / dot(lvo2, lvo2)) * lvo2;
  // var d = length(lpo1 + lpo2);
}

public float3 sdf_line_segment_normal(float3 ray_origin, float3 start, float3 end) {
  var lv = end - start;
  let rd = ray_origin - start;
  // For projection onto the line itself.
  let p_t = clamp(dot(rd, lv) / dot(lv, lv), 0.0, 1.0);
  return normalize(rd - lv * p_t);

  // let p = (dot(rd, lv) / dot(lv, lv)) * lv;
  // var lvo1 = float3(1.0, 0.0, 0.0);
  // var lvo2 = float3(0.0, 1.0, 0.0);
  // if (lv.x != 0) {
  //   lvo1 = float3(-lv.y / lv.x, 1.0, 0.0);
  //   lvo2 = float3(-lv.z / lv.x, 0.0, 1.0);
  // } else if(lv.y != 0) {
  //   lvo1 = float3(1.0, 0.0, 0.0);
  //   lvo2 = float3(0.0, -lv.z / lv.y, 1.0);
  // }
  // let lpo1 = (dot(rd, lvo1) / dot(lvo1, lvo1)) * lvo1;
  // let lpo2 = (dot(rd, lvo2) / dot(lvo2, lvo2)) * lvo2;
  // return normalize(lpo1 + lpo2);
}
