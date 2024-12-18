// Constants ---------------------------
const EPSILON: f32 = 0.0001;
const RENDER_INDICES_LENGTH: u32 = 3u;
const NULL_ATTACHMENT: u32 = 0xFFFFFFFF;

const TERRAIN_CHUNK_WORLD_UNIT_LENGTH: f32 = 8.0;

// Morton ----------------------------------

// Split first 10 bits by inserting two 0s to the left of each bit.
fn morton_split_by_2(x: u32) -> u32 {
  var y = x & 0x000003ff; //      00000000000000000000001111111111
  y = (y | (y << 16)) & 0x030000ff; // 00000011000000000000000011111111
  y = (y | (y << 8)) & 0x0300f00f; //  00000011000000001111000000001111
  y = (y | (y << 4)) & 0x030c30c3; //  00000011000011000011000011000011
  y = (y | (y << 2)) & 0x09249249; //  00001001001001001001001001001001
  return y;
}

fn morton_encode_3(x: u32, y: u32, z: u32) -> u32 {
  return morton_split_by_2(x) | (morton_split_by_2(y) << 1) | (morton_split_by_2(z) << 2);
}

fn morton_compact_by_1(x: u32) -> u32 {
  var y = x & 0x55555555; //      01010101010101010101010101010101
  y = (y | (y >> 1)) & 0x33333333; //  00110011001100110011001100110011
  y = (y | (y >> 2)) & 0x0f0f0f0f; //  00001111000011110000111100001111
  y = (y | (y >> 4)) & 0x00ff00ff; //  00000000111111110000000011111111
  y = (y | (y >> 8)) & 0x0000ffff; //  00000000000000001111111111111111
  return y;
}

fn morton_decode_2(morton: u32) -> vec2<u32> {
  return vec2<u32>(morton_compact_by_1(morton), morton_compact_by_1(morton >> 1));
}

fn morton_compact_by_2(x: u32) -> u32 {
  var y = x & 0x09249249; //      00001001001001001001001001001001
  y = (y | (y >> 2)) & 0x030c30c3; //  00000011000011000011000011000011
  y = (y | (y >> 4)) & 0x0300f00f; //  00000011000000001111000000001111
  y = (y | (y >> 8)) & 0x030000ff; //  00000011000000000000000011111111
  y = (y | (y >> 16)) & 0x000003ff; // 00000000000000000000001111111111
  return y;
}

fn morton_decode_3(morton: u32) -> vec3<u32> {
  return vec3<u32>(morton_compact_by_2(morton), morton_compact_by_2(morton >> 1), morton_compact_by_2(morton >> 2));
}

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

// RANDOM ----------------------------------

// A slightly modified version of the "One-at-a-Time Hash" function by Bob Jenkins.
// See https://www.burtleburtle.net/bob/hash/doobs.html
fn jenkins_hash(i: u32) -> u32 {
  var x = i;
  x += x << 10u;
  x ^= x >> 6u;
  x += x << 3u;
  x ^= x >> 11u;
  x += x << 15u;
  return x;
}

var<private> rng_state: u32 = 0u;
const TAU: f32 = 6.28318530717958647692528676655900577;
const PI: f32 = 3.14159265358979323846264338327950288;
fn init_seed(coord: vec2<u32>) {
  var n = 0x12341234u;
  n = (n<<13)^n; n=n*(n*n*15731+789221)+1376312589; // hash by Hugo Elias
  n += coord.y;
  n = (n<<13)^n; n=n*(n*n*15731+789221)+1376312589;
  n += coord.x;
  n = (n<<13)^n; n=n*(n*n*15731+789221)+1376312589;
  // Uncomment for temporal noise.
  n += jenkins_hash(u_world_info.total_frame_count);
  n = (n<<13)^n; n=n*(n*n*15731+789221)+1376312589;

  rng_state = jenkins_hash(n);
}

// The 32-bit "xor" function from Marsaglia G., "Xorshift RNGs", Section 3.
fn xorshift32() -> u32 {
  var x = rng_state;
  x ^= x << 13;
  x ^= x >> 17;
  x ^= x << 5;
  rng_state = x;
  return x;
}

fn rand_u32() -> u32 {
  let x = xorshift32();
  return x;
}

fn rand_f32() -> f32 {
  return bitcast<f32>(0x3f800000u | (rand_u32() >> 9u)) - 1.0;
}

fn rand_unit_vec3f() -> vec3f {
  let phi = rand_f32() * TAU;
  let cos_theta = 1.0 - rand_f32() * 2;
  let sin_theta = sqrt(1.0 - cos_theta * cos_theta);

  return vec3f(
    cos(phi) * sin_theta,
    sin(phi) * sin_theta,
    cos_theta
  );
}

// normal should be normalized.
fn rand_hemisphere(normal: vec3f) -> vec3f {
  let v = rand_unit_vec3f();
  if (dot(normal, v) < 0.0) {
    return -v;
  }

  return v;
}

fn dither(v: vec3f) -> vec3f {
  let n = rand_f32() + rand_f32() - 1.0;  // triangular noise
  return v + n * exp2(-8.0);
}

// COLOR SPACE ----------------------------------

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
  voxel_entity_count: u32,
  frame_count: u32,
  total_frame_count: u32,
};

struct WorldModelInfo {
  data: array<u32>
}

struct WorldAcceleration {
  data: array<u32>
}

struct WorldTerrainAcceleration {
  chunk_origin: vec3<i32>,
  side_length: u32,
  data: array<u32>
}

struct WorldData {
  data: array<u32>
}

@group(0) @binding(0) var u_backbuffer: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(1) var u_radiance_total: texture_storage_2d<rgba32float, write>;
@group(0) @binding(2) var u_radiance_total_prev: texture_2d<f32>;
@group(0) @binding(3) var<uniform> u_world_info: WorldInfo; 
@group(0) @binding(4) var<storage, read> u_world_terrain_acceleration: WorldTerrainAcceleration; 
@group(0) @binding(5) var<storage, read> u_world_model_info: WorldModelInfo; 
@group(0) @binding(6) var<storage, read> u_world_data: WorldData; 

// Finds the intersections of the axes planes with the origin of this point.
fn ray_to_point(ray: Ray, point: vec3f) -> vec3f {
  return ray.inv_dir * (point - ray.origin);
}

struct RayAABBInfo {
  // The ray t-value the ray enters the aabb, where t >= 0.0.
  t_enter: f32,
  // The ray t-value the ray enters the aabb.
  t_enter_unbound: f32,
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
  let t_enter = max(temp.x, temp.y);
  temp = min(t_max.xx, t_max.yz);
  let t_exit = min(temp.x, temp.y);

  let bound_t_enter = max(t_enter, 0.0);
  let hit = t_exit > bound_t_enter;

  return RayAABBInfo(bound_t_enter, t_enter, t_exit, t_min, t_max, hit);
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
fn esvo_get_node_attachment_ptr(esvo_info_data_ptr: u32,
                                attachment_index: u32,
                                node_index: u32,
                                octant: u32) -> u32 {
  let node_data_ptr = u_world_model_info.data[esvo_info_data_ptr];
  let attachment_lookup_ptr = u_world_model_info.data[esvo_info_data_ptr + 1];
  let attachment_raw_ptr = u_world_model_info.data[esvo_info_data_ptr + 1 + RENDER_INDICES_LENGTH + attachment_index];
  if (attachment_lookup_ptr == 0xFFFFFFFFu || attachment_raw_ptr == 0xFFFFFFFFu) {
    return 0xFFFFFFFFu;
  }

  // Get the current bucket info index
  let page_header_index = node_index & ~(0x1FFFu);
  let bucket_info_index = page_header_index + u_world_data.data[node_data_ptr + page_header_index];


  // Coincides with page header, the start index of this bucket. 
  let bucket_info_ptr = node_data_ptr + bucket_info_index;
  let bucket_start_index = u_world_data.data[bucket_info_ptr]; 
  let lookup_offset = node_index - bucket_start_index;
  //let lookup_index = u_world_data.data[bucket_info_ptr + 1] + lookup_offset;
  let lookup_index = lookup_offset;
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

struct RayVoxelHit {
  albedo: vec3f,
  radiance_outgoing: vec3f,

  normal: vec3f,
  sample_position: vec3f,
  hit_position: vec3f,
};

fn ray_voxel_hit_empty() -> RayVoxelHit {
  return RayVoxelHit(vec3f(0.0), vec3f(0.0), vec3f(0.0), vec3f(0.0), vec3f(0.0));
}

struct VoxelModelTrace {
  hit: bool,
  hit_data: RayVoxelHit,
}

fn voxel_model_trace_miss() -> VoxelModelTrace {
  return VoxelModelTrace(false, ray_voxel_hit_empty());
}

fn voxel_model_trace_hit(albedo: vec3f, radiance_outgoing: vec3f, 
                         normal: vec3f, sample_position: vec3f, hit_position: vec3f) -> VoxelModelTrace {
  return VoxelModelTrace(true, RayVoxelHit(albedo, radiance_outgoing, normal, sample_position, hit_position));
}

struct ESVOStackItem {
  node_index: u32,
  octant: u32,
}

fn trace_esvo(voxel_model: VoxelModelHit) -> VoxelModelTrace {
  let node_data_ptr = u_world_model_info.data[voxel_model.model_data_ptr];
  let root_hit_info = voxel_model.hit_info;
  let root_aabb = voxel_model.aabb;
  let ray = voxel_model.ray;

  // TODO: Dynamically choose a stack size given the voxel model being rendered.
  var stack = array<ESVOStackItem, 8>();

  var curr_octant = esvo_next_octant(ray, voxel_model.aabb, root_hit_info.t_enter);
  var curr_aabb = esvo_next_octant_aabb(voxel_model.aabb, curr_octant);
  // 1 is the root node since 0 is a page header.
  var curr_node_index = 1u; 
  var curr_node_data = u_world_data.data[node_data_ptr + curr_node_index];
  var curr_height = 0u;
  var should_push = true;

  var alb = vec3f(0.0);
  for (var i = 0; i < 1028; i++) {
    let curr_hit_info = ray_to_aabb(ray, curr_aabb);
    let value_mask = (curr_node_data >> 8) & 0xFF;
    let in_octant = (value_mask & (0x1u << curr_octant)) > 0;

    alb = vec3f(f32(i + 1) / 128.0);
    if (in_octant && should_push) {
      let is_leaf = (curr_node_data & (0x1u << curr_octant)) > 0;
      if (is_leaf) {
        let material_ptr = esvo_get_node_attachment_ptr(
          voxel_model.model_data_ptr,
          0u,
          curr_node_index,
          curr_octant
        );

        if (material_ptr == 0xFFFFFFF1u) {
          alb = vec3f(1.0, 0.0, 1.0);
          break;
        }

        // Check if this voxel has a material, if it doesn't then skip it.
        if (material_ptr != 0xFFFFFFFFu) {
          let hit_position = ray.origin + ray.dir * curr_hit_info.t_enter;
          alb = vec3f(1.0, 0.0, 0.0);

          let compresed_material = u_world_data.data[material_ptr];
          let material_type = compresed_material >> 30;

          // Process the diffuse material.
          if (material_type == 0u) {
            let albedo = srgb_to_lrgb(vec3f(
              f32((compresed_material >> 16u) & 0xFFu) / 255.0,
              f32((compresed_material >> 8u) & 0xFFu) / 255.0,
              f32(compresed_material & 0xFFu) / 255.0,
            ));

            let normal = vec3f(vec3<bool>(
              curr_hit_info.t_min.x == curr_hit_info.t_enter,
              curr_hit_info.t_min.y == curr_hit_info.t_enter,
              curr_hit_info.t_min.z == curr_hit_info.t_enter,
            ));

            let l = normal.x + normal.y * 0.6 + normal.z * 0.4;
            return voxel_model_trace_hit(albedo * l, vec3f(0.0), vec3f(0.0), vec3f(0.0), hit_position);
          } else {
            // Unknown material.
            return VoxelModelTrace(false, RayVoxelHit(vec3f(1.0, 1.0, 0.0), vec3f(0.0), vec3f(0.0), vec3f(0.0), hit_position));
          }
          //let normal_ptr = esvo_get_node_attachment_ptr(
          //  voxel_model.model_data_ptr,
          //  1u,
          //  curr_node_index,
          //  curr_octant
          //);
          // Check if this voxel has a normal, if it doesn't then skip it, we can't .
          // if (normal_ptr != 0xFFFFFFFFu) {
          //   // This is a valid path tracing voxel.
          //   let compressed_normal = u_world_data.data[normal_ptr];
          //   let stored_normal = normalize(vec3f(
          //     (f32((compressed_normal >> 16u) & 0xFFu) / 255.0) * 2.0 - 1.0,
          //     (f32((compressed_normal >> 8u) & 0xFFu) / 255.0) * 2.0 - 1.0,
          //     (f32(compressed_normal & 0xFFu) / 255.0) * 2.0 - 1.0,
          //   ));

          //  let compresed_material = u_world_data.data[material_ptr];
          //  let material_type = compresed_material >> 30;

          //  // Process the diffuse material.
          //  if (material_type == 0u) {
          //    let albedo = vec3f(
          //      f32((compresed_material >> 16u) & 0xFFu) / 255.0,
          //      f32((compresed_material >> 8u) & 0xFFu) / 255.0,
          //      f32(compresed_material & 0xFFu) / 255.0,
          //    );

          //    let emmisive_ptr = esvo_get_node_attachment_ptr(
          //      voxel_model.model_data_ptr,
          //      2u,
          //      curr_node_index,
          //      curr_octant
          //    );

          //    var radiance_outgoing = vec3f(0.0);
          //    // This voxel is emmisive so it generates it's own radiance.
          //    if (emmisive_ptr != 0xFFFFFFFFu) {
          //      let candela = f32(u_world_data.data[emmisive_ptr]);
          //      radiance_outgoing = albedo * candela;
          //    }

          //    let hit_position = ray.origin + ray.dir * curr_hit_info.t_enter;
          //    // Multiply the normal which has a length of one by some tiny epsilon.
          ////    let sample_position = curr_aabb.center + (stored_normal * 1.5);

          //    return voxel_model_trace_hit(radiance_outgoing, albedo, vec3f(0.0), vec3f(0.0), hit_position);
            //}
          //}
        }
      } else {
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

  return VoxelModelTrace(false, RayVoxelHit(alb, vec3f(0.0), vec3f(0.0), vec3f(0.0), vec3(0.0)));
}

const FLAT_MODEL_DATA_ATTACHMENT_OFFSET: u32 = 4;
fn flat_get_attachment_ptr(flat_model_ptr: u32,
                           attachment_index: u32,
                           voxel_index: u32) -> u32 {
  let flat_attachment_presence_ptr = u_world_model_info.data[flat_model_ptr + FLAT_MODEL_DATA_ATTACHMENT_OFFSET + attachment_index];
  let flat_attachment_data_ptr = u_world_model_info.data[flat_model_ptr + FLAT_MODEL_DATA_ATTACHMENT_OFFSET + 
                                                         RENDER_INDICES_LENGTH + attachment_index];

  let has_attachment = (u_world_data.data[flat_attachment_presence_ptr + voxel_index / 32] & (1u << (voxel_index % 32))) > 0;
  if (!has_attachment) {
    return NULL_ATTACHMENT;
  }

  let attachment_word_count = 1u;
  let attachment_data_ptr = flat_attachment_data_ptr + voxel_index * attachment_word_count;
  return attachment_data_ptr;
}

fn trace_flat(model_hit_info: VoxelModelHit) -> VoxelModelTrace {
  let model_data_ptr = model_hit_info.model_data_ptr;
  let length = vec3<i32>(vec3<u32>(
    u_world_model_info.data[model_data_ptr],
    u_world_model_info.data[model_data_ptr + 1],
    u_world_model_info.data[model_data_ptr + 2],
  ));
  let xy_area = length.x * length.y;
  let voxel_presence_ptr = u_world_model_info.data[model_data_ptr + 3];

  let root_aabb = model_hit_info.aabb;
  let root_hit_info = model_hit_info.hit_info;

  let world_space_ray_pos = ray_advance(model_hit_info.ray, root_hit_info.t_enter).origin - (root_aabb.center - root_aabb.side_length);

  // Normalize and clamp the ray origin against the voxel models
  // AABB min and max, then multiple by length to perform DDA.
  let flat_ray_pos = saturate(world_space_ray_pos / (root_aabb.side_length * 2)) * vec3f(length);
  let flat_ray = Ray(flat_ray_pos, model_hit_info.ray.dir, model_hit_info.ray.inv_dir);

  let grid_step = vec3<i32>(sign(flat_ray.dir));
  let unit_t = abs(flat_ray.inv_dir);

  var grid_pos = clamp(vec3<i32>(floor(flat_ray.origin)), vec3<i32>(0), length - 1);
  var curr_t = ray_to_point(flat_ray, vec3f(grid_pos) + vec3f(grid_step) * 0.5 + 0.5);
  var last_t = vec3f(0.0);

  var alb = vec3f(0);
  var j = 1;
  while (grid_pos.x >= 0 && grid_pos.y >= 0 && grid_pos.z >= 0 &&
         grid_pos.x < length.x && grid_pos.y < length.y && grid_pos.z < length.z) {
    // Use manhattan distance since DDA steps an axis at a time. Multiply by 2 to be less bright.
    alb = vec3f(j) / (vec3f(length.x * 2 + length.y * 2 + length.z * 2));
    j++; 

    let voxel_index = u32(grid_pos.x + grid_pos.y * length.x + grid_pos.z * xy_area);
    let voxel_is_present = (u_world_data.data[voxel_presence_ptr + voxel_index / 32] & (1u << (voxel_index % 32))) > 0;
    if (voxel_is_present) {
      // Scales the local voxel space to world space in terms of our t-value.
      let t_world_scaling = (root_aabb.side_length * 2) / vec3f(length);
      let scaled_last_t = last_t * t_world_scaling;

      let local_ray_hit_t = min(scaled_last_t.x, min(scaled_last_t.y, scaled_last_t.z));
      let world_hit_t = root_hit_info.t_enter + local_ray_hit_t;

      var mask = scaled_last_t.xyz <= min(scaled_last_t.zxy, scaled_last_t.yzx);
      // This is the first iteration, thus we can't rely on last_t to form out mask since it isn't set yet.
      if (j == 2) {
        mask = root_hit_info.t_min == vec3f(root_hit_info.t_enter);
      }
      let normal = vec3f(mask);
      
      let material_ptr = flat_get_attachment_ptr(model_data_ptr, 0u, voxel_index);
      // Check if this voxel has a material, if it doesn't then skip it.
      if (material_ptr != 0xFFFFFFFFu) {
        alb = vec3f(1.0, 0.0, 0.0);

        let compresed_material = u_world_data.data[material_ptr];
        let material_type = compresed_material >> 30;

        // Process the diffuse material.
        if (material_type == 0u) {
          let albedo = srgb_to_lrgb(vec3f(
            f32((compresed_material >> 16u) & 0xFFu) / 255.0,
            f32((compresed_material >> 8u) & 0xFFu) / 255.0,
            f32(compresed_material & 0xFFu) / 255.0,
          ));
          let world_hit_pos = model_hit_info.ray.origin + model_hit_info.ray.dir * world_hit_t;
          let l = normal.x + normal.y * 0.6 + normal.z * 0.4;
          return voxel_model_trace_hit(albedo * l, vec3f(0.0), normal, 
                                       vec3f(0.0), world_hit_pos);
        } else {
          // Unknown material.
          return VoxelModelTrace(false, RayVoxelHit(vec3f(1.0, 1.0, 0.0), vec3f(0.0), normal, vec3f(0.0), vec3f(0.0)));
        }
      }
    }

    let mask = curr_t <= min(curr_t.zxy, curr_t.yzx);
    grid_pos += vec3<i32>(mask) * grid_step;
    last_t = curr_t;
    curr_t += vec3<f32>(mask) * unit_t;
  }
  return VoxelModelTrace(false, RayVoxelHit(alb, vec3f(0.0), vec3f(0.0), vec3f(0.0), vec3(0.0)));
}

struct VoxelModelHit {
  ray: Ray,
  aabb: AABB,
  hit_info: RayAABBInfo,
  model_schema: u32,
  model_data_ptr: u32,
}

fn trace_voxel_model(model_hit_info: VoxelModelHit) -> VoxelModelTrace {
  switch (model_hit_info.model_schema) {
    case 1u: {
      // TODO: Check the height of the esvo and choose the appropriate trace_esvo with stack size closest.
      return trace_esvo(model_hit_info);    
    }
    case 2u: {
      return trace_flat(model_hit_info);
    }
    default: {
      return voxel_model_trace_miss();
    }
  }
}

fn sample_background_radiance(ray: Ray) -> vec3f {
  // Linear scale to make the room a box skybox
  //let linear_scale = 1.0 / max(max(abs(rayDir.x), abs(rayDir.y)), abs(rayDir.z));
  //var background_color = vec3f(srgb_to_lrgb((rayDir * linear_scale + 1) * 0.5));

  // Colors each axes face rgb (xyz) irrespective of sign. Sinusoidal-like interpolation
  //var background_color = vec3f(srgb_to_lrgb(abs(rayDir)));

  // Colors interpolating on the cosine angle of each axis in srgb.
  // From 0.0 <= t <= 1.0, 0.0 <= theta <= PI.
  let background_intensity = 0.3;
  var background_color = srgb_to_lrgb(vec3f(acos(-ray.dir) / 3.14) * background_intensity);

#ifdef GRID
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

    let RADIUS_OF_GRID = 40.0;
    let FADE_DISTANCE = 5.0;
    if (influence != 0.0) {
      // Fade out the grid over a distance of FADE_DISTANCE
      influence = 1.0 - smoothstep(RADIUS_OF_GRID - FADE_DISTANCE,
                                   RADIUS_OF_GRID + FADE_DISTANCE,
                                   influence);
    }
    return mix(background_color, color * background_intensity, influence);
  }
#endif

  return background_color;
}

const BOUNCES: u32 = 6;
const T_LIMIT: f32 = 1000.0;
fn next_ray_terrain_hit(ray: Ray) -> vec3f {
  let chunk_render_origin = u_world_terrain_acceleration.chunk_origin;
  let render_distance = i32(u_world_terrain_acceleration.side_length);

  let root_min = vec3f(chunk_render_origin) * TERRAIN_CHUNK_WORLD_UNIT_LENGTH;
  let root_max = root_min + vec3f(render_distance) * TERRAIN_CHUNK_WORLD_UNIT_LENGTH;
  let root_aabb = aabb_min_max(root_min, root_max);
  let root_hit_info = ray_to_aabb(ray, root_aabb);
  var hit_ray: Ray;
  if (!root_hit_info.hit) {
    return sample_background_radiance(ray);
  } else {
    hit_ray = ray_advance(ray, root_hit_info.t_enter + EPSILON);
  }

  let chunk_ray_pos = (hit_ray.origin / TERRAIN_CHUNK_WORLD_UNIT_LENGTH) - vec3f(chunk_render_origin);
  let chunk_ray = Ray(chunk_ray_pos, hit_ray.dir, hit_ray.inv_dir);

  let grid_step = vec3<i32>(sign(chunk_ray.dir));
  let unit_t = abs(chunk_ray.inv_dir);

  var grid_pos = vec3<i32>(floor(chunk_ray.origin));
  var curr_t = ray_to_point(chunk_ray, vec3f(grid_pos) + vec3f(grid_step) * 0.5 + 0.5);
  var last_t = vec3f(0.0);

  var alb = vec3f(0);
  var j = 1;
  while (grid_pos.x >= 0 && grid_pos.y >= 0 && grid_pos.z >= 0 &&
         grid_pos.x < render_distance && grid_pos.y < render_distance && grid_pos.z < render_distance) {
    // Use manhattan distance since DDA steps an axis at a time. Multiply by 2 to be less bright.
    alb = vec3f(j) / (vec3f(render_distance * 6));
    j++;

    let chunk_index = morton_encode_3(u32(grid_pos.x), u32(grid_pos.y), u32(grid_pos.z));
    let model_ptr = u_world_terrain_acceleration.data[chunk_index];
    if (model_ptr != 0xFFFFFFFF) {
       let model_schema = u_world_model_info.data[model_ptr];
       let min = vec3f(grid_pos + chunk_render_origin) * TERRAIN_CHUNK_WORLD_UNIT_LENGTH;
       let aabb = aabb_min_max(min, min + TERRAIN_CHUNK_WORLD_UNIT_LENGTH);
       let hit_info = ray_to_aabb(ray, aabb);
       let model_hit_info = VoxelModelHit(ray, aabb, hit_info, model_schema, model_ptr + 1);
       let trace = trace_voxel_model(model_hit_info);
       if (trace.hit) {
         let trace_data = trace.hit_data;
         let depth = distance(ray.origin, trace_data.hit_position);
         return vec3f(trace_data.albedo);
       }
    }

    let mask = curr_t <= min(curr_t.zxy, curr_t.yzx);
    grid_pos += vec3<i32>(mask) * grid_step;
    last_t = curr_t;
    curr_t += vec3<f32>(mask) * unit_t;
  }
  // The last voxel model t-value the ray intersected with. Tracked so we only look
  // at intersections after this value.
  // var last_model_t = -T_LIMIT;

  // for (var passed = 0u; passed < 100; passed++) {
  //   var closest_voxel_model_hit: VoxelModelHit;
  //   var closest_model_t = T_LIMIT;

  //   for (var entity_index = 0u; entity_index < u_world_info.voxel_entity_count; entity_index++) {
  //     // Test intersection with OBB.
  //     let i = entity_index * 19;
  //     let aabb_bounds = aabb_min_max(
  //       bitcast<vec3<f32>>(vec3<u32>(
  //         u_world_acceleration.data[i],
  //         u_world_acceleration.data[i + 1],
  //         u_world_acceleration.data[i + 2]
  //       )),
  //       bitcast<vec3<f32>>(vec3<u32>(
  //         u_world_acceleration.data[i + 3],
  //         u_world_acceleration.data[i + 4],
  //         u_world_acceleration.data[i + 5]
  //       )),
  //     );

  //     let obb_rotation_anchor = bitcast<vec3<f32>>(vec3<u32>(
  //       u_world_acceleration.data[i + 6],
  //       u_world_acceleration.data[i + 7],
  //       u_world_acceleration.data[i + 8]
  //     ));

  //     let obb_rotation = mat3x3f(
  //       bitcast<vec3<f32>>(vec3<u32>(
  //         u_world_acceleration.data[i + 9],
  //         u_world_acceleration.data[i + 10],
  //         u_world_acceleration.data[i + 11]
  //       )),
  //       bitcast<vec3<f32>>(vec3<u32>(
  //         u_world_acceleration.data[i + 12],
  //         u_world_acceleration.data[i + 13],
  //         u_world_acceleration.data[i + 14]
  //       )),
  //       bitcast<vec3<f32>>(vec3<u32>(
  //         u_world_acceleration.data[i + 15],
  //         u_world_acceleration.data[i + 16],
  //         u_world_acceleration.data[i + 17]
  //       )),
  //     );

  //     let ray_rot_origin = obb_rotation * (ray.origin - obb_rotation_anchor) + obb_rotation_anchor;
  //     let ray_rot_dir = normalize(obb_rotation * ray.dir);
  //     let ray_rot = Ray(ray_rot_origin, ray_rot_dir, 1.0 / ray_rot_dir);

  //     let hit_info = ray_to_aabb(ray_rot, aabb_bounds);
  //     let is_closer = hit_info.t_enter < closest_model_t;
  //     let is_after_prev_model_t = hit_info.t_enter > last_model_t;
  //     if (hit_info.hit && is_after_prev_model_t && is_closer) {
  //       let model_ptr = u_world_acceleration.data[i + 18];
  //       let model_schema = u_world_model_info.data[model_ptr];
  //       let model_hit_info = VoxelModelHit(ray_rot, aabb_bounds, hit_info, model_schema, model_ptr + 1);

  //       closest_model_t = hit_info.t_enter;
  //       closest_voxel_model_hit = model_hit_info;
  //     }
  //   }

  //   // We didn't hit anything so end early.
  //   if (closest_model_t == last_model_t) {
  //     break;
  //   }

  //   let trace = trace_voxel_model(closest_voxel_model_hit);
  //   if (trace.hit) {
  //     let hit_data = trace.hit_data;
  //     let n = abs(hit_data.normal);
  //     let l = n.y + n.x * 0.8 + n.z * 0.5;
  //     return hit_data.albedo * l; 
  //   } else {
  //     last_model_t = closest_model_t;
  //   }
  // }

  return sample_background_radiance(ray);
}

// Pixel sample is the discrete screen pixel with some random sub-pixel offset applied.
// The pixel sample has an origin at the top-left of the screen.
fn construct_camera_ray(pixel_sample: vec2f, render_size: vec2f) -> Ray {
  let ndc = pixel_sample / render_size;
  let uv = vec2f(ndc.x * 2.0 - 1.0, 1.0 - ndc.y * 2.0);

  let aspect_ratio = f32(render_size.x) / f32(render_size.y);
  var scaled_uv = vec2f(uv.x * aspect_ratio, uv.y) * tan(u_world_info.camera.half_fov);

  let ct = u_world_info.camera.transform;
  let ray_origin = vec3f(ct[0][3], ct[1][3], ct[2][3]);
  let ray_dir = normalize(vec3f(scaled_uv, 1.0) * u_world_info.camera.rotation);

  return Ray(ray_origin, ray_dir, 1.0 / ray_dir);
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
  let dimensions = textureDimensions(u_backbuffer);
  let coords = id.xy;
  if (coords.x >= dimensions.x || coords.y >= dimensions.y) {
    return;
  }

  // Initialize prng with a seed, this should be new every frame.
  init_seed(coords);

  // Generate ray depending on camera and a random offset within the pixel.
  // Assumed to be a uniformly distributed random variable.
  let offset = vec2f(
    rand_f32() - 0.5,
    rand_f32() - 0.5
  );
  
  let ray = construct_camera_ray(vec2f(coords) + offset, vec2f(dimensions));
  let sampled_pixel_radiance = next_ray_terrain_hit(ray);

  // Apply monte carlo estimator using stored accumulated pixel radiance samples.
  var pixel_radiance_prev = vec3f(0.0);
  if (u_world_info.frame_count > 1) {
    pixel_radiance_prev = textureLoad(u_radiance_total_prev, coords, 0).xyz;
  }

  // Store our new total accumulated radiance over `n` samples.
  let total_pixel_radiance = pixel_radiance_prev + sampled_pixel_radiance;
  textureStore(u_radiance_total, coords, vec4f(total_pixel_radiance, 0.0));

  // This is our monte carlo estimation over `n` samples where `n = frame_count` for the expected
  // radiance is given a uniform pdf over the pixels area.
  var estimated_expected_radiance = total_pixel_radiance / f32(u_world_info.frame_count);

  // Since the backbuffer is a 32-bit color, convert the expected radiance to srgb
  // then dither to avoid any color banding.
  let gamma_corrected_color = lrgb_to_srgb(estimated_expected_radiance);
  let backbuffer_color = vec4f(dither(gamma_corrected_color), 1.0);
  textureStore(u_backbuffer, coords, backbuffer_color);  
}
