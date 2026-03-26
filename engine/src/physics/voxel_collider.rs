use nalgebra::Vector3;

use crate::{
    asset::asset::GameAssetPath,
    common::geometry::aabb::AABB,
    physics::{collider::Collider, collider_voxel_registry::VoxelColliderRegistry},
    voxel::voxel_registry::VoxelModelId,
};
pub struct VoxelModelColliderData {}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct VoxelModelCollider {
    #[serde(skip)]
    pub model_id: VoxelModelId,
    pub asset_path: Option<GameAssetPath>,
}

impl Default for VoxelModelCollider {
    fn default() -> Self {
        Self {
            model_id: VoxelModelId::null(),
            asset_path: None,
        }
    }
}

impl Collider for VoxelModelCollider {
    const NAME: &str = "voxel_collider";

    fn aabb(
        &self,
        world_transform: &super::transform::Transform,
        voxel_registry: &VoxelColliderRegistry,
    ) -> Option<AABB> {
        if self.model_id.is_null() {
            return None;
        }
        let Some(collider_model) = voxel_registry.collider_models.get(&self.model_id) else {
            return None;
        };
        todo!()
    }

    fn serialize_collider(
        &self,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()> {
        use erased_serde::Serialize;
        self.erased_serialize(ser)
    }

    unsafe fn deserialize_collider(
        de: &mut dyn erased_serde::Deserializer,
        dst_ptr: *mut u8,
    ) -> erased_serde::Result<()> {
        let dst_ptr = dst_ptr as *mut Self;
        // Safety: dst_ptr should be allocated with the memory layout for this type.
        unsafe { dst_ptr.write(erased_serde::deserialize::<Self>(de)?) };
        Ok(())
    }
}
