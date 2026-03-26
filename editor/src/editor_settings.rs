use std::{collections::HashMap, path::PathBuf};

use nalgebra::Vector3;
use rogue_engine::{
    asset::{
        asset::{AssetLoadError, AssetPath, Assets},
        repr::project::ProjectAsset,
    },
    impl_asset_load_save_serde, impl_asset_save_serde,
};

use crate::{
    editor_project_settings::EditorProjectSettings, editor_settings, init_ecs_world, ui::EditorUI,
};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UserEditorSettingsAsset {
    pub last_project_dir: Option<PathBuf>,

    /// Saved editor UI state.
    pub editor_ui: EditorUI,

    pub user_project_settings: EditorProjectSettings,
}

#[derive(serde::Serialize)]
pub struct UserEditorSettingsAssetProxy<'a> {
    pub last_project_dir: &'a Option<PathBuf>,
    pub editor_ui: &'a EditorUI,
    pub user_project_settings: &'a EditorProjectSettings,
}

impl_asset_load_save_serde!(UserEditorSettingsAsset);
impl_asset_save_serde!(UserEditorSettingsAssetProxy<'_>);

impl Default for UserEditorSettingsAsset {
    fn default() -> Self {
        Self::new()
    }
}

impl UserEditorSettingsAsset {
    const ASSET_PATH: &str = "editor::editor_settings::json";

    pub fn new() -> Self {
        Self {
            last_project_dir: None,
            editor_ui: EditorUI::new(),
            user_project_settings: EditorProjectSettings::new(),
        }
    }

    pub fn load_editor_settings() -> Self {
        let editor_settings_path = AssetPath::new_user_dir(Self::ASSET_PATH);
        log::info!(
            "Looking for editor user settings at {:?}",
            editor_settings_path
        );
        match Assets::load_asset_sync::<UserEditorSettingsAsset>(editor_settings_path.clone()) {
            Ok(editor_settings) => editor_settings,
            Err(err) => {
                match err {
                    AssetLoadError::NotFound { path } => {
                        log::info!(
                            "Couldn't find existing editor settings at {:?}",
                            editor_settings_path
                        );
                    }
                    AssetLoadError::Other(error) => {
                        log::error!(
                            "Error when trying to load editor settings at {:?}. Error: {:?}",
                            editor_settings_path,
                            error
                        );
                    }
                }
                Self::new()
            }
        }
    }

    pub fn load_project(&self) -> ProjectAsset {
        log::info!(
            "Trying to load last project from editor settings at {:?}",
            self.last_project_dir
        );
        self.last_project_dir
            .as_ref()
            .map(|last_project_dir| {
                log::info!("Deserializing last project at {:?}", last_project_dir);
                ProjectAsset::from_existing_raw(last_project_dir, crate::init_ecs_world())
                    .map_err(|err| {
                        log::error!(
                            "Error when trying to deserialize last project. Error: {:?}",
                            err
                        );
                        err
                    })
                    .ok()
            })
            .flatten()
            .unwrap_or_else(|| ProjectAsset::new_empty(init_ecs_world()))
    }
}

impl UserEditorSettingsAssetProxy<'_> {
    pub fn save_settings(&self) {
        let editor_settings_path = AssetPath::new_user_dir(UserEditorSettingsAsset::ASSET_PATH);
        Assets::save_asset_sync(editor_settings_path, self).unwrap();
    }
}
