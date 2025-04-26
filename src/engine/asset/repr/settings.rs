use nalgebra::Vector3;

use crate::engine::asset::asset::{AssetFile, AssetLoadError, AssetLoader, AssetSaver};

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(default)]
pub struct SettingsAsset {
    pub mouse_sensitivity: f32,
    pub controller_sensitivity: f32,
    pub chunk_render_distance: u32,
    pub player_position: Vector3<f32>,
    pub player_rotation: Vector3<f32>,
}

impl Default for SettingsAsset {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 0.001,
            controller_sensitivity: 90.0f32.to_radians(),
            chunk_render_distance: 4,
            player_position: Vector3::new(-0.0, -2.0, -6.0),
            player_rotation: Vector3::new(0.0, 0.0, 0.0),
        }
    }
}

impl AssetLoader for SettingsAsset {
    fn load(data: &AssetFile) -> std::result::Result<Self, AssetLoadError>
    where
        Self: Sized + std::any::Any,
    {
        match data.read_contents() {
            Ok(contents) => Ok(serde_json::from_str::<SettingsAsset>(&contents)
                .expect("Failed to deserialize settings file.")),
            Err(err) => match err.kind() {
                std::io::ErrorKind::NotFound => Err(AssetLoadError::NotFound { path: None }),
                _ => Err(AssetLoadError::Other(anyhow::anyhow!(err.to_string()))),
            },
        }
    }
}

impl AssetSaver for SettingsAsset {
    fn save(data: &Self, out_file: &AssetFile) -> anyhow::Result<()>
    where
        Self: Sized,
    {
        match out_file.write_contents(
            serde_json::to_string_pretty(data).expect("Failed to serialize settings."),
        ) {
            Ok(()) => Ok(()),
            Err(err) => match err.kind() {
                _ => Err(anyhow::anyhow!(err.to_string())),
            },
        }
    }
}
