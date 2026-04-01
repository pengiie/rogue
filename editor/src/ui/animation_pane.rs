use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use rogue_engine::asset::asset::GameAssetPath;

use crate::ui::{
    EditorCommand, EditorUIContext, FilePickerType, asset_properties_pane::AssetPropertiesPane,
    pane::EditorUIPane,
};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default = "AnimationPane::new")]
pub struct AnimationPane {}

impl AnimationPane {
    pub fn new() -> Self {
        Self {}
    }

    pub fn show_header(ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Animation").size(20.0));
            if ui.button("Create").clicked() {
                ctx.commands.push(EditorCommand::FilePicker {
                    picker_type: FilePickerType::CreateFile,
                    callback: Box::new(|ctx, file_path| {}),
                    extensions: vec!["ranim".to_owned()],
                    preset_file_path: None,
                });
            }
        });
    }
}

impl EditorUIPane for AnimationPane {
    const ID: &'static str = "animation";
    const NAME: &'static str = "Animation";

    fn show(&mut self, ui: &mut egui::Ui, ctx: &mut super::EditorUIContext<'_>) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                Self::show_header(ui, ctx);
            });
        });
    }
}
