use nalgebra::Vector3;

use crate::voxel::voxel_registry::VoxelModelId;
pub struct VoxelModelColliderData {
    pub side_length: Vector3<u32>,
    pub corners: Vec<Vector3<u32>>,
    pub edges: Vec<Vector3<u32>>,
}

#[derive(Clone)]
pub struct VoxelModelCollider {
    pub model_id: VoxelModelId,
}

// impl Collider for VoxelModelCollider {
//     fn concrete_collider_type() -> ColliderType {
//         ColliderType::Null
//     }
// }
//
// impl ColliderMethods for VoxelModelCollider {
//     fn test_collision(
//         &self,
//         other: &dyn ColliderMethods,
//         transform_a: &Transform,
//         transform_b: &Transform,
//     ) -> Option<ContactManifold> {
//         match other.collider_type() {
//             ColliderType::Voxel => {
//                 todo!()
//             }
//             _ => unimplemented!(),
//         }
//     }
//
//     fn aabb(&self, world_transform: &Transform, voxel_world: &VoxelWorld) -> AABB {
//         todo!()
//     }
//
//     fn collider_type(&self) -> ColliderType {
//         todo!()
//     }
// }
