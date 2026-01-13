use rogue_engine::entity::ecs_world::ECSWorld;

use crate::ui::EditorUIContext;

pub struct EntityPropertiesPane {}

impl EntityPropertiesPane {
    fn title_bar(ui: &mut egui::Ui, ecs_world: &mut ECSWorld) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Entity properties").size(20.0));
            //if let Some(selected_entity) = &editor.selected_entity {
            //    ui.menu_button("Add component", |ui| {
            //        let mut selected_entity_components = HashSet::new();
            //        for component_type_info in
            //            &ecs_world.entities.get(*selected_entity).unwrap().components
            //        {
            //            selected_entity_components.insert(component_type_info.type_id());
            //        }

            //        for component_type_id in ecs_world.get_constructible_game_components() {
            //            let game_component =
            //                ecs_world.game_components.get(&component_type_id).unwrap();
            //            let entity_has_component =
            //                selected_entity_components.contains(&component_type_id);
            //            if ui
            //                .add_enabled(
            //                    !entity_has_component,
            //                    egui::Button::new(&game_component.component_name),
            //                )
            //                .clicked()
            //            {
            //                ecs_world.construct_and_insert_game_component(
            //                    *selected_entity,
            //                    component_type_id,
            //                );
            //                ui.close_menu();
            //            }
            //        }
            //    });
            //}
        });
    }

    pub fn show(ui: &mut egui::Ui, ctx: &mut EditorUIContext<'_>) {
        Self::title_bar(ui, ctx.ecs_world);
        ui.add_space(16.0);
    }
}
