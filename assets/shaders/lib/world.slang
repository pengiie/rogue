implementing lib;

public struct FrameWorldInfo {
  public Camera camera;
  public float3 sun_dir;
}

public struct Camera {
  public matrix4x4 proj_view;
  matrix4x4 transform;
  matrix3x3 rotation;
  float fov;
  public float near_plane;
  public float far_plane;

  public Ray create_ray(float2 pixel_pos, float2 screen_dimensions) {
    let uv = pixel_pos / screen_dimensions;
    let ndc = float2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);

    let aspect_ratio = screen_dimensions.x / screen_dimensions.y;
    var scaled_ndc = float2(ndc.x * aspect_ratio, ndc.y) * tan(this.fov * 0.5);

    var ray_origin = float3(this.transform._m03, this.transform._m13, this.transform._m23);
    let ray_dir = normalize(mul(this.rotation, float3(scaled_ndc.xy, 1.0)));

    return Ray(ray_origin, ray_dir);
  }
}

public float3 sample_sky(Ray ray) {
  // Fun colorful sky.
  //var background_color = color::srgb_to_lsrgb(float3(acos(-ray.dir) / 3.14));
  const float3 really_dark_blue = float3(0.01, 0.02, 0.05);
  const float3 dark_blue = float3(0.4, 0.5, 0.75);
  const float3 light_blue = float3(0.6, 0.78, 0.98);

  let sun_dir = u_frame.world_info.sun_dir;
  let t = saturate(sun_dir.y * 2.0);

  let sun_l = dot(ray.dir, normalize(sun_dir));
  let sun_viewing_angle = acos(sun_l);
  const float sun_half_angle = 0.1 * saturate(ceil(sun_dir.y + 0.3));
  if (sun_viewing_angle < sun_half_angle) {
    const float sun_half_grad = 0.008;
    return color::srgb_to_lsrgb(float3(0.9, 0.85, 0.4));
  }

  let moon_l = dot(ray.dir, normalize(-sun_dir));
  let moon_viewing_angle = acos(moon_l);
  const float moon_half_angle = 0.1 * saturate(ceil(-sun_dir.y + 0.3));
  if (moon_viewing_angle < moon_half_angle) {
    return color::srgb_to_lsrgb(float3(1.0));
  }

  if (t > 0.6) {
    return color::srgb_to_lsrgb(lerp(dark_blue, light_blue, saturate((t - 0.3) / 0.7)));
  } else if (t > 0.3) {
    return color::srgb_to_lsrgb(dark_blue);
  } else {
    return color::srgb_to_lsrgb(lerp(really_dark_blue, dark_blue, saturate(t / 0.3)));
  }
}
