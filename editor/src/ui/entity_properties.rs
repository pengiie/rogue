use nalgebra::Vector3;
use rogue_engine::{
    animation::animator::Animator,
    asset::asset::{Assets, GameAssetPath},
    common::dyn_vec::TypeInfo,
    egui::egui_util,
    entity::{
        EntityChildren, EntityParent, GameEntity, RenderableVoxelEntity,
        component::{GameComponent, RawComponentRef},
        ecs_world::{ECSWorld, EntityCommandEvent},
    },
    event::Events,
    graphics::camera::Camera,
    material::MaterialId,
    physics::{
        box_collider::BoxCollider,
        capsule_collider::CapsuleCollider,
        collider_component::EntityColliders,
        collider_registry::ColliderId,
        physics_world::PhysicsWorld,
        plane_collider::PlaneCollider,
        rigid_body::{RigidBody, RigidBodyPositionInterpolation, RigidBodyType},
        transform::Transform,
    },
    voxel::{
        attachment::Attachment,
        sft_compressed::VoxelModelSFTCompressed,
        voxel::{VoxelModelEdit, VoxelModelEditRegion, VoxelModelImplMethods},
        voxel_registry::{VoxelModelId, VoxelModelRegistry},
    },
};
use rogue_game::player::player_controller::PlayerController;
use std::{
    any::TypeId,
    collections::{HashMap, HashSet},
    fmt::Display,
};

use crate::{
    editing::voxel_editing::{EditorVoxelEditing, EditorVoxelEditingTarget},
    editor_transform_euler::EditorTransformEuler,
    session::{EditorCommandEvent, EditorSession},
    ui::{
        EditorCommand, EditorCommands, EditorDialog, EditorUIContext, FilePickerType,
        create_voxel_model_dialog::{CreateVoxelModelDialogCreateInfo, create_voxel_model_dialog},
        pane::{EditorUIPane, EditorUIPaneMethods},
        resize_model_dialog::{ResizeVoxelModelDialogCreateInfo, resize_voxel_model_dialog_cmd},
    },
};

pub struct ShowComponentContext<'a> {
    physics_world: &'a mut PhysicsWorld,
    voxel_registry: &'a mut VoxelModelRegistry,
    component_state: &'a mut HashMap<TypeId, Box<dyn std::any::Any>>,
    session: &'a mut EditorSession,
    commands: &'a mut EditorCommands,
    assets: &'a mut Assets,
    events: &'a mut Events,
    voxel_editing: &'a mut EditorVoxelEditing,
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
        s.register_component_ui::<Animator>(Self::show_animator_component);

        // TODO: Expose the editor api as a library and then have the game code able to register
        // editor stuff with a feature or something. Possibly just make the these show fns a global
        // variable.
        // Game component show fns
        fn show_player_controller(
            player_controller: &mut PlayerController,
            ui: &mut egui::Ui,
            ctx: &mut ShowComponentContext,
        ) {
            egui_util::game_asset_path_button(
                ui,
                &mut player_controller.idle_animation,
                "Idle Animation:".to_owned(),
                |_| {},
            );
        }
        s.register_component_ui::<PlayerController>(show_player_controller);

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
            let menu_response = ui.menu_button(text, |ui| {
                // Open new model dialog within the editor.
                if ui.button("Create new").clicked() {
                    ctx.commands.push(create_voxel_model_dialog(
                        CreateVoxelModelDialogCreateInfo {
                            target_entity: Some(selected_entity),
                        },
                    ));
                }

                // Choose existing model saved on the file system within the project asset
                // directory.
                if ui.button("Choose existing").clicked() {
                    ui.close_menu();
                }

                let can_save = renderable.model_asset_path().is_some() && renderable.is_dynamic() && renderable.voxel_model_id().is_some();
                if ui.add_enabled(can_save,
                        egui::Button::new("Save")).clicked() {
                            ctx.events
                                .push(EditorCommandEvent::SaveVoxelModel(renderable.voxel_model_id().unwrap()));
                            ui.close_menu();
                }

                if ui
                    .add_enabled(
                        can_save,
                        egui::Button::new("Save as"),
                    )
                    .clicked() &&
                    // This should always exist but might as well do the check.
                    let Some(project_dir) = ctx.assets.project_dir()
                {
                    let model_asset_path = renderable.model_asset_path().unwrap().clone();
                    let path = model_asset_path.as_file_asset_path(project_dir);
                    let saving_entity = selected_entity.clone();
                    let existing_path = ctx.commands.push(EditorCommand::FilePicker {
                        picker_type: FilePickerType::CreateFile,
                        callback: Box::new(move |ctx, asset_path| {
                            let game_asset_path = GameAssetPath::from_relative_path(&asset_path);
                            let Ok(mut renderable) = ctx
                                .ecs_world
                                .get::<&mut RenderableVoxelEntity>(saving_entity)
                            else {
                                return;
                            };
                            assert!(renderable.is_dynamic(), "Only makes sense to save dynamic models since they are the only ones which can be edited.");
                            let Some(voxel_model_id) = renderable.voxel_model_id() else {
                                return;
                            };
                            if Some(&game_asset_path) != renderable.model_asset_path() {
                                let new_model_id = ctx.voxel_registry.clone_model(voxel_model_id);
                                renderable.set_model_id(new_model_id);
                                // New model has our new asset path.
                                ctx.voxel_registry.set_model_asset_path(
                                    renderable.voxel_model_id().unwrap(),
                                    Some(game_asset_path.clone()),
                                );
                                renderable.set_model_asset_path(Some(game_asset_path.clone()));
                            } else {
                                // We need to update static model asset and reload any dynamic
                                // models. kinda weird.
                            }

                            ctx.events
                                .push(EditorCommandEvent::SaveVoxelModel(renderable.voxel_model_id().unwrap()));
                        }),
                        extensions: vec!["rvox".to_owned()],
                        preset_file_path: Some(path.path().to_path_buf()),
                    });
                    ui.close_menu();
                }

                let can_edit = renderable.is_dynamic() && renderable.voxel_model_id().is_some();
                if ui.add_enabled(can_edit, egui::Button::new("Resize")).clicked() {
                    ctx.commands.push(resize_voxel_model_dialog_cmd(ResizeVoxelModelDialogCreateInfo {
                        target_model: renderable.voxel_model_id().unwrap(),
                        associated_entity: selected_entity,
                    }));
                    ui.close_menu();
                }

                if ui.add_enabled(can_edit, egui::Button::new("Set editing target")).clicked() {
                    let side_length = ctx.voxel_registry.get_dyn_model(renderable.voxel_model_id().unwrap()).length();
                    ctx.voxel_editing.edit_target = Some(EditorVoxelEditingTarget::Entity(selected_entity));
                    ui.close_menu();
                }
            });
            if let Some(new_asset_path) = menu_response.response.dnd_release_payload::<GameAssetPath>() {
                // Set id to null since entity models are loaded automatically.
                renderable.set_model(Some((*new_asset_path).clone()), VoxelModelId::null());
            }
        });

        ui.horizontal(|ui| {
            ui.label("Dynamic:");

            // Doesn't make sense to change if the model is dynamic (has its own unique model) if it doesn't have an asset
            // path attached.
            let can_change_dynamic = renderable.model_asset_path().is_some();
            let mut is_dynamic = renderable.is_dynamic();
            ui.add_enabled(can_change_dynamic, egui::Checkbox::new(&mut is_dynamic, ""));

            if let Some(model_id) = renderable.voxel_model_id() {
                if !renderable.is_dynamic()
                    && is_dynamic
                {
                    assert!(
                        renderable.model_asset_path().is_some(),
                        "RenderableVoxelEntity with a model id and changed to dynamic should have an asset path."
                    );
                    let new_model_id = ctx
                        .voxel_registry.clone_model(model_id);
                    renderable.set_model_id(new_model_id);
                } else if renderable.is_dynamic() && !is_dynamic {
                    // TODO: Unload old model and itll auto load the asset model.
                }
            }
            renderable.set_dynamic(is_dynamic);
        });
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
            ui.label("Interpolation");
            egui::ComboBox::from_id_salt("rigid_body_interpolation")
                .selected_text(rigid_body.interpolation.to_string())
                .show_ui(ui, |ui| {
                    use strum::VariantArray;
                    for val in RigidBodyPositionInterpolation::VARIANTS {
                        ui.selectable_value(&mut rigid_body.interpolation, *val, val.to_string());
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
            "Velocity  X: {:.2}, Y: {:.2}, Z: {:.2}",
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

    fn show_animator_component(
        animator: &mut Animator,
        ui: &mut egui::Ui,
        ctx: &mut ShowComponentContext,
    ) {
        struct AnimatorUIState {}

        //let ui_state = ctx
        //    .component_state
        //    .entry(std::any::TypeId::of::<EntityColliders>())
        //    .or_insert_with(|| {
        //        Box::new(ColliderUIState {
        //            selected_collider: None,
        //        })
        //    })
        //    .downcast_mut::<ColliderUIState>()
        //    .unwrap();
        ui.horizontal(|ui| {
            ui.label("Animations:");
            if animator.animations.is_empty() {
                ui.label("None");
            }
        });
        let (res, new_animation) = ui.dnd_drop_zone::<GameAssetPath, _>(egui::Frame::new(), |ui| {
            ui.vertical(|ui| {
                for animation in &animator.animations {
                    ui.label(animation.as_relative_path_str());
                }
            });
        });
        if let Some(new_animation) = new_animation {
            animator.animations.insert((*new_animation).clone());
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
                    if ui
                        .add_enabled(parent_entity.is_some(), egui::Button::new("Remove"))
                        .clicked()
                    {
                        ctx.events.push(EntityCommandEvent::SetParent {
                            parent: None,
                            child: selected_entity,
                            modify_transform: true,
                        });
                        ui.close_menu();
                    }
                });
            });
        });

        // Like to keep the transform hoisted at the top.
        if let Ok(mut transform) = ctx.ecs_world.get::<&mut Transform>(selected_entity) {
            Self::component_widget(ui, "Transform", None, |ui| {
                rogue_engine::egui::util::position_ui(ui, &mut transform.position);
                if let Ok(mut editor_euler) = ctx
                    .ecs_world
                    .get::<&mut EditorTransformEuler>(selected_entity)
                {
                    let mut prev_euler = editor_euler.euler().map(|x| x.to_degrees());
                    let mut new_euler = prev_euler;
                    rogue_engine::egui::util::rotation_ui_euler(ui, &mut new_euler);
                    if new_euler != prev_euler {
                        let new_quat = editor_euler.set_euler(new_euler.map(|x| x.to_radians()));
                        transform.rotation = new_quat;
                    }
                } else {
                    rogue_engine::egui::util::rotation_ui(ui, &mut transform.rotation);
                }
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
            assets: ctx.assets,
            events: ctx.events,
            voxel_editing: ctx.voxel_editing,
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
            let Some(component_name) = ctx
                .ecs_world
                .game_components
                .get(&ty.type_id)
                .map(|c| &c.component_name)
            else {
                continue;
            };

            let mut should_remove = false;
            let component_ref = ctx.ecs_world.get_unchecked(selected_entity, ty.type_id);
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
                    if let Some(EditorVoxelEditingTarget::Entity(target_entity)) =
                        &ctx.voxel_editing.edit_target
                        && target_entity == selected_entity
                    {
                        ctx.voxel_editing.edit_target = None;
                    }
                    ctx.session.selected_entity = None;
                }
            }
        });
    }
}
