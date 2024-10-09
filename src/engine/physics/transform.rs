use log::debug;
use nalgebra::{
    Isometry, Isometry3, Matrix4, Quaternion, Rotation3, Translation3, UnitQuaternion, Vector3,
};

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

    pub fn to_view_matrix(&self) -> Matrix4<f32> {
        let rotation = self.isometry.rotation.euler_angles();

        self.isometry.to_homogeneous()
    }

    pub fn rotation(&self) -> Rotation3<f32> {
        self.isometry.rotation.into()
    }
}
