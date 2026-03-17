implementing lib;

public struct FrameWorldInfo {
  public Camera camera;
  public Sky sky;
}

public struct Sky {
  public float3 sun_dir;

  public static Sky get() {
    return u_frame.world_info.sky;
  }

  public float3 sample_sky(Ray ray) {
    // Fun colorful sky.
    //var background_color = color::srgb_to_lsrgb(float3(acos(-ray.dir) / 3.14));
    const float3 really_dark_blue = float3(0.01, 0.02, 0.05);
    const float3 dark_blue = float3(0.4, 0.5, 0.75);
    const float3 light_blue = float3(0.6, 0.73, 0.88);

    let t = saturate(sun_dir.y * 2.0);

    let sun_l = dot(ray.dir, normalize(sun_dir));
    let sun_viewing_angle = acos(sun_l);
    const float sun_half_angle = 0.1 * saturate(ceil(sun_dir.y + 0.3));
    if (sun_viewing_angle < sun_half_angle) {
      const float sun_half_grad = 0.008;
      return color::srgb_to_lsrgb(float3(0.95, 0.9, 0.4));
    }

    let moon_l = dot(ray.dir, normalize(-sun_dir));
    let moon_viewing_angle = acos(moon_l);
    const float moon_half_angle = 0.1 * saturate(ceil(-sun_dir.y + 0.3));
    if (moon_viewing_angle < moon_half_angle) {
      return this.moon_color();
    }

    let dithered_dir = dither(ray.dir);
    var c = color::srgb_to_lsrgb(lerp(dark_blue, light_blue, smoothstep_saturate((dithered_dir.y + 0.3) / 0.5)));
    if (sun_dir.y < 0.0) {
      c *= 1.0 - saturate(abs(sun_dir.y) / 0.2) * 0.8;
    }
    return c;
  }

  public static float3 sun_color() {
      return color::srgb_to_lsrgb(float3(1.0, 0.98, 0.95));
  }

  public static float3 moon_color() {
      return color::srgb_to_lsrgb(float3(1.0));
  }
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
    let ray_dir = normalize(mul(this.rotation, float3(scaled_ndc.xy, -1.0)));

    return Ray(ray_origin, ray_dir);
  }
}
