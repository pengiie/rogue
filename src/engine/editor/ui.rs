use std::{fs::Metadata, path::PathBuf, str::FromStr, sync::Arc};

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
            repr::{editor_settings::EditorProjectAsset, image::ImageAsset},
        },
        entity::{ecs_world::ECSWorld, RenderableVoxelEntity},
        physics::transform::Transform,
        ui::{
            gui::Egui, EditorAssetBrowserState, EditorNewProjectDialog, EditorNewVoxelModelDialog,
            EditorUIState, UI,
        },
        voxel::{
            factory::VoxelModelFactory,
            flat::VoxelModelFlat,
            voxel::{VoxelModel, VoxelModelType},
            voxel_registry::VoxelModelId,
            voxel_world::{self, VoxelWorld},
        },
    },
    game::entity::GameEntity,
    session::Session,
};

use super::editor::Editor;

#[derive(Clone)]
pub enum EditorTab {
    WorldInspector,
    EntityInspector,
    Terrain,
    Assets,
}

impl EditorTab {
    pub fn name(&self) -> &str {
        match self {
            EditorTab::WorldInspector => "Inspector",
            EditorTab::EntityInspector => "Entity",
            EditorTab::Terrain => "Terrain",
            EditorTab::Assets => "Assets",
        }
    }
}

struct EditorTabViewer<'a> {
    ecs_world: &'a mut ECSWorld,
    editor: &'a mut Editor,
}

impl egui_dock::TabViewer for EditorTabViewer<'_> {
    type Tab = EditorTab;

    fn title(&mut self, tab: &mut Self::Tab) -> egui_dock::egui::WidgetText {
        egui_dock::egui::WidgetText::from(tab.name())
    }

    fn ui(&mut self, ui: &mut egui_dock::egui::Ui, tab: &mut Self::Tab) {
        ui.add(egui::Label::new(egui::RichText::new(tab.name()).size(20.0)))
            .rect
            .size();
    }
}

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
    editor: &mut Editor,
    mut ui_state: &mut EditorUIState,
    session: &mut Session,
    assets: &mut Assets,
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

    let mut dock_viewer = EditorTabViewer { ecs_world, editor };
    content_padding.x = egui::TopBottomPanel::top("top_editor_pane")
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
            }
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
                    if ui.button("Cube").clicked() {
                        let model_id = voxel_world.registry.register_renderable_voxel_model(
                            "entity",
                            VoxelModelFactory::create_cuboid(
                                Vector3::new(32, 32, 32),
                                Color::new_srgb(1.0, 0.0, 0.0),
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
        .frame(
            egui::Frame::new()
                .fill(ctx.style().visuals.window_fill)
                .inner_margin(8.0),
        )
        .default_width(300.0)
        .show(ctx, |ui| {
            right_editor_pane(ui, ecs_world, &editor, voxel_world, &mut ui_state, &session);
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

fn right_editor_pane(
    ui: &mut egui::Ui,
    ecs_world: &mut ECSWorld,
    editor: &Editor,
    voxel_world: &mut VoxelWorld,
    ui_state: &mut EditorUIState,
    session: &Session,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Entity properties").size(20.0));
        if let Some(selected_entity) = &editor.selected_entity {
            ui.menu_button("Add component", |ui| {
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
                add_contents: impl FnOnce(&mut egui::Ui) -> R,
            ) {
                let last_spacing = ui.style().spacing.item_spacing.y;
                ui.style_mut().spacing.item_spacing.y = 0.0;
                ui.label(egui::RichText::new(header).size(16.0));
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
            component_widget(ui, "General", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name: ");
                    ui.text_edit_singleline(&mut game_entity.name);
                });
                ui.label(format!("UUID: {}", game_entity.uuid));
            });

            if let Ok(mut transform) = ecs_world.get::<&mut Transform>(*selected_entity) {
                component_widget(ui, "Transform", |ui| {
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

            if let Ok(mut renderable_voxel_model) =
                ecs_world.get::<&mut RenderableVoxelEntity>(*selected_entity)
            {
                component_widget(ui, "Renderable", |ui| {
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
                                .unwrap_or(String::new())
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
                                ui.close_menu();
                            }
                        });
                    });
                });
            }
        } else {
            ui.label("No entity selected.");
        }
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, content);
}
