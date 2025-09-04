use std::{path::PathBuf, str::FromStr};

use hecs::With;

use crate::{
    engine::{
        asset::repr::editor_settings::EditorProjectAsset,
        entity::{ecs_world::ECSWorld, GameEntity},
        ui::EditorUIState,
    },
    session::Session,
};

pub fn new_project_dialog(
    ctx: &egui::Context,
    ui_state: &mut EditorUIState,
    ecs_world: &mut ECSWorld,
    session: &mut Session,
) {
    if let Some(new_project_dialog) = &mut ui_state.new_project_dialog {
        let mut force_close = false;
        egui::Window::new("New Project")
            .collapsible(false)
            .resizable(true)
            .open(&mut new_project_dialog.open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    match new_project_dialog.rx_file_name.try_recv() {
                        Ok(chosen_name) => new_project_dialog.file_name = chosen_name,
                        Err(_) => {}
                    }
                    ui.label("Project location: ");
                    ui.text_edit_singleline(&mut new_project_dialog.file_name);
                    if ui.button("Browse").clicked() {
                        let send = new_project_dialog.tx_file_name.clone();
                        std::thread::spawn(|| {
                            pollster::block_on(async move {
                                let folder = rfd::AsyncFileDialog::new().pick_folder().await;
                                let Some(folder) = folder else {
                                    return;
                                };
                                send.send(folder.path().to_string_lossy().to_string());
                            });
                        });
                    }
                });

                let path = PathBuf::from_str(&new_project_dialog.file_name);
                let mut error = String::new();
                let mut is_valid = 'is_path_valid: {
                    if new_project_dialog.last_file_name.0 == new_project_dialog.file_name {
                        error = new_project_dialog.last_file_name.2.clone();
                        break 'is_path_valid new_project_dialog.last_file_name.1;
                    }

                    if new_project_dialog.file_name.is_empty() {
                        break 'is_path_valid false;
                    }
                    let Ok(path) = path.as_ref() else {
                        break 'is_path_valid false;
                    };

                    if !path.is_absolute() {
                        error = "Path must be absolute.".to_owned();
                        break 'is_path_valid false;
                    }
                    let Ok(metadata) = std::fs::metadata(path) else {
                        error = "Directory doesn't exist.".to_owned();
                        break 'is_path_valid false;
                    };
                    if !metadata.is_dir() {
                        error = "Path must be a directory.".to_owned();
                        break 'is_path_valid false;
                    }
                    let Ok(read) = std::fs::read_dir(path) else {
                        error = "Failed to read directory.".to_owned();
                        break 'is_path_valid false;
                    };
                    if read.count() > 0 {
                        error = "Directory must be empty.".to_owned();
                        //break 'is_path_valid false;
                    }

                    true
                };
                if !error.is_empty() {
                    ui.add(egui::Label::new(
                        egui::RichText::new(error.clone()).color(egui::Color32::RED),
                    ));
                }
                new_project_dialog.last_file_name =
                    (new_project_dialog.file_name.clone(), is_valid, error);

                if ui
                    .add_enabled(is_valid, egui::Button::new("Create"))
                    .clicked()
                {
                    let mut existing_entities_query = ecs_world.query::<With<(), &GameEntity>>();
                    let existing_entities = existing_entities_query
                        .into_iter()
                        .map(|(entity_id, _)| entity_id)
                        .collect::<Vec<_>>();
                    drop(existing_entities_query);
                    for id in existing_entities {
                        ecs_world.despawn(id);
                    }

                    session.project_save_dir = Some(path.clone().unwrap());
                    session.editor_settings.last_project_dir = Some(path.unwrap());
                    session.project = EditorProjectAsset::new_empty();
                    force_close = true;
                }
            });
        if !new_project_dialog.open || force_close {
            ui_state.new_project_dialog = None;
        }
    }
}
