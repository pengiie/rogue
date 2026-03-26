use nalgebra::Vector3;
use rogue_engine::{
    common::color::Color,
    debug::debug_renderer::{DebugRenderer, DebugShapeFlags},
    entity::{RenderableVoxelEntity, ecs_world::ECSWorld},
    graphics::camera::MainCamera,
    physics::transform::Transform,
    resource::{Res, ResMut},
    voxel::voxel_registry::VoxelModelRegistry,
};
use rogue_macros::Resource;

use crate::{editing::voxel_editing::EditorVoxelEditing, session::EditorSession};

/// Tool for modifying the currently selected entity.
#[derive(Resource)]
pub struct EditorGizmo {}

impl EditorGizmo {
    pub fn new() -> Self {
        Self {}
    }

    pub fn update(
        mut gizmo: ResMut<EditorGizmo>,
        mut editor_session: ResMut<EditorSession>,
        mut debug_renderer: ResMut<DebugRenderer>,
        ecs_world: Res<ECSWorld>,
        main_camera: Res<MainCamera>,
        voxel_editing: Res<EditorVoxelEditing>,
    ) {
        if voxel_editing.is_enabled() {
            return;
        }
        let Some(selected_entity) = editor_session.selected_entity else {
            return;
        };
        if main_camera.camera() != Some(editor_session.editor_camera()) {
            return;
        }

        let local_transform = ecs_world
            .get::<&Transform>(selected_entity)
            .expect("Should have a transform");
        let world_transform = ecs_world.get_world_transform(selected_entity, &local_transform);

        // Draw translation.
        let scale = 0.5;
        debug_renderer.draw_arrow(
            world_transform.position,
            world_transform.position + Vector3::x(),
            scale,
            Color::new_srgba(1.0, 0.0, 0.0, 1.0),
            DebugShapeFlags::NONE,
        );
        debug_renderer.draw_arrow(
            world_transform.position,
            world_transform.position + Vector3::y(),
            scale,
            Color::new_srgba(0.0, 1.0, 0.0, 1.0),
            DebugShapeFlags::NONE,
        );
        debug_renderer.draw_arrow(
            world_transform.position,
            world_transform.position + Vector3::z(),
            scale,
            Color::new_srgba(0.0, 0.0, 1.0, 1.0),
            DebugShapeFlags::NONE,
        );
    }

    pub fn visualize_selected_entity(
        mut editor_session: ResMut<EditorSession>,
        mut debug_renderer: ResMut<DebugRenderer>,
        ecs_world: Res<ECSWorld>,
        voxel_registry: Res<VoxelModelRegistry>,
        main_camera: Res<MainCamera>,
        voxel_editing: Res<EditorVoxelEditing>,
    ) {
        if voxel_editing.is_enabled() {
            return;
        }
        const SELECTION_COLOR: &'static str = "#ffffff";
        let Some(selected_entity) = editor_session.selected_entity else {
            return;
        };
        if main_camera.camera() != Some(editor_session.editor_camera()) {
            return;
        }

        let local_transform = ecs_world
            .get::<&Transform>(selected_entity)
            .expect("Should have a transform");
        let world_transform = ecs_world.get_world_transform(selected_entity, &local_transform);

        let color = Color::new_srgba_hex(SELECTION_COLOR, 1.0);
        if let Ok(renderable) = ecs_world.get::<&RenderableVoxelEntity>(selected_entity)
            && let Some(model_id) = renderable.voxel_model_id()
        {
            let side_length = voxel_registry.get_dyn_model(model_id).length();
            let obb = world_transform.as_voxel_model_obb(side_length);
            debug_renderer.draw_obb_outline(
                &obb,
                0.025 * world_transform.scale.min(),
                color,
                DebugShapeFlags::NONE,
            );
        } else {
            //debug_renderer.draw_sphere(world_transform.position, 0.2, color, DebugShapeFlags::NONE);
        }
    }
}
