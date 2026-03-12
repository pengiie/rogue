use nalgebra::Vector3;
use rogue_engine::{
    common::color::Color,
    debug::debug_renderer::DebugRenderer,
    entity::ecs_world::ECSWorld,
    physics::transform::Transform,
    resource::{Res, ResMut},
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

        debug_renderer.draw_arrow(
            world_transform.position,
            world_transform.position + world_transform.right(),
            0.1,
            Color::new_srgb(1.0, 0.0, 0.0),
        );
    }
}
