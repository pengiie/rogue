use std::collections::HashMap;

use nalgebra::Vector3;

use crate::{
    physics::{
        box_collider::BoxCollider, collider::ContactManifold, transform::Transform,
        voxel_collider::VoxelModelCollider,
    },
    voxel::voxel_registry::VoxelModelId,
};

pub struct VoxelColliderData {
    pub side_length: Vector3<u32>,
}

pub struct VoxelColliderRegistry {
    pub collider_models: HashMap<VoxelModelId, VoxelColliderData>,
}

impl VoxelColliderRegistry {
    pub fn new() -> Self {
        Self {
            collider_models: HashMap::new(),
        }
    }
}

pub fn test_intersection_voxel_voxel(
    model_a: &VoxelModelCollider,
    model_b: &VoxelModelCollider,
    entity_transform_a: &Transform,
    entity_transform_b: &Transform,
    voxel_registry: &VoxelColliderRegistry,
) -> Option<ContactManifold> {
    todo!()
}
