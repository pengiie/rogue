use crate::engine::{
    asset::asset::{AssetHandle, AssetPath},
    voxel::voxel_registry::VoxelModelId,
};

pub struct RenderableVoxelEntity {
    /// Nullable.
    voxel_model_id: VoxelModelId,
}

impl RenderableVoxelEntity {
    pub fn new(voxel_model_id: VoxelModelId) -> Self {
        Self { voxel_model_id }
    }

    pub fn set_id(&mut self, id: VoxelModelId) {
        self.voxel_model_id = id;
    }

    pub fn new_null() -> Self {
        Self {
            voxel_model_id: VoxelModelId::null(),
        }
    }

    pub fn is_null(&self) -> bool {
        self.voxel_model_id.is_null()
    }

    pub fn voxel_model_id(&self) -> Option<VoxelModelId> {
        (!self.voxel_model_id.is_null()).then_some(self.voxel_model_id)
    }

    pub fn voxel_model_id_unchecked(&self) -> VoxelModelId {
        self.voxel_model_id
    }
}

pub struct ScriptableEntity {
    pub scripts: AssetHandle,
}
