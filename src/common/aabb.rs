use nalgebra::Vector3;

#[derive(Debug)]
pub struct AABB {
    pub min: Vector3<f32>,
    pub max: Vector3<f32>,
}

impl AABB {
    pub fn new_two_point(a: Vector3<f32>, b: Vector3<f32>) -> Self {
        let min = a.zip_map(&b, |x, y| x.min(y));
        let max = a.zip_map(&b, |x, y| x.max(y));

        Self { min, max }
    }

    pub fn new_center_extents(center: Vector3<f32>, extents: Vector3<f32>) -> Self {
        assert!(
            extents.iter().all(|x| *x > 0.0),
            "AABB extents must be greater than 0."
        );
        Self {
            min: center - extents,
            max: center + extents,
        }
    }

    pub fn side_length(&self) -> Vector3<f32> {
        self.max - self.min
    }
}
