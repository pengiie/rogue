use nalgebra::Vector3;

use crate::asset::asset::{
    impl_asset_load_save_serde, AssetFile, AssetLoadError, AssetLoader, AssetSaver,
};

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct UserSettingsAsset {
    pub mouse_sensitivity: f32,
    pub controller_sensitivity: f32,
    pub chunk_render_distance: u32,
}

impl Default for UserSettingsAsset {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 0.001,
            controller_sensitivity: 90.0f32.to_radians(),
            chunk_render_distance: 24,
        }
    }
}

impl_asset_load_save_serde!(UserSettingsAsset);
