use std::collections::VecDeque;

use nalgebra::Vector3;
use rogue_engine::{
    asset::asset::GameAssetPath,
    common::color::{Color, ColorSpaceSrgb},
    entity::{GameEntity, RenderableVoxelEntity},
    material::material_bank::MaterialId,
    voxel::voxel::{VoxelModelEditMaskLayer, VoxelModelEditRegion},
    world::terrain::region_map::{
        VoxelTerrainEdit, VoxelTerrainEditMask, VoxelTerrainEditMaskLayer, VoxelTerrainRegion,
    },
};
use strum::VariantArray;

use crate::{
    editing::{
        voxel_editing::{
            EditorEditingMaterial, EditorEditingTool, EditorEditingToolType,
            EditorVoxelEditingTarget,
        },
        voxel_editing_edit_tools::EditorVoxelEditingEditTools,
    },
    ui::{material_selection_dialog, pane::EditorUIPane},
};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct EditingPane {
    recent_materials: VecDeque<MaterialId>,
}

impl Default for EditingPane {
    fn default() -> Self {
        Self::new()
    }
}

impl EditingPane {
    pub fn new() -> Self {
        Self {
            recent_materials: VecDeque::new(),
        }
    }

    const RECENT_MATEIRAL_COUNT: usize = 10;
    pub fn push_recent_material(&mut self, material: MaterialId) {
        if let Some(pos) = self.recent_materials.iter().position(|m| *m == material) {
            self.recent_materials.remove(pos);
        }
        self.recent_materials.push_front(material);
        if self.recent_materials.len() > Self::RECENT_MATEIRAL_COUNT {
            self.recent_materials.pop_back();
        }
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
        ui.horizontal(|ui| {
            ui.label("Target lock:");
            ui.checkbox(&mut ctx.voxel_editing.target_lock, "");
        });
        if matches!(
            ctx.voxel_editing.edit_target,
            Some(EditorVoxelEditingTarget::Entity(_)),
        ) {
            ui.horizontal(|ui| {
                ui.label("Show bounds:");
                ui.checkbox(&mut ctx.voxel_editing.draw_entity_bounds, "");
            });
        }

        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Current Material:");
            if ui
                .add_enabled(
                    ctx.voxel_editing.editing_material != EditorEditingMaterial::Color,
                    egui::Button::new("Color"),
                )
                .clicked()
            {
                ctx.voxel_editing.editing_material = EditorEditingMaterial::Color;
            }
            if ui
                .add_enabled(
                    ctx.voxel_editing.editing_material != EditorEditingMaterial::Material,
                    egui::Button::new("Material"),
                )
                .clicked()
            {
                ctx.voxel_editing.editing_material = EditorEditingMaterial::Material;
            }
        });
        match ctx.voxel_editing.editing_material {
            EditorEditingMaterial::Color => {
                self.show_color_picker(ui, &mut ctx.voxel_editing.color);
            }
            EditorEditingMaterial::Material => {
                let selected_material = ctx.voxel_editing.material;
                ui.vertical(|ui| {
                    ui.label("Recent Materials:");
                    for material in &self.recent_materials {
                        let material_name = ctx.material_bank.id_to_name.get(material).unwrap();
                        if ui.button(material_name).clicked() {
                            ctx.voxel_editing.material = Some(*material);
                        }
                    }

                    ui.horizontal(|ui| {
                        ui.label("Selected Material:");
                        let material_name = selected_material
                            .and_then(|mat| ctx.material_bank.id_to_name.get(&mat))
                            .map(|s| s.as_str())
                            .unwrap_or("None");
                        if ui.button(material_name).clicked() {
                            ctx.commands
                                .push(material_selection_dialog::material_selection_dialog());
                        }
                    });
                });
            }
        }

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
        let current_voxel_material = ctx.voxel_editing.current_voxel_material().clone();
        let tool = ctx
            .voxel_editing
            .tools
            .get_mut(&ctx.voxel_editing.selected_tool_type)
            .unwrap();
        match tool {
            EditorEditingTool::Pencil {
                brush_size,
                air_place,
            } => {
                brush_size_ui(ui, brush_size);
                if ctx
                    .voxel_editing
                    .edit_target
                    .as_ref()
                    .and_then(|target| target.is_entity().then_some(()))
                    .is_some()
                {
                    ui.add_enabled_ui(ctx.voxel_editing.target_lock, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Air place:");
                            ui.checkbox(air_place, "");
                        })
                    });
                }

                if matches!(
                    ctx.voxel_editing.edit_target,
                    Some(EditorVoxelEditingTarget::Terrain)
                ) {
                    if ui.button("Fill at origin").clicked()
                        && let Some(voxel_material) = current_voxel_material
                    {
                        let origin_pos = Vector3::new(0, 0, 0);
                        let (brush_min, brush_max) =
                            EditorVoxelEditingEditTools::calculate_brush_min_max(
                                origin_pos,
                                *brush_size,
                            );
                        let edit = VoxelTerrainEdit {
                            region: VoxelTerrainRegion::new_rect(brush_min, brush_max),
                            mask: VoxelTerrainEditMask {
                                layers: vec![VoxelTerrainEditMaskLayer(
                                    VoxelModelEditMaskLayer::Sphere {
                                        center: origin_pos,
                                        diameter: *brush_size,
                                    },
                                )],
                            },
                            operator: rogue_engine::voxel::voxel::VoxelModelEditOperator::Replace(
                                Some(voxel_material),
                            ),
                        };
                        ctx.voxel_editing.apply_terrain_edit(
                            ctx.region_map,
                            ctx.voxel_registry,
                            edit,
                            true,
                        );
                    }
                }
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
