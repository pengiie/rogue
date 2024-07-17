#[include_wgsl_oil::include_wgsl_oil("./ray_compute.wgsl")]
pub mod ray_march {}

#[include_wgsl_oil::include_wgsl_oil("./blit.wgsl")]
pub mod blit {}
