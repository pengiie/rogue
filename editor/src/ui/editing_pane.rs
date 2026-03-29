use rogue_engine::{
    common::color::{Color, ColorSpaceSrgb},
    entity::{GameEntity, RenderableVoxelEntity},
    voxel::voxel::VoxelModelEditRegion,
};
use strum::VariantArray;

use crate::{
    editing::voxel_editing::{EditorEditingTool, EditorEditingToolType},
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
        ui.separator();

        match ctx.voxel_editing.edit_target {
            Some(crate::editing::voxel_editing::EditorVoxelEditingTarget::Entity(entity)) => {
                let (game_entity, renderable) = ctx
                    .ecs_world
                    .query_one::<(&GameEntity, &RenderableVoxelEntity)>(entity)
                    .get()
                    .unwrap();
                ui.label(format!("Current editing entity: {}", game_entity.name));
                ui.label(if renderable.is_dynamic() {
                    "Entity renderable is dynamic, can read/write."
                } else {
                    "Entity renderable is NOT dynamic, can only read."
                });
            }
            Some(crate::editing::voxel_editing::EditorVoxelEditingTarget::Terrain) => {
                ui.label("Currently editing terrain.");
            }
            None => {
                if ctx.voxel_editing.enabled {
                    ui.label("No editing target");
                } else {
                    ui.label("No editing target, editing is disabled.");
                }
            }
        }

        ui.separator();
        ui.label("Current Material:");
        self.show_color_picker(ui, &mut ctx.voxel_editing.color);

        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for tool_type in EditorEditingToolType::VARIANTS {
                if ui
                    .add_enabled(
                        *tool_type != ctx.voxel_editing.selected_tool_type,
                        egui::Button::new(tool_type.to_string()),
                    )
                    .clicked()
                {
                    ctx.voxel_editing.selected_tool_type = *tool_type;
                }
            }
        });

        fn brush_size_ui(ui: &mut egui::Ui, brush_size: &mut u32) {
            ui.horizontal(|ui| {
                ui.label("Brush Size:");
                ui.add(egui::DragValue::new(brush_size).range(1..=128));
            });
        }
        let tool = ctx
            .voxel_editing
            .tools
            .get_mut(&ctx.voxel_editing.selected_tool_type)
            .unwrap();
        match tool {
            EditorEditingTool::Pencil { brush_size } => {
                brush_size_ui(ui, brush_size);
            }
            EditorEditingTool::Paint { brush_size } => {
                brush_size_ui(ui, brush_size);
            }
            EditorEditingTool::Eraser { brush_size } => {
                brush_size_ui(ui, brush_size);
            }
            EditorEditingTool::Selection => {
                ui.label("Rectangle Selection:");
            }
            EditorEditingTool::ColorPicker => {
                ui.label("Color picker");
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Masks");
        });
        let mut has_presence_mask = ctx.voxel_editing.masks.iter().any(|mask| {
            matches!(
                mask,
                rogue_engine::voxel::voxel::VoxelModelEditMaskLayer::Presence
            )
        });
        let old_has_presence_mask = has_presence_mask;
        ui.checkbox(&mut has_presence_mask, "Presence Mask");
        if has_presence_mask && !old_has_presence_mask {
            ctx.voxel_editing
                .masks
                .push(rogue_engine::voxel::voxel::VoxelModelEditMaskLayer::Presence);
        } else if !has_presence_mask && old_has_presence_mask {
            ctx.voxel_editing.masks.retain(|mask| {
                !matches!(
                    mask,
                    rogue_engine::voxel::voxel::VoxelModelEditMaskLayer::Presence
                )
            });
        }
        ui.vertical(|ui| {
            let mut to_remove = None;
            for (i, mask) in ctx.voxel_editing.masks.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(mask.to_string());
                    if ui.button("Remove").clicked() {
                        to_remove = Some(i);
                    }
                });
                match mask {
                    rogue_engine::voxel::voxel::VoxelModelEditMaskLayer::Presence => {}
                    rogue_engine::voxel::voxel::VoxelModelEditMaskLayer::Sphere {
                        center,
                        diameter,
                    } => {}
                }
            }
            if let Some(i) = to_remove {
                ctx.voxel_editing.masks.remove(i);
            }
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
