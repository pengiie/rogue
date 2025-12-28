use std::path::PathBuf;
use std::str::FromStr;

use egui::Sense;

use crate::engine::asset::asset::{AssetPath, GameAssetPath};
use crate::engine::asset::repr::image::ImageAsset;
use crate::engine::editor::ui::ui_state::EditorUIState;
use crate::engine::material::MaterialBank;
use crate::{
    common::color::Color,
    engine::{
        asset::asset::Assets,
        editor::{self, editor::Editor},
        entity::ecs_world::ECSWorld,
    },
    session::EditorSession,
};

fn color_picker(ui: &mut egui::Ui, color: &mut Color) {
    let mut egui_color = egui::Color32::from_rgb(color.r_u8(), color.g_u8(), color.b_u8());

    egui::color_picker::color_picker_color32(
        ui,
        &mut egui_color,
        egui::color_picker::Alpha::Opaque,
    );
    color.set_rgb_u8(egui_color.r(), egui_color.g(), egui_color.b());
}

pub fn editing_pane(
    ui: &mut egui::Ui,
    ecs_world: &mut ECSWorld,
    editor: &mut Editor,
    ui_state: &mut EditorUIState,
    session: &mut EditorSession,
    assets: &mut Assets,
    material_bank: &mut MaterialBank,
) {
    'material_texture_rx: {
        match ui_state.material_texture_dialog.rx_file_name.try_recv() {
            Ok(texture_path) => {
                let Ok(path) = PathBuf::from_str(&texture_path) else {
                    break 'material_texture_rx;
                };
                if !path.is_absolute() {
                    break 'material_texture_rx;
                }

                let project_assets_dir = session.project_assets_dir().as_ref().unwrap().clone();
                let Ok(relative_asset_path) = path.strip_prefix(&project_assets_dir) else {
                    log::error!(
                        "Picked existing model path {:?} does start with the assets dir.",
                        path
                    );
                    break 'material_texture_rx;
                };

                let Ok(metadata) = std::fs::metadata(&path) else {
                    log::error!("Failed to get existing model file metadata.");
                    break 'material_texture_rx;
                };
                if !metadata.is_file() {
                    log::error!("Existing model path must be a file.");
                    break 'material_texture_rx;
                }

                let material_texture_path = PathBuf::from_str(&texture_path).unwrap();
                let asset_path = GameAssetPath::from_relative_path(relative_asset_path);
                material_bank.update_material_texture(
                    ui_state.material_texture_dialog.associated_material_id,
                    ui_state.material_texture_dialog.associated_texture_type,
                    Some(asset_path),
                );
            }
            Err(_) => {}
        }
    }

    let content = |ui: &mut egui::Ui| {
        ui.label(egui::RichText::new("Voxel Editing").size(20.0));

        ui.horizontal(|ui| {
            ui.label("Entity editing enabled:");
            ui.add(egui::Checkbox::without_text(
                &mut editor.world_editing.entity_enabled,
            ));
        });
        ui.horizontal(|ui| {
            ui.label("Terrain editing enabled:");
            ui.checkbox(&mut editor.world_editing.terrain_enabled, "");
        });

        color_picker(ui, &mut editor.world_editing.color);
        bmat_picker(ui, &mut editor.world_editing.bmat_index);

        material_picker(ui, &mut editor.world_editing.material);

        ui.add_enabled_ui(
            editor.world_editing.terrain_enabled || editor.world_editing.entity_enabled,
            |ui| {
                ui.label("Tools");
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            editor.world_editing.tool != EditorEditingTool::Pencil,
                            egui::Button::new("Pencil"),
                        )
                        .clicked()
                    {
                        editor.world_editing.tool = EditorEditingTool::Pencil;
                    }
                    if ui
                        .add_enabled(
                            editor.world_editing.tool != EditorEditingTool::Eraser,
                            egui::Button::new("Eraser"),
                        )
                        .clicked()
                    {
                        editor.world_editing.tool = EditorEditingTool::Eraser;
                    }
                });
                ui.add_space(8.0);

                let size = &mut editor.world_editing.size;
                match &mut editor.world_editing.tool {
                    EditorEditingTool::Pencil => {
                        ui.label(egui::RichText::new("Pencil").size(18.0));
                        ui.horizontal(|ui| {
                            ui.label("Size:");
                            ui.add(egui::Slider::new(size, 0..=100).step_by(1.0));
                        });
                    }
                    EditorEditingTool::Brush => {}
                    EditorEditingTool::Eraser => {
                        ui.label(egui::RichText::new("Eraser").size(18.0));
                        ui.horizontal(|ui| {
                            ui.label("Size:");
                            ui.add(egui::Slider::new(size, 0..=100).step_by(1.0));
                        });
                    }
                }
            },
        );

        ui.add_space(32.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Materials").size(20.0));
            if ui.button("Add").clicked() {
                let mut name = "New Material".to_owned();
                if material_bank.name_map.contains_key(&name) {
                    let mut i = 1;
                    while material_bank
                        .name_map
                        .contains_key(&format!("{} {}", name, i))
                    {
                        i += 1;
                    }
                    name = format!("{} {}", name, i);
                }
                material_bank.register_material(Material {
                    name,
                    color_texture: None,
                });
            }
        });
        for (material_name, material_id) in &material_bank.name_map {
            ui.push_id(format!("material_{}", material_name), |ui| {
                let material_info = material_bank
                    .materials
                    .get(*material_id)
                    .expect("Material should exist if it is in the name map.");
                let is_open = ui_state.open_materials.contains(material_id);
                ui.horizontal(|ui| {
                    let (id, rect) = ui.allocate_space(egui::vec2(6.0, 6.0));
                    let rect_response = ui.interact(rect, id, Sense::click());
                    editor::ui::asset_browser::paint_node_icon(ui, !is_open, &rect_response);

                    let material_empty_text =
                        material_info.is_empty().then_some(" (Empty)").unwrap_or("");
                    let label_response =
                        ui.label(format!("{}{}", material_name, material_empty_text));
                    if label_response.clicked() || rect_response.clicked() {
                        if is_open {
                            ui_state.open_materials.remove(material_id);
                        } else {
                            ui_state.open_materials.insert(*material_id);
                        }
                    }
                });
                if !is_open {
                    return;
                }
                ui.horizontal(|ui| {
                    ui.label("Color texture:");
                    let text = material_info
                        .color_texture
                        .as_ref()
                        .map(|asset_path| asset_path.as_relative_path_str())
                        .unwrap_or("None".to_owned());
                    let button = ui
                        .add(egui::Button::new(&text).truncate())
                        .on_hover_text(text);
                    if button.clicked() {
                        let send = ui_state.material_texture_dialog.tx_file_name.clone();
                        ui_state.material_texture_dialog.associated_material_id = *material_id;
                        ui_state.material_texture_dialog.associated_texture_type =
                            MaterialTextureType::Color;

                        let mut start_dir = session
                            .project_assets_dir()
                            .expect("Project directory should exist if this is clicked.")
                            .clone();
                        if let Some(color_texture_asset_dir) = material_info.color_texture.as_ref()
                        {
                            start_dir = start_dir.join(
                                color_texture_asset_dir
                                    .as_relative_path()
                                    .parent()
                                    .expect("Asset path should have file name"),
                            );
                        }
                        log::info!(
                            "Opening material texture dialog with start dir {:?}",
                            start_dir
                        );
                        std::thread::spawn(|| {
                            pollster::block_on(async move {
                                let file = rfd::AsyncFileDialog::new()
                                    .set_directory(start_dir)
                                    .add_filter("Image", ImageAsset::supported_extensions())
                                    .pick_file()
                                    .await;
                                let Some(file) = file else {
                                    return;
                                };
                                send.send(file.path().to_string_lossy().to_string());
                            });
                        });
                        ui.close_menu();
                    }
                });
            });
        }
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, content);
}
