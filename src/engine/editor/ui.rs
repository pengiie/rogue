use nalgebra::{SimdValue, Vector4};

use crate::{engine::entity::ecs_world::ECSWorld, game::entity::GameEntity};

/// Returns padding [top, bottom, left right].
pub fn egui_editor_ui(ctx: &egui::Context, ecs_world: &mut ECSWorld) -> Vector4<f32> {
    let mut content_padding = Vector4::zeros();

    content_padding.x = egui::TopBottomPanel::top("top_editor_pane")
        .resizable(true)
        .frame(
            egui::Frame::default()
                .inner_margin(4.0)
                .fill(egui::Color32::from_hex("#11111b").unwrap()),
        )
        .show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {});
                ui.menu_button("Help", |ui| {
                    ui.label("Good luck :)");
                });
            });
        })
        .response
        .rect
        .height()
        * ctx.pixels_per_point();

    content_padding.z = egui::SidePanel::left("left_editor_pane")
        .resizable(true)
        .frame(
            egui::Frame::default()
                .inner_margin(12.0)
                .fill(egui::Color32::from_hex("#11111b").unwrap()),
        )
        .default_width(300.0)
        .max_width(500.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let label_size = ui
                    .add(egui::Label::new(
                        egui::RichText::new("Inspector").size(20.0),
                    ))
                    .rect
                    .size();
                let button_width = ui.add(egui::Button::new("teswt")).rect.size();
            });

            let mut game_entity_query = ecs_world.query::<&GameEntity>();
            for (entity_id, game_entity) in game_entity_query.into_iter() {
                ui.label(game_entity.name.clone());
            }
            //ui.label(egui::RichText::new("Performance:").size(16.0));
            //ui.label(format!("FPS: {}", debug_state.fps));
            //ui.label(format!("Frame time: {}ms", debug_state.delta_time_ms));
            //ui.label(format!("Voxel data allocation: {}", total_allocation_str));
        })
        .response
        .rect
        .width()
        * ctx.pixels_per_point();
    return content_padding;
}
