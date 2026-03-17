use nalgebra::{Isometry3, Vector3};
use rogue_engine::{
    common::color::{Color, ColorSpaceSrgb},
    consts,
    entity::{RenderableVoxelEntity, ecs_world::ECSWorld},
    event::Events,
    input::{Input, mouse},
    material::MaterialId,
    physics::transform::Transform,
    resource::{Res, ResMut},
    voxel::{
        attachment::Attachment,
        sft_compressed::VoxelModelSFTCompressed,
        voxel::{VoxelEditData, VoxelMaterialData, VoxelModelEdit, VoxelModelImpl},
        voxel_registry::{VoxelModelEvent, VoxelModelId, VoxelModelRegistry},
    },
};
use rogue_macros::Resource;

use crate::session::EditorSession;

struct VoxelSelection {
    min: Vector3<u32>,
    max: Vector3<u32>,
}

#[derive(Resource)]
pub struct EditorVoxelEditing {
    pub brush: EditorBrush,
    pub color: Color<ColorSpaceSrgb>,
    pub material: MaterialId,
    pub enabled: bool,

    selection: Option<VoxelSelection>,
    selection_model: Option<VoxelModelId>,
    selection_model_updated: bool,

    preview_brush: Option<EditorBrush>,
    preview_model: Option<VoxelModelId>,
    preview_model_updated: bool,
    show_preview: bool,
    preview_model_transform: Transform,
}

impl EditorVoxelEditing {
    pub fn new() -> Self {
        Self {
            brush: EditorBrush::new(),
            enabled: false,
            material: MaterialId::null(),
            color: Color::new_srgb(1.0, 0.0, 1.0),

            selection: None,
            selection_model: None,
            selection_model_updated: false,

            show_preview: false,
            preview_model_updated: false,
            preview_brush: None,
            preview_model: None,
            preview_model_transform: Transform::new(),
        }
    }

    pub fn should_show_preview(&self) -> bool {
        self.show_preview
    }

    pub fn did_preview_model_update(&self) -> bool {
        self.preview_model_updated
    }

    pub fn should_show_selection(&self) -> bool {
        self.selection.is_some()
    }

    pub fn did_selection_model_update(&self) -> bool {
        self.selection_model_updated
    }

    pub fn selection_model(&self) -> Option<VoxelModelId> {
        self.selection_model
    }

    pub fn preview_model(&self) -> Option<VoxelModelId> {
        self.preview_model
    }

    pub fn preview_model_transform(&self) -> &Transform {
        &self.preview_model_transform
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn update_voxel_editing_entity(
        mut editing: ResMut<EditorVoxelEditing>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        ecs_world: Res<ECSWorld>,
        editor_session: Res<EditorSession>,
        mut events: ResMut<Events>,
        input: Res<Input>,
    ) {
        editing.show_preview = false;
        editing.preview_model_updated = false;

        if !editing.enabled {
            return;
        }

        // Only edit the selected entity and only if we are actually pointing at it.
        let Some(selected_entity) = editor_session.selected_entity else {
            return;
        };
        let Some(raycast_hit) = &editor_session.entity_raycast else {
            return;
        };
        if raycast_hit.entity != selected_entity {
            return;
        }

        // Selected entity should have a renderable and voxel model if we are editing.
        let Some((transform, renderable)) = ecs_world
            .query_one::<(&Transform, &RenderableVoxelEntity)>(selected_entity)
            .get()
        else {
            return;
        };
        let Some(entity_model_id) = renderable.voxel_model_id() else {
            return;
        };
        let hit_pos = raycast_hit.model_trace.local_position.cast::<i32>();
        let entity_model_side_length = voxel_registry.get_dyn_model_mut(entity_model_id).length();

        // Update preview.
        {
            if editing.preview_brush.as_ref() != Some(&editing.brush) {
                editing.preview_brush = Some(editing.brush.clone());

                if editing.preview_model.is_none() {
                    let mut sft_compressed_model = VoxelModelSFTCompressed::new_empty(4096);
                    sft_compressed_model.initialize_attachment_buffers(&Attachment::BMAT);
                    editing.preview_model =
                        Some(voxel_registry.register_voxel_model(sft_compressed_model, None));
                }
                let mut preview_model = voxel_registry
                    .get_model_mut::<VoxelModelSFTCompressed>(editing.preview_model.unwrap());
                preview_model.clear();

                let preview_model_side_length = preview_model.length();
                let center_pos = preview_model_side_length.map(|c| c as i32 / 2);
                let mut edit = editing.brush.create_voxel_entity_edit(
                    center_pos,
                    preview_model_side_length,
                    &editing.color,
                    &editing.material,
                );
                if editing.brush.brush_type == EditorBrushType::Erase {
                    match &mut edit.data {
                        VoxelEditData::Sphere {
                            material,
                            center,
                            radius,
                        } => {
                            *material = Some(VoxelMaterialData::Baked {
                                color: Color::new_srgb_hex("#FF2222"),
                            });
                        }
                        _ => {}
                    }
                }
                preview_model.set_voxel_range_impl(&edit);
                editing.preview_model_updated = true;
            }

            if let Some(preview_model_id) = editing.preview_model {
                editing.show_preview = true;
                let model_side_length = voxel_registry.get_dyn_model(preview_model_id).length();
                let mut preview_transform = Transform::new();
                let edit_preview_offset = transform.rotation
                    * ((hit_pos.cast::<f32>() - entity_model_side_length.cast::<f32>() * 0.5)
                        .component_mul(&transform.scale)
                        * consts::voxel::VOXEL_METER_LENGTH);
                preview_transform.position = transform.position + edit_preview_offset;
                preview_transform.rotation = transform.rotation;
                preview_transform.scale = transform.scale;
                editing.preview_model_transform = preview_transform;
            }
        }

        // Needs left click to be pressed.
        if !input.is_mouse_button_pressed(mouse::Button::Left) {
            return;
        }

        let edit = editing.brush.create_voxel_entity_edit(
            hit_pos,
            entity_model_side_length,
            &editing.color,
            &editing.material,
        );
        voxel_registry
            .get_dyn_model_mut(entity_model_id)
            .set_voxel_range_impl(&edit);
        events.push(VoxelModelEvent::UpdatedModel(entity_model_id))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorBrushMaterial {
    Color,
    Material,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorBrush {
    pub brush_type: EditorBrushType,
    pub brush_size: u32,
    pub brush_material: EditorBrushMaterial,
}

impl EditorBrush {
    pub fn new() -> Self {
        Self {
            brush_type: EditorBrushType::Sphere,
            brush_size: 1,
            brush_material: EditorBrushMaterial::Color,
        }
    }

    pub fn create_voxel_entity_edit(
        &self,
        hit_pos: Vector3<i32>,
        side_length: Vector3<u32>,
        color: &Color<ColorSpaceSrgb>,
        material: &MaterialId,
    ) -> VoxelModelEdit {
        let bz = self.brush_size.saturating_sub(1) as i32;
        let brush_size = Vector3::new(bz, bz, bz);
        let min = (hit_pos - brush_size).map(|c| c.max(0) as u32);
        let max =
            (hit_pos + brush_size).zip_map(&side_length, |c, max| c.min(max as i32 - 1) as u32);

        let voxel_data = match self.brush_type {
            EditorBrushType::Erase => VoxelEditData::Sphere {
                material: None,
                center: hit_pos,
                radius: bz as u32,
            },
            EditorBrushType::Sphere => VoxelEditData::Sphere {
                material: Some(VoxelMaterialData::Baked { color: *color }),
                center: hit_pos,
                radius: bz as u32,
            },
        };
        VoxelModelEdit {
            min,
            max,
            data: voxel_data,
        }
    }
}

#[derive(strum_macros::VariantArray, strum_macros::Display, Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorBrushType {
    /// Sphere but erases.
    Erase,
    Sphere,
}
