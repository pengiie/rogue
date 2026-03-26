use std::collections::{HashMap, VecDeque};

use nalgebra::{Isometry3, Vector3};
use rogue_engine::{
    common::color::{Color, ColorSpaceSrgb, ColorSrgba},
    consts,
    entity::{
        RenderableVoxelEntity,
        ecs_world::{ECSWorld, Entity},
    },
    event::Events,
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

use crate::session::EditorSession;

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

#[derive(Clone, Debug, PartialEq, Eq)]
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

pub enum InProgressSelection {
    Rect {
        start: Vector3<u32>,
        end: Vector3<u32>,
    },
}

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
    pub entity_state: HashMap<Entity, EditorVoxelEditingEntityState>,
    pub in_progress_selection: Option<InProgressSelection>,

    history: EditorVoxelEditingHistory,

    preview_model: Option<VoxelModelId>,
    preview_model_updated: bool,
    show_preview: bool,
    preview_model_transform: Transform,
}

pub struct EditorVoxelEditingEntityState {
    pub selection: Option<VoxelModelEditRegion>,
}

impl EditorVoxelEditingEntityState {
    pub fn new() -> Self {
        Self { selection: None }
    }
}

impl EditorVoxelEditing {
    pub fn new() -> Self {
        let mut tools = HashMap::new();
        for tool in EditorEditingTool::iter() {
            tools.insert(tool.discriminant(), tool);
        }
        Self {
            enabled: false,

            masks: Vec::new(),
            tools,
            selected_tool_type: EditorEditingToolType::Pencil,
            editing_material: EditorEditingMaterial::Color,
            color: ColorSrgba::new(1.0, 0.0, 1.0, 1.0),
            material: MaterialId::null(),

            edit_target: None,
            entity_state: HashMap::new(),
            in_progress_selection: None,

            history: EditorVoxelEditingHistory {
                undo_buffer: VecDeque::new(),
            },

            show_preview: false,
            preview_model_updated: false,
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

    //pub fn should_show_selection(&self) -> bool {
    //    self.selection.is_some()
    //}

    //pub fn did_selection_model_update(&self) -> bool {
    //    self.selection_model_updated
    //}

    //pub fn selection_model(&self) -> Option<VoxelModelId> {
    //    self.selection_model
    //}

    pub fn preview_model(&self) -> Option<VoxelModelId> {
        self.preview_model
    }

    pub fn preview_model_transform(&self) -> &Transform {
        &self.preview_model_transform
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

    pub fn update_color_picker(
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

    pub fn update_entity_selection(
        mut editing: ResMut<EditorVoxelEditing>,
        editor_session: Res<EditorSession>,
        input: Res<Input>,
        ecs_world: Res<ECSWorld>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        mut events: ResMut<Events>,
    ) {
        let editing = &mut *editing;
        let Some(EditorVoxelEditingTarget::Entity(entity)) = &editing.edit_target else {
            return;
        };

        let renderable = ecs_world
            .get::<&RenderableVoxelEntity>(*entity)
            .expect("Target entity should have a renderable model attached.");
        let model_id = renderable
            .voxel_model_id()
            .expect("Target entity should have a voxel model.");

        let entity_state = editing
            .entity_state
            .entry(*entity)
            .or_insert_with(|| EditorVoxelEditingEntityState::new());

        if input.is_key_pressed(Key::Delete)
            && let Some(region) = &entity_state.selection
        {
            // Delete/erase the contents of the selection.
            let edit = VoxelModelEdit {
                region: region.clone(),
                mask: VoxelModelEditMask { layers: Vec::new() },
                operator: VoxelModelEditOperator::Replace(None),
            };

            editing
                .history
                .undo_buffer
                .push_back(EditorVoxelEditingHistoryItem::ModelEdit {
                    model_id,
                    saved_model_state: voxel_registry
                        .get_model::<VoxelModelSFTCompressed>(model_id)
                        .clone(),
                });
            voxel_registry
                .get_dyn_model_mut(model_id)
                .set_voxel_range_impl(&edit);
            events.push(VoxelModelEvent::UpdatedModel(model_id))
        }

        let tool = editing.tools.get(&editing.selected_tool_type).unwrap();
        match tool {
            EditorEditingTool::Selection => {
                if let Some(raycast_hit) = &editor_session.entity_raycast {
                    assert_eq!(*entity, raycast_hit.entity);
                    let hit_pos = raycast_hit.model_trace.local_position;
                    if input.is_mouse_button_pressed(mouse::Button::Left) {
                        editing.in_progress_selection = Some(InProgressSelection::Rect {
                            start: hit_pos,
                            end: hit_pos,
                        });
                    }
                    if input.is_mouse_button_down(mouse::Button::Left) {
                        if let Some(InProgressSelection::Rect { start, end }) =
                            &mut editing.in_progress_selection
                        {
                            *end = hit_pos;
                        }
                    }
                }

                if input.is_mouse_button_released(mouse::Button::Left) {
                    if let Some(selection) = editing.in_progress_selection.take() {
                        match selection {
                            InProgressSelection::Rect { start, end } => {
                                if start == end {
                                    entity_state.selection = None;
                                    return;
                                } else {
                                    let min = start.zip_map(&end, |a, b| a.min(b));
                                    let max = start.zip_map(&end, |a, b| a.max(b));
                                    entity_state.selection =
                                        Some(VoxelModelEditRegion::Rect { min, max });
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub fn update_voxel_editing_systems(rb: &ResourceBank) {
        let mut editing = rb.get_resource_mut::<EditorVoxelEditing>();
        editing.show_preview = false;
        editing.preview_model_updated = false;

        if !editing.enabled {
            return;
        }
        drop(editing);

        rb.run_system(Self::update_editing_target);
        rb.run_system(Self::update_color_picker);
        rb.run_system(Self::update_undo_redo);
        rb.run_system(Self::update_entity_selection);
        rb.run_system(Self::update_voxel_editing_entity);
    }

    pub fn update_editing_target(
        mut editing: ResMut<EditorVoxelEditing>,
        editor_session: Res<EditorSession>,
    ) {
        if let Some(raycast_hit) = &editor_session.entity_raycast {
            editing.edit_target = Some(EditorVoxelEditingTarget::Entity(raycast_hit.entity));
        }
    }

    pub fn update_voxel_editing_entity(
        mut editing: ResMut<EditorVoxelEditing>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        ecs_world: Res<ECSWorld>,
        editor_session: Res<EditorSession>,
        mut events: ResMut<Events>,
        input: Res<Input>,
    ) {
        let editing = &mut *editing;
        let Some(EditorVoxelEditingTarget::Entity(entity)) = &editing.edit_target else {
            return;
        };
        let Some(raycast_hit) = &editor_session.entity_raycast else {
            return;
        };
        assert_eq!(*entity, raycast_hit.entity);

        // Selected entity should have a renderable and voxel model if we are editing.
        let Some((transform, renderable)) = ecs_world
            .query_one::<(&Transform, &RenderableVoxelEntity)>(*entity)
            .get()
        else {
            return;
        };
        let Some(entity_model_id) = renderable.voxel_model_id() else {
            return;
        };
        let mut hit_pos = raycast_hit.model_trace.local_position.cast::<i32>();
        let entity_model_side_length = voxel_registry.get_dyn_model_mut(entity_model_id).length();

        // Update preview.
        let tool = editing.tools.get(&editing.selected_tool_type).unwrap();
        if matches!(
            tool,
            EditorEditingTool::Pencil { .. }
                | EditorEditingTool::Eraser { .. }
                | EditorEditingTool::Paint { .. }
        ) {
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
            let center_pos = preview_model_side_length.map(|c| c / 2);
            if let Some(edit) = editing.create_voxel_entity_edit(
                center_pos,
                Vector3::zeros(),
                preview_model_side_length,
            ) {
                preview_model.set_voxel_range_impl(&edit);
                editing.preview_model_updated = true;
            }

            if let Some(preview_model_id) = editing.preview_model {
                editing.show_preview = true;
                let model_side_length = voxel_registry.get_dyn_model(preview_model_id).length();
                let mut preview_transform = Transform::new();
                if matches!(tool, EditorEditingTool::Pencil { .. }) {
                    hit_pos = hit_pos.zip_map(&raycast_hit.model_trace.local_normal, |c, n| c + n);
                }
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

        // Apply the edit.
        let tool = editing.tools.get(&editing.selected_tool_type).unwrap();
        let apply_edit = match tool {
            EditorEditingTool::Pencil { .. } | EditorEditingTool::Eraser { .. } => {
                input.is_mouse_button_pressed(mouse::Button::Left)
            }
            EditorEditingTool::Paint { brush_size } => {
                input.is_mouse_button_down(mouse::Button::Left)
            }
            _ => false,
        };

        if apply_edit
            && let Some(edit) = editing.create_voxel_entity_edit(
                raycast_hit.model_trace.local_position,
                raycast_hit.model_trace.local_normal,
                entity_model_side_length,
            )
        {
            editing
                .history
                .undo_buffer
                .push_back(EditorVoxelEditingHistoryItem::ModelEdit {
                    model_id: entity_model_id,
                    saved_model_state: voxel_registry
                        .get_model::<VoxelModelSFTCompressed>(entity_model_id)
                        .clone(),
                });
            voxel_registry
                .get_dyn_model_mut(entity_model_id)
                .set_voxel_range_impl(&edit);
            events.push(VoxelModelEvent::UpdatedModel(entity_model_id))
        }
    }

    pub fn create_voxel_entity_edit(
        &self,
        mut hit_pos: Vector3<u32>,
        hit_normal: Vector3<i32>,
        model_length: Vector3<u32>,
    ) -> Option<VoxelModelEdit> {
        let tool = self.tools.get(&self.selected_tool_type).unwrap();
        let entity_state = self
            .entity_state
            .get(&self.edit_target.as_ref().unwrap().target_entity())
            .unwrap();

        let mut total_region = entity_state.selection.clone();
        let mut mask = VoxelModelEditMask {
            layers: self.masks.clone(),
        };

        match tool {
            EditorEditingTool::Pencil { brush_size } | EditorEditingTool::Paint { brush_size } => {
                if tool.should_offset() {
                    hit_pos = hit_pos.zip_map(&hit_normal, |c, n| c.saturating_add_signed(n))
                }
                let br = brush_size / 2;
                let min = if brush_size % 2 == 0 {
                    hit_pos.map(|x| x.saturating_sub(br.saturating_sub(1)))
                } else {
                    hit_pos.map(|x| x.saturating_sub(br))
                };
                let max = (hit_pos + Vector3::new(br, br, br))
                    .zip_map(&model_length, |c, max| c.min(max - 1));
                total_region = total_region.map_or_else(
                    || Some(VoxelModelEditRegion::Rect { min, max }),
                    |existing_region| Some(existing_region.with_intersect_rect(min, max)),
                );
                mask.layers.push(VoxelModelEditMaskLayer::Sphere {
                    center: hit_pos.cast::<i32>(),
                    diameter: *brush_size,
                });
                if matches!(tool, EditorEditingTool::Paint { .. }) {
                    mask.layers.push(VoxelModelEditMaskLayer::Presence);
                }
            }
            EditorEditingTool::Eraser { brush_size } => {
                let br = brush_size / 2;
                let min = if brush_size % 2 == 0 {
                    hit_pos.map(|x| x.saturating_sub(br.saturating_sub(1)))
                } else {
                    hit_pos.map(|x| x.saturating_sub(br))
                };
                let max = (hit_pos + Vector3::new(br, br, br))
                    .zip_map(&model_length, |c, max| c.min(max - 1));
                let region = VoxelModelEditRegion::Rect { min, max };
                total_region = total_region.map_or_else(
                    || Some(VoxelModelEditRegion::Rect { min, max }),
                    |existing_region| Some(existing_region.with_intersect_rect(min, max)),
                );
                mask.layers.push(VoxelModelEditMaskLayer::Sphere {
                    center: hit_pos.cast::<i32>(),
                    diameter: *brush_size,
                });
            }
            _ => {
                return None;
            }
        }

        let operator = match tool {
            EditorEditingTool::Pencil { .. } => {
                VoxelModelEditOperator::Replace(Some(self.current_voxel_material()))
            }
            EditorEditingTool::Paint { .. } => {
                VoxelModelEditOperator::Replace(Some(self.current_voxel_material()))
            }
            EditorEditingTool::Eraser { .. } => VoxelModelEditOperator::Replace(None),
            _ => unreachable!(),
        };

        return total_region.map(|region| VoxelModelEdit {
            region,
            mask,
            operator,
        });
    }

    fn current_voxel_material(&self) -> VoxelMaterialData {
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

//#[derive(Clone, Debug, PartialEq, Eq)]
//pub struct EditorBrush {
//    pub brush_type: EditorBrushType,
//    pub brush_size: u32,
//    pub brush_material: EditorBrushMaterial,
//}
//
//impl EditorBrush {
//    pub fn new() -> Self {
//        Self {
//            brush_type: EditorBrushType::Sphere,
//            brush_size: 1,
//            brush_material: EditorBrushMaterial::Color,
//        }
//    }
//
//    pub fn create_voxel_entity_edit(
//        &self,
//        hit_pos: Vector3<i32>,
//        side_length: Vector3<u32>,
//        color: &Color<ColorSpaceSrgb>,
//        material: &MaterialId,
//    ) -> VoxelModelEdit {
//        let bz = self.brush_size as i32;
//        let brush_size = Vector3::new(bz, bz, bz);
//
//        let min = if bz % 2 == 0 {
//            (hit_pos - brush_size + Vector3::new(1, 1, 1)).map(|c| c.max(0) as u32)
//        } else {
//            (hit_pos - brush_size).map(|c| c.max(0) as u32)
//        };
//        let max =
//            (hit_pos + brush_size).zip_map(&side_length, |c, max| c.min(max as i32 - 1) as u32);
//
//        let voxel_data = match self.brush_type {
//            EditorBrushType::Erase => VoxelEditData::Sphere {
//                material: None,
//                center: hit_pos.cast::<f32>(),
//                radius: bz as u32,
//            },
//            EditorBrushType::Sphere => VoxelEditData::Sphere {
//                material: Some(VoxelMaterialData::Baked { color: *color }),
//                center: hit_pos.cast::<f32>(),
//                radius: bz as u32,
//            },
//        };
//        VoxelModelEdit {
//            min,
//            max,
//            data: voxel_data,
//        }
//    }
//}
