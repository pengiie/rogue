use std::collections::{HashMap, VecDeque};

use nalgebra::{Isometry3, UnitQuaternion, Vector2, Vector3};
use rogue_engine::{
    common::{
        color::{Color, ColorSpaceSrgb, ColorSrgba},
        geometry::ray::Ray,
    },
    consts,
    debug::debug_renderer::{DebugRenderer, DebugShapeFlags},
    entity::{
        RenderableVoxelEntity,
        ecs_world::{ECSWorld, Entity},
    },
    event::{EventReader, Events},
    input::{
        Input,
        keyboard::{self, Key},
        mouse,
    },
    material::MaterialId,
    physics::transform::Transform,
    resource::{Res, ResMut, ResourceBank},
    voxel::{
        attachment::Attachment,
        sft_compressed::VoxelModelSFTCompressed,
        voxel::{
            VoxelMaterialData, VoxelModelEdit, VoxelModelEditMask, VoxelModelEditMaskLayer,
            VoxelModelEditOperator, VoxelModelEditRegion, VoxelModelImpl,
        },
        voxel_registry::{VoxelModelEvent, VoxelModelId, VoxelModelRegistry},
    },
};
use rogue_macros::Resource;
use strum::{IntoDiscriminant, IntoEnumIterator, VariantArray};

use crate::{
    editing::{
        voxel_editing_color_picker::EditorVoxelEditingColorPicker,
        voxel_editing_edit_tools::EditorVoxelEditingEditTools,
        voxel_editing_preview::EditorVoxelEditingPreview,
        voxel_editing_selection::EditorVoxelEditingSelections,
        voxel_editing_selections_gpu::EditorVoxelEditingSelectionsGpu,
    },
    session::{EditorEvent, EditorSession},
};

struct EditorVoxelSelection {
    min: Vector3<u32>,
    max: Vector3<u32>,
}

#[derive(strum_macros::EnumDiscriminants, strum_macros::EnumIter, PartialEq, Eq, Clone, Debug)]
#[strum_discriminants(name(EditorEditingToolType))]
#[strum_discriminants(derive(
    strum_macros::VariantArray,
    strum_macros::Display,
    strum_macros::EnumIter,
    Hash
))]
pub enum EditorEditingTool {
    Selection,
    /// Fills voxels with
    Pencil {
        brush_size: u32,
        /// If true will place on the bounding box.
        air_place: bool,
    },
    Paint {
        brush_size: u32,
    },
    Eraser {
        brush_size: u32,
    },
    ColorPicker,
}

impl EditorEditingTool {
    pub fn should_offset(&self) -> bool {
        match self {
            EditorEditingTool::Pencil { .. } => true,
            _ => false,
        }
    }
}

pub enum EditorVoxelEditingHistoryItem {
    ModelEdit {
        model_id: VoxelModelId,
        saved_model_state: VoxelModelSFTCompressed,
    },
}

pub struct EditorVoxelEditingHistory {
    pub undo_buffer: VecDeque<EditorVoxelEditingHistoryItem>,
}

#[derive(Clone, Debug, PartialEq, Eq, strum_macros::EnumIs)]
pub enum EditorVoxelEditingTarget {
    Entity(Entity),
    Terrain,
}

impl EditorVoxelEditingTarget {
    pub fn target_entity(&self) -> Entity {
        match self {
            EditorVoxelEditingTarget::Entity(entity) => *entity,
            _ => panic!("Target is expected to be an entity."),
        }
    }
}

pub struct VoxelEditingSelection {}

#[derive(Resource)]
pub struct EditorVoxelEditing {
    pub enabled: bool,

    pub masks: Vec<VoxelModelEditMaskLayer>,
    pub tools: HashMap<EditorEditingToolType, EditorEditingTool>,
    pub selected_tool_type: EditorEditingToolType,
    pub editing_material: EditorEditingMaterial,
    pub color: ColorSrgba,
    pub material: MaterialId,

    pub edit_target: Option<EditorVoxelEditingTarget>,
    /// True if can't change the edit target.
    pub target_lock: bool,
    pub draw_entity_bounds: bool,
    pub is_click_consumed: bool,

    history: EditorVoxelEditingHistory,

    editor_event_reader: EventReader<EditorEvent>,
}

impl EditorVoxelEditing {
    pub fn new() -> Self {
        let mut tools = HashMap::new();
        for tool in EditorEditingTool::iter() {
            tools.insert(tool.discriminant(), tool);
        }
        Self {
            enabled: false,
            is_click_consumed: false,

            masks: Vec::new(),
            tools,
            selected_tool_type: EditorEditingToolType::Pencil,
            editing_material: EditorEditingMaterial::Color,
            color: ColorSrgba::new(1.0, 0.0, 1.0, 1.0),
            material: MaterialId::null(),
            draw_entity_bounds: false,

            edit_target: None,
            target_lock: false,

            history: EditorVoxelEditingHistory {
                undo_buffer: VecDeque::new(),
            },

            editor_event_reader: EventReader::new(),
        }
    }

    pub fn current_tool(&self) -> &EditorEditingTool {
        self.tools.get(&self.selected_tool_type).unwrap()
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn update_undo_redo(
        mut editing: ResMut<EditorVoxelEditing>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        input: Res<Input>,
        mut events: ResMut<Events>,
    ) {
        // In the case of large models this matters cause of memory, we could move this to the disk
        // though which would help but this is fine for now.
        const UNDO_STACK_SIZE: usize = 50;
        while editing.history.undo_buffer.len() > UNDO_STACK_SIZE {
            editing.history.undo_buffer.pop_front();
        }

        if input.is_key_pressed_with_modifiers(keyboard::Key::Z, &[keyboard::Modifier::Control]) {
            if let Some(item) = editing.history.undo_buffer.pop_back() {
                match item {
                    EditorVoxelEditingHistoryItem::ModelEdit {
                        model_id,
                        saved_model_state,
                    } => {
                        let mut sft =
                            voxel_registry.get_model_mut::<VoxelModelSFTCompressed>(model_id);
                        *sft = saved_model_state;
                        events.push(VoxelModelEvent::UpdatedModel(model_id));
                    }
                }
            }
        }
    }

    pub fn on_update_voxel_editing_systems(rb: &ResourceBank) {
        // Always keep the target up to date.
        rb.run_system(Self::update_editing_target);
        // Reset show_preview at the start of each frame.
        rb.get_resource_mut::<EditorVoxelEditingPreview>()
            .show_preview = false;

        let mut editing = rb.get_resource_mut::<EditorVoxelEditing>();
        let can_edit = editing.enabled
            && rb
                .get_resource::<EditorSession>()
                .is_editor_camera_focused();
        if !can_edit {
            return;
        }
        drop(editing);

        rb.run_system(Self::update_undo_redo);
        rb.run_system(EditorVoxelEditingColorPicker::try_update_color_picker_tool);
        rb.run_system(EditorVoxelEditingSelections::update_selection_systems);
        rb.run_system(EditorVoxelEditingEditTools::update_edit_application_systems);
        rb.run_system(EditorVoxelEditingPreview::update_preview_systems);

        // Gpu related.
        rb.run_system(EditorVoxelEditingSelectionsGpu::update_selections_preview_gpu);
    }

    pub fn update_editing_target(
        mut editing: ResMut<EditorVoxelEditing>,
        editor_session: Res<EditorSession>,
        input: Res<Input>,
        ecs_world: Res<ECSWorld>,
        events: Res<Events>,
    ) {
        let editing = &mut *editing;
        editing.is_click_consumed = false;
        if editing.target_lock {
            return;
        }

        let entity_t = editor_session
            .entity_raycast
            .as_ref()
            .map_or(100000.0, |hit| hit.model_trace.depth_t);
        if input.is_mouse_button_pressed(mouse::Button::Left)
            && let Some(terrain_hit) = &editor_session.terrain_raycast
            && terrain_hit.model_trace.depth_t < entity_t
        {
            editing.edit_target = Some(EditorVoxelEditingTarget::Terrain);
            editing.is_click_consumed = true;
        }

        for event in editing.editor_event_reader.read(&events) {
            let EditorEvent::SelectedEntity(Some(entity)) = event else {
                continue;
            };
            let Ok(renderable) = ecs_world.get::<&RenderableVoxelEntity>(*entity) else {
                return;
            };
            if renderable.voxel_model_id().is_some() {
                editing.edit_target = Some(EditorVoxelEditingTarget::Entity(*entity));
                editing.is_click_consumed = true;
            }
        }
    }

    //pub fn update_voxel_editing_entity(
    //    mut editing: ResMut<EditorVoxelEditing>,
    //    mut voxel_registry: ResMut<VoxelModelRegistry>,
    //    ecs_world: Res<ECSWorld>,
    //    editor_session: Res<EditorSession>,
    //    mut events: ResMut<Events>,
    //    input: Res<Input>,
    //) {
    //    let editing = &mut *editing;
    //    let Some(EditorVoxelEditingTarget::Entity(target_entity)) = &editing.edit_target else {
    //        return;
    //    };

    //    // Selected entity should have a renderable and voxel model if we are editing.
    //    let Some((transform, renderable)) = ecs_world
    //        .query_one::<(&Transform, &RenderableVoxelEntity)>(*target_entity)
    //        .get()
    //    else {
    //        return;
    //    };
    //    let world_transform = ecs_world.get_world_transform(*target_entity, transform);
    //    let Some(entity_model_id) = renderable.voxel_model_id() else {
    //        return;
    //    };
    //    if !renderable.is_dynamic() {
    //        return;
    //    }

    //    let tool = editing.tools.get(&editing.selected_tool_type).unwrap();
    //    let can_airplace = matches!(
    //        tool,
    //        EditorEditingTool::Pencil {
    //            air_place: true,
    //            ..
    //        }
    //    );

    //    let entity_model_side_length = voxel_registry.get_dyn_model_mut(entity_model_id).length();
    //    let model_obb = world_transform.as_voxel_model_obb(entity_model_side_length);
    //    let ray = &editor_session.editor_camera_ray;
    //    let valid_raycast_hit = &editor_session
    //        .entity_raycast
    //        .as_ref()
    //        .filter(|hit| hit.entity == *target_entity);
    //    let (mut hit_pos, hit_normal) = if let Some(raycast_hit) = valid_raycast_hit {
    //        (
    //            raycast_hit.model_trace.local_position,
    //            raycast_hit.model_trace.local_normal,
    //        )
    //    } else if can_airplace && let Some(hit_info) = ray.intersect_obb(&model_obb) {
    //        let center = world_transform.position;
    //        let inv_rot = model_obb.rotation.inverse();
    //        let rotated_ray_pos = inv_rot.transform_vector(&(ray.origin - center)) + center;
    //        let rotated_ray_dir = inv_rot.transform_vector(&ray.dir);
    //        let exit_pos = rotated_ray_pos + rotated_ray_dir * hit_info.t_exit;
    //        let norm_pos = (exit_pos - model_obb.aabb.min)
    //            .component_div(&model_obb.aabb.side_length())
    //            .map(|x| x.clamp(0.0, 1.0));
    //        let exit_voxel = norm_pos
    //            .component_mul(&entity_model_side_length.cast::<f32>())
    //            .map(|x| x.floor() as u32);
    //        (exit_voxel, Vector3::new(0, 0, 0))
    //    } else {
    //        return;
    //    };

    //    // Apply the edit.
    //    let tool = editing.tools.get(&editing.selected_tool_type).unwrap();
    //    let apply_edit = match tool {
    //        EditorEditingTool::Pencil { .. } | EditorEditingTool::Eraser { .. } => {
    //            input.is_mouse_button_pressed(mouse::Button::Left)
    //        }
    //        EditorEditingTool::Paint { brush_size } => {
    //            input.is_mouse_button_down(mouse::Button::Left)
    //        }
    //        _ => false,
    //    };
    //}

    pub fn apply_edit<'a>(
        &mut self,
        voxel_registry: &'a mut VoxelModelRegistry,
        events: &mut Events,
        edit: VoxelModelEdit<'a>,
        model_id: VoxelModelId,
        save_history: bool,
    ) {
        if save_history {
            self.history
                .undo_buffer
                .push_back(EditorVoxelEditingHistoryItem::ModelEdit {
                    model_id,
                    saved_model_state: voxel_registry
                        .get_model::<VoxelModelSFTCompressed>(model_id)
                        .clone(),
                });
        }
        voxel_registry
            .get_dyn_model_mut(model_id)
            .set_voxel_range_impl(&edit);
        events.push(VoxelModelEvent::UpdatedModel(model_id))
    }

    pub fn current_voxel_material(&self) -> VoxelMaterialData {
        match self.editing_material {
            EditorEditingMaterial::Color => VoxelMaterialData::Baked { color: self.color },
            EditorEditingMaterial::Material => VoxelMaterialData::Unbaked(self.material.index()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorEditingMaterial {
    Color,
    Material,
}
