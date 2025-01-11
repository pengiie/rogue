implementing lib;

public struct FrameWorldInfo {
  public Camera camera;
}

public struct Camera {
  matrix4x4 transform;
  matrix3x3 rotation;
  float fov;

  public Ray create_ray(float2 pixel_pos, float2 screen_dimensions) {
    let ndc = pixel_pos / screen_dimensions;
    let uv = float2(ndc.x * 2.0 - 1.0, 1.0 - ndc.y * 2.0);

    let aspect_ratio = screen_dimensions.x / screen_dimensions.y;
    var scaled_uv = float2(uv.x * aspect_ratio, uv.y);

    var ray_origin = float3(transform._m03, transform._m13, transform._m23);
    let ray_dir = normalize(mul(rotation, float3(scaled_uv.xy, 1.0)));

    return Ray(ray_origin, ray_dir);
  }
}

public float3 sample_sky(Ray ray) {
  var background_color = color::srgb_to_lsrgb(float3(acos(-ray.dir) / 3.14));
  return background_color;
}
