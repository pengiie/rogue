use nalgebra::Vector3;

#[derive(Debug)]
pub struct AABB {
    pub min: Vector3<f32>,
    pub max: Vector3<f32>,
}

impl AABB {
    pub fn new(a: Vector3<f32>, b: Vector3<f32>) -> Self {
        let min = a.zip_map(&b, |x, y| x.min(y));
        let max = a.zip_map(&b, |x, y| x.max(y));

        Self { min, max }
    }
}
