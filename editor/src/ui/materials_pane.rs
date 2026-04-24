use std::collections::HashSet;

use crate::ui::pane::EditorUIPane;
use egui::Sense;
use rogue_engine::asset::asset::GameAssetPath;
use rogue_engine::material::MaterialAsset;
use rogue_engine::material::material_bank::{MaterialAssetId, MaterialId};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct MaterialsPane {
    #[serde(skip)]
    #[serde(default = "HashSet::new")]
    open_materials: HashSet<MaterialId>,
}

impl MaterialsPane {
    pub fn new() -> Self {
        Self {
            open_materials: HashSet::new(),
        }
    }
}

impl EditorUIPane for MaterialsPane {
    const ID: &'static str = "materials";
    const NAME: &'static str = "Materials";

    fn show(&mut self, ui: &mut egui::Ui, ctx: &mut super::EditorUIContext<'_>) {
        Self::show_header(ui, ctx);
        ui.add_space(8.0);
        Self::show_materials(self, ui, ctx);
    }
}

impl MaterialsPane {
    pub fn show_header(ui: &mut egui::Ui, ctx: &mut super::EditorUIContext<'_>) {
        let material_bank = &mut ctx.material_bank;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Materials").size(20.0));
            if ui.button("Add").clicked() {
                let mut name = "New Material".to_owned();
                material_bank.create_material(name);
            }
        });
    }

    pub fn show_materials(&mut self, ui: &mut egui::Ui, ctx: &mut super::EditorUIContext<'_>) {
        let material_bank = &mut ctx.material_bank;

        for (material_id, material_name) in material_bank.id_to_name.clone().iter() {
            ui.push_id(format!("material_{}", material_id), |ui| {});
            ui.horizontal(|ui| {
                let mut new_material_name = material_name.clone();
                egui::TextEdit::singleline(&mut new_material_name)
                    .desired_width(100.0)
                    .show(ui);
                if &new_material_name != material_name {
                    material_bank
                        .id_to_name
                        .insert(*material_id, new_material_name);
                }

                ui.label(":");
                let existing_asset = material_bank.id_to_asset_map.get(material_id);
                let material_path = existing_asset.map(|mat_asset_id| {
                    material_bank
                        .get_material(*mat_asset_id)
                        .unwrap()
                        .asset_path
                        .as_ref()
                });

                let (res, new_material_asset_path) =
                    ui.dnd_drop_zone::<GameAssetPath, _>(egui::Frame::new(), |ui| {
                        match material_path {
                            Some(Some(path)) => {
                                let text = path.as_relative_path_str();
                                ui.label(text);
                            }
                            Some(None) => {
                                ui.label("In memory");
                            }
                            None => {
                                ui.label("None");
                            }
                        }
                    });
                if let Some(new_material_asset_path) = new_material_asset_path {
                    material_bank
                        .set_material_asset(*material_id, (*new_material_asset_path).clone());
                }
            });
        }
        //    ui.push_id(format!("material_{}", material_name), |ui| {
        //        let material_info = material_bank
        //            .materials
        //            .get(*material_id)
        //            .expect("Material should exist if it is in the name map.");
        //        let is_open = self.open_materials.contains(material_id);
        //        let response = ui.horizontal(|ui| {
        //            let (id, rect) = ui.allocate_space(egui::vec2(6.0, 6.0));
        //            let rect_response = ui.interact(rect, id, Sense::click());
        //            crate::ui::util::paint_chevron_icon(ui, !is_open, &rect_response);

        //            let material_empty_text =
        //                material_info.is_empty().then_some(" (Empty)").unwrap_or("");
        //            let label_response = ui.label(format!("{}:", material_name));
        //            if label_response.clicked() || rect_response.clicked() {
        //                if is_open {
        //                    self.open_materials.remove(material_id);
        //                } else {
        //                    self.open_materials.insert(*material_id);
        //                }
        //            }

        //            let game_asset_path = &mut material_info.asset_path;
        //            let (res, new_animation) =
        //                ui.dnd_drop_zone::<GameAssetPath, _>(egui::Frame::new(), |ui| {
        //                    let asset_title = match game_asset_path {
        //                        Some(path) => path.as_relative_path_str(),
        //                        None => "None".to_string(),
        //                    };
        //                    ui.label(asset_title);
        //                });
        //            if let Some(new_animation) = new_animation {
        //                *game_asset_path = Some((*new_animation).clone());
        //            }
        //        });
        //        if !is_open {
        //            return;
        //        }
        //        ui.horizontal(|ui| {
        //            ui.label("Color texture:");
        //            let text = material_info
        //                .color_texture
        //                .as_ref()
        //                .map(|asset_path| asset_path.as_relative_path_str())
        //                .unwrap_or("None".to_owned());
        //            let button = ui
        //                .add(egui::Button::new(&text).truncate())
        //                .on_hover_text(text);
        //            if button.clicked() {
        //                //let send = self.material_texture_dialog.tx_file_name.clone();
        //                //self.material_texture_dialog.associated_material_id = *material_id;
        //                //self.material_texture_dialog.associated_texture_type =
        //                //    MaterialTextureType::Color;

        //                //let mut start_dir = ctx
        //                //    .assets
        //                //    .project_assets_dir()
        //                //    .expect("Project directory should exist if this is clicked.")
        //                //    .clone();
        //                //if let Some(color_texture_asset_dir) = material_info.color_texture.as_ref()
        //                //{
        //                //    start_dir = start_dir.join(
        //                //        color_texture_asset_dir
        //                //            .as_relative_path()
        //                //            .parent()
        //                //            .expect("Asset path should have file name"),
        //                //    );
        //                //}
        //                //log::info!(
        //                //    "Opening material texture dialog with start dir {:?}",
        //                //    start_dir
        //                //);
        //                //std::thread::spawn(|| {
        //                //    pollster::block_on(async move {
        //                //        let file = rfd::AsyncFileDialog::new()
        //                //            .set_directory(start_dir)
        //                //            .add_filter("Image", ImageAsset::supported_extensions())
        //                //            .pick_file()
        //                //            .await;
        //                //        let Some(file) = file else {
        //                //            return;
        //                //        }//;
        //                //        s//end.send(file.path().to_string_lossy().to_string());
        //                //    });
        //                //});
        //                ui.close_me //nu();
        //            }
        //        });
        //    });
        //}
    }
}
