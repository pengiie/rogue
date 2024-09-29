use nalgebra::{Isometry, Isometry3, Matrix4, Rotation3, Translation3, Vector3};

pub struct Transform {
    pub isometry: Isometry3<f32>,
}

impl Transform {
    pub fn new() -> Self {
        Self {
            isometry: Isometry3::identity(),
        }
    }

    pub fn with_translation(translation: Translation3<f32>) -> Self {
        Self {
            isometry: Isometry3::translation(translation.x, translation.y, translation.z),
        }
    }

    pub fn to_matrix(&self) -> Matrix4<f32> {
        self.isometry.to_homogeneous()
    }

    pub fn rotation(&self) -> Rotation3<f32> {
        self.isometry.rotation.into()
    }
}
