use nalgebra::Vector3;
use rogue_engine::{
    asset::asset::GameAssetPath,
    common::dyn_vec::TypeInfo,
    entity::{
        EntityChildren, EntityParent, GameEntity, RenderableVoxelEntity,
        component::{GameComponent, RawComponentRef},
        ecs_world::{ECSWorld, EntityCommandEvent},
    },
    graphics::camera::Camera,
    material::MaterialId,
    physics::{
        box_collider::BoxCollider,
        capsule_collider::CapsuleCollider,
        collider_component::EntityColliders,
        collider_registry::ColliderId,
        physics_world::PhysicsWorld,
        plane_collider::PlaneCollider,
        rigid_body::{RigidBody, RigidBodyType},
        transform::Transform,
    },
    voxel::{
        sft_compressed::VoxelModelSFTCompressed,
        voxel::{VoxelEditData, VoxelModelEdit, VoxelModelImplMethods},
        voxel_registry::VoxelModelRegistry,
    },
};
use std::{
    any::TypeId,
    collections::{HashMap, HashSet},
    fmt::Display,
};

use crate::{
    session::EditorSession,
    ui::{
        EditorCommand, EditorCommands, EditorDialog, EditorUIContext, FilePickerType,
        pane::{EditorUIPane, EditorUIPaneMethods},
    },
};

pub struct ShowComponentContext<'a> {
    physics_world: &'a mut PhysicsWorld,
    voxel_registry: &'a mut VoxelModelRegistry,
    component_state: &'a mut HashMap<TypeId, Box<dyn std::any::Any>>,
    session: &'a mut EditorSession,
    commands: &'a mut EditorCommands,
}

type ShowComponentFn<T> = fn(&mut T, &mut egui::Ui, &mut ShowComponentContext);
pub struct ErasedShowFn {
    show_fn_impl: fn(*const (), *mut (), &mut egui::Ui, &mut ShowComponentContext),
    show_fn_ptr: *const (),
}

impl ErasedShowFn {
    fn new<T: GameComponent + 'static>(show_fn: ShowComponentFn<T>) -> Self {
        fn show_fn_impl<T: GameComponent>(
            show_fn_ptr: *const (),
            component: *mut (),
            ui: &mut egui::Ui,
            ctx: &mut ShowComponentContext,
        ) {
            let component = unsafe { &mut *(component as *mut T) };
            let show_fn =
                unsafe { std::mem::transmute::<*const (), ShowComponentFn<T>>(show_fn_ptr) };
            show_fn(component, ui, ctx);
        }

        Self {
            show_fn_impl: show_fn_impl::<T>,
            show_fn_ptr: unsafe { std::mem::transmute::<ShowComponentFn<T>, *const ()>(show_fn) },
        }
    }

    /// Type of `component_ptr` should be the same type that this fn expects.
    unsafe fn call(
        &self,
        component_ptr: *mut (),
        ui: &mut egui::Ui,
        ctx: &mut ShowComponentContext,
    ) {
        (self.show_fn_impl)(self.show_fn_ptr, component_ptr, ui, ctx);
    }
}

pub struct EntityPropertiesShowFns {
    fns: HashMap<TypeId, ErasedShowFn>,
}

impl EntityPropertiesShowFns {
    pub fn new() -> Self {
        let mut s = Self {
            fns: HashMap::new(),
        };

        s.register_component_ui::<EntityColliders>(Self::show_colliders_component);
        s.register_component_ui::<Camera>(Self::show_camera_component);
        s.register_component_ui::<RenderableVoxelEntity>(Self::show_renderable_voxel);
        s.register_component_ui::<RigidBody>(Self::show_rigid_body_component);

        s
    }

    pub fn get(&self, type_id: &TypeId) -> Option<&ErasedShowFn> {
        self.fns.get(type_id)
    }

    fn register_component_ui<T: GameComponent + 'static>(&mut self, show_fn: ShowComponentFn<T>) {
        self.fns
            .insert(std::any::TypeId::of::<T>(), ErasedShowFn::new(show_fn));
    }

    fn show_renderable_voxel(
        renderable: &mut RenderableVoxelEntity,
        ui: &mut egui::Ui,
        ctx: &mut ShowComponentContext,
    ) {
        let selected_entity = ctx.session.selected_entity.clone().unwrap();

        ui.horizontal(|ui| {
            ui.label("Voxel model:");

            // User selected model name with submenu.
            let text = if let Some(asset_path) = renderable.model_asset_path() {
                let status = if renderable.voxel_model_id().is_none() {
                    " (Unloaded)".to_owned()
                } else {
                    String::new()
                };
                format!(
                    "{}{}",
                    asset_path
                        .as_relative_path()
                        .to_string_lossy()
                        .strip_prefix(".")
                        .unwrap(),
                    status
                )
            } else if renderable.voxel_model_id().is_some() {
                "In memory (unsaved)".to_owned()
            } else {
                "None".to_owned()
            };
            ui.menu_button(text, |ui| {
                // Open new model dialog within the editor.
                if ui.button("Create new").clicked() {
                    const DIALOG_ID: &str = "create_voxel_model_dialog";
                    ctx.commands.push(EditorCommand::OpenDialog(EditorDialog {
                        id: DIALOG_ID.to_owned(),
                        title: "Create Voxel Model".to_owned(),
                        show_fn: Box::new(move |ui, ctx| {
                            ui.vertical(|ui| {
                                #[derive(
                                    Copy, Clone, Debug, PartialEq, Eq, strum_macros::VariantArray,
                                )]
                                enum CreateModelPreset {
                                    Empty,
                                    Solid,
                                }
                                #[derive(Clone)]
                                struct NewModelDialogState {
                                    side_length: u32,
                                    preset: CreateModelPreset,
                                    material: MaterialId,
                                }
                                let id = egui::Id::new(format!("new_voxel_model_dialog"));
                                let mut state = ui.data_mut(|w| {
                                    w.get_temp_mut_or_insert_with(id, || {
                                        let default_material = ctx
                                            .material_bank
                                            .contains_material(&MaterialId::new(0, 0))
                                            .then_some(MaterialId::new(0, 0))
                                            .unwrap_or(MaterialId::null());
                                        NewModelDialogState {
                                            side_length: 16,
                                            preset: CreateModelPreset::Solid,
                                            material: default_material,
                                        }
                                    })
                                    .clone()
                                });

                                ui.horizontal(|ui| {
                                    const MAX_DIMENSION: u32 = 4u32.pow(10);
                                    ui.label("Side Length:");
                                    ui.label("X");
                                    ui.add(
                                        egui::DragValue::new(&mut state.side_length)
                                            .range(4..=MAX_DIMENSION),
                                    );
                                });

                                ui.horizontal(|ui| {
                                    ui.label("Preset:");
                                    egui::ComboBox::from_id_salt("Create model preset")
                                        .selected_text(format!("{:?}", state.preset))
                                        .show_ui(ui, |ui| {
                                            use strum::VariantArray as _;
                                            for val in CreateModelPreset::VARIANTS {
                                                ui.selectable_value(
                                                    &mut state.preset,
                                                    val.clone(),
                                                    format!("{:?}", val),
                                                );
                                            }
                                        });
                                });

                                ui.add_enabled_ui(state.preset != CreateModelPreset::Empty, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label("Fill Material:");
                                        use crate::ui::material_picker::material_picker;
                                        material_picker(ui, ctx.material_bank, &mut state.material);
                                    });
                                });

                                if ui.button("Create").clicked() {
                                    ctx.commands.push(EditorCommand::FilePicker {
                                        picker_type: FilePickerType::CreateFile,
                                        callback: Box::new(move |ctx, file_path| {
                                            let asset_path =
                                                GameAssetPath::from_relative_path(&file_path);

                                            let mut model = VoxelModelSFTCompressed::new_empty(
                                                state.side_length,
                                            );
                                            match state.preset {
                                                CreateModelPreset::Empty => {
                                                    // Do nothing, already empty.
                                                }
                                                CreateModelPreset::Solid => {
                                                    let edit = VoxelModelEdit {
                                                        min: Vector3::new(0, 0, 0),
                                                        max: Vector3::new(
                                                            state.side_length,
                                                            state.side_length,
                                                            state.side_length,
                                                        ),
                                                        data: VoxelEditData::Fill {
                                                            material: state.material,
                                                        },
                                                    };
                                                    model.set_voxel_range_impl(&edit);
                                                }
                                            }
                                            let model_id = ctx.voxel_registry.register_voxel_model(
                                                model,
                                                Some(asset_path.clone()),
                                            );

                                            if let Ok(mut renderable) =
                                                ctx.ecs_world.get::<&mut RenderableVoxelEntity>(
                                                    selected_entity,
                                                )
                                            {
                                                renderable.set_model(Some(asset_path), model_id);
                                            }
                                            ctx.commands.push(EditorCommand::CloseDialog(
                                                DIALOG_ID.to_owned(),
                                            ));
                                        }),
                                        extensions: vec!["rvox".to_owned()],
                                    });
                                }

                                ui.data_mut(|w| {
                                    w.insert_temp(id, state);
                                });
                            });

                            //ui_state.new_model_dialog =
                            //    Some(EditorNewVoxelModelDialog::new(*selected_entity));
                            ui.close_menu();
                            false
                        }),
                    }));
                }

                // Choose existing model saved on the file system within the project asset
                // directory.
                if ui.button("Choose existing").clicked() {
                    //let send = ui_state.open_model_dialog.tx_file_name.clone();
                    //ui_state.open_model_dialog.associated_entity = *selected_entity;
                    //let asset_dir = session
                    //    .project_assets_dir()
                    //    .expect("Project directory should exist if this is clicked.")
                    //    .clone();
                    //std::thread::spawn(|| {
                    //    pollster::block_on(async move {
                    //        let file = rfd::AsyncFileDialog::new()
                    //            .set_directory(asset_dir)
                    //            .add_filter("RVox", &["rvox"])
                    //            .pick_file()
                    //            .await;
                    //        let Some(file) = file else {
                    //            return;
                    //        };
                    //        send.send(file.path().to_string_lossy().to_string());
                    //    });
                    //});
                    ui.close_menu();
                }

                //// Save the model to its currently saved to file.
                //let model_info = renderable.voxel_model_id().map_or(None, |id| {
                //    Some(voxel_world.registry.get_model_info(id).unwrap().clone())
                //});
                //let model_info_exists = model_info.is_some();

                //let has_asset_path = model_info
                //    .as_ref()
                //    .filter(|info| info.asset_path.is_some())
                //    .is_some();
                //if ui
                //    .add_enabled(has_asset_path, egui::Button::new("Save"))
                //    .clicked()
                //{
                //    let model_id = renderable_voxel_model.voxel_model_id().unwrap();
                //    let asset_path = model_info.unwrap().asset_path.clone().unwrap();
                //    voxel_world.save_model(assets, model_id, asset_path);
                //    ui.close_menu();
                //}

                //// Save the model to a specific file.
                //if ui
                //    .add_enabled(model_info_exists, egui::Button::new("Save as"))
                //    .clicked()
                //{
                //    let send = ui_state.save_model_dialog.tx_file_name.clone();
                //    ui_state.save_model_dialog.model_id =
                //        renderable_voxel_model.voxel_model_id().unwrap();
                //    std::thread::spawn(|| {
                //        pollster::block_on(async move {
                //            let file = rfd::AsyncFileDialog::new()
                //                .add_filter("RVox", &["rvox"])
                //                .save_file()
                //                .await;
                //            let Some(file) = file else {
                //                return;
                //            };
                //            send.send(file.path().to_string_lossy().to_string());
                //        });
                //    });
                //    ui.close_menu();
                //}

                //// Essentially calculates the bounds of the content, and allows
                //// the user to move the bounds of the model as long as the content
                //// still fits. During this process the user can also resize the model following
                //// the models resizing rules.
                //if ui
                //    .add_enabled(model_info_exists, egui::Button::new("Resize/Rebound"))
                //    .on_hover_text(
                //        "Move around and resize the model bounds. Model must be dynamic.",
                //    )
                //    .clicked()
                //{
                //    // TODO: Edit the model bounds.
                //}

                //let has_asset_path = renderable_voxel_model.model_asset_path().is_some();
                //if ui
                //    .add_enabled(has_asset_path, egui::Button::new("Reload"))
                //    .on_hover_text(
                //        "Reloads the model for this entity from the defined asset path.",
                //    )
                //    .clicked()
                //{
                //    events.push(EventVoxelRenderableEntityLoad {
                //        entity: *selected_entity,
                //        reload: true,
                //    });
                //    ui.close_menu();
                //}
            });
        });

        // Model type UI with conversion.
        if let Some(model_id) = renderable.voxel_model_id() {
            //let info = ctx.voxel_registry.get_model_info(model_id).unwrap().clone();
            //let text = match &info.model_type {
            //    Some(ty) => ty.as_ref(),
            //    None => "Unknown",
            //};
            //ui.horizontal(|ui| {
            //    ui.label("Model type:");
            //    if let Some(model_type) = info.model_type {
            //        convert_model_ui(
            //            ui,
            //            voxel_world,
            //            &mut renderable_voxel_model,
            //            &info,
            //            text,
            //            model_id,
            //            model_type,
            //        );
            //    } else {
            //        ui.label(text);
            //    }
            //});

            //let model_dyn = voxel_world.registry.get_dyn_model(model_id);
            //let length = model_dyn
            //    .length()
            //    .map(|x| x as f32 * consts::voxel::VOXEL_METER_LENGTH);
            //ui.label(format!(
            //    "Bounds: {:.2}mX{:.2}mX{:.2}m",
            //    length.x, length.y, length.z
            //));
        }
    }

    fn show_rigid_body_component(
        rigid_body: &mut RigidBody,
        ui: &mut egui::Ui,
        ctx: &mut ShowComponentContext,
    ) {
        ui.horizontal(|ui| {
            ui.label("Type");
            egui::ComboBox::from_id_salt("Rigid body type")
                .selected_text(format!("{:?}", rigid_body.rigid_body_type))
                .show_ui(ui, |ui| {
                    for val in [
                        RigidBodyType::Static,
                        RigidBodyType::Dynamic,
                        RigidBodyType::Kinematic,
                        RigidBodyType::KinematicPositionBased,
                    ] {
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
            ui.add(egui::DragValue::new(&mut rigid_body.restitution).range(0.0..=50.0));
        });
        ui.horizontal(|ui| {
            ui.label("Friction");
            ui.add(egui::DragValue::new(&mut rigid_body.friction).range(0.0..=50.0));
        });
        ui.horizontal(|ui| {
            ui.label("Locked rotation");
            ui.checkbox(&mut rigid_body.locked_rotational_axes.x, "X");
            ui.checkbox(&mut rigid_body.locked_rotational_axes.y, "Y");
            ui.checkbox(&mut rigid_body.locked_rotational_axes.z, "Z");
        });
        ui.label(format!(
            "Velocity  X: {}, Y: {}, Z: {}",
            rigid_body.velocity.x, rigid_body.velocity.y, rigid_body.velocity.z
        ));
    }

    fn show_camera_component(
        camera: &mut Camera,
        ui: &mut egui::Ui,
        ctx: &mut ShowComponentContext,
    ) {
        ui.horizontal(|ui| {
            ui.label("FOV");
            let mut deg = camera.fov.to_degrees();
            ui.add(egui::Slider::new(&mut deg, 1.0..=180.0));
            camera.fov = deg.to_radians();
        });
    }

    fn show_colliders_component(
        colliders: &mut EntityColliders,
        ui: &mut egui::Ui,
        ctx: &mut ShowComponentContext,
    ) {
        struct ColliderUIState {
            selected_collider: Option<ColliderId>,
        }

        let ui_state = ctx
            .component_state
            .entry(std::any::TypeId::of::<EntityColliders>())
            .or_insert_with(|| {
                Box::new(ColliderUIState {
                    selected_collider: None,
                })
            })
            .downcast_mut::<ColliderUIState>()
            .unwrap();

        ui.menu_button("Add collider", |ui| {
            if ui.button("Capsule collider").clicked() {
                let capsule_collider = ctx
                    .physics_world
                    .colliders
                    .register_collider(CapsuleCollider::new());
                colliders.colliders.push(capsule_collider);
                ui.close_menu();
            }
            if ui.button("Plane collider").clicked() {
                let plane_collider = ctx
                    .physics_world
                    .colliders
                    .register_collider(PlaneCollider::default());
                colliders.colliders.push(plane_collider);
                ui.close_menu();
            }
            if ui.button("Box collider").clicked() {
                let box_collider = ctx
                    .physics_world
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
                    let collider_name = ctx
                        .physics_world
                        .colliders
                        .collider_names
                        .get(&collider_id.collider_type)
                        .map_or("UnregisteredCollider (uh oh)", |s| s);
                    let selected_text = if ui_state.selected_collider == Some(*collider_id) {
                        " (Selected)"
                    } else {
                        ""
                    };
                    let mut text = egui::RichText::new(format!(
                        "{} collider #{}{}",
                        collider_name, collider_id.index, selected_text
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

        // If the selected entity was switched, ensure the selected collider is as well.
        if let Some(selected_collider) = &ui_state.selected_collider {
            if colliders
                .colliders
                .iter()
                .find(|id| *id == selected_collider)
                .is_none()
            {
                ui_state.selected_collider = None;
            }
        }

        match &ui_state.selected_collider {
            Some(collider_id) => {
                let collider_name = ctx
                    .physics_world
                    .colliders
                    .collider_names
                    .get(&collider_id.collider_type)
                    .map_or("UnregisteredCollider (uh oh)", |s| s);
                let mut text = egui::RichText::new(format!(
                    "{} collider #{}",
                    collider_name, collider_id.index
                ));
                ctx.physics_world
                    .colliders
                    .get_collider_dyn_mut(collider_id)
                    .collider_component_ui(ui);
            }
            None => {
                ui.label("None selected");
            }
            _ => {}
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct EntityPropertiesPane {
    #[serde(skip)]
    component_ui_state: HashMap<TypeId, Box<dyn std::any::Any>>,
}

impl EditorUIPane for EntityPropertiesPane {
    const ID: &'static str = "entity_properties";
    const NAME: &'static str = "Entity Properties";

    fn show(&mut self, ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        Self::title_bar(ui, ctx);
        ui.add_space(16.0);
        self.components(ui, ctx);
    }
}

impl EntityPropertiesPane {
    pub fn new() -> Self {
        Self {
            component_ui_state: HashMap::new(),
        }
    }

    fn components(&mut self, ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        let Some(selected_entity) = ctx.session.selected_entity else {
            ui.label("No entity selected");
            return;
        };

        Self::component_widget(ui, "General", None, |ui| {
            let mut game_entity = ctx
                .ecs_world
                .get::<&mut GameEntity>(selected_entity)
                .unwrap();
            ui.horizontal(|ui| {
                ui.label("Name: ");
                ui.text_edit_singleline(&mut game_entity.name);
            });
            ui.label(format!("UUID: {}", game_entity.uuid));
            drop(game_entity);

            ui.horizontal(|ui| {
                ui.label("Parent: ");

                let parent = ctx.ecs_world.get::<&EntityParent>(selected_entity).ok();
                let parent_entity = parent.as_ref().map(|parent| parent.parent());
                let parent_name = parent.as_ref().map_or_else(
                    || "None".to_owned(),
                    |parent| {
                        let parent_game_entity = ctx
                            .ecs_world
                            .get::<&GameEntity>(parent.parent())
                            .expect("Parent should be a GameEntity");
                        parent_game_entity.name.clone()
                    },
                );
                drop(parent);
                ui.menu_button(parent_name, |ui| {
                    // TODO: Transform entities transform so it stays the same in world space.
                    ui.label("Set parent:");
                    if ui.button("Select parent entity").clicked() {
                        //ui_state.selecting_new_parent = Some(*selected_entity);
                        ui.close_menu();
                    }
                    if ui
                        .add_enabled(parent_entity.is_some(), egui::Button::new("Remove"))
                        .clicked()
                    {
                        //ecs_world.set_parent(*selected_entity, None);
                        ui.close_menu();
                    }
                });
            });
        });

        // Like to keep the transform hoisted at the top.
        if let Ok(mut transform) = ctx.ecs_world.get::<&mut Transform>(selected_entity) {
            Self::component_widget(ui, "Transform", None, |ui| {
                rogue_engine::egui::util::position_ui(ui, &mut transform.position);
                rogue_engine::egui::util::rotation_ui(ui, &mut transform.rotation);
                rogue_engine::egui::util::scale_ui(ui, &mut transform.scale);
            });
        }

        let component_types = ctx.ecs_world.get_entity_components(selected_entity);
        let mut component_ctx = ShowComponentContext {
            physics_world: ctx.physics_world,
            component_state: &mut self.component_ui_state,
            voxel_registry: ctx.voxel_registry,
            commands: ctx.commands,
            session: ctx.session,
        };

        // Components we are rendering manually.
        let to_avoid_components = HashSet::from([
            std::any::TypeId::of::<Transform>(),
            std::any::TypeId::of::<GameEntity>(),
            std::any::TypeId::of::<EntityParent>(),
            std::any::TypeId::of::<EntityChildren>(),
        ]);
        for ty in component_types {
            if to_avoid_components.contains(&ty.type_id) {
                continue;
            }

            let mut should_remove = false;
            let component_ref = ctx.ecs_world.get_unchecked(selected_entity, ty.type_id);
            let component_name = &ctx
                .ecs_world
                .game_components
                .get(&ty.type_id)
                .unwrap()
                .component_name;
            Self::component_widget(ui, component_name, Some(&mut should_remove), |ui| {
                if let Some(show_fn) = ctx.ui_state.show_fns().get(&ty.type_id) {
                    // SAFETY: Show fn was registered with the same type as the type id it is keyed by,
                    // and the component ref data is a valid ptr to that same type.
                    unsafe {
                        show_fn.call(component_ref.get_component_ptr(), ui, &mut component_ctx);
                    }
                } else {
                    ui.label("No UI registered.");
                }
            });
            drop(component_ref);

            if should_remove {
                // Safety: We dont use the returned ptr.
                unsafe {
                    ctx.ecs_world
                        .try_remove_one_raw(selected_entity, &ty.type_id)
                }
                .unwrap_or_else(|| {
                    panic!(
                        "Component {} should exist if it is removable via UI.",
                        ty.name()
                    )
                });
            }
        }
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

    fn title_bar(ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        ui.label(egui::RichText::new("Entity properties").size(20.0));
        ui.horizontal(|ui| {
            if let Some(selected_entity) = &ctx.session.selected_entity {
                ui.menu_button("Add component", |ui| {
                    let mut selected_entity_components = HashSet::new();
                    for component_type_info in &ctx
                        .ecs_world
                        .entities
                        .get(*selected_entity)
                        .unwrap()
                        .components
                    {
                        selected_entity_components.insert(component_type_info.type_id());
                    }

                    for component_type_id in ctx.ecs_world.get_constructible_game_components() {
                        let game_component = ctx
                            .ecs_world
                            .game_components
                            .get(&component_type_id)
                            .unwrap();
                        let entity_has_component =
                            selected_entity_components.contains(&component_type_id);
                        if ui
                            .add_enabled(
                                !entity_has_component,
                                egui::Button::new(&game_component.component_name),
                            )
                            .clicked()
                        {
                            ctx.ecs_world.construct_and_insert_game_component(
                                *selected_entity,
                                component_type_id,
                            );
                            ui.close_menu();
                        }
                    }
                });
                if ui.button("Delete Entity").clicked() {
                    ctx.events.push(EntityCommandEvent::Despawn {
                        entity: *selected_entity,
                        despawn_children: true,
                    });
                    ctx.session.selected_entity = None;
                }
            }
        });
    }
}
