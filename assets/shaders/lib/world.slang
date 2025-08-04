implementing lib;

public struct FrameWorldInfo {
  public Camera camera;
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
  const float3 dark_blue = float3(0.25, 0.4, 0.95);
  const float3 light_blue = float3(0.3, 0.7, 0.95);
  var background_color = color::srgb_to_lsrgb(lerp(dark_blue, light_blue, saturate(ray.dir.y + 0.3)));


  return background_color;
}
