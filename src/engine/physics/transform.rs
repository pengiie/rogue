use nalgebra::{Isometry, Isometry3, Matrix4};

pub struct Transform {
    pub isometry: Isometry3<f32>,
}

impl Transform {
    pub fn new() -> Self {
        Self {
            isometry: Isometry3::identity(),
        }
    }

    pub fn to_matrix(&self) -> Matrix4<f32> {
        self.isometry.to_homogeneous()
    }
}
