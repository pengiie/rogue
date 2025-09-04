use crate::{
    common::color::Color,
    engine::{
        asset::asset::Assets,
        editor::editor::{Editor, EditorEditingMaterial, EditorEditingTool},
        entity::ecs_world::ECSWorld,
        ui::EditorUIState,
        voxel::voxel_world::VoxelWorld,
    },
    session::Session,
};

fn color_picker(ui: &mut egui::Ui, color: &mut Color) {
    let mut egui_color = egui::Color32::from_rgb(color.r_u8(), color.g_u8(), color.b_u8());

    egui::color_picker::color_picker_color32(
        ui,
        &mut egui_color,
        egui::color_picker::Alpha::Opaque,
    );
    color.set_rgb_u8(egui_color.r(), egui_color.g(), egui_color.b());
}

fn bmat_picker(ui: &mut egui::Ui, bmat_index: &mut u16) {
    if ui
        .add_enabled(*bmat_index != 0, egui::Button::new("Grass"))
        .clicked()
    {
        *bmat_index = 0;
    }
    if ui
        .add_enabled(*bmat_index != 1, egui::Button::new("Dirt"))
        .clicked()
    {
        *bmat_index = 1;
    }
}

fn material_picker(ui: &mut egui::Ui, material: &mut EditorEditingMaterial) {
    if ui
        .add_enabled(
            *material != EditorEditingMaterial::BMat,
            egui::Button::new("BMat"),
        )
        .clicked()
    {
        *material = EditorEditingMaterial::BMat;
    }
    if ui
        .add_enabled(
            *material != EditorEditingMaterial::PTMat,
            egui::Button::new("PTMat"),
        )
        .clicked()
    {
        *material = EditorEditingMaterial::PTMat;
    }
}

pub fn editing_pane(
    ui: &mut egui::Ui,
    ecs_world: &mut ECSWorld,
    editor: &mut Editor,
    voxel_world: &mut VoxelWorld,
    ui_state: &mut EditorUIState,
    session: &mut Session,
    assets: &mut Assets,
) {
    let content = |ui: &mut egui::Ui| {
        ui.label(egui::RichText::new("Voxel Editing").size(20.0));

        ui.horizontal(|ui| {
            ui.label("Entity editing enabled:");
            ui.add(egui::Checkbox::without_text(
                &mut editor.world_editing.entity_enabled,
            ));
        });
        ui.horizontal(|ui| {
            ui.label("Terrain editing enabled:");
            ui.checkbox(&mut editor.world_editing.terrain_enabled, "");
        });

        color_picker(ui, &mut editor.world_editing.color);
        bmat_picker(ui, &mut editor.world_editing.bmat_index);

        material_picker(ui, &mut editor.world_editing.material);

        ui.add_enabled_ui(
            editor.world_editing.terrain_enabled || editor.world_editing.entity_enabled,
            |ui| {
                ui.label("Tools");
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            editor.world_editing.tool != EditorEditingTool::Pencil,
                            egui::Button::new("Pencil"),
                        )
                        .clicked()
                    {
                        editor.world_editing.tool = EditorEditingTool::Pencil;
                    }
                    if ui
                        .add_enabled(
                            editor.world_editing.tool != EditorEditingTool::Eraser,
                            egui::Button::new("Eraser"),
                        )
                        .clicked()
                    {
                        editor.world_editing.tool = EditorEditingTool::Eraser;
                    }
                });
                ui.add_space(8.0);

                let size = &mut editor.world_editing.size;
                match &mut editor.world_editing.tool {
                    EditorEditingTool::Pencil => {
                        ui.label(egui::RichText::new("Pencil").size(18.0));
                        ui.horizontal(|ui| {
                            ui.label("Size:");
                            ui.add(egui::Slider::new(size, 0..=100).step_by(1.0));
                        });
                    }
                    EditorEditingTool::Brush => {}
                    EditorEditingTool::Eraser => {
                        ui.label(egui::RichText::new("Eraser").size(18.0));
                        ui.horizontal(|ui| {
                            ui.label("Size:");
                            ui.add(egui::Slider::new(size, 0..=100).step_by(1.0));
                        });
                    }
                }
            },
        );
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, content);
}
