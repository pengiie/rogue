use std::{collections::HashMap, path::PathBuf};

use nalgebra::Vector3;
use rogue_engine::asset::asset::Assets;
use rogue_macros::Resource;

use crate::{camera_controller::EditorCameraController, editor_settings::UserEditorSettingsAsset};

#[derive(Resource, serde::Serialize, serde::Deserialize)]
pub struct EditorProjectSettings {
    pub projects: HashMap<PathBuf, EditorProjectSettingsData>,
}

impl EditorProjectSettings {
    pub fn new() -> Self {
        Self {
            projects: HashMap::new(),
        }
    }

    pub fn get_project_settings(&self, assets: &Assets) -> Option<&EditorProjectSettingsData> {
        assets
            .project_dir()
            .as_ref()
            .map(|project_dir| self.projects.get(project_dir))
            .flatten()
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct EditorProjectSettingsData {
    pub editor_camera_anchor: Vector3<f32>,
    pub editor_camera_rotation: Vector3<f32>,
    pub editor_camera_distance: f32,
}
