// Ray -------------------------------------------------

struct Ray {
  origin: vec3f,
  dir: vec3f,
  inv_dir: vec3f
};

fn ray_advance(ray: Ray, t: f32) -> Ray {
  return Ray(ray.origin + t * ray.dir, ray.dir, ray.inv_dir);
}

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

// Finds the intersections of the axes planes with the origin of this point.
fn ray_to_point(ray: Ray, point: vec3f) -> vec3f {
  return ray.inv_dir * (point - ray.origin);
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

fn esvo_next_octant(ray: Ray, aabb: AABB, tEnter: f32) -> u32 {
  let tCenter = ray_to_point(ray, aabb.center);

  var octant = 0u;
  if (tCenter.x <= tEnter) {
    if (ray.dir.x >= 0.0) {
      octant |= 1u;
     } 
  } else {
    if (ray.dir.x < 0.0) {
      octant |= 1u;
    }
  }

  // Note: Passing through the y plane with dir +y is
  if (tCenter.y <= tEnter) {
    if (ray.dir.y >= 0.0) {
      octant |= 2u;
     } 
  } else {
    if (ray.dir.y < 0.0) {
      octant |= 2u;
    }
  }

  if (tCenter.z <= tEnter) {
    if (ray.dir.z >= 0.0) {
      octant |= 4u;
    } 
  } else {
    if (ray.dir.z < 0.0) {
      octant |= 4u;
    }
  }

  return octant;
}

fn esvo_next_octant_aabb(aabb: AABB, octant: u32) -> AABB {
  let signs = vec3f(
    f32(octant & 1) * 2.0 - 1.0,
    f32((octant & 2) >> 1) * 2.0 - 1.0,
    f32((octant & 4) >> 2) * 2.0 - 1.0,
  );

  let side_length = aabb.side_length / 2.0;
  return AABB(aabb.center + side_length * signs, side_length);
}

fn esvo_next_octant_aabb_reverse(aabb: AABB, octant: u32) -> AABB {
  let signs = vec3f(
    f32(octant & 1) * -2.0 + 1.0,
    f32((octant & 2) >> 1) * -2.0 + 1.0,
    f32((octant & 4) >> 2) * -2.0 + 1.0,
  );

  return AABB(aabb.center + aabb.side_length * signs, aabb.side_length * 2.0);
}

// Calculates the normal vector from the face the Ray intersects the AABB.
fn esvo_ray_aabb_normal(ray_aabb_info: RayAABBInfo, ray: Ray) -> vec3f {
  return vec3f(
    f32(ray_aabb_info.t_min.x == ray_aabb_info.t_enter) * -sign(ray.dir.x),
    f32(ray_aabb_info.t_min.y == ray_aabb_info.t_enter) * -sign(ray.dir.y),
    f32(ray_aabb_info.t_min.z == ray_aabb_info.t_enter) * -sign(ray.dir.z),
  );
}

// Returns the raw attachment ptr into the raw buffer, or 0xFFFF_FFFF if it doesn't exist.
fn esvo_get_node_attachment_ptr(esvo_data_ptrs_ptr: u32,
                                attachment_index: u32,
                                node_index: u32,
                                octant: u32) -> u32 {
  let node_data_ptr = u_world_acceleration.data[esvo_data_ptrs_ptr];
  let attachment_lookup_ptr = u_world_acceleration.data[esvo_data_ptrs_ptr + 1 + attachment_index];
  let attachment_raw_ptr = u_world_acceleration.data[esvo_data_ptrs_ptr + 2 + attachment_index];

  // Get the current bucket info index
  let page_header_index = node_index & ~(0x1FFFu);
  let bucket_info_index = page_header_index + u_world_data.data[node_data_ptr + page_header_index];

  // Coincides with page header, the start index of this bucket. 
  let bucket_info_ptr = node_data_ptr + bucket_info_index;
  let bucket_start_index = u_world_data.data[bucket_info_ptr]; 
  let lookup_offset = node_index - bucket_start_index;
  let lookup_index = u_world_data.data[bucket_info_ptr + 1] + lookup_offset;
  let lookup_info = u_world_data.data[attachment_lookup_ptr + lookup_index];

  // Check that the voxel has this attachment type.
  let attachment_mask = lookup_info & 0xFFu;
  let octant_bit = 0x1u << octant;
  let has_attachment = (attachment_mask & octant_bit) > 0;
  if(!has_attachment) {
    return 0xFFFFFFFFu;
  }

  let word_size = 1u;
  // TODO: Switch on the attachment index for word size changes.
  let raw_offset = countOneBits(attachment_mask & (octant_bit - 1));
  let raw_ptr = attachment_raw_ptr + (lookup_info >> 8) + raw_offset * word_size;

  return raw_ptr;
}

struct VoxelModelTrace {
  hit: bool,
  radiance: vec3f,
}

fn voxel_model_trace_miss() -> VoxelModelTrace {
  return VoxelModelTrace(false, vec3f(0.0));
}

fn voxel_model_trace_hit(radiance: vec3f) -> VoxelModelTrace {
  return VoxelModelTrace(true, radiance);
}

struct ESVOStackItem {
  node_index: u32,
  octant: u32,
}

fn esvo_trace(voxel_model: VoxelModelHit, ray: Ray) -> VoxelModelTrace {
  let node_data_ptr = u_world_acceleration.data[voxel_model.data_ptrs_ptr];
  let root_hit_info = voxel_model.hit_info;
  let root_aabb = voxel_model.aabb;

  // TODO: Dynamically choose a stack size given the voxel model being rendered.
  var stack = array<ESVOStackItem, 8>();

  var curr_octant = esvo_next_octant(ray, voxel_model.aabb, root_hit_info.t_enter);
  var curr_aabb = esvo_next_octant_aabb(voxel_model.aabb, curr_octant);
  // 1 is the root node since 0 is a page header.
  var curr_node_index = 1u; 
  var curr_node_data = u_world_data.data[node_data_ptr + curr_node_index];
  var curr_height = 0u;
  var should_push = true;

  for (var i = 0; i < 16; i++) {
    let curr_hit_info = ray_to_aabb(ray, curr_aabb);
    let value_mask = (curr_node_data >> 8) & 0xFF;
    let in_octant = (value_mask & (0x1u << curr_octant)) > 0;

    if (in_octant && should_push) {
      let is_leaf = (curr_node_data & (0x1u << curr_octant)) > 0;
      if (is_leaf) {
        let albedo_ptr = esvo_get_node_attachment_ptr(
          voxel_model.data_ptrs_ptr,
          0u,
          curr_node_index,
          curr_octant
        );

        // Check if this voxel has albedo, if it doesn't then skip it.
        if (albedo_ptr != 0xFFFFFFFFu) {
          let compresed_albedo = u_world_data.data[albedo_ptr];
          let albedo = vec3f(
            f32((compresed_albedo >> 24u) & 0xFFu) / 255.0,
            f32((compresed_albedo >> 16u) & 0xFFu) / 255.0,
            f32((compresed_albedo >> 8u) & 0xFFu) / 255.0,
          );
          return voxel_model_trace_hit(albedo);
        }
      }
      
      stack[curr_height] = ESVOStackItem(curr_node_index, curr_octant);
      curr_height += 1u;

      let child_offset = curr_node_data >> 17;
      let octant_offset = countOneBits(value_mask & ((1u << curr_octant) - 1));
      curr_octant = esvo_next_octant(ray, curr_aabb, curr_hit_info.t_enter);
      curr_aabb = esvo_next_octant_aabb(curr_aabb, curr_octant);
      curr_node_index = curr_node_index + child_offset + octant_offset;
      curr_node_data = u_world_data.data[node_data_ptr + curr_node_index];

      continue;
    }

    let exit = vec3<bool>(
      curr_hit_info.t_exit == curr_hit_info.t_max.x,
      curr_hit_info.t_exit == curr_hit_info.t_max.y,
      curr_hit_info.t_exit == curr_hit_info.t_max.z,
    );

    let exit_axes = u32(exit.x) | 
                    (u32(exit.y) << 1) |
                    (u32(exit.z) << 2);
    let advanced_octant = curr_octant ^ exit_axes;
    var should_pop = false;
    if(((advanced_octant & 1u) > (curr_octant & 1u)) && ray.dir.x < 0) {
      should_pop = true;
    }
    if(((advanced_octant & 2u) > (curr_octant & 2u)) && ray.dir.y < 0) {
      should_pop = true;
    }
    if(((advanced_octant & 4u) > (curr_octant & 4u)) && ray.dir.z < 0) {
      should_pop = true;
    }

    if(((advanced_octant & 1u) < (curr_octant & 1u)) && ray.dir.x > 0) {
      should_pop = true;
    }
    if(((advanced_octant & 2u) < (curr_octant & 2u)) && ray.dir.y > 0) {
      should_pop = true;
    }
    if(((advanced_octant & 4u) < (curr_octant & 4u)) && ray.dir.z > 0) {
      should_pop = true;
    }

    // Don't push next iteration when popping so we can advance an octant first.
    should_push = !should_pop;
    if (should_pop) {
      if (curr_height == 0u) {
        break;
      }
      curr_height -= 1u;

      let item: ESVOStackItem = stack[curr_height];
      curr_aabb = esvo_next_octant_aabb_reverse(curr_aabb, curr_octant);
      curr_octant = item.octant;
      curr_node_index = item.node_index;
      curr_node_data = u_world_data.data[node_data_ptr + curr_node_index];
    } else {
      curr_aabb.center += (curr_aabb.side_length * 2.0) * vec3f(exit) * sign(ray.dir); 
      curr_octant = advanced_octant;
    }
  }

  return voxel_model_trace_miss();
}

// WORLD SPACE RAY CONSTRUCTION AND TRAVERSAL

struct VoxelModelHit {
  // The pointer to where this model's data ptrs are located in u_world_acceleration.
  data_ptrs_ptr: u32,
  schema: u32,
  hit_info: RayAABBInfo,
  aabb: AABB,
}

fn get_next_voxel_model(ray: Ray) -> VoxelModelHit {
  var closest: VoxelModelHit = VoxelModelHit(0, 0, 
                                             RayAABBInfo(0.0, 0.0, vec3f(0.0), vec3f(0.0), false), 
                                             AABB(vec3f(0.0), vec3f(0.0)));
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

    let aabb = aabb_min_max(min, max);
    let hit_info = ray_to_aabb(ray, aabb);
    if (hit_info.hit && hit_info.t_enter < min_t) {
      min_t = hit_info.t_enter;
      closest = VoxelModelHit(current_index + 8,
                              u_world_acceleration.data[current_index + 1],
                              hit_info,
                              aabb);
    }

    current_index = current_index + model_data_size;
  }

  return closest;
}

fn trace_voxel_model(voxel_model: VoxelModelHit, ray: Ray) -> VoxelModelTrace {
  switch (voxel_model.schema) {
    case 1u: {
      // TODO: Check the height of the esvo and choose the appropriate esvo_trace with stack size closest.
      return esvo_trace(voxel_model, ray);    
    }
    default: {
      return voxel_model_trace_miss();
    }
  }

}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
  let dimensions = textureDimensions(u_backbuffer);
  let coords = id.xy;
  if (coords.x >= dimensions.x || coords.y >= dimensions.y) {
    return;
  }
  init_seed(coords);

  // Generate ray depending on camera and a random offset within the pixel.
  let offset = vec2f(
    f32(u_world_info.frame_count % 4) * 0.25 - 0.5,
    f32((u_world_info.frame_count % 16) / 4) * 0.25 - 0.5
  );
  
  // Comment or uncomment offset depending if TAA is enabled.
  let ndc = (vec2f(coords) + offset) / vec2f(dimensions);
  let uv = vec2f(ndc.x * 2.0 - 1.0, 1.0 - ndc.y * 2.0);

  let aspect_ratio = f32(dimensions.x) / f32(dimensions.y);
  var scaled_uv = vec2f(uv.x * aspect_ratio, uv.y) * tan(u_world_info.camera.half_fov);

  let ct = u_world_info.camera.transform;
  let ray_origin = vec3f(ct[0][3], ct[1][3], ct[2][3]);
  let ray_dir = normalize(vec3f(scaled_uv, 1.0) * u_world_info.camera.rotation);
  let inv_ray_dir = 1.0 / ray_dir;
  let ray = Ray(ray_origin, ray_dir, inv_ray_dir);

  // Linear scale to make the room a box skybox
  //let linear_scale = 1.0 / max(max(abs(rayDir.x), abs(rayDir.y)), abs(rayDir.z));
  //var radiance = vec3f(srgb_to_lrgb((rayDir * linear_scale + 1) * 0.5));

  // Colors each axes face rgb (xyz) irrespective of sign. Sinusoidal-like interpolation
  //var radiance = vec3f(srgb_to_lrgb(abs(rayDir)));

  // Colors each axes on the unit circle interpolating linearly based on ray angle.

  var radiance: vec3f;
  var curr_ray = ray;
  var curr_voxel_model = get_next_voxel_model(curr_ray);
  for (var i = 0; i < 100; i++) {
    if (curr_voxel_model.schema == 0) {
      var background_color = vec3f(srgb_to_lrgb(vec3f(acos(-ray_dir) / 3.14)));

      // Draw the XZ plane grid.
      let t_axes = ray_to_point(ray, vec3f(0.0));
      if (t_axes.y > 0.0) {
        let grid_xz = (ray.origin + ray.dir * t_axes.y).xz; 
        let f = modf(abs(grid_xz));
        let LINE_WIDTH: f32 = 0.02;
        let HALF_LINE_WIDTH: f32 = LINE_WIDTH * 0.5;
        let GRID_COLOR = srgb_to_lrgb(vec3f(0.75));
        let GRID_X_COLOR = srgb_to_lrgb(vec3f(1.0, 0.0, 0.0));
        let GRID_Z_COLOR = srgb_to_lrgb(vec3f(0.0, 0.0, 1.0));

        var color = vec3f(0.0);
        var influence = 0.0;
        // We are on the X-axis.
        if (f.fract.x < HALF_LINE_WIDTH) {
          influence = distance(grid_xz, ray.origin.xz);
          color = GRID_COLOR;
          if (f.whole.x == 0.0) {
            color = GRID_X_COLOR;
          } else {
            color = mix(mix(GRID_X_COLOR, GRID_COLOR, 0.3),
                        GRID_COLOR,
                        smoothstep(0.0, 1.0, abs(grid_xz.x - ray.origin.x)));
          }
        }
        // We are on the Z-axis.
        if (f.fract.y < HALF_LINE_WIDTH) {
          influence = distance(grid_xz, ray.origin.xz);
          color = GRID_COLOR;
          if (f.whole.y == 0.0) {
            // Fully color the XZ axes
            color = GRID_Z_COLOR;
          } else {
            // Color the xz lines depending on how close the line is to the ray.
            color = mix(mix(GRID_Z_COLOR, GRID_COLOR, 0.3),
                        GRID_COLOR,
                        smoothstep(0.0, 1.0, abs(grid_xz.y - ray.origin.z)));
          }
        }

        let RADIUS_OF_GRID = 20.0;
        let FADE_DISTANCE = 3.0;
        if (influence != 0.0) {
          // Fade out the grid over a distance of FADE_DISTANCE
          influence = 1.0 - smoothstep(RADIUS_OF_GRID - FADE_DISTANCE,
                                       RADIUS_OF_GRID + FADE_DISTANCE,
                                       influence);
        }
        background_color = mix(background_color, color, influence);
      }

      radiance = background_color;
      break;
    }

    let trace_result = trace_voxel_model(curr_voxel_model, curr_ray);
    if (trace_result.hit) {
      radiance = trace_result.radiance;
      break;
    }

    // Reposition the ray right after the last voxel model including some epsilon.
    let RAY_EXIT_EPSILON: f32 = 0.0001;
    curr_ray = ray_advance(curr_ray, curr_voxel_model.hit_info.t_exit + RAY_EXIT_EPSILON);
    curr_voxel_model = get_next_voxel_model(curr_ray);
  }

  var radiance_prev = vec3f(0.0);
  if (u_world_info.frame_count > 1) {
    radiance_prev = textureLoad(u_radiance_total_prev, coords, 0).xyz;
  }

  // Store the total radiance so we can average it allowing for temporal effects.
  let total_radiance = radiance_prev + radiance;
  textureStore(u_radiance_total, coords, vec4f(total_radiance, 0.0));

  var avg_radiance = total_radiance / f32(u_world_info.frame_count);
  // Uncomment to not apply any temporal effects.
  // avg_radiance = radiance;

  // Convert to sRGB then dither to avoid any banding.
  let gamma_corrected_color = lrgb_to_srgb(avg_radiance);
  let backbuffer_color = vec4f(dither(gamma_corrected_color), 1.0);

  textureStore(u_backbuffer, coords, backbuffer_color);  
}
