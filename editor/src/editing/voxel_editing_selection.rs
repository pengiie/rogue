use nalgebra::Vector3;
use rogue_engine::{
    debug::debug_renderer::DebugRenderer,
    entity::{RenderableVoxelEntity, ecs_world::ECSWorld},
    event::Events,
    input::{self, Input, keyboard::Key, mouse},
    physics::transform::Transform,
    resource::{Res, ResMut, ResourceBank},
    voxel::{
        voxel::{VoxelModelEdit, VoxelModelEditRegion},
        voxel_registry::{self, VoxelModelRegistry},
    },
};
use rogue_macros::Resource;

use crate::{
    editing::voxel_editing::{
        self, EditorEditingToolType, EditorVoxelEditing, EditorVoxelEditingTarget,
    },
    session::EditorSession,
};

pub struct VoxelEditingInProgressSelection {
    start: Vector3<i32>,
    end: Vector3<i32>,
}

impl VoxelEditingInProgressSelection {
    pub fn min_max(&self) -> (Vector3<i32>, Vector3<i32>) {
        let min = self.start.zip_map(&self.end, |x, y| x.min(y));
        let max = self.start.zip_map(&self.end, |x, y| x.max(y));
        (min, max)
    }

    pub fn min_max_saturated(
        &self,
        model_side_length: Vector3<u32>,
    ) -> (Vector3<i32>, Vector3<i32>) {
        let (min, max) = self.min_max();
        let min = min.zip_map(&model_side_length.cast::<i32>(), |x, y| x.clamp(0, y - 1));
        let max = max.zip_map(&model_side_length.cast::<i32>(), |x, y| x.clamp(0, y - 1));
        (min, max)
    }
}

pub struct EditorVoxelEditingSelection {
    pub min: Vector3<i32>,
    pub max: Vector3<i32>,
}

impl EditorVoxelEditingSelection {
    pub fn as_model_edit_region(&self) -> VoxelModelEditRegion {
        assert!(self.min.iter().all(|x| *x >= 0));
        assert!(self.max.iter().all(|x| *x >= 0));
        VoxelModelEditRegion::Rect {
            min: self.min.map(|x| x as u32),
            max: self.max.map(|x| x as u32),
        }
    }
}

/// Selections are only applicable to the current editing target, if the target is new, the
/// selections are wiped.
#[derive(Resource)]
pub struct EditorVoxelEditingSelections {
    pub in_progress_selection: Option<VoxelEditingInProgressSelection>,
    pub selection: Option<EditorVoxelEditingSelection>,
    pub selection_target: Option<EditorVoxelEditingTarget>,
}

impl EditorVoxelEditingSelections {
    pub fn new() -> Self {
        Self {
            in_progress_selection: None,
            selection: None,
            selection_target: None,
        }
    }

    pub fn update_selection_systems(rb: &ResourceBank) {
        rb.run_system(Self::update_selection_target);

        if rb.get_resource::<EditorVoxelEditing>().selected_tool_type
            != EditorEditingToolType::Selection
        {
            let mut selections = rb.get_resource_mut::<EditorVoxelEditingSelections>();
            selections.in_progress_selection = None;
            return;
        }
        rb.run_system(Self::update_in_progress_selection);
        rb.run_system(Self::update_selection_scale_handles);
        rb.run_system(Self::update_kb_delete_and_f);
    }

    pub fn clear_selections(&mut self) {
        self.in_progress_selection = None;
        self.selection = None;
    }

    pub fn update_selection_target(
        mut editing_selection: ResMut<EditorVoxelEditingSelections>,
        editing: Res<EditorVoxelEditing>,
    ) {
        if editing_selection.selection_target != editing.edit_target {
            editing_selection.selection_target = editing.edit_target.clone();
            editing_selection.clear_selections();
        }
    }

    pub fn update_in_progress_selection(
        editor_session: Res<EditorSession>,
        mut editing_selection: ResMut<EditorVoxelEditingSelections>,
        editing: Res<EditorVoxelEditing>,
        input: Res<Input>,
    ) {
        let hit_pos = match &editing.edit_target {
            Some(EditorVoxelEditingTarget::Entity(target_entity)) => {
                // We can select any non-target.
                if let Some(raycast_hit) = &editor_session.entity_raycast
                    && raycast_hit.entity == *target_entity
                {
                    raycast_hit.model_trace.local_position.cast::<i32>()
                } else {
                    return;
                }
            }
            Some(EditorVoxelEditingTarget::Terrain) => {
                let Some(raycast_hit) = &editor_session.terrain_raycast else {
                    return;
                };
                raycast_hit.world_voxel_pos
            }
            None => {
                return;
            }
        };

        if input.is_mouse_button_pressed(mouse::Button::Left) {
            editing_selection.in_progress_selection = Some(VoxelEditingInProgressSelection {
                start: hit_pos,
                end: hit_pos,
            });
        }

        // If we have a selection active and the user is holding down left click over
        // the target model.
        if input.is_mouse_button_down(mouse::Button::Left)
            && let Some(VoxelEditingInProgressSelection { start, end }) =
                &mut editing_selection.in_progress_selection
        {
            *end = hit_pos;
        }

        // Update the in progress selection
        if input.is_mouse_button_released(mouse::Button::Left) {
            if let Some(in_progress_selection) = editing_selection.in_progress_selection.take() {
                if in_progress_selection.start != in_progress_selection.end {
                    let (min, max) = in_progress_selection.min_max();
                    editing_selection.selection = Some(EditorVoxelEditingSelection { min, max });
                } else {
                    editing_selection.selection = None;
                }
            }
        }
    }

    /// F is for fill, delete also fills, so they share this function.
    pub fn update_kb_delete_and_f(
        mut editing: ResMut<EditorVoxelEditing>,
        mut editing_selection: ResMut<EditorVoxelEditingSelections>,
        editor_session: Res<EditorSession>,
        mut debug_renderer: ResMut<DebugRenderer>,
        input: Res<Input>,
        ecs_world: Res<ECSWorld>,
        mut voxel_registry: ResMut<VoxelModelRegistry>,
        mut events: ResMut<Events>,
    ) {
        if !(input.is_key_pressed(Key::Delete) || input.is_key_pressed(Key::F)) {
            return;
        }
        let Some(selection) = &editing_selection.selection else {
            return;
        };

        let fill_material = if input.is_key_pressed(Key::Delete) {
            None
        } else if input.is_key_pressed(Key::F) {
            let Some(voxel_material) = editing.current_voxel_material() else {
                return;
            };
            Some(voxel_material)
        } else {
            unreachable!()
        };

        match &editing.edit_target {
            Some(EditorVoxelEditingTarget::Entity(target_entity)) => {
                let renderable = ecs_world
                    .get::<&RenderableVoxelEntity>(*target_entity)
                    .expect("Target entity should have a renderable model attached.");
                if !renderable.is_dynamic() {
                    return;
                }
                let entity_model_id = renderable
                    .voxel_model_id()
                    .expect("Target entity should have a voxel model");
                let entity_model_side_length =
                    voxel_registry.get_dyn_model(entity_model_id).length();

                let edit = VoxelModelEdit {
                    region: selection.as_model_edit_region(),
                    mask: rogue_engine::voxel::voxel::VoxelModelEditMask {
                        layers: Vec::new(),
                        mask_source: None,
                    },
                    operator: rogue_engine::voxel::voxel::VoxelModelEditOperator::Replace(
                        fill_material,
                    ),
                };
                editing.apply_entity_edit(
                    &mut voxel_registry,
                    &mut events,
                    edit,
                    entity_model_id,
                    true,
                );
            }
            Some(EditorVoxelEditingTarget::Terrain) => {}
            None => {
                return;
            }
        }
    }

    pub fn update_selection_scale_handles(
        mut editing: ResMut<EditorVoxelEditing>,
        mut editing_selection: ResMut<EditorVoxelEditingSelections>,
    ) {
    }

    //pub fn update_entity_selection(
    //    mut editing: ResMut<EditorVoxelEditing>,
    //    editor_session: Res<EditorSession>,
    //    mut debug_renderer: ResMut<DebugRenderer>,
    //    input: Res<Input>,
    //    ecs_world: Res<ECSWorld>,
    //    mut voxel_registry: ResMut<VoxelModelRegistry>,
    //    mut events: ResMut<Events>,
    //) {
    //    let editing = &mut *editing;
    //    let Some(EditorVoxelEditingTarget::Entity(entity)) = &editing.edit_target else {
    //        return;
    //    };

    //    let renderable = ecs_world
    //        .get::<&RenderableVoxelEntity>(*entity)
    //        .expect("Target entity should have a renderable model attached.");
    //    let entity_transform = ecs_world
    //        .get::<&Transform>(*entity)
    //        .expect("Target entity should have a tranform attached.");
    //    let entity_world_transform = ecs_world.get_world_transform(*entity, &entity_transform);
    //    let model_id = renderable
    //        .voxel_model_id()
    //        .expect("Target entity should have a voxel model.");

    //    let entity_state = editing
    //        .entity_state
    //        .entry(*entity)
    //        .or_insert_with(|| EditorVoxelEditingEntityState::new());

    //    // ===== SELECTION KEYBOARD SHORTCUTS ======
    //    // Delete selection.
    //    //if input.is_key_pressed(Key::Delete)
    //    //    && renderable.is_dynamic()
    //    //    && let Some(region) = &entity_state.selection
    //    //{
    //    //    // Delete/erase the contents of the selection.
    //    //    let edit = VoxelModelEdit {
    //    //        region: region.clone(),
    //    //        mask: VoxelModelEditMask::new(),
    //    //        operator: VoxelModelEditOperator::Replace(None),
    //    //    };

    //    //    editing
    //    //        .history
    //    //        .undo_buffer
    //    //        .push_back(EditorVoxelEditingHistoryItem::ModelEdit {
    //    //            model_id,
    //    //            saved_model_state: voxel_registry
    //    //                .get_model::<VoxelModelSFTCompressed>(model_id)
    //    //                .clone(),
    //    //        });
    //    //    voxel_registry
    //    //        .get_dyn_model_mut(model_id)
    //    //        .set_voxel_range_impl(&edit);
    //    //    events.push(VoxelModelEvent::UpdatedModel(model_id))
    //    //}

    //    let model_side_length = voxel_registry
    //        .get_dyn_model(renderable.voxel_model_id().unwrap())
    //        .length();

    //    let tool = editing.tools.get(&editing.selected_tool_type).unwrap();
    //    if tool == &EditorEditingTool::Selection {
    //        let consume_click = false;
    //        if let Some(VoxelModelEditRegion::Rect { min, max }) = &entity_state.selection {
    //            let model_min_world_pos = entity_world_transform.position
    //                - model_side_length
    //                    .cast::<f32>()
    //                    .component_mul(&entity_world_transform.scale)
    //                    * consts::voxel::VOXEL_METER_LENGTH
    //                    * 0.5;
    //            let min = min
    //                .cast::<f32>()
    //                .component_mul(&entity_world_transform.scale)
    //                * consts::voxel::VOXEL_METER_LENGTH
    //                + (model_min_world_pos);
    //            // Add one since for n voxel length, indexing is n-1 but visualization goes to the
    //            // nth number.
    //            let max = (max + Vector3::new(1, 1, 1))
    //                .cast::<f32>()
    //                .component_mul(&entity_world_transform.scale)
    //                * consts::voxel::VOXEL_METER_LENGTH
    //                + model_min_world_pos;
    //            let rota = entity_world_transform.position;
    //            let rot = entity_world_transform.rotation;
    //            let mut center = rot.transform_vector(&((min + max) * 0.5 - rota)) + rota;
    //            // To prevent z-fighting with the existing selection box.
    //            const OFFSET: f32 = 0.001;
    //            let half_length = ((max - min) * 0.5).map(|x| x + OFFSET);
    //            let ray = &editor_session.editor_camera_ray;
    //            let mut draw_plane = |normal: Vector3<f32>| {
    //                let offset = rot.transform_vector(&normal.component_mul(&half_length));
    //                let color = Color::new_srgba_hex("#AAAAFF", 0.7);
    //                let handle_pos = center + offset;

    //                let plane_rot = UnitQuaternion::rotation_between(&Vector3::y(), &offset)
    //                    .unwrap_or_else(|| {
    //                        if offset.y > 0.0 {
    //                            UnitQuaternion::identity()
    //                        } else {
    //                            UnitQuaternion::from_axis_angle(
    //                                &Vector3::x_axis(),
    //                                std::f32::consts::PI,
    //                            )
    //                        }
    //                    });
    //                // Create an orthonormal frame based on the normal then project the scaling axes onto that.
    //                let bitangent = if normal.abs() != Vector3::x() {
    //                    normal.cross(&Vector3::x()).cross(&normal)
    //                } else {
    //                    normal.cross(&Vector3::y()).cross(&normal)
    //                };
    //                let tangent = bitangent.cross(&normal).abs();
    //                assert_ne!(normal, tangent);
    //                assert_ne!(normal, bitangent);
    //                let sx = bitangent.dot(&half_length);
    //                let sy = tangent.dot(&half_length);

    //                let scale = Vector2::new(sx, sy);
    //                debug_renderer.draw_plane(
    //                    handle_pos,
    //                    plane_rot,
    //                    scale,
    //                    color,
    //                    DebugShapeFlags::NONE,
    //                );

    //                if ray.intersect_plane(handle_pos, plane_rot, scale).is_some() {}
    //            };
    //            draw_plane(Vector3::x());
    //            draw_plane(Vector3::y());
    //            draw_plane(Vector3::z());
    //            draw_plane(-Vector3::x());
    //            draw_plane(-Vector3::y());
    //            draw_plane(-Vector3::z());
    //        }

    //        if input.is_mouse_button_released(mouse::Button::Left) {
    //            if let Some(selection) = editing.in_progress_selection.take() {
    //                match selection {
    //                    InProgressSelection::Rect { start, end } => {
    //                        if start == end {
    //                            entity_state.selection = None;
    //                            return;
    //                        } else {
    //                            let min = start.zip_map(&end, |a, b| a.min(b));
    //                            let max = start.zip_map(&end, |a, b| a.max(b));
    //                            entity_state.selection =
    //                                Some(VoxelModelEditRegion::Rect { min, max });
    //                        }
    //                    }
    //                }
    //            }
    //        }
    //    }
    //}
}
