use std::collections::HashSet;

use crate::engine::{
    asset::asset::{AssetHandle, AssetPath},
    voxel::voxel_registry::VoxelModelId,
};

use super::ecs_world::Entity;

#[derive(Clone)]
pub struct GameEntity {
    pub uuid: uuid::Uuid,
    pub name: String,
}

impl GameEntity {
    pub fn new(name: impl ToString) -> Self {
        Self {
            uuid: uuid::Uuid::new_v4(),
            name: name.to_string(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
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

#[derive(Clone, PartialEq, Eq)]
pub struct EntityParent {
    pub parent: Entity,
}

impl EntityParent {
    pub fn new(parent: Entity) -> Self {
        Self { parent: parent }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct EntityChildren {
    pub children: HashSet<Entity>,
}
