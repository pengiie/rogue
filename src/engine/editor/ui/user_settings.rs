use crate::{
    engine::{
        asset::asset::Assets, editor::editor::Editor, entity::ecs_world::ECSWorld,
        ui::EditorUIState, voxel::voxel_world::VoxelWorld, window::time::Time,
    },
    session::Session,
    settings::Settings,
};

pub fn user_pane(
    ui: &mut egui::Ui,
    ecs_world: &mut ECSWorld,
    editor: &mut Editor,
    voxel_world: &mut VoxelWorld,
    ui_state: &mut EditorUIState,
    session: &mut Session,
    assets: &mut Assets,
    settings: &mut Settings,
    time: &Time,
) {
    let content = |ui: &mut egui::Ui| {
        ui.label(egui::RichText::new("User Settings").size(20.0));
        ui.add_space(16.0);

        ui.horizontal(|ui| {
            ui.label("Render distance:");
            let original = settings.chunk_render_distance;
            ui.add(egui::Slider::new(
                &mut settings.chunk_render_distance,
                0..=64,
            ));
        })
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, content);
}
