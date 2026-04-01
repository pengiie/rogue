use nalgebra::Vector3;
use rogue_engine::{
    common::{
        color::Color,
        geometry::{aabb::AABB, obb::OBB},
    },
    consts,
    debug::debug_renderer::{DebugRenderer, DebugShapeFlags},
    entity::{
        RenderableVoxelEntity,
        ecs_world::{ECSWorld, Entity},
    },
    physics::transform::Transform,
    resource::{Res, ResMut},
    voxel::{voxel_registry::VoxelModelRegistry, voxel_registry_gpu::VoxelModelRegistryGpu},
    window::time::Time,
};

use crate::{
    editing::{
        voxel_editing::{EditorEditingToolType, EditorVoxelEditing, EditorVoxelEditingTarget},
        voxel_editing_preview_gpu::EditorVoxelEditingPreviewGpu,
        voxel_editing_selection::{
            self, EditorVoxelEditingSelection, EditorVoxelEditingSelections,
            VoxelEditingInProgressSelection,
        },
    },
    session::EditorSession,
};

pub struct EditorVoxelEditingSelectionsGpu;

impl EditorVoxelEditingSelectionsGpu {
    /// Runs in on_update since it uses the debug renderer.
    pub fn update_selections_preview_gpu(
        voxel_editing: Res<EditorVoxelEditing>,
        voxel_editing_selection: Res<EditorVoxelEditingSelections>,
        preview_gpu: Res<EditorVoxelEditingPreviewGpu>,
        voxel_registry: Res<VoxelModelRegistry>,
        mut voxel_registry_gpu: ResMut<VoxelModelRegistryGpu>,
        mut debug_renderer: ResMut<DebugRenderer>,
        ecs_world: Res<ECSWorld>,
        time: Res<Time>,
        editor_session: Res<EditorSession>,
    ) {
        let mut draw_selection = |selection_obb: &OBB| {
            debug_renderer.draw_obb_filled(
                &selection_obb,
                Color::new_srgba(1.0, 1.0, 1.0, 0.1),
                DebugShapeFlags::NONE,
            );
            debug_renderer.draw_obb_outline(
                &selection_obb,
                0.001,
                Color::new_srgba_hex("#0080FF", 0.5),
                DebugShapeFlags::NONE,
            );
        };

        let mut create_entity_selection_obb =
            |target_entity: Entity, min: &Vector3<i32>, max: &Vector3<i32>| -> Option<OBB> {
                let Some((transform, renderable)) = ecs_world
                    .query_one::<(&Transform, &RenderableVoxelEntity)>(target_entity)
                    .get()
                else {
                    return None;
                };
                let Some(model_id) = renderable.voxel_model_id() else {
                    return None;
                };
                let world_transform = ecs_world.get_world_transform(target_entity, &transform);
                let entity_model_side_length = voxel_registry.get_dyn_model(model_id).length();
                let model_obb = world_transform.as_voxel_model_obb(entity_model_side_length);
                let selection_aabb_min = model_obb.aabb.min
                    + min.cast::<f32>().component_mul(&world_transform.scale)
                        * consts::voxel::VOXEL_METER_LENGTH;
                let selection_aabb_max = model_obb.aabb.min
                    + (max + Vector3::new(1, 1, 1))
                        .cast::<f32>()
                        .component_mul(&world_transform.scale)
                        * consts::voxel::VOXEL_METER_LENGTH;
                let selection_aabb_center = (selection_aabb_min + selection_aabb_max) * 0.5;
                let rotation_anchor = model_obb.aabb.center() - selection_aabb_center;
                Some(OBB::new(
                    AABB::new_two_point(selection_aabb_min, selection_aabb_max),
                    world_transform.rotation,
                    rotation_anchor,
                ))
            };

        match &voxel_editing.edit_target {
            Some(EditorVoxelEditingTarget::Entity(target_entity)) => {
                if let Some(EditorVoxelEditingSelection { min, max }) =
                    &voxel_editing_selection.selection
                {
                    draw_selection(
                        &create_entity_selection_obb(*target_entity, min, max).expect(
                            "Currently selected entity should have a renderable and transform",
                        ),
                    );
                }

                // Draw our in progress selection, or if there isn't one, draw which voxel we are
                // currently selecting.
                if let Some(in_progress_selection) = &voxel_editing_selection.in_progress_selection
                {
                    let renderable = ecs_world
                        .get::<&RenderableVoxelEntity>(*target_entity)
                        .unwrap();
                    let entity_model_side_length = voxel_registry
                        .get_dyn_model(renderable.voxel_model_id().unwrap())
                        .length();
                    let (min, max) =
                        in_progress_selection.min_max_saturated(entity_model_side_length);
                    draw_selection(&create_entity_selection_obb(*target_entity, &min, &max).expect(
                            "Currently in progress selecting entity should have a renderable and transform",
                        ));
                }
            }
            Some(EditorVoxelEditingTarget::Terrain) => {}
            None => {}
        }

        if voxel_editing.selected_tool_type == EditorEditingToolType::Selection
            && voxel_editing_selection.in_progress_selection.is_none()
            && let Some(entity_hit) = &editor_session.entity_raycast
        {
            let hit_pos = entity_hit.model_trace.local_position.cast::<i32>();
            if let Some(hit_model_obb) =
                create_entity_selection_obb(entity_hit.entity, &hit_pos, &hit_pos)
            {
                draw_selection(&hit_model_obb);
            }
        }
    }
}
