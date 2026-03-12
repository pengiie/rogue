use std::path::{Path, PathBuf};

use rogue_engine::{
    asset::asset::{AssetPath, GameAssetPath},
    material::{Material, MaterialTextureType},
};

use crate::ui::{EditorCommand, FilePickerType, pane::EditorUIPane};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct AssetPropertiesPane {}

impl AssetPropertiesPane {
    pub fn new() -> Self {
        Self {}
    }

    pub fn show_header(ui: &mut egui::Ui, ctx: &mut super::EditorUIContext<'_>) {
        let selected_asset_extension = ctx.ui_state.selected_asset_extension();
        let mut title = match selected_asset_extension.as_deref() {
            Some("rmat") => "Material".to_owned(),
            Some(ext) => ext.to_uppercase(),
            None => "Asset".to_owned(),
        };
        title.push_str(" Properties");
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(title).size(20.0));
        });
        if let Some(selected_asset) = &ctx.ui_state.selected_asset {
            let file_path = selected_asset
                .strip_prefix(ctx.assets.project_assets_dir().unwrap())
                .unwrap();
            ui.label(format!("Selected asset: /{}", file_path.display()));
        }
    }

    pub fn show_properties(ui: &mut egui::Ui, ctx: &mut super::EditorUIContext<'_>) {
        let selected_asset = ctx.ui_state.selected_asset.clone().unwrap();
        let selected_asset = AssetPath::from_project_dir_path(
            ctx.assets.project_dir().as_ref().unwrap(),
            &selected_asset,
        );
        let ext = ctx.ui_state.selected_asset_extension();
        match ext.as_deref() {
            Some("rmat") => {
                Self::show_material_properties(ui, ctx, selected_asset);
            }
            _ => {
                ui.label("Unknown asset type.");
            }
        }
    }

    pub fn show_material_properties(
        ui: &mut egui::Ui,
        ctx: &mut super::EditorUIContext<'_>,
        asset_path: AssetPath,
    ) {
        let asset_handle = ctx
            .assets
            .get_asset_handle::<Material>(&asset_path)
            .unwrap_or_else(|| {
                let handle = ctx.assets.load_asset::<Material>(asset_path);
                ctx.assets.wait_until_all_loaded();
                handle
            });
        let Some(material_asset) = ctx.assets.get_asset::<Material>(&asset_handle) else {
            ui.label("Failed to load material asset.");
            return;
        };
        ui.horizontal(|ui| {
            ui.label("Color texture:");
            let text = material_asset
                .color_texture
                .as_ref()
                .map(|asset_path| asset_path.as_relative_path_str())
                .unwrap_or("None".to_owned());
            let button = ui
                .add(egui::Button::new(&text).truncate())
                .on_hover_text(text);
            if button.clicked() {
                let asset_handle = asset_handle.clone();
                ctx.commands.push(EditorCommand::FilePicker {
                    picker_type: FilePickerType::OpenFile,
                    callback: Box::new(move |ctx, asset_path| {
                        // Update the assets representation and the loaded material representation
                        // if applicable.
                        let asset_path = GameAssetPath::from_relative_path(&asset_path);
                        ctx.assets
                            .get_asset_mut::<Material>(&asset_handle)
                            .unwrap()
                            .color_texture = Some(asset_path.clone());
                        if let Some(material_id) = ctx
                            .material_bank
                            .asset_path_map
                            .get(asset_handle.asset_path().asset_path.as_ref().unwrap())
                        {
                            ctx.material_bank.update_material_texture(
                                *material_id,
                                MaterialTextureType::Color,
                                Some(asset_path),
                            );
                        }
                    }),
                    extensions: vec!["png".to_owned()],
                });
                ui.close_menu();
            }
        });
    }
}

impl EditorUIPane for AssetPropertiesPane {
    const ID: &'static str = "asset_properties";
    const NAME: &'static str = "Asset Properties";

    fn show(&mut self, ui: &mut egui::Ui, ctx: &mut super::EditorUIContext<'_>) {
        ui.vertical(|ui| {
            Self::show_header(ui, ctx);
            ui.add_space(4.0);
            Self::show_properties(ui, ctx);
        });
    }
}
