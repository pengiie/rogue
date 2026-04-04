use rogue_engine::{
    common::color::{Color, ColorSrgba},
    debug::debug_renderer::{DebugRenderer, DebugShapeFlags},
    entity::{RenderableVoxelEntity, ecs_world::ECSWorld},
    graphics::camera::MainCamera,
    physics::{
        collider_component::EntityColliders, physics_world::PhysicsWorld, transform::Transform,
    },
    resource::{Res, ResMut},
    voxel::voxel_registry::VoxelModelRegistry,
};
use rogue_macros::Resource;

use crate::{editing::voxel_editing::EditorVoxelEditing, session::EditorSession};

pub struct SelectedEntityVisualizer;

impl SelectedEntityVisualizer {
    pub fn visualize_selected_entity(
        mut editor_session: ResMut<EditorSession>,
        mut debug_renderer: ResMut<DebugRenderer>,
        ecs_world: Res<ECSWorld>,
        voxel_registry: Res<VoxelModelRegistry>,
        main_camera: Res<MainCamera>,
        voxel_editing: Res<EditorVoxelEditing>,
        physics_world: Res<PhysicsWorld>,
    ) {
        if voxel_editing.is_enabled() || !editor_session.is_editor_camera_focused() {
            return;
        }
        const SELECTION_COLOR: &'static str = "#ffffff";
        let Some(selected_entity) = editor_session.selected_entity else {
            return;
        };

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
        }

        if editor_session.render_colliders {
            if let Ok(colliders) = ecs_world.get::<&EntityColliders>(selected_entity) {
                for collider_id in &colliders.colliders {
                    physics_world
                        .colliders
                        .get_collider_dyn(collider_id)
                        .render_debug(
                            &world_transform,
                            &mut debug_renderer,
                            ColorSrgba::new_srgb_hex("#22FF22", 0.1),
                        );
                }
            }
        }
    }
}
