use std::path::PathBuf;

use crate::ui::entity_properties::EntityPropertiesShowFns;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GlobalStateEditorUI {
    #[serde(skip)]
    show_fns: EntityPropertiesShowFns,
    pub selected_asset: Option<PathBuf>,
}

impl GlobalStateEditorUI {
    pub fn new() -> Self {
        Self {
            show_fns: EntityPropertiesShowFns::new(),
            selected_asset: None,
        }
    }

    pub fn show_fns(&mut self) -> &mut EntityPropertiesShowFns {
        &mut self.show_fns
    }

    pub fn selected_asset_extension(&self) -> Option<String> {
        self.selected_asset.as_ref().and_then(|path| {
            path.extension()
                .map(|ext| ext.to_string_lossy().to_lowercase())
        })
    }
}

impl Default for GlobalStateEditorUI {
    fn default() -> Self {
        Self::new()
    }
}
