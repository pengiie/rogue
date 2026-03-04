use nalgebra::Translation3;
use rogue_engine::{
    entity::{
        component::GameComponentCloneContext,
        ecs_world::{Entity, EventEntityDespawn},
        EntityChildren, EntityParent, GameEntity,
    },
    physics::transform::Transform,
};

use crate::ui::{
    entity_properties::EntityPropertiesPane,
    pane::{EditorUIPane, EditorUIPaneMethods},
    EditorCommand, EditorUIContext,
};

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
    fn section_header(ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        ui.horizontal(|ui| {
            ui.add(egui::Label::new(
                egui::RichText::new("Inspector").size(20.0),
            ));

            ui.menu_button("Add", |ui| {
                if ui.button("Empty").clicked() {
                    ctx.ecs_world.spawn((
                        GameEntity::new("new_entity"),
                        Transform::with_translation(Translation3::from(
                            ctx.session.editor_camera_controller().rotation_anchor,
                        )),
                    ));
                }
            });
        });
    }

    fn section_entities(ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        egui::ScrollArea::vertical()
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
        let label_id = egui::Id::new(format!(
            "left_panel_{}_{}_entity_label",
            entity_id.index(),
            entity_id.generation()
        ));
        let is_hovering = ui.data(|w| w.get_temp(label_id).unwrap_or(false));

        let mut text = egui::RichText::new(entity_name);
        if is_hovering {
            text = text.background_color(egui::Color32::from_white_alpha(2));
        }
        if ctx.session.selected_entity.is_some()
            && ctx.session.selected_entity.unwrap() == entity_id
        {
            text = text.background_color(egui::Color32::from_white_alpha(3));
        }
        let mut label = ui.add(egui::Label::new(text).truncate());

        ui.data_mut(|w| w.insert_temp(label_id, label.hovered()));
        if label.hovered() {
            ctx.session.hovered_entity = Some(entity_id);
        }

        if label.clicked() {
            ctx.session.selected_entity = Some(entity_id);
            ctx.commands
                .push(EditorCommand::open_ui(EntityPropertiesPane::ID));
        }

        label.context_menu(|ui| {
            if ui.button("Copy").clicked() {
                log::error!("TODOOOOOOOOOOOO Enityt copying is not implement yet!!!!!");
                ui.close_menu();
            }
            if ui.button("Duplicate").clicked() {
                ctx.session.selected_entity = Some(ctx.ecs_world.duplicate(
                    entity_id,
                    GameComponentCloneContext {
                        voxel_registry: ctx.voxel_registry,
                        collider_registry: &mut ctx.physics_world.colliders,
                    },
                ));
                ui.close_menu();
            }
            if ui.button("Delete").clicked() {
                // Ensure we do it as an event since we are iterating over
                // the e
                ctx.events.push(EventEntityDespawn(entity_id));
                ctx.session.selected_entity = None;
                ui.close_menu();
            }
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
            ui.vertical(|ui| {
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
        });
    }
}
