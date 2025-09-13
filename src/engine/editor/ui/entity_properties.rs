use std::{path::PathBuf, str::FromStr};

use nalgebra::{UnitQuaternion, Vector3};

use crate::{
    engine::{
        asset::{
            asset::{AssetPath, Assets},
            repr::voxel::any::VoxelModelAnyAsset,
        },
        editor::editor::Editor,
        entity::{
            ecs_world::{ECSWorld, Entity},
            scripting::{ScriptableEntity, Scripts},
            EntityChildren, EntityParent, GameEntity, RenderableVoxelEntity,
        },
        graphics::camera::Camera,
        physics::{
            box_collider::BoxCollider,
            capsule_collider::CapsuleCollider,
            collider::{ColliderType, Colliders},
            collider_registry::ColliderId,
            physics_world::{self, PhysicsWorld},
            plane_collider::PlaneCollider,
            rigid_body::{RigidBody, RigidBodyType},
            transform::Transform,
        },
        ui::{EditorNewVoxelModelDialog, EditorUIState},
        voxel::{
            flat::VoxelModelFlat,
            thc::{VoxelModelTHC, VoxelModelTHCCompressed},
            voxel::{VoxelModel, VoxelModelImplConcrete, VoxelModelType},
            voxel_registry::{VoxelModelId, VoxelModelInfo},
            voxel_world::VoxelWorld,
        },
    },
    session::Session,
};
use crate::common::geometry::aabb::AABB;

fn position_ui(ui: &mut egui::Ui, position: &mut Vector3<f32>) {
    ui.horizontal(|ui| {
        ui.label("Position:");
        ui.label("X");
        ui.add(
            egui::DragValue::new(&mut position.x)
                .suffix(" m")
                .speed(0.01)
                .fixed_decimals(2),
        );
        ui.label("Y");
        ui.add(
            egui::DragValue::new(&mut position.y)
                .suffix(" m")
                .speed(0.01)
                .fixed_decimals(2),
        );
        ui.label("Z");
        ui.add(
            egui::DragValue::new(&mut position.z)
                .suffix(" m")
                .speed(0.01)
                .fixed_decimals(2),
        );
    });
}

fn rotation_ui(ui: &mut egui::Ui, rotation: &mut UnitQuaternion<f32>) {
    ui.horizontal(|ui| {
        ui.label("Rotation:");
        ui.label("X");
        let (mut roll, mut pitch, mut yaw) = rotation.euler_angles();
        let original = Vector3::new(roll, pitch, yaw).map(|x| x.to_degrees());
        let mut edit = original.clone();
        // nalgebra uses positive rotation for clockwise but intuitively
        // counter-clockwise makes more sense since math.
        edit.x *= -1.0;
        edit.z *= -1.0;
        ui.add(
            egui::DragValue::new(&mut edit.x)
                .suffix("°")
                .speed(0.05)
                .fixed_decimals(2),
        );
        ui.label("Y");
        ui.add(
            egui::DragValue::new(&mut edit.y)
                .suffix("°")
                .speed(0.05)
                .fixed_decimals(2),
        );
        ui.label("Z");
        ui.add(
            egui::DragValue::new(&mut edit.z)
                .suffix("°")
                .speed(0.05)
                .fixed_decimals(2),
        );
        edit.x *= -1.0;
        edit.z *= -1.0;
        let diff = edit - original;
        if diff.x != 0.0 {
            *rotation = UnitQuaternion::from_euler_angles(edit.x.to_radians(), pitch, yaw);
        }
        if diff.y != 0.0 {
            *rotation = UnitQuaternion::from_euler_angles(roll, edit.y.to_radians(), yaw);
        }
        if diff.z != 0.0 {
            *rotation = UnitQuaternion::from_euler_angles(roll, pitch, edit.z.to_radians());
        }
    });
}

fn scale_ui(ui: &mut egui::Ui, scale: &mut Vector3<f32>) {
    ui.horizontal(|ui| {
        ui.label("Scale:");
        ui.label("X");
        ui.add(
            egui::DragValue::new(&mut scale.x)
                .range(0.001..=1000.0)
                .speed(0.01)
                .fixed_decimals(2),
        );
        ui.label("Y");
        ui.add(
            egui::DragValue::new(&mut scale.y)
                .range(0.001..=1000.0)
                .speed(0.01)
                .fixed_decimals(2),
        );
        ui.label("Z");
        ui.add(
            egui::DragValue::new(&mut scale.z)
                .range(0.001..=1000.0)
                .speed(0.01)
                .fixed_decimals(2),
        );
    });
}

pub fn entity_properties_pane(
    ui: &mut egui::Ui,
    ecs_world: &mut ECSWorld,
    editor: &Editor,
    voxel_world: &mut VoxelWorld,
    physics_world: &mut PhysicsWorld,
    ui_state: &mut EditorUIState,
    session: &mut Session,
    assets: &mut Assets,
    scripts: &mut Scripts,
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
    'add_script_rx: {
        match ui_state.add_script_dialog.rx_file_name.try_recv() {
            Ok(model_path) => {
                let Ok(path) = PathBuf::from_str(&model_path) else {
                    break 'add_script_rx;
                };
                if !path.is_absolute() {
                    break 'add_script_rx;
                }
                if !path.starts_with(session.project_assets_dir().unwrap()) {
                    log::error!(
                        "Picked script that {:?} does not start with the assets dir.",
                        path
                    );
                    break 'add_script_rx;
                }

                let Ok(metadata) = std::fs::metadata(&path) else {
                    log::error!("Failed to get script file metadata.");
                    break 'add_script_rx;
                };
                if !metadata.is_file() {
                    log::error!("Script path must be a file.");
                    break 'add_script_rx;
                }

                let model_path = PathBuf::from_str(&model_path).unwrap();
                let asset_path = AssetPath::from_project_dir_path(
                    session.project_save_dir.as_ref().unwrap(),
                    &model_path,
                );
                if let Ok(mut scriptable) = ecs_world
                    .get::<&mut ScriptableEntity>(ui_state.add_script_dialog.associated_entity)
                {
                    if scriptable
                        .scripts
                        .iter()
                        .find(|path| &&asset_path == path)
                        .is_none()
                    {
                        scriptable.scripts.push(asset_path.clone());
                        scripts.load_script(asset_path);
                    }
                }
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
                if ui.button("Script").clicked() {
                    ecs_world.insert_one(*selected_entity, ScriptableEntity::new());
                    ui.close_menu();
                }
                if ui.button("Rigidbody").clicked() {
                    ecs_world.insert_one(*selected_entity, RigidBody::default());
                    ui.close_menu();
                }
                if ui.button("Colliders").clicked() {
                    ecs_world.insert_one(*selected_entity, Colliders::new());
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
            drop(game_entity);

            ui.style_mut().spacing.item_spacing.y = 8.0;

            game_entity_info(ui, ui_state, ecs_world, selected_entity);
            transform_component(ui, ui_state, ecs_world, selected_entity);
            renderable_component(
                ui,
                ui_state,
                ecs_world,
                voxel_world,
                session,
                assets,
                selected_entity,
            );
            camera_component(ui, ui_state, ecs_world, session, selected_entity);

            let mut remove_scripts = false;
            if let Ok(mut scriptable) = ecs_world.get::<&mut ScriptableEntity>(*selected_entity) {
                component_widget(ui, "Scripts", Some(&mut remove_scripts), |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Add script").clicked() {
                            let send = ui_state.add_script_dialog.tx_file_name.clone();
                            ui_state.add_script_dialog.associated_entity = *selected_entity;
                            std::thread::spawn(|| {
                                pollster::block_on(async move {
                                    let file = rfd::AsyncFileDialog::new()
                                        .add_filter("lua", &["lua"])
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
                    if scriptable.scripts.is_empty() {
                    } else {
                        for asset_path in &scriptable.scripts {
                            if ui.label(asset_path.asset_path.as_ref().unwrap()).clicked() {}
                        }
                    }
                });
            }
            if remove_scripts {
                ecs_world.remove_one::<ScriptableEntity>(*selected_entity);
            } // End scripts

            let mut remove_rigid_body = false;
            if let Ok(mut rigid_body) = ecs_world.get::<&mut RigidBody>(*selected_entity) {
                component_widget(ui, "Rigid body", Some(&mut remove_rigid_body), |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Type");
                        egui::ComboBox::from_id_salt("Rigid body type")
                            .selected_text(format!("{:?}", rigid_body.rigid_body_type))
                            .show_ui(ui, |ui| {
                                for val in [RigidBodyType::Static, RigidBodyType::Dynamic] {
                                    ui.selectable_value(
                                        &mut rigid_body.rigid_body_type,
                                        val,
                                        format!("{:?}", val),
                                    );
                                }
                            });
                    });
                    ui.horizontal(|ui| {
                        ui.label("Mass");
                        let mut mass = rigid_body.mass();
                        ui.add(egui::DragValue::new(&mut mass).range(0.1..=100.0));
                        rigid_body.set_mass(mass);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Restitution");
                        ui.add(egui::DragValue::new(&mut rigid_body.restitution).range(0.0..=1.0));
                    });
                });
            }
            if remove_scripts {
                ecs_world.remove_one::<RigidBody>(*selected_entity);
            } // End rigid body

            colliders_component(ui, ui_state, ecs_world, physics_world, selected_entity);
        } else {
            ui.label("No entity selected.");
        }
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, content);
}

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
}

fn game_entity_info(
    ui: &mut egui::Ui,
    ui_state: &mut EditorUIState,
    ecs_world: &mut ECSWorld,
    selected_entity: &Entity,
) {
    component_widget(ui, "General", None, |ui| {
        let mut game_entity = ecs_world.get::<&mut GameEntity>(*selected_entity).unwrap();
        ui.horizontal(|ui| {
            ui.label("Name: ");
            ui.text_edit_singleline(&mut game_entity.name);
        });
        ui.label(format!("UUID: {}", game_entity.uuid));
        drop(game_entity);

        ui.horizontal(|ui| {
            ui.label("Parent: ");

            let parent = ecs_world.get::<&EntityParent>(*selected_entity).ok();
            let parent_entity = parent.as_ref().map(|parent| parent.parent);
            let parent_name = parent.as_ref().map_or_else(
                || "None".to_owned(),
                |parent| {
                    let parent_game_entity = ecs_world
                        .get::<&GameEntity>(parent.parent)
                        .expect("Parent should be a GameEntity");
                    parent_game_entity.name.clone()
                },
            );
            drop(parent);
            ui.menu_button(parent_name, |ui| {
                // TODO: Transform entities transform so it stays the same in world space.
                ui.label("Set parent:");
                if ui.button("Select parent entity").clicked() {
                    ui_state.selecting_new_parent = Some(*selected_entity);
                    ui.close_menu();
                }
                if ui
                    .add_enabled(parent_entity.is_some(), egui::Button::new("Remove"))
                    .clicked()
                {
                    let mut parent_children = ecs_world
                        .get::<&mut EntityChildren>(parent_entity.unwrap())
                        .expect("Parent should have a children component");
                    parent_children.children.remove(selected_entity);
                    drop(parent_children);
                    ecs_world.remove_one::<EntityParent>(*selected_entity);
                    ui.close_menu();
                }
            });
        });
    });
}

fn transform_component(
    ui: &mut egui::Ui,
    ui_state: &mut EditorUIState,
    ecs_world: &mut ECSWorld,
    selected_entity: &Entity,
) {
    if let Ok(mut transform) = ecs_world.get::<&mut Transform>(*selected_entity) {
        component_widget(ui, "Transform", None, |ui| {
            position_ui(ui, &mut transform.position);
            rotation_ui(ui, &mut transform.rotation);
            scale_ui(ui, &mut transform.scale);
        });
    } // End Transform
}

fn convert_model<T: VoxelModelImplConcrete, C: VoxelModelImplConcrete + for<'a> From<&'a T>>(
    voxel_world: &mut VoxelWorld,
    renderable_voxel_model: &mut RenderableVoxelEntity,
    info: &VoxelModelInfo,
    original_id: VoxelModelId,
) {
    let converted_model = C::from(voxel_world.registry.get_model::<T>(original_id));
    let converted_model_id = voxel_world
        .registry
        .register_renderable_voxel_model(&info.name, VoxelModel::new(converted_model));
    voxel_world
        .registry
        .set_voxel_model_asset_path(converted_model_id, info.asset_path.clone());
    renderable_voxel_model.set_id(converted_model_id);
    voxel_world.to_update_normals.insert(converted_model_id);
}

fn convert_model_ui(
    ui: &mut egui::Ui,
    voxel_world: &mut VoxelWorld,
    mut renderable_voxel_model: &mut RenderableVoxelEntity,
    info: &VoxelModelInfo,
    model_name: &str,
    model_id: VoxelModelId,
    model_type: VoxelModelType,
) {
    ui.menu_button(model_name, |ui| {
        ui.label("Convert to");
        if ui
            .add_enabled(
                model_type != VoxelModelType::Flat,
                egui::Button::new("Flat"),
            )
            .clicked()
        {
            match model_type {
                VoxelModelType::THC => convert_model::<VoxelModelTHCCompressed, VoxelModelFlat>(
                    voxel_world,
                    &mut renderable_voxel_model,
                    &info,
                    model_id,
                ),
                _ => unreachable!(),
            }
            ui.close_menu();
        }
        if ui
            .add_enabled(model_type != VoxelModelType::THC, egui::Button::new("THC"))
            .clicked()
        {
            match model_type {
                VoxelModelType::Flat => convert_model::<VoxelModelFlat, VoxelModelTHC>(
                    voxel_world,
                    &mut renderable_voxel_model,
                    &info,
                    model_id,
                ),
                VoxelModelType::THCCompressed => {
                    convert_model::<VoxelModelTHCCompressed, VoxelModelTHC>(
                        voxel_world,
                        &mut renderable_voxel_model,
                        &info,
                        model_id,
                    )
                }
                ty => {
                    log::error!(
                        "Can't convert from {} to THC since it's not implemented yet.",
                        ty.to_string()
                    );
                }
            }
            ui.close_menu();
        }
    });
}

fn renderable_component(
    ui: &mut egui::Ui,
    ui_state: &mut EditorUIState,
    ecs_world: &mut ECSWorld,
    voxel_world: &mut VoxelWorld,
    session: &mut Session,
    assets: &mut Assets,
    selected_entity: &Entity,
) {
    let mut remove_renderable = false;
    if let Ok(mut renderable_voxel_model) =
        ecs_world.get::<&mut RenderableVoxelEntity>(*selected_entity)
    {
        component_widget(ui, "Renderable", Some(&mut remove_renderable), |ui| {
            ui.horizontal(|ui| {
                ui.label("Voxel model:");

                // User selected model name with submenu.
                let text = if let Some(model_id) = renderable_voxel_model.voxel_model_id() {
                    let info = voxel_world.registry.get_model_info(model_id).unwrap();
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
                    // Open new model dialog within the editor.
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

                    // Choose existing model saved on the file system within the project asset
                    // directory.
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

                    // Save the model to the project asset directory.
                    let model_info = renderable_voxel_model.voxel_model_id().map_or(None, |id| {
                        Some(voxel_world.registry.get_model_info(id).unwrap())
                    });
                    // TOOO: Save in memory things.
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
                                let flat =
                                    voxel_world.get_model::<VoxelModelFlat>(model_id).clone();
                                assets.save_asset(asset_path, flat);
                            }
                            Some(VoxelModelType::THC) => {
                                let thc = voxel_world.get_model::<VoxelModelTHC>(model_id);
                                assets.save_asset(asset_path, VoxelModelTHCCompressed::from(thc));
                            }
                            Some(VoxelModelType::THCCompressed) => {
                                let thc_compressed = voxel_world
                                    .get_model::<VoxelModelTHCCompressed>(model_id)
                                    .clone();
                                assets.save_asset(asset_path, thc_compressed);
                            }
                            None => {
                                log::error!("Don't know how to save this asset format");
                            }
                            ty => todo!("Save model type {:?}", ty),
                        }
                        ui.close_menu();
                    }
                });
            });

            // Model type UI with conversion.
            if let Some(model_id) = renderable_voxel_model.voxel_model_id() {
                let info = voxel_world
                    .registry
                    .get_model_info(model_id)
                    .unwrap()
                    .clone();
                let text = match &info.model_type {
                    Some(ty) => ty.as_ref(),
                    None => "Unknown",
                };
                ui.horizontal(|ui| {
                    ui.label("Model type:");
                    if let Some(model_type) = info.model_type {
                        convert_model_ui(
                            ui,
                            voxel_world,
                            &mut renderable_voxel_model,
                            &info,
                            text,
                            model_id,
                            model_type,
                        );
                    } else {
                        ui.label(text);
                    }
                });
            }
        });
    }
    if remove_renderable {
        ecs_world.remove_one::<RenderableVoxelEntity>(*selected_entity);
    } // End RenderableVoxelEntity
}

fn camera_component(
    ui: &mut egui::Ui,
    ui_state: &mut EditorUIState,
    ecs_world: &mut ECSWorld,
    session: &mut Session,
    selected_entity: &Entity,
) {
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
    } // End Camera
}

fn collider_type_to_str(collider_type: ColliderType) -> &'static str {
    return match collider_type {
        ColliderType::Null => "Null (oops)",
        ColliderType::Capsule => "Capsule",
        ColliderType::Plane => "Plane",
        ColliderType::Box => "Box",
    };
}

fn capsule_collider_ui(
    collider_id: &ColliderId,
    physics_world: &mut PhysicsWorld,
    ui: &mut egui::Ui,
) {
    let mut collider = physics_world
        .colliders
        .get_collider_mut::<CapsuleCollider>(collider_id);
    position_ui(ui, &mut collider.center);
    rotation_ui(ui, &mut collider.orientation);
    ui.horizontal(|ui| {
        ui.label("Radius:");
        ui.add(
            egui::DragValue::new(&mut collider.radius)
                .suffix(" m")
                .speed(0.005)
                .fixed_decimals(2),
        );
    });
    ui.horizontal(|ui| {
        ui.label("Height:");
        ui.add(
            egui::DragValue::new(&mut collider.half_height)
                .suffix(" m")
                .speed(0.01)
                .fixed_decimals(2),
        );
    });
}

fn box_collider_ui(collider_id: &ColliderId, physics_world: &mut PhysicsWorld, ui: &mut egui::Ui) {
    let mut collider = physics_world
        .colliders
        .get_collider_mut::<BoxCollider>(collider_id);

    let mut center = collider.obb.aabb.center();
    let original_center = center.clone();
    position_ui(ui, &mut center);

    let mut half_side_length = collider.obb.aabb.half_side_length();
    let original_half_side_length = half_side_length.clone();

    rotation_ui(ui, &mut collider.obb.rotation);

    scale_ui(ui, &mut half_side_length);
    if center != original_center || half_side_length != original_half_side_length {
        collider.obb.aabb = AABB::new_center_extents(center, half_side_length);
    }
}

fn plane_collider_ui(
    collider_id: &ColliderId,
    physics_world: &mut PhysicsWorld,
    ui: &mut egui::Ui,
) {
    let mut collider = physics_world
        .colliders
        .get_collider_mut::<PlaneCollider>(collider_id);
    position_ui(ui, &mut collider.center);
    ui.horizontal(|ui| {
        ui.label("Rotation:");
        ui.label("X");
        let mut orientation =
            UnitQuaternion::rotation_between(&Vector3::z(), &collider.normal).unwrap_or(
                UnitQuaternion::from_axis_angle(&Vector3::y_axis(), std::f32::consts::PI),
            );
        let (mut roll, mut pitch, mut yaw) = orientation.euler_angles();
        let original = Vector3::new(roll, pitch, yaw).map(|x| x.to_degrees());
        let mut edit = original.clone();
        // nalgebra uses positive rotation for clockwise but intuitively
        // counter-clockwise makes more sense since math.
        edit.x *= -1.0;
        edit.z *= -1.0;
        ui.add(
            egui::DragValue::new(&mut edit.x)
                .suffix("°")
                .speed(0.05)
                .fixed_decimals(2),
        );
        ui.label("Y");
        ui.add(
            egui::DragValue::new(&mut edit.y)
                .suffix("°")
                .speed(0.05)
                .fixed_decimals(2),
        );
        ui.label("Z");
        ui.add(
            egui::DragValue::new(&mut edit.z)
                .suffix("°")
                .speed(0.05)
                .fixed_decimals(2),
        );
        edit.x *= -1.0;
        edit.z *= -1.0;
        let diff = edit - original;
        if diff.x != 0.0 {
            orientation = UnitQuaternion::from_euler_angles(edit.x.to_radians(), pitch, yaw);
        }
        if diff.y != 0.0 {
            orientation = UnitQuaternion::from_euler_angles(roll, edit.y.to_radians(), yaw);
        }
        if diff.z != 0.0 {
            orientation = UnitQuaternion::from_euler_angles(roll, pitch, edit.z.to_radians());
        }

        collider.normal = orientation * Vector3::z();
    });
    ui.horizontal(|ui| {
        ui.label("Size:");
        ui.label("X");
        ui.add(
            egui::DragValue::new(&mut collider.size.x)
                .suffix(" m")
                .speed(0.01)
                .fixed_decimals(2),
        );
        ui.label("Y");
        ui.add(
            egui::DragValue::new(&mut collider.size.y)
                .suffix(" m")
                .speed(0.01)
                .fixed_decimals(2),
        );
    });
}

fn colliders_component(
    ui: &mut egui::Ui,
    ui_state: &mut EditorUIState,
    ecs_world: &mut ECSWorld,
    physics_world: &mut PhysicsWorld,
    selected_entity: &Entity,
) {
    let mut remove_colliders = false;
    if let Ok(mut colliders) = ecs_world.get::<&mut Colliders>(*selected_entity) {
        component_widget(ui, "Colliders", Some(&mut remove_colliders), |ui| {
            ui.menu_button("Add collider", |ui| {
                if ui.button("Capsule collider").clicked() {
                    let capsule_collider = physics_world
                        .colliders
                        .register_collider(CapsuleCollider::new());
                    colliders.colliders.push(capsule_collider);
                    ui.close_menu();
                }
                if ui.button("Plane collider").clicked() {
                    let plane_collider = physics_world
                        .colliders
                        .register_collider(PlaneCollider::default());
                    colliders.colliders.push(plane_collider);
                    ui.close_menu();
                }
                if ui.button("Box collider").clicked() {
                    let box_collider = physics_world
                        .colliders
                        .register_collider(BoxCollider::default());
                    colliders.colliders.push(box_collider);
                    ui.close_menu();
                }
            });

            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for collider_id in colliders.colliders.iter() {
                        let mut text = egui::RichText::new(format!(
                            "{} collider #{}",
                            collider_type_to_str(collider_id.collider_type),
                            collider_id.index
                        ));
                        if let Some(selected_collider_id) = &ui_state.selected_collider {
                            if collider_id == selected_collider_id {
                                text = text.background_color(egui::Color32::from_white_alpha(2));
                            }
                        }
                        if ui.label(text).clicked() {
                            ui_state.selected_collider = Some(*collider_id);
                        }
                    }
                });
            ui.separator();
            ui.label("Currently selected collider:");
            match &ui_state.selected_collider {
                Some(collider_id) => {
                    let mut text = egui::RichText::new(format!(
                        "{} collider #{}",
                        collider_type_to_str(collider_id.collider_type),
                        collider_id.index
                    ));
                    match collider_id.collider_type {
                        ColliderType::Null => {}
                        ColliderType::Capsule => {
                            capsule_collider_ui(collider_id, physics_world, ui);
                        }
                        ColliderType::Plane => {
                            plane_collider_ui(collider_id, physics_world, ui);
                        }
                        ColliderType::Box => box_collider_ui(collider_id, physics_world, ui),
                    }
                }
                None => {
                    ui.label("None selected");
                }
                _ => {}
            }
        });
    }
    if remove_colliders {
        ecs_world.remove_one::<Colliders>(*selected_entity);
    } // End colliders
}
