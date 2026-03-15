use nalgebra::Vector3;
use rogue_engine::{
    common::color::Color,
    debug::debug_renderer::DebugRenderer,
    entity::{RenderableVoxelEntity, ecs_world::ECSWorld},
    physics::transform::Transform,
    resource::{Res, ResMut},
    voxel::voxel_registry::VoxelModelRegistry,
};
use rogue_macros::Resource;

use crate::session::EditorSession;

/// Tool for modifying the currently selected entity.
#[derive(Resource)]
pub struct EditorGizmo {}

impl EditorGizmo {
    pub fn new() -> Self {
        Self {}
    }

    pub fn update(
        mut gizmo: ResMut<EditorGizmo>,
        mut session: ResMut<EditorSession>,
        mut debug_renderer: ResMut<DebugRenderer>,
        ecs_world: Res<ECSWorld>,
    ) {
        let Some(selected_entity) = session.selected_entity else {
            return;
        };

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
            Color::new_srgb(1.0, 0.0, 0.0),
        );
        debug_renderer.draw_arrow(
            world_transform.position,
            world_transform.position + Vector3::y(),
            scale,
            Color::new_srgb(0.0, 1.0, 0.0),
        );
        debug_renderer.draw_arrow(
            world_transform.position,
            world_transform.position + Vector3::z(),
            scale,
            Color::new_srgb(0.0, 0.0, 1.0),
        );
    }

    pub fn visualize_selected_entity(
        mut session: ResMut<EditorSession>,
        mut debug_renderer: ResMut<DebugRenderer>,
        ecs_world: Res<ECSWorld>,
        voxel_registry: Res<VoxelModelRegistry>,
    ) {
        const SELECTION_COLOR: &'static str = "#ffffff";
        let Some(selected_entity) = session.selected_entity else {
            return;
        };

        let local_transform = ecs_world
            .get::<&Transform>(selected_entity)
            .expect("Should have a transform");
        let world_transform = ecs_world.get_world_transform(selected_entity, &local_transform);

        let color = Color::new_srgb_hex(SELECTION_COLOR);
        if let Ok(renderable) = ecs_world.get::<&RenderableVoxelEntity>(selected_entity)
            && let Some(model_id) = renderable.voxel_model_id()
        {
            let side_length = voxel_registry.get_dyn_model(model_id).length();
            let obb = world_transform.as_voxel_model_obb(side_length);
            debug_renderer.draw_obb(&obb, 0.025, color);
        } else {
            debug_renderer.draw_sphere(world_transform.position, 0.2, color);
        }
    }
}
