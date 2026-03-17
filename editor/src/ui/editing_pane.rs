use rogue_engine::common::color::{Color, ColorSpaceSrgb};
use strum::VariantArray;

use crate::{
    editing::voxel_editing::{EditorBrush, EditorBrushType},
    ui::pane::EditorUIPane,
};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct EditingPane {}

impl Default for EditingPane {
    fn default() -> Self {
        Self::new()
    }
}

impl EditingPane {
    pub fn new() -> Self {
        Self {}
    }

    pub fn show_header(ui: &mut egui::Ui, ctx: &mut super::EditorUIContext<'_>) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Voxel Editing").size(20.0));
        });
    }

    pub fn show_color_picker(&mut self, ui: &mut egui::Ui, color: &mut Color<ColorSpaceSrgb>) {
        let mut color32 = egui::Color32::from_rgb(
            (color.r() * 255.0) as u8,
            (color.g() * 255.0) as u8,
            (color.b() * 255.0) as u8,
        );
        egui::color_picker::color_picker_color32(
            ui,
            &mut color32,
            egui::color_picker::Alpha::Opaque,
        );
        *color = Color::new_srgb(
            color32.r() as f32 / 255.0,
            color32.g() as f32 / 255.0,
            color32.b() as f32 / 255.0,
        );
    }

    pub fn show_contents(&mut self, ui: &mut egui::Ui, ctx: &mut super::EditorUIContext<'_>) {
        ui.horizontal(|ui| {
            ui.label("Editing Enabled:");
            ui.checkbox(&mut ctx.voxel_editing.enabled, "");
        });

        self.show_color_picker(ui, &mut ctx.voxel_editing.color);

        ui.horizontal_wrapped(|ui| {
            for brush_type in EditorBrushType::VARIANTS {
                if ui
                    .add_enabled(
                        *brush_type != ctx.voxel_editing.brush.brush_type,
                        egui::Button::new(brush_type.to_string()),
                    )
                    .clicked()
                {
                    ctx.voxel_editing.brush.brush_type = *brush_type;
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("Brush Size:");
            ui.add(egui::DragValue::new(&mut ctx.voxel_editing.brush.brush_size).range(1..=128));
        });
    }
}

impl EditorUIPane for EditingPane {
    const ID: &'static str = "voxel_editing";
    const NAME: &'static str = "Editing";

    fn show(&mut self, ui: &mut egui::Ui, ctx: &mut super::EditorUIContext<'_>) {
        Self::show_header(ui, ctx);
        self.show_contents(ui, ctx);
    }
}
