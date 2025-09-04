use std::{path::PathBuf, str::FromStr};

use crate::{
    common::color::Color,
    engine::{
        asset::asset::{AssetPath, Assets},
        entity::{ecs_world::ECSWorld, RenderableVoxelEntity},
        ui::EditorUIState,
        voxel::{factory::VoxelModelFactory, voxel::VoxelModelType, voxel_world::VoxelWorld},
    },
    session::Session,
};

pub fn new_voxel_model_dialog(
    ctx: &egui::Context,
    ui_state: &mut EditorUIState,
    ecs_world: &mut ECSWorld,
    session: &mut Session,
    assets: &mut Assets,
    voxel_world: &mut VoxelWorld,
) {
    if let Some(dialog) = &mut ui_state.new_model_dialog {
        let mut force_close = false;
        egui::Window::new("New Voxel Model")
            .collapsible(false)
            .resizable(true)
            .open(&mut dialog.open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    match dialog.rx_file_name.try_recv() {
                        Ok(chosen_name) => dialog.file_path = chosen_name,
                        Err(_) => {}
                    }
                    ui.label("New asset location: ");
                    ui.text_edit_singleline(&mut dialog.file_path);
                    if ui.button("Browse").clicked() {
                        let send = dialog.tx_file_name.clone();
                        let assets_dir = session.project_assets_dir().unwrap();
                        std::thread::spawn(|| {
                            pollster::block_on(async move {
                                let file = rfd::AsyncFileDialog::new()
                                    .set_title("Choose asset location")
                                    .set_file_name("untitled.rvox")
                                    .set_directory(assets_dir)
                                    .save_file()
                                    .await;
                                let Some(file) = file else {
                                    return;
                                };
                                send.send(file.path().to_string_lossy().to_string());
                            });
                        });
                    }
                });

                let path = PathBuf::from_str(&dialog.file_path);
                let mut error = String::new();
                let mut is_valid = 'is_path_valid: {
                    if dialog.last_file_path.0 == dialog.file_path {
                        error = dialog.last_file_path.2.clone();
                        break 'is_path_valid dialog.last_file_path.1;
                    }

                    if dialog.file_path.is_empty() {
                        break 'is_path_valid false;
                    }
                    let Ok(path) = path.as_ref() else {
                        break 'is_path_valid false;
                    };

                    if !path.is_absolute() {
                        error = "Path must be absolute.".to_owned();
                        break 'is_path_valid false;
                    }

                    if !path.starts_with(session.project_assets_dir().unwrap()) {
                        error = "Path must be within the project asset directory.".to_owned();
                        break 'is_path_valid false;
                    }

                    true
                };
                if !error.is_empty() {
                    ui.add(egui::Label::new(
                        egui::RichText::new(error.clone()).color(egui::Color32::RED),
                    ));
                }
                dialog.last_file_path = (dialog.file_path.clone(), is_valid, error);

                ui.label("Dimensions:");
                ui.horizontal(|ui| {
                    ui.label("X:");
                    let mut x_temp = dialog.dimensions.x.to_string();
                    egui::TextEdit::singleline(&mut x_temp)
                        .desired_width(32.0)
                        .show(ui);
                    if let Ok(x) = x_temp.parse() {
                        dialog.dimensions.x = x;
                    }

                    ui.label("Y:");
                    let mut y_temp = dialog.dimensions.y.to_string();
                    egui::TextEdit::singleline(&mut y_temp)
                        .desired_width(32.0)
                        .show(ui);
                    if let Ok(y) = y_temp.parse() {
                        dialog.dimensions.y = y;
                    }

                    ui.label("Z:");
                    let mut z_temp = dialog.dimensions.z.to_string();
                    egui::TextEdit::singleline(&mut z_temp)
                        .desired_width(32.0)
                        .show(ui);
                    if let Ok(z) = z_temp.parse() {
                        dialog.dimensions.z = z;
                    }
                });
                is_valid = is_valid && dialog.dimensions.iter().all(|x| *x > 0);

                ui.horizontal(|ui| {
                    ui.label("Model type: ");
                    egui::ComboBox::from_id_salt("new_voxel_model_dropdown")
                        .selected_text(format!("{:?}", dialog.model_type))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut dialog.model_type,
                                VoxelModelType::Flat,
                                "Flat",
                            );
                        });
                });

                if ui
                    .add_enabled(is_valid, egui::Button::new("Create"))
                    .clicked()
                {
                    let flat = VoxelModelFactory::create_cuboid(
                        dialog.dimensions,
                        Color::new_srgb(0.5, 0.5, 0.5),
                    );
                    let file_path = PathBuf::from_str(&dialog.file_path).unwrap();
                    let asset_path = AssetPath::from_project_dir_path(
                        session.project_save_dir.as_ref().unwrap(),
                        &file_path,
                    );
                    assets.save_asset(asset_path.clone(), flat.model.clone());
                    let model_id = voxel_world.register_renderable_voxel_model(
                        format!(
                            "asset_{:?}",
                            file_path.strip_prefix(session.project_assets_dir().unwrap())
                        ),
                        flat,
                    );
                    voxel_world
                        .registry
                        .set_voxel_model_asset_path(model_id, Some(asset_path));
                    if let Ok(mut renderable) =
                        ecs_world.get::<&mut RenderableVoxelEntity>(dialog.associated_entity)
                    {
                        renderable.set_id(model_id);
                    }
                    force_close = true;
                }
            });
        if !dialog.open || force_close {
            ui_state.new_model_dialog = None;
        }
    }
}
