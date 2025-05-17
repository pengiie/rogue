use std::{
    fs::Metadata,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use egui::Color32;
use egui_dock::DockState;
use hecs::With;
use nalgebra::{SimdValue, Translation3, Vector2, Vector3, Vector4};

use crate::{
    common::color::Color,
    consts,
    engine::{
        asset::{
            asset::{AssetPath, Assets},
            repr::{
                editor_settings::EditorProjectAsset, image::ImageAsset,
                world::voxel::VoxelModelAnyAsset,
            },
        },
        entity::{ecs_world::ECSWorld, RenderableVoxelEntity},
        graphics::camera::Camera,
        physics::transform::Transform,
        ui::{
            gui::Egui, EditorAssetBrowserState, EditorNewProjectDialog, EditorNewVoxelModelDialog,
            EditorTab, EditorUIState, UI,
        },
        voxel::{
            factory::VoxelModelFactory,
            flat::VoxelModelFlat,
            thc::VoxelModelTHC,
            voxel::{VoxelModel, VoxelModelImpl, VoxelModelImplConcrete, VoxelModelType},
            voxel_registry::{VoxelModelId, VoxelModelInfo},
            voxel_world::{self, VoxelWorld},
        },
        window::window::Window,
    },
    game::entity::GameEntity,
    session::Session,
};

use super::editor::{Editor, EditorEditingTool, EditorView};

pub fn init_editor_ui_textures(ctx: &egui::Context, ui_state: &mut EditorUIState) {
    let icon_color = ctx
        .style()
        .visuals
        .text_color()
        .blend(Color32::from_black_alpha((1.0 * 255.0) as u8));

    let mut load_icon = |icon_name: &str, asset_path: &str| {
        let mut image =
            Assets::load_asset_sync::<ImageAsset>(AssetPath::new_binary_dir(asset_path)).unwrap();
        for height in 0..image.size.y {
            for width in 0..image.size.x {
                let offset = ((width + height * image.size.x) * 4) as usize;
                image.data[offset] = icon_color.r();
                image.data[offset + 1] = icon_color.g();
                image.data[offset + 2] = icon_color.b();
            }
        }
        let mut color_image = egui::ColorImage::from_rgba_unmultiplied(
            [image.size.x as usize, image.size.y as usize],
            &image.data,
        );
        let ex_img = ctx.load_texture(icon_name, color_image, egui::TextureOptions::default());
        ui_state.texture_map.insert(icon_name.to_owned(), ex_img);
    };

    load_icon(
        consts::egui::icons::FOLDER,
        consts::egui::icons::FOLDER_ASSET,
    );
    load_icon(
        consts::egui::icons::UNKNOWN,
        consts::egui::icons::UNKNOWN_ASSET,
    );
    load_icon(
        consts::egui::icons::VOXEL_MODEL_FILE,
        consts::egui::icons::VOXEL_MODEL_FILE_ASSET,
    );
    load_icon(
        consts::egui::icons::TEXT_FILE,
        consts::egui::icons::TEXT_FILE_ASSET,
    );
    load_icon(
        consts::egui::icons::LUA_FILE,
        consts::egui::icons::LUA_FILE_ASSET,
    );
}

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

/// Returns padding [top, bottom, left right].
pub fn egui_editor_ui(
    ctx: &egui::Context,
    ecs_world: &mut ECSWorld,
    voxel_world: &mut VoxelWorld,
    mut editor: &mut Editor,
    mut ui_state: &mut EditorUIState,
    session: &mut Session,
    assets: &mut Assets,
    window: &mut Window,
) -> Vector4<f32> {
    if !ui_state.initialized_icons {
        init_editor_ui_textures(ctx, ui_state);
        ui_state.initialized_icons = true;
    }

    //catppuccin_egui::set_theme(&ctx, catppuccin_egui::MOCHA);
    let mut content_padding = Vector4::zeros();

    let mut dock_style = egui_dock::Style::from_egui(ctx.style().as_ref());
    dock_style.main_surface_border_stroke.width = 0.0;
    dock_style.main_surface_border_stroke.color = egui::Color32::TRANSPARENT;

    content_padding.x = egui::TopBottomPanel::top("top_editor_pane")
        .frame(
            egui::Frame::new()
                .fill(ctx.style().visuals.window_fill)
                .inner_margin(6.0),
        )
        .show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New").clicked() {
                        if ui_state.new_project_dialog.is_none() {
                            let (tx, rx) = std::sync::mpsc::channel();
                            ui_state.new_project_dialog = Some(EditorNewProjectDialog {
                                open: true,
                                file_name: String::new(),
                                tx_file_name: tx,
                                rx_file_name: rx,
                                last_file_name: (String::new(), false, String::new()),
                            });
                        }
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(
                            session.project_save_dir.is_some(),
                            egui::Button::new("Save"),
                        )
                        .clicked()
                    {
                        session.save_project(assets, session, editor, ecs_world, voxel_world);
                        ui.close_menu();
                    }
                    if ui.button("Open").clicked() {}
                });
                ui.menu_button("View", |ui| {
                    ui.menu_button("Open Window...", |ui| {
                        if ui.button("World inspector").clicked() {}
                        if ui.button("Entity inspector").clicked() {}
                    })
                });
                ui.menu_button("Help", |ui| {
                    ui.label("Good luck :)");
                });
            });
            if session.project_save_dir.is_none() {
                ui.label("Please perform File -> New to start a project.");
                return;
            }

            ui.add_space(4.0);

            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        editor.curr_editor_view != EditorView::PanOrbit,
                        egui::Button::new("Pan/Orbit/Zoom"),
                    )
                    .clicked()
                {
                    editor.switch_to_pan_orbit(ecs_world, window);
                }

                if ui
                    .add_enabled(
                        editor.curr_editor_view != EditorView::Fps,
                        egui::Button::new("First person"),
                    )
                    .clicked()
                {
                    editor.switch_to_fps(window);
                }
            });
        })
        .response
        .rect
        .height()
        * ctx.pixels_per_point();

    // DIALOGS
    new_project_dialog(ctx, ui_state, ecs_world, session);
    new_voxel_model_dialog(ctx, ui_state, ecs_world, session, assets, voxel_world);

    // Hide left and right panel if we don't have a project selected.
    if session.project_save_dir.is_none() {
        return content_padding;
    }

    // LEFT PANEL

    content_padding.z = egui::SidePanel::left("left_editor_pane")
        .resizable(true)
        .frame(
            egui::Frame::new()
                .fill(ctx.style().visuals.window_fill)
                .inner_margin(8.0),
        )
        .default_width(300.0)
        .max_width(500.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(
                    egui::RichText::new("Inspector").size(20.0),
                ));
                ui.menu_button("Add", |ui| {
                    if ui.button("Empty").clicked() {
                        ecs_world.spawn((
                            GameEntity::new("new_entity"),
                            Transform::with_translation(Translation3::from(
                                editor.editor_camera.rotation_anchor,
                            )),
                        ));
                    }
                    if ui.button("Cube").clicked() {
                        let model_id = voxel_world.registry.register_renderable_voxel_model(
                            "entity",
                            VoxelModelFactory::create_cuboid(
                                Vector3::new(32, 32, 32),
                                editor.world_editing.color.clone(),
                            ),
                        );
                        ecs_world.spawn((
                            GameEntity::new("new_entity"),
                            Transform::with_translation(Translation3::from(
                                editor.editor_camera.rotation_anchor,
                            )),
                            RenderableVoxelEntity::new(model_id),
                        ));
                    }
                });
            });

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut game_entity_query = ecs_world.query::<&GameEntity>();
                    for (entity_id, game_entity) in game_entity_query.into_iter() {
                        let label_id =
                            egui::Id::new(format!("left_panel_{}_entity_label", entity_id.id()));
                        let is_hovering = ui.data(|w| w.get_temp(label_id).unwrap_or(false));

                        let mut text = egui::RichText::new(game_entity.name.clone());
                        if is_hovering {
                            text = text.background_color(egui::Color32::from_white_alpha(2));
                        }
                        if editor.selected_entity.is_some()
                            && editor.selected_entity.unwrap() == entity_id
                        {
                            text = text.background_color(egui::Color32::from_white_alpha(3));
                        }
                        let mut label = ui.add(egui::Label::new(text).truncate());

                        ui.data_mut(|w| w.insert_temp(label_id, label.hovered()));
                        if label.hovered() {
                            editor.hovered_entity = Some(entity_id);
                        }
                        if label.clicked() {
                            editor.selected_entity = Some(entity_id);
                        }
                    }
                });
            //ui.label(egui::RichText::new("Performance:").size(8.0));
            //ui.label(format!("FPS: {}", debug_state.fps));
            //ui.label(format!("Frame time: {}ms", debug_state.delta_time_ms));
            //ui.label(format!("Voxel data allocation: {}", total_allocation_str));
        })
        .response
        .rect
        .width()
        * ctx.pixels_per_point();

    // RIGHT PANEL

    content_padding.w = egui::SidePanel::right("right_editor_pane")
        .resizable(true)
        .frame(egui::Frame::new().fill(ctx.style().visuals.panel_fill))
        .default_width(300.0)
        .show(ctx, |ui| {
            right_editor_pane(
                ui,
                ecs_world,
                &mut editor,
                voxel_world,
                &mut ui_state,
                session,
                assets,
            );
        })
        .response
        .rect
        .width()
        * ctx.pixels_per_point();

    content_padding.y = egui::TopBottomPanel::bottom("bottom_editor_pane")
        .resizable(true)
        .frame(
            egui::Frame::new()
                .fill(ctx.style().visuals.window_fill)
                .inner_margin(8.0),
        )
        .default_height(300.0)
        .show(ctx, |ui| {
            bottom_editor_pane(ui, &session, &mut ui_state);
        })
        .response
        .rect
        .height()
        * ctx.pixels_per_point();

    return content_padding;
}

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
                    let model_id = voxel_world.registry.register_renderable_voxel_model(
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
                    voxel_world.to_update_normals.insert(model_id);
                    force_close = true;
                }
            });
        if !dialog.open || force_close {
            ui_state.new_model_dialog = None;
        }
    }
}

fn bottom_editor_pane(ui: &mut egui::Ui, session: &Session, state: &mut EditorUIState) {
    let Some(project_dir) = &session.project_save_dir else {
        return;
    };
    if state.asset_browser.needs_reload {
        state.asset_browser.reload(&project_dir.join("assets"));
        state.asset_browser.needs_reload = false;
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui: &mut egui::Ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Asset Browser").size(18.0));
                ui.label(egui::RichText::new("|").size(16.0));
                ui.label(
                    egui::RichText::new(format!(
                        "{}/",
                        state.asset_browser.sub_path.to_string_lossy()
                    ))
                    .size(14.0),
                );
            });
            ui.horizontal(|ui| {
                let is_at_root = state.asset_browser.sub_path.to_string_lossy() != "./";
                if ui
                    .add_enabled(is_at_root, egui::Button::new("Back"))
                    .clicked()
                {
                    state.asset_browser.sub_path.pop();
                    state.asset_browser.needs_reload = true;
                }
                if ui.button("Reload").clicked() {
                    state.asset_browser.needs_reload = true;
                }
            });

            egui::Grid::new("asset_grid").show(ui, |ui| {
                for asset in &state.asset_browser.contents {
                    let item_id = egui::Id::new(format!(
                        "asset_browser_{}_label",
                        asset.file_sub_path.to_string_lossy()
                    ));
                    let is_hovering = ui.data(|w| w.get_temp(item_id).unwrap_or(false));

                    let file_image_icon = if asset.is_dir {
                        consts::egui::icons::FOLDER
                    } else {
                        let Some(ext) = asset.file_sub_path.extension() else {
                            log::error!(
                                "Couldn't get extension of file {}",
                                asset.file_sub_path.to_string_lossy()
                            );
                            return;
                        };

                        match ext.to_string_lossy().to_string().as_str() {
                            "lua" => consts::egui::icons::LUA_FILE,
                            "txt" => consts::egui::icons::TEXT_FILE,
                            "rvox" => consts::egui::icons::VOXEL_MODEL_FILE,
                            _ => consts::egui::icons::UNKNOWN,
                        }
                    };

                    let mut frame = egui::Frame::new().inner_margin(egui::Margin {
                        left: 0,
                        right: 0,
                        top: 0,
                        bottom: 4,
                    });
                    if is_hovering {
                        frame = frame.fill(egui::Color32::from_white_alpha(2))
                    }
                    let res = ui.scope_builder(
                        egui::UiBuilder::new().sense(egui::Sense::click()),
                        |ui| {
                            frame.show(ui, |ui| {
                                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                                    ui.image(
                                        state.get_image(file_image_icon, egui::vec2(96.0, 96.0)),
                                    );
                                    ui.label(
                                        egui::RichText::new(
                                            asset
                                                .file_sub_path
                                                .file_name()
                                                .unwrap()
                                                .to_string_lossy(),
                                        )
                                        .size(16.0),
                                    );
                                });
                            });
                        },
                    );

                    ui.data_mut(|w| w.insert_temp(item_id, res.response.hovered()));
                    if res.response.clicked() {
                        if asset.is_dir {
                            state.asset_browser.sub_path = asset.file_sub_path.clone();
                            state.asset_browser.needs_reload = true;
                        }
                    }
                }
            });
        });
}

fn entity_properties_pane(
    ui: &mut egui::Ui,
    ecs_world: &mut ECSWorld,
    editor: &Editor,
    voxel_world: &mut VoxelWorld,
    ui_state: &mut EditorUIState,
    session: &mut Session,
    assets: &mut Assets,
) {
    'existing_model_dialog_rx: {
        match ui_state.existing_model_dialog.rx_file_name.try_recv() {
            Ok(model_path) => {
                let Ok(path) = PathBuf::from_str(&model_path) else {
                    break 'existing_model_dialog_rx;
                };
                if !path.is_absolute() {
                    break 'existing_model_dialog_rx;
                }
                if !path.starts_with(session.project_assets_dir().unwrap()) {
                    log::error!(
                        "Picked existing model path {:?} does start with the assets dir.",
                        path
                    );
                    break 'existing_model_dialog_rx;
                }

                let Ok(metadata) = std::fs::metadata(&path) else {
                    log::error!("Failed to get existing model file metadata.");
                    break 'existing_model_dialog_rx;
                };
                if !metadata.is_file() {
                    log::error!("Existing model path must be a file.");
                    break 'existing_model_dialog_rx;
                }

                let model_path = PathBuf::from_str(&model_path).unwrap();
                let asset_path = AssetPath::from_project_dir_path(
                    session.project_save_dir.as_ref().unwrap(),
                    &model_path,
                );
                let model = Assets::load_asset_sync::<VoxelModelAnyAsset>(asset_path.clone())
                    .expect("Failed to load model");
                let model_id = voxel_world.registry.register_renderable_voxel_model_any(
                    format!(
                        "asset_{:?}",
                        model_path.strip_prefix(session.project_assets_dir().unwrap())
                    ),
                    model,
                );
                voxel_world
                    .registry
                    .set_voxel_model_asset_path(model_id, Some(asset_path));
                if let Ok(mut renderable) = ecs_world.get::<&mut RenderableVoxelEntity>(
                    ui_state.existing_model_dialog.associated_entity,
                ) {
                    renderable.set_id(model_id);
                }
                voxel_world.to_update_normals.insert(model_id);
            }
            Err(_) => {}
        }
    }

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Entity properties").size(20.0));
        if let Some(selected_entity) = &editor.selected_entity {
            ui.menu_button("Add component", |ui| {
                if ui.button("Camera").clicked() {
                    ecs_world.insert_one(*selected_entity, Camera::new(Camera::FOV_90));
                    ui.close_menu();
                }
                if ui.button("Renderable").clicked() {
                    ecs_world.insert_one(*selected_entity, RenderableVoxelEntity::new_null());
                    ui.close_menu();
                }
            });
        }
    });
    ui.add_space(16.0);

    let content = |ui: &mut egui::Ui| {
        if let Some(selected_entity) = &editor.selected_entity {
            let Ok(mut game_entity) = ecs_world.get::<&mut GameEntity>(*selected_entity) else {
                log::error!(
                    "Selected entity should be a game entity, and selected_entity should be valid."
                );
                return;
            };

            fn component_widget<R>(
                ui: &mut egui::Ui,
                header: &str,
                on_remove: Option<&mut bool>,
                add_contents: impl FnOnce(&mut egui::Ui) -> R,
            ) {
                let last_spacing = ui.style().spacing.item_spacing.y;
                ui.style_mut().spacing.item_spacing.y = 2.0;

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(header).size(16.0));
                    if let Some(on_remove) = on_remove {
                        if ui.button("Remove").clicked() {
                            *on_remove = true;
                        }
                    }
                });
                ui.style_mut().spacing.item_spacing.y = last_spacing;
                egui::Frame::new()
                    .stroke(egui::Stroke::new(
                        2.0,
                        ui.style().visuals.window_stroke.color.gamma_multiply(0.3),
                    ))
                    .corner_radius(4.0)
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.style_mut().spacing.item_spacing.y = 4.0;
                        add_contents(ui);
                    });
            };

            ui.style_mut().spacing.item_spacing.y = 8.0;
            component_widget(ui, "General", None, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name: ");
                    ui.text_edit_singleline(&mut game_entity.name);
                });
                ui.label(format!("UUID: {}", game_entity.uuid));
            });
            drop(game_entity);

            if let Ok(mut transform) = ecs_world.get::<&mut Transform>(*selected_entity) {
                component_widget(ui, "Transform", None, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Position:");
                        ui.label("X");
                        ui.add(
                            egui::DragValue::new(&mut transform.position.x)
                                .suffix(" m")
                                .speed(0.01)
                                .fixed_decimals(2),
                        );
                        ui.label("Y");
                        ui.add(
                            egui::DragValue::new(&mut transform.position.y)
                                .suffix(" m")
                                .speed(0.01)
                                .fixed_decimals(2),
                        );
                        ui.label("Z");
                        ui.add(
                            egui::DragValue::new(&mut transform.position.z)
                                .suffix(" m")
                                .speed(0.01)
                                .fixed_decimals(2),
                        );
                    });
                });
            }

            let mut remove_renderable = false;
            if let Ok(mut renderable_voxel_model) =
                ecs_world.get::<&mut RenderableVoxelEntity>(*selected_entity)
            {
                component_widget(ui, "Renderable", Some(&mut remove_renderable), |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Voxel model:");
                        let text = if let Some(model_id) = renderable_voxel_model.voxel_model_id() {
                            let info = voxel_world.registry.get_model_info(model_id);
                            info.asset_path
                                .as_ref()
                                .map(|path| {
                                    format!(
                                        "/{}",
                                        path.path()
                                            .strip_prefix(&session.project_assets_dir().unwrap())
                                            .unwrap()
                                            .to_string_lossy()
                                    )
                                })
                                .unwrap_or("In memory (unsaved)".to_string())
                        } else {
                            "None".to_owned()
                        };
                        ui.menu_button(text, |ui| {
                            if ui.button("Create new").clicked() {
                                let (tx, rx) = std::sync::mpsc::channel();
                                ui_state.new_model_dialog = Some(EditorNewVoxelModelDialog {
                                    open: true,
                                    associated_entity: *selected_entity,
                                    file_path: String::new(),
                                    tx_file_name: tx,
                                    rx_file_name: rx,
                                    last_file_path: (String::new(), false, String::new()),
                                    dimensions: Vector3::new(32, 32, 32),
                                    model_type: VoxelModelType::Flat,
                                });
                                ui.close_menu();
                            }
                            if ui.button("Choose existing").clicked() {
                                let send = ui_state.existing_model_dialog.tx_file_name.clone();
                                ui_state.existing_model_dialog.associated_entity = *selected_entity;
                                std::thread::spawn(|| {
                                    pollster::block_on(async move {
                                        let file = rfd::AsyncFileDialog::new()
                                            .add_filter("RVox", &["rvox"])
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
                            let model_info = renderable_voxel_model
                                .voxel_model_id()
                                .map_or(None, |id| Some(voxel_world.registry.get_model_info(id)));
                            if ui
                                .add_enabled(
                                    model_info
                                        .filter(|info| info.asset_path.is_some())
                                        .is_some(),
                                    egui::Button::new("Save"),
                                )
                                .clicked()
                            {
                                let model_id = renderable_voxel_model.voxel_model_id().unwrap();
                                let model_info = model_info.unwrap();
                                let asset_path = model_info.asset_path.clone().unwrap();
                                match &model_info.model_type {
                                    Some(VoxelModelType::Flat) => {
                                        let flat = voxel_world
                                            .get_model::<VoxelModelFlat>(model_id)
                                            .clone();
                                        assets.save_asset(asset_path, flat);
                                    }
                                    Some(VoxelModelType::THC) => {
                                        let thc = voxel_world
                                            .get_model::<VoxelModelTHC>(model_id)
                                            .clone();
                                        assets.save_asset(asset_path, thc);
                                    }
                                    None => {
                                        log::error!("Don't know how to save this asset format");
                                    }
                                }
                                ui.close_menu();
                            }
                        });
                    });
                    if let Some(model_id) = renderable_voxel_model.voxel_model_id() {
                        let info = voxel_world.registry.get_model_info(model_id).clone();
                        let text = match &info.model_type {
                            Some(ty) => ty.as_ref(),
                            None => "Unknown",
                        };
                        ui.horizontal(|ui| {
                            ui.label("Model type:");
                            if let Some(model_type) = info.model_type {
                                ui.menu_button(text, |ui| {
                                    fn convert_model<
                                        T: VoxelModelImplConcrete,
                                        C: VoxelModelImplConcrete + for<'a> From<&'a T>,
                                    >(
                                        voxel_world: &mut VoxelWorld,
                                        renderable_voxel_model: &mut RenderableVoxelEntity,
                                        info: &VoxelModelInfo,
                                        original_id: VoxelModelId,
                                    ) {
                                        let converted_model = C::from(
                                            voxel_world.registry.get_model::<T>(original_id),
                                        );
                                        let converted_model_id =
                                            voxel_world.registry.register_renderable_voxel_model(
                                                &info.name,
                                                VoxelModel::new(converted_model),
                                            );
                                        voxel_world.registry.set_voxel_model_asset_path(
                                            converted_model_id,
                                            info.asset_path.clone(),
                                        );
                                        renderable_voxel_model.set_id(converted_model_id);
                                        voxel_world.to_update_normals.insert(converted_model_id);
                                    }

                                    ui.label("Convert to");
                                    if ui
                                        .add_enabled(
                                            model_type != VoxelModelType::Flat,
                                            egui::Button::new("Flat"),
                                        )
                                        .clicked()
                                    {
                                        match model_type {
                                            VoxelModelType::THC => {
                                                convert_model::<VoxelModelTHC, VoxelModelFlat>(
                                                    voxel_world,
                                                    &mut renderable_voxel_model,
                                                    &info,
                                                    model_id,
                                                )
                                            }
                                            _ => unreachable!(),
                                        }
                                        ui.close_menu();
                                    }
                                    if ui
                                        .add_enabled(
                                            model_type != VoxelModelType::THC,
                                            egui::Button::new("THC"),
                                        )
                                        .clicked()
                                    {
                                        match model_type {
                                            VoxelModelType::Flat => {
                                                convert_model::<VoxelModelFlat, VoxelModelTHC>(
                                                    voxel_world,
                                                    &mut renderable_voxel_model,
                                                    &info,
                                                    model_id,
                                                )
                                            }
                                            _ => unreachable!(),
                                        }
                                        ui.close_menu();
                                    }
                                });
                            } else {
                                ui.label(text);
                            }
                        });
                    }
                });
            }
            if remove_renderable {
                ecs_world.remove_one::<RenderableVoxelEntity>(*selected_entity);
            }

            let mut remove_camera = false;
            if let Ok(mut camera) = ecs_world.get::<&mut Camera>(*selected_entity) {
                component_widget(ui, "Camera", Some(&mut remove_camera), |ui| {
                    ui.horizontal(|ui| {
                        ui.label("FOV");
                        let mut deg = camera.fov.to_degrees();
                        ui.add(egui::Slider::new(&mut deg, 1.0..=180.0));
                        camera.fov = deg.to_radians();
                    });
                });
            }
            if remove_camera {
                ecs_world.remove_one::<Camera>(*selected_entity);
                if Some(*selected_entity) == session.game_camera {
                    session.game_camera = None;
                }
            }
            ui.label("No entity selected.");
        }
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, content);
}

fn world_pane(
    ui: &mut egui::Ui,
    ecs_world: &mut ECSWorld,
    editor: &mut Editor,
    voxel_world: &mut VoxelWorld,
    mut assets: &mut Assets,
    ui_state: &mut EditorUIState,
    session: &mut Session,
) {
    'terrain_dialog_rx: {
        match ui_state.terrain_dialog.rx_file_name.try_recv() {
            Ok(new_terrain_dir) => {
                let Ok(path) = PathBuf::from_str(&new_terrain_dir) else {
                    break 'terrain_dialog_rx;
                };
                if !path.is_absolute() {
                    break 'terrain_dialog_rx;
                }
                if !path.starts_with(session.project_assets_dir().unwrap()) {
                    log::error!("Terrain path {:?} does start with the assets dir.", path);
                    break 'terrain_dialog_rx;
                }

                let Ok(metadata) = std::fs::metadata(&path) else {
                    log::error!("Failed to get terrian path metadata.");
                    break 'terrain_dialog_rx;
                };
                if !metadata.is_dir() {
                    log::error!("Terrain path must be a directory.");
                    break 'terrain_dialog_rx;
                }
                let Ok(read) = std::fs::read_dir(&path) else {
                    log::error!("Failed to read terrian path.");
                    break 'terrain_dialog_rx;
                };
                let is_dir_empty = read.count() == 0;

                session.terrain_dir = Some(path);
                voxel_world.chunks.enqueue_save_all();
                voxel_world
                    .chunks
                    .save_terrain(assets, &voxel_world.registry, &session);
                if !is_dir_empty {
                    voxel_world.chunks.clear();
                }
            }
            Err(_) => {}
        }
    }

    let content = |ui: &mut egui::Ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("World").size(20.0));
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Terrain directory:");
            let text = if let Some(terrain_dir) = &session.terrain_dir {
                format!(
                    "/{}",
                    terrain_dir
                        .strip_prefix(&session.project_assets_dir().unwrap())
                        .unwrap()
                        .to_string_lossy()
                )
            } else {
                "None".to_owned()
            };
            if ui.button(text).clicked() {
                let send = ui_state.terrain_dialog.tx_file_name.clone();
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
        if ui
            .add_enabled(!voxel_world.chunks.is_saving(), egui::Button::new("Save"))
            .clicked()
        {
            voxel_world
                .chunks
                .save_terrain(assets, &voxel_world.registry, session);
        }

        'editor_camera_props: {
            let Some(editor_camera_entity) = editor.editor_camera_entity else {
                break 'editor_camera_props;
            };
            let Ok(editor_transform) = ecs_world.get::<&Transform>(editor_camera_entity) else {
                break 'editor_camera_props;
            };
            ui.label(format!(
                "Editor camera position: {:.3} {:.3} {:.3}",
                editor_transform.position.x,
                editor_transform.position.y,
                editor_transform.position.z
            ));
            let current_region = editor_transform
                .position
                .map(|x| (x / consts::voxel::TERRAIN_REGION_METER_LENGTH).floor() as i32);
            ui.label(format!(
                "Region: {} {} {}",
                current_region.x, current_region.y, current_region.z
            ));

            let current_chunk = editor_transform
                .position
                .map(|x| (x / consts::voxel::TERRAIN_CHUNK_METER_LENGTH).floor() as i32);
            ui.label(format!(
                "Chunk: {} {} {}",
                current_chunk.x, current_chunk.y, current_chunk.z
            ));

            let current_chunk = editor
                .editor_camera
                .rotation_anchor
                .map(|x| (x / consts::voxel::TERRAIN_CHUNK_METER_LENGTH).floor() as i32);
            ui.add_space(8.0);
            ui.label(format!(
                "Current chunk {} {} {}",
                current_chunk.x, current_chunk.y, current_chunk.z
            ));
            ui.horizontal(|ui| {
                ui.label("Generation radius:");
                ui.add(
                    egui::Slider::new(&mut editor.terrain_generation.generation_radius, 0..=4)
                        .step_by(1.0),
                );
            });
            ui.horizontal(|ui| {
                let is_currently_generating = voxel_world.async_edit_count() > 0;
                if ui
                    .add_enabled(
                        !is_currently_generating,
                        egui::Button::new("Generate chunks"),
                    )
                    .clicked()
                {
                    let center = current_chunk;
                    let rad = editor.terrain_generation.generation_radius as i32;
                    let min = center - Vector3::new(rad, rad, rad);
                    let side_length = (rad * 2 - 1).max(0);
                    let max = min + Vector3::new(side_length, side_length, side_length);
                    for x in min.x..=max.x {
                        for y in min.y..=max.y {
                            for z in min.z..=max.z {
                                editor
                                    .terrain_generation
                                    .chunk_generator
                                    .generate_chunk(voxel_world, Vector3::new(x, y, z));
                            }
                        }
                    }
                }
                if is_currently_generating {
                    ui.label(format!(
                        "Generating, {} chunk{} remaining",
                        voxel_world.async_edit_count(),
                        if voxel_world.async_edit_count() > 1 {
                            "s"
                        } else {
                            ""
                        }
                    ));
                }
            });
        }
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, content);
}

fn editing_pane(
    ui: &mut egui::Ui,
    ecs_world: &mut ECSWorld,
    editor: &mut Editor,
    voxel_world: &mut VoxelWorld,
    ui_state: &mut EditorUIState,
    session: &mut Session,
    assets: &mut Assets,
) {
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

        let editor_color = &mut editor.world_editing.color;
        let mut egui_color = egui::Color32::from_rgb(
            editor_color.r_u8(),
            editor_color.g_u8(),
            editor_color.b_u8(),
        );

        egui::color_picker::color_picker_color32(
            ui,
            &mut egui_color,
            egui::color_picker::Alpha::Opaque,
        );
        editor_color.set_rgb_u8(egui_color.r(), egui_color.g(), egui_color.b());

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
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, content);
}

fn right_editor_pane(
    ui: &mut egui::Ui,
    ecs_world: &mut ECSWorld,
    editor: &mut Editor,
    voxel_world: &mut VoxelWorld,
    ui_state: &mut EditorUIState,
    session: &mut Session,
    assets: &mut Assets,
) {
    ui.horizontal(|ui| {
        ui.style_mut().spacing.item_spacing.x = 1.0;
        if ui
            .add_enabled(
                ui_state.right_pane_state != EditorTab::EntityProperties,
                egui::Button::new("Entity"),
            )
            .clicked()
        {
            ui_state.right_pane_state = EditorTab::EntityProperties;
        }
        if ui
            .add_enabled(
                ui_state.right_pane_state != EditorTab::WorldProperties,
                egui::Button::new("World"),
            )
            .clicked()
        {
            ui_state.right_pane_state = EditorTab::WorldProperties;
        }

        if ui
            .add_enabled(
                ui_state.right_pane_state != EditorTab::Editing,
                egui::Button::new("Editing"),
            )
            .clicked()
        {
            ui_state.right_pane_state = EditorTab::Editing;
        }
        if ui
            .add_enabled(
                ui_state.right_pane_state != EditorTab::Game,
                egui::Button::new("Game"),
            )
            .clicked()
        {
            ui_state.right_pane_state = EditorTab::Game;
        }
    });

    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| match ui_state.right_pane_state {
            EditorTab::EntityProperties => {
                entity_properties_pane(
                    ui,
                    ecs_world,
                    editor,
                    voxel_world,
                    ui_state,
                    session,
                    assets,
                );
            }
            EditorTab::WorldProperties => {
                world_pane(
                    ui,
                    ecs_world,
                    editor,
                    voxel_world,
                    assets,
                    ui_state,
                    session,
                );
            }
            EditorTab::Editing => {
                editing_pane(
                    ui,
                    ecs_world,
                    editor,
                    voxel_world,
                    ui_state,
                    session,
                    assets,
                );
            }
            EditorTab::Game => {
                game_pane(
                    ui,
                    ecs_world,
                    editor,
                    voxel_world,
                    ui_state,
                    session,
                    assets,
                );
            }
        });
}

pub fn game_pane(
    ui: &mut egui::Ui,
    ecs_world: &mut ECSWorld,
    editor: &mut Editor,
    voxel_world: &mut VoxelWorld,
    ui_state: &mut EditorUIState,
    session: &mut Session,
    assets: &mut Assets,
) {
    let content = |ui: &mut egui::Ui| {
        ui.label(egui::RichText::new("Game Settings").size(20.0));
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, content);
}
