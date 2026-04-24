use nalgebra::Translation3;
use rogue_engine::{
    entity::{
        EntityChildren, EntityParent, GameEntity,
        component::GameComponentCloneContext,
        ecs_world::{Entity, EntityCommandEvent},
    },
    physics::transform::Transform,
};

use crate::ui::{
    EditorCommand, EditorUIContext,
    entity_properties::EntityPropertiesPane,
    pane::{EditorUIPane, EditorUIPaneMethods},
};

type EntityPayload = Entity;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct EntityHierarchyUI;

impl EditorUIPane for EntityHierarchyUI {
    const ID: &'static str = "entity_hierarchy";
    const NAME: &'static str = "Entity Hierarchy";

    fn show(&mut self, ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        Self::section_header(ui, ctx);
        Self::section_entities(ui, ctx);
    }
}

impl EntityHierarchyUI {
    fn add_menu(ui: &mut egui::Ui, parent_entity: Option<Entity>, ctx: &mut EditorUIContext<'_>) {
        ui.menu_button("Add", |ui| {
            if ui.button("Empty").clicked() {
                let transform = if parent_entity.is_some() {
                    Transform::new()
                } else {
                    Transform::with_translation(Translation3::from(
                        ctx.session.editor_camera_controller().rotation_anchor,
                    ))
                };
                let entity = ctx
                    .ecs_world
                    .spawn((GameEntity::new("new_entity"), transform));
                if let Some(parent) = parent_entity {
                    ctx.events.push(EntityCommandEvent::SetParent {
                        parent: Some(parent),
                        child: entity,
                        modify_transform: false,
                    });
                }
                ui.close_menu();
            }
        });
    }

    fn section_header(ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        ui.horizontal(|ui| {
            let label = ui.add(egui::Label::new(
                egui::RichText::new("Inspector").size(20.0),
            ));
            // Unparent entity since it was dragged onto the top of the hierarchy.
            // Probably doesn't need to be a command here but its safest to do so in ui code.
            if let Some(new_child) = label.dnd_release_payload::<EntityPayload>() {
                ctx.events.push(EntityCommandEvent::SetParent {
                    parent: None,
                    child: *new_child,
                    modify_transform: true,
                });
            }

            Self::add_menu(ui, None, ctx);
        });
    }

    fn section_entities(ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        let scroll_area_output = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut game_entity_query = ctx
                    .ecs_world
                    .query::<&GameEntity>()
                    .without::<(EntityParent,)>();
                let game_entities = game_entity_query
                    .into_iter()
                    .map(|(entity, game_entity)| (entity, game_entity.clone()))
                    .collect::<Vec<_>>();

                for (entity_id, game_entity) in game_entities {
                    Self::render_entity_label(ui, ctx, entity_id, game_entity.name.clone());
                    Self::render_children(ui, ctx, entity_id);
                }
            });
    }

    // Renders the label of the entity and provides interaction.
    fn render_entity_label(
        ui: &mut egui::Ui,
        ctx: &mut EditorUIContext<'_>,
        entity_id: Entity,
        entity_name: String,
    ) {
        ui.push_id(format!("entity_{}", entity_id.index()), |ui| {
            let dnd_source_id = egui::Id::new(format!(
                "left_panel_{}_{}_entity_label_dnd_source",
                entity_id.index(),
                entity_id.generation()
            ));

            let label_hover_id = egui::Id::new(format!(
                "left_panel_{}_{}_entity_label_hover",
                entity_id.index(),
                entity_id.generation()
            ));
            let label_click_id = egui::Id::new(format!(
                "left_panel_{}_{}_entity_label_hover",
                entity_id.index(),
                entity_id.generation()
            ));
            let is_hovering = ui.data(|w| w.get_temp(label_hover_id).unwrap_or(false));

            let mut text = egui::RichText::new(entity_name);
            if is_hovering {
                text = text.background_color(egui::Color32::from_white_alpha(2));
            }
            if ctx.session.selected_entity.is_some()
                && ctx.session.selected_entity.unwrap() == entity_id
            {
                text = text.background_color(egui::Color32::from_white_alpha(3));
            }

            let label = ui
                .dnd_drag_source::<EntityPayload, _>(dnd_source_id, entity_id, |ui| {
                    ui.add(egui::Label::new(text).truncate());
                })
                .response;
            // dnd_drag_source makes it grabby hand icon and I dont like that.
            if label.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
            }

            // Parent entity with drag and drop.
            if let Some(new_child) = label.dnd_release_payload::<EntityPayload>()
                && *new_child != entity_id
            {
                // Check that we don't create a parent cycle.
                if !ctx.ecs_world.has_parent(entity_id, *new_child) {
                    ctx.events.push(EntityCommandEvent::SetParent {
                        parent: Some(entity_id),
                        child: *new_child,
                        modify_transform: true,
                    });
                }
            }

            ui.data_mut(|w| w.insert_temp(label_hover_id, label.hovered()));
            if label.hovered() {
                ctx.session.hovered_entity = Some(entity_id);
            }

            if label.interact(egui::Sense::click()).clicked() {
                ctx.session.selected_entity = Some(entity_id);
                if !ctx.voxel_editing.enabled {
                    ctx.commands
                        .push(EditorCommand::open_ui(EntityPropertiesPane::ID));
                }
            }

            label.context_menu(|ui| {
                Self::add_menu(ui, Some(entity_id), ctx);

                if ui.button("Save as prefab").clicked() {
                    ui.close_menu();
                }
                if ui.button("Copy").clicked() {
                    log::error!("TODOOOOOOOOOOOO Enityt copying is not implement yet!!!!!");
                    ui.close_menu();
                }
                if ui.button("Duplicate").clicked() {
                    let existing_parent = ctx
                        .ecs_world
                        .get::<&EntityParent>(entity_id)
                        .map(|p| p.parent())
                        .ok();
                    ctx.session.selected_entity = Some(ctx.ecs_world.duplicate_entity(
                        entity_id,
                        existing_parent,
                        &mut GameComponentCloneContext {
                            voxel_registry: ctx.voxel_registry,
                            collider_registry: &mut ctx.physics_world.colliders,
                        },
                    ));
                    ui.close_menu();
                }
                if ui.button("Delete").clicked() {
                    // Ensure we do it as an event since we are iterating over
                    // the e
                    ctx.events.push(EntityCommandEvent::Despawn {
                        entity: entity_id,
                        despawn_children: true,
                    });
                    ctx.session.selected_entity = None;
                    ui.close_menu();
                }
            });
        });
    }

    // Renders any children the entity has, if any.
    fn render_children(ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>, entity_id: Entity) {
        let Ok(children_query) = ctx.ecs_world.get::<&EntityChildren>(entity_id) else {
            return;
        };
        let children = children_query.children.clone();
        drop(children_query);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            let res = ui.vertical(|ui| {
                for child in children {
                    let Ok(child_game_entity) = ctx.ecs_world.get::<&GameEntity>(child) else {
                        continue;
                    };
                    let child_name = child_game_entity.name.clone();
                    drop(child_game_entity);
                    Self::render_entity_label(ui, ctx, child, child_name);
                    Self::render_children(ui, ctx, child);
                }
            });
            if res.response.interact(egui::Sense::click()).clicked() {
                ctx.session.selected_entity = None;
            }
        });
    }
}
