use nalgebra::Vector3;

use crate::engine::entity::ecs_world::Entity;

#[derive(Clone)]
pub enum EventEditorZoom {
    Entity { target_entity: Entity },
    Position { position: Vector3<f32> },
}
