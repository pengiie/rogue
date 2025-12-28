use std::ops::Deref;

use nalgebra::{Translation3, Vector3};

use crate::engine::editor::ui::ui_state::EditorUIState;
use crate::engine::voxel::voxel_registry::VoxelModelRegistry;
use crate::engine::{
    editor::{editor::Editor, events::EventEditorZoom},
    entity::{
        component::GameComponentCloneContext,
        ecs_world::{ECSWorld, Entity, EventEntityDespawn},
        EntityChildren, EntityParent, GameEntity, RenderableVoxelEntity,
    },
    event::Events,
    physics::{physics_world::PhysicsWorld, transform::Transform},
    voxel::factory::VoxelModelFactory,
};

pub fn entity_hierarchy_ui(
    ui: &mut egui::Ui,
    ecs_world: &mut ECSWorld,
    editor: &mut Editor,
    voxel_registry: &mut VoxelModelRegistry,
    ui_state: &mut EditorUIState,
    events: &mut Events,
    physics_world: &mut PhysicsWorld,
) {
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
                let model_id = voxel_registry.register_voxel_model(
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
                    RenderableVoxelEntity::new(None, false, model_id),
                ));
            }
        });
    });

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut game_entity_query = ecs_world
                .query::<&GameEntity>()
                .without::<(EntityParent,)>();
            let game_entities = game_entity_query
                .into_iter()
                .map(|(entity, game_entity)| (entity, game_entity.clone()))
                .collect::<Vec<_>>();

            for (entity_id, game_entity) in game_entities {
                render_entity_label(
                    ui,
                    editor,
                    ecs_world,
                    ui_state,
                    entity_id,
                    &game_entity,
                    events,
                    voxel_world,
                    physics_world,
                );

                render_children(
                    ui,
                    editor,
                    ecs_world,
                    ui_state,
                    events,
                    entity_id,
                    voxel_world,
                    physics_world,
                );
            }
        });
    //ui.label(egui::RichText::new("Performance:").size(8.0));
    //ui.label(format!("FPS: {}", debug_state.fps));
    //ui.label(format!("Frame time: {}ms", debug_state.delta_time_ms));
    //ui.label(format!("Voxel data allocation: {}", total_allocation_str));
}

// Renders any children the entity has, if any.
fn render_children(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    ecs_world: &mut ECSWorld,
    ui_state: &mut EditorUIState,
    events: &mut Events,
    entity_id: Entity,
    voxel_world: &mut VoxelWorld,
    physics_world: &mut PhysicsWorld,
) {
    let Ok(children_query) = ecs_world.get::<&EntityChildren>(entity_id) else {
        return;
    };
    let children = children_query.children.clone();
    drop(children_query);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.vertical(|ui| {
            for child in children {
                let child_game_entity = ecs_world.get::<&GameEntity>(child);
                if child_game_entity.is_err() {
                    continue;
                }
                let ge = child_game_entity.as_ref().unwrap().deref().clone();
                drop(child_game_entity);
                render_entity_label(
                    ui,
                    editor,
                    ecs_world,
                    ui_state,
                    child,
                    &ge,
                    events,
                    voxel_world,
                    physics_world,
                );
                render_children(
                    ui,
                    editor,
                    ecs_world,
                    ui_state,
                    events,
                    child,
                    voxel_world,
                    physics_world,
                );
            }
        });
    });
}

// Renders the label of the entity and provides interaction.
fn render_entity_label(
    ui: &mut egui::Ui,
    editor: &mut Editor,
    ecs_world: &mut ECSWorld,
    ui_state: &mut EditorUIState,
    entity_id: Entity,
    game_entity: &GameEntity,
    events: &mut Events,
    voxel_world: &mut VoxelWorld,
    physics_world: &mut PhysicsWorld,
) {
    let label_id = egui::Id::new(format!(
        "left_panel_{}_{}_entity_label",
        entity_id.index(),
        entity_id.generation()
    ));
    let is_hovering = ui.data(|w| w.get_temp(label_id).unwrap_or(false));

    let mut text = egui::RichText::new(game_entity.name.clone());
    if is_hovering {
        text = text.background_color(egui::Color32::from_white_alpha(2));
    }
    if editor.selected_entity.is_some() && editor.selected_entity.unwrap() == entity_id {
        text = text.background_color(egui::Color32::from_white_alpha(3));
    }
    let mut label = ui.add(egui::Label::new(text).truncate());

    ui.data_mut(|w| w.insert_temp(label_id, label.hovered()));
    if label.hovered() {
        editor.hovered_entity = Some(entity_id);
    }

    if label.clicked() {
        // Check if we are select a new parent for the currently selected entity.
        if let Some(new_child) = ui_state.selecting_new_parent.take() {
            ecs_world.set_parent(new_child, Some(entity_id));
        } else {
            editor.selected_entity = Some(entity_id);
        }
    }

    label.context_menu(|ui| {
        if ui.button("Copy").clicked() {
            log::error!("TODOOOOOOOOOOOO Enityt copying is not implement yet!!!!!");
            ui.close_menu();
        }
        if ui.button("Duplicate").clicked() {
            editor.selected_entity = Some(ecs_world.duplicate(
                entity_id,
                GameComponentCloneContext {
                    voxel_world,
                    collider_registry: &mut physics_world.colliders,
                },
            ));
            ui.close_menu();
        }
        if ui.button("Delete").clicked() {
            // Ensure we do it as an event since we are iterating over the e
            events.push(EventEntityDespawn(entity_id));
            editor.selected_entity = None;
            ui.close_menu();
        }
    });

    // Double-right click to zoom editor camera to entity.
    if label.double_clicked_by(egui::PointerButton::Secondary) {
        events.push(EventEditorZoom::Entity {
            target_entity: entity_id,
        });
    }
}
