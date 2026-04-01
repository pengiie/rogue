use rogue_engine::{
    entity::ecs_world::ECSWorld,
    input::{Input, mouse},
    resource::{Res, ResMut},
    voxel::{
        sft_compressed::VoxelModelSFTCompressed, voxel::VoxelMaterialData,
        voxel_registry::VoxelModelRegistry,
    },
};
use rogue_macros::Resource;

use crate::{
    editing::voxel_editing::{EditorEditingTool, EditorVoxelEditing, EditorVoxelEditingTarget},
    session::EditorSession,
};

pub struct EditorVoxelEditingColorPicker;

impl EditorVoxelEditingColorPicker {
    pub fn try_update_color_picker_tool(
        mut editing: ResMut<EditorVoxelEditing>,
        input: Res<Input>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        editor_session: Res<EditorSession>,
        ecs_world: Res<ECSWorld>,
    ) {
        let tool = editing.tools.get(&editing.selected_tool_type).unwrap();
        if tool != &EditorEditingTool::ColorPicker {
            return;
        }

        if !input.is_mouse_button_pressed(mouse::Button::Left) {
            return;
        }

        match &editing.edit_target {
            Some(EditorVoxelEditingTarget::Entity(entity)) => {
                let Some(raycast) = editor_session.entity_raycast() else {
                    return;
                };
                assert_eq!(*entity, raycast.entity);
                let model = voxel_registry.get_model::<VoxelModelSFTCompressed>(raycast.model_id);
                match model.get_voxel(raycast.model_trace.local_position) {
                    Some(VoxelMaterialData::Baked { color }) => {
                        editing.color = color;
                    }
                    Some(VoxelMaterialData::Unbaked(material_id)) => {}
                    None => unreachable!(),
                }
            }
            _ => {}
        }
    }
}
