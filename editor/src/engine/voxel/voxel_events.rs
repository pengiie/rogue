use crate::engine::entity::ecs_world::Entity;

/// Sent when an entities renderable model should be loaded if not
/// already.
///
/// Listened to by the voxel model registry for loading and setting the renderable's
/// voxel model ID.
pub struct EventVoxelRenderableEntityLoad {
    /// The associated entity that should have their RenderableVoxelEntity model loaded.
    /// Expects the component to exist and also have an existing game asset path.
    pub entity: Entity,
    /// Whether to reload the model if it is already loaded.
    pub reload: bool,
}
