use crate::engine::{graphics::camera::Camera, physics::transform::Transform};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct CameraAsset {
    pub camera: Camera,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct TransformAsset {
    pub transform: Transform,
}
