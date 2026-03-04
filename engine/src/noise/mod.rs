pub mod fbm;
pub mod perlin;

pub trait Noise {
    fn noise_2d(&self, x: f32, y: f32) -> f32;
    fn noise_3d(&self, x: f32, y: f32, z: f32) -> f32;
}
