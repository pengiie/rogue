// Ray -------------------------------------------------

struct Ray {
  origin: vec3f,
  dir: vec3f,
  invDir: vec3f
};

struct AABB {
  center: vec3f,
  side_length: vec3f
};

fn aabb_min_max(min: vec3f, max: vec3f) -> AABB {
  let length = max - min;
  let side_length = length / 2.0;
  let center = min + side_length;

  return AABB(center, side_length);
}

// ESVO -------------------------------------------------

struct ESVONodes {
  data: array<u32>
};

struct ESVOLookup {
  data: array<u32>
};

struct ESVOAttachment {
  data: array<u32>
};

// voxel_trace ------------------------------------------

struct TraceResult {
  albedo: vec4f,
};

struct Camera {
  transform: mat4x4f,
  rotation: mat3x3f,
  half_fov: f32,
};

struct WorldInfo {
  camera: Camera,
  voxel_model_count: u32,
  frame_count: u32,
};

struct WorldAcceleration {
  data: array<u32>
}

struct WorldData {
  data: array<u32>
}

@group(0) @binding(0) var u_backbuffer: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(1) var u_radiance_total: texture_storage_2d<rgba32float, write>;
@group(0) @binding(2) var u_radiance_total_prev: texture_2d<f32>;
@group(0) @binding(3) var<uniform> u_world_info: WorldInfo; 
@group(0) @binding(4) var<storage, read> u_world_acceleration: WorldAcceleration; 
@group(0) @binding(5) var<storage, read> u_world_data: WorldData; 

fn ray_to_point(ray: Ray, point: vec3f) -> vec3f {
  return ray.invDir * (point - ray.origin);
}

struct RayAABBInfo {
  // The ray t-value the ray enters the aabb.
  t_enter: f32,
  // The ray t-value the ray exits the aabb.
  t_exit: f32,
  // The ray t-values the ray intersects the min point axes.
  t_min: vec3f,
  // The ray t-values the ray intersects the max point axes.
  t_max: vec3f,
  // A hit if t_exit is greater than t_enter
  hit: bool
}

fn ray_to_aabb(ray: Ray, aabb: AABB) -> RayAABBInfo {
  let t0 = ray_to_point(ray, aabb.center - aabb.side_length);
  let t1 = ray_to_point(ray, aabb.center + aabb.side_length);
  let t_min = min(t0, t1);
  let t_max = max(t0, t1);

  var temp = max(t_min.xx, t_min.yz);
  let t_enter = max(max(temp.x, temp.y), 0.0);
  temp = min(t_max.xx, t_max.yz);
  let t_exit = min(temp.x, temp.y);

  let hit = t_exit > t_enter;

  return RayAABBInfo(t_enter, t_exit, t_min, t_max, hit);
}

// fn aabb_octant(ray: Ray, aabb: AABB, tEnter: f32) -> u32 {
//   let tCenter = ray_to_point(ray, aabb.center);
// 
//   var octant = 0u;
//   if (tCenter.x <= tEnter) {
//     if (ray.dir.x >= 0.0) {
//       octant |= 1u;
//      } 
//   } else {
//     if (ray.dir.x < 0.0) {
//       octant |= 1u;
//     }
//   }
// 
//   if (tCenter.y <= tEnter) {
//     if (ray.dir.y >= 0.0) {
//       octant |= 2u;
//      } 
//   } else {
//     if (ray.dir.y < 0.0) {
//       octant |= 2u;
//     }
//   }
// 
//   if (tCenter.z <= tEnter) {
//     if (ray.dir.z >= 0.0) {
//       octant |= 4u;
//      } 
//   } else {
//     if (ray.dir.z < 0.0) {
//       octant |= 4u;
//     }
//   }
//   return octant;
// }
// 
// fn child_aabb(aabb: AABB, octant: u32) -> AABB {
//   let signs = vec3f(
//     f32(octant & 1) * 2.0 - 1.0,
//     f32((octant & 2) >> 1) * 2.0 - 1.0,
//     f32((octant & 4) >> 2) * 2.0 - 1.0,
//   );
// 
//   let side_length = aabb.side_length / 2.0;
//   return AABB(aabb.center + side_length * signs, side_length);
// }
// 
// fn get_node_child_albedo(node_ptr: u32, octant: u32) -> vec3f {
//   let page_header_ptr = node_ptr & ~(0x1FFFu);
//   let block_info_ptr = page_header_ptr + u_esvo_nodes.data[page_header_ptr];
//   let block_start_ptr = u_esvo_nodes.data[block_info_ptr]; // Coincides with page header. 
//   let lookup_offset = node_ptr - block_start_ptr;
//   let lookup_ptr = u_esvo_nodes.data[block_info_ptr + 1] + lookup_offset;
//   let lookup_info = u_esvo_lookup.data[lookup_ptr];
//   let attachment_mask = lookup_info & 0xffu;
//   let octant_bit = 0x1u << octant;
//   let has_attachment = (attachment_mask & octant_bit) > 0;
//   if(!has_attachment) {
//     return vec3f(0.7, 0.8, 0.2);
//   }
// 
//   let raw_offset = countonebits(attachment_mask & (octant_bit - 1));
//   let raw_ptr = (lookup_info >> 8) + raw_offset;
//   let raw_packed_albedo = u_esvo_attachment.data[raw_ptr];
//   let albedo_r = raw_packed_albedo >> 24;
//   let albedo_g = (raw_packed_albedo >> 16) & 0xFFu;
//   let albedo_b = (raw_packed_albedo >> 8) & 0xFFu;
//   let albedo = vec3f(f32(albedo_r) / 255.0, f32(albedo_g) / 255.0, f32(albedo_b) / 255.0);
// 
//   return albedo;
// }
// 
// struct stackitem {
//   aabb: aabb,
//   node: u32,
//   pointer: u32,
//   octant: u32,
// }
//
// fn trace_esvo(ray: ray, root: aabb) -> traceResult {
//   let root_intersection = ray_to_aabb(ray, root);
//   if(root_intersection.hit) {
//     var curr_ptr = 1u;
//     var curr_node = u_esvo_nodes.data[curr_ptr];
//     var curr_octant = aabb_octant(ray, root, root_intersection.tEnter);
//     var curr_aabb = child_aabb(root, curr_octant);
//     var height = 0;
//     var should_push = true;
//     var stack = array<stackitem, 15>();
// 
//     var color = vec4f(0.1, 0.1, 0.1, 1);
//     for (var i = 0; (i < 500 && height >= 0); i++) {
//       var curr_intersection = ray_to_aabb(ray, curr_aabb);
// 
//       let in_octant = (curr_node & (0x100u << curr_octant)) > 0;
//       if (should_push && in_octant) {
//         let is_leaf = (curr_node & (0x1u << curr_octant)) > 0;
//         let child_ptr = curr_node >> 17;
//         if (is_leaf) {
//           let color = get_node_child_albedo(curr_ptr, curr_octant);
//           return traceresult(vec4f(color, 1));
//         }
//         stack[height] = stackitem(curr_aabb, curr_node, curr_ptr, curr_octant);
// 
//         curr_ptr += child_ptr;
//         curr_node = u_esvo_nodes.data[curr_ptr];
//         curr_octant = aabb_octant(ray, curr_aabb, curr_intersection.tEnter);
//         curr_aabb = child_aabb(curr_aabb, curr_octant);
//         should_push = true;
//         height++;
//         continue;
//       }
//       let exit = vec3<bool>(
//         curr_intersection.texit == curr_intersection.tMax.x,
//         curr_intersection.texit == curr_intersection.tMax.y,
//         curr_intersection.texit == curr_intersection.tMax.z,
//       );
//       //var color = vec3f(0);
//       //if (exit.x) {
//       //  color.x = 1.0;
//       //}
//       //if (exit.y) {
//       //  color.y = 1.0;
//       //}
//       //if (exit.z) {
//       //  color.z = 1.0;
//       //}
//       //return traceresult(vec4f(color, 1.0));
//       let exit_axes = u32(exit.x) | 
//                       (u32(exit.y) << 1) |
//                       (u32(exit.z) << 2);
//       let advance = curr_octant ^ exit_axes;
//       var should_pop = false;
//       if(((advance & 1u) > (curr_octant & 1u)) && ray.dir.x < 0) {
//         should_pop = true;
//       }
//       if(((advance & 2u) > (curr_octant & 2u)) && ray.dir.y < 0) {
//         should_pop = true;
//       }
//       if(((advance & 4u) > (curr_octant & 4u)) && ray.dir.z < 0) {
//         should_pop = true;
//       }
// 
//       if(((advance & 1u) < (curr_octant & 1u)) && ray.dir.x > 0) {
//         should_pop = true;
//       }
//       if(((advance & 2u) < (curr_octant & 2u)) && ray.dir.y > 0) {
//         should_pop = true;
//       }
//       if(((advance & 4u) < (curr_octant & 4u)) && ray.dir.z > 0) {
//         should_pop = true;
//       }
// 
//       // don't push when popping so we can advance one first.
//       should_push = !should_pop;
//       if (should_pop) {
//         height--;
//         let item: stackitem = stack[height];
//         curr_aabb = item.aabb;
//         curr_octant = item.octant;
//         curr_node = item.node;
//         curr_ptr = item.pointer;
//       } else {
//         curr_aabb.center += (curr_aabb.side_length * 2.0) * vec3f(exit) * sign(ray.dir); 
//         curr_octant = advance;
//       }
// 
//       //let oct = vec3f(
//       //  f32(octant & 1u),
//       //  f32((octant & 2u) >> 1u),
//       //  f32((octant & 4u) >> 2u),
//       //);
//       //color = vec4f(oct, 1);
//     }
// 
//     return traceresult(color);
//   }
// 
//   return traceresult(vec4f(0, 0, 0, 1));
// }

struct VoxelModelHit {
  // The pointer to where this model's data ptrs are located in u_world_acceleration.
  data_ptrs_ptr: u32,
  data_ptrs_size: u32,
  schema: u32,
  hit_info: RayAABBInfo,
}

fn get_next_voxel_model(ray: Ray) -> VoxelModelHit {
  var closest: VoxelModelHit = VoxelModelHit(0, 0, 0, RayAABBInfo(0.0, 0.0, vec3f(0.0), vec3f(0.0), false));
  var min_t = 100000.0;
  var current_index = 0u;
  for (var _i = 0u; _i < u_world_info.voxel_model_count; _i++) {
    let model_data_size = u_world_acceleration.data[current_index];
    let min = bitcast<vec3<f32>>(vec3<u32>(
      u_world_acceleration.data[current_index + 2],
      u_world_acceleration.data[current_index + 3],
      u_world_acceleration.data[current_index + 4],
    ));
    let max = bitcast<vec3<f32>>(vec3<u32>(
      u_world_acceleration.data[current_index + 5],
      u_world_acceleration.data[current_index + 6],
      u_world_acceleration.data[current_index + 7],
    ));

    let hit_info = ray_to_aabb(ray, aabb_min_max(min, max));
    if (hit_info.hit) {
      closest = VoxelModelHit(current_index + 8,
                              model_data_size - 8,
                              u_world_acceleration.data[current_index + 1],
                              hit_info);
      min_t = hit_info.t_enter;
    }

    current_index = current_index + model_data_size;
  }

  return closest;
}

fn trace_esvo(voxel_model: VoxelModelHit, ray: Ray) {

}

fn trace_voxel_model(voxel_model: VoxelModelHit, ray: Ray) {
  switch (voxel_model.schema) {
    case 1u: {
      trace_esvo(voxel_model, ray);
      break;
    }
    default: {
      break;
    }
  }
}

var<private> seed: u32 = 1;
fn init_seed(coord: vec2<u32>) {
  var n = seed;
  n = (n<<13)^n; n=n*(n*n*15731+789221)+1376312589; // hash by Hugo Elias
  n += coord.y;
  n = (n<<13)^n; n=n*(n*n*15731+789221)+1376312589;
  n += coord.x;
  n = (n<<13)^n; n=n*(n*n*15731+789221)+1376312589;
  seed = n;
}

fn rand() -> u32 {
  seed = seed * 0x343fd + 0x269ec3;
  return (seed >> 16) & 32767;
}

fn frand() -> f32 {
  return f32(rand()) / 32767.0;
}

fn dither(v: vec3f) -> vec3f {
  let n = frand()+frand() - 1.0;  // triangular noise
  return v + n * exp2(-8.0);
}

fn lrgb_to_srgb(rgb: vec3<f32>) -> vec3<f32> {
    let cutoff = rgb < vec3<f32>(0.0031308);
    let lower = rgb * vec3<f32>(12.92);
    let higher = vec3<f32>(1.055) * pow(rgb, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055);
    return select(higher, lower, cutoff);
}

fn srgb_to_lrgb(srgb: vec3<f32>) -> vec3<f32> {
    let cutoff = srgb < vec3<f32>(0.04045);
    let lower = srgb / vec3<f32>(12.92);
    let higher = pow((srgb + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4));
    return select(higher, lower, cutoff);
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
  let dimensions = textureDimensions(u_backbuffer);
  let coords = id.xy;
  if (coords.x >= dimensions.x || coords.y >= dimensions.y) {
    return;
  }
  init_seed(coords);

  let offset = vec2f(
    f32(u_world_info.frame_count % 4) * 0.25 - 0.5,
    f32((u_world_info.frame_count % 16) / 4) * 0.25 - 0.5
  );
  
  let ndc = (vec2f(coords) + offset) / vec2f(dimensions);
  let uv = vec2f(ndc.x * 2.0 - 1.0, 1.0 - ndc.y * 2.0);

  let aspect_ratio = f32(dimensions.x) / f32(dimensions.y);
  var scaled_uv = vec2f(uv.x * aspect_ratio, uv.y) * tan(u_world_info.camera.half_fov);

  let ct = u_world_info.camera.transform;
  let rayOrigin = vec3f(ct[0][3], ct[1][3], ct[2][3]);
  let rayDir = normalize(vec3f(scaled_uv, 1.0) * u_world_info.camera.rotation);
  let invRayDir = 1.0 / rayDir;
  let ray = Ray(rayOrigin, rayDir, invRayDir);

  let next_voxel_model = get_next_voxel_model(ray);

  // Linear scale to make the room a box skybox
  //let linear_scale = 1.0 / max(max(abs(rayDir.x), abs(rayDir.y)), abs(rayDir.z));
  //var radiance = vec3f(srgb_to_lrgb((rayDir * linear_scale + 1) * 0.5));

  // Colors each axes face rgb (xyz) irrespective of sign. Sinusoidal-like interpolation
  //var radiance = vec3f(srgb_to_lrgb(abs(rayDir)));

  // Colors each axes on the unit circle interpolating linearly based on ray angle.
  var radiance = vec3f(srgb_to_lrgb(vec3f(acos(-rayDir) / 3.14)));

  if (next_voxel_model.schema != 0) {
    radiance = vec3f(0.75);
    trace_voxel_model(next_voxel_model, ray);
  }

  var radiance_prev = vec3f(0.0);
  if (u_world_info.frame_count > 1) {
    radiance_prev = textureLoad(u_radiance_total_prev, coords, 0).xyz;
  }

  let total_radiance = radiance_prev + radiance;
  textureStore(u_radiance_total, coords, vec4f(total_radiance, 0.0));

  var avg_radiance = total_radiance / f32(u_world_info.frame_count);
  avg_radiance = radiance;

  // Convert to sRGB then dither to avoid any banding.
  let gamma_corrected_color = lrgb_to_srgb(avg_radiance);
  let backbuffer_color = vec4f(dither(gamma_corrected_color), 1.0);

  textureStore(u_backbuffer, coords, backbuffer_color);  
}
