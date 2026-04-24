use std::{collections::HashMap, path::PathBuf};

use rogue_engine::{
    asset::asset::GameAssetPath,
    event::Events,
    graphics::backend::{Image, ResourceId},
    material::{
        material_bank::{MaterialAssetId, MaterialBank},
        material_gpu::MaterialBankGpu,
    },
};

use crate::ui::entity_properties::EntityPropertiesShowFns;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GlobalStateEditorUI {
    #[serde(skip)]
    show_fns: EntityPropertiesShowFns,
    #[serde(skip)]
    material_textures: EditorMaterialTextures,

    pub selected_asset: Option<GameAssetPath>,
}

impl GlobalStateEditorUI {
    pub fn new() -> Self {
        Self {
            show_fns: EntityPropertiesShowFns::new(),
            selected_asset: None,
            material_textures: EditorMaterialTextures::new(),
        }
    }

    pub fn show_fns(&mut self) -> &mut EntityPropertiesShowFns {
        &mut self.show_fns
    }

    pub fn selected_asset_extension(&self) -> Option<String> {
        self.selected_asset
            .as_ref()
            .map(|path| path.extension().to_lowercase())
    }
}

impl Default for GlobalStateEditorUI {
    fn default() -> Self {
        Self::new()
    }
}

pub struct EditorMaterialTextures {
    pub material_textures: HashMap<MaterialAssetId, egui::TextureId>,
}

impl EditorMaterialTextures {
    pub fn new() -> Self {
        Self {
            material_textures: HashMap::new(),
        }
    }

    pub fn update_material_textures(
        &mut self,
        material_bank: &MaterialBank,
        material_bank_gpu: &MaterialBankGpu,
        events: &Events,
    ) {
    }
}
