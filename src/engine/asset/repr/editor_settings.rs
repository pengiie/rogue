use nalgebra::Vector3;

use crate::engine::asset::asset::AssetLoader;

use super::components::{CameraAsset, TransformAsset};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct EditorSessionAsset {
    pub editor_camera_transform: TransformAsset,
    pub editor_camera: CameraAsset,
    pub rotation_anchor: Vector3<f32>,
}
