use crate::engine::entity::ecs_world::Entity;

/// Sent when an entities renderable model should be loaded if not
/// already.
pub struct EventVoxelRenderableEntityLoad {
    pub entity: Entity,
    /// Whether to reload the model if it is already loaded.
    pub reload: bool,
}
