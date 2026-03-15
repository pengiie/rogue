use nalgebra::Vector3;
use rogue_engine::{
    common::color::{Color, ColorSpaceSrgb},
    entity::{RenderableVoxelEntity, ecs_world::ECSWorld},
    event::Events,
    input::{Input, mouse},
    resource::{Res, ResMut},
    voxel::{
        voxel::{VoxelEditData, VoxelMaterialData, VoxelModelEdit},
        voxel_registry::{VoxelModelEvent, VoxelModelRegistry},
    },
};
use rogue_macros::Resource;

use crate::session::EditorSession;

#[derive(strum_macros::VariantArray, strum_macros::Display, Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorBrush {
    Erase,
    Fill,
}

#[derive(Resource)]
pub struct EditorVoxelEditing {
    pub color: Color<ColorSpaceSrgb>,
    pub brush: EditorBrush,
    pub brush_size: u32,
    pub enabled: bool,
}

impl EditorVoxelEditing {
    pub fn new() -> Self {
        Self {
            color: Color::new_srgb(1.0, 0.0, 1.0),
            brush: EditorBrush::Fill,
            brush_size: 1,
            enabled: false,
        }
    }

    pub fn update_voxel_editing_entity(
        editing: ResMut<EditorVoxelEditing>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        ecs_world: Res<ECSWorld>,
        editor_session: Res<EditorSession>,
        mut events: ResMut<Events>,
        input: Res<Input>,
    ) {
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
        let Ok(renderable) = ecs_world.get::<&RenderableVoxelEntity>(selected_entity) else {
            return;
        };
        let Some(model_id) = renderable.voxel_model_id() else {
            return;
        };

        // Needs left click to be held down.
        if !input.is_mouse_button_down(mouse::Button::Left) {
            return;
        }

        let mut model = voxel_registry.get_dyn_model_mut(model_id);
        let side_length = model.length();

        let hit_pos = raycast_hit.model_trace.local_position.cast::<i32>();
        let brush_size = Vector3::new(
            editing.brush_size as i32,
            editing.brush_size as i32,
            editing.brush_size as i32,
        );
        let min = (hit_pos - brush_size).map(|c| c.max(0) as u32);
        let max =
            (hit_pos + brush_size).zip_map(&side_length, |c, max| c.min(max as i32 - 1) as u32);

        let color_mat = VoxelMaterialData::Baked {
            color: editing.color,
        };
        let material = match editing.brush {
            EditorBrush::Erase => None,
            EditorBrush::Fill => Some(color_mat),
        };

        model.set_voxel_range_impl(&VoxelModelEdit {
            min,
            max,
            data: VoxelEditData::Sphere {
                material,
                center: hit_pos,
                radius: editing.brush_size,
            },
        });
        events.push(VoxelModelEvent::UpdatedModel(model_id))
    }
}
