use std::collections::HashSet;

use crate::engine::{
    asset::asset::{AssetHandle, AssetPath},
    entity::component::GameComponent,
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

impl GameComponent for GameEntity {
    fn clone_component(
        &self,
        ctx: &mut super::component::GameComponentContext<'_>,
        dst_ptr: *mut u8,
    ) {
        let dst_ptr = dst_ptr as *mut Self;
        // Safety: dst_ptr should be allocated with the memory layout for this type.
        unsafe { dst_ptr.write(self.clone()) };
    }

    fn serialize_component(
        &self,
        ctx: super::component::GameComponentContext<'_>,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()> {
        todo!()
    }

    fn deserialize_component(
        &self,
        ctx: super::component::GameComponentContext<'_>,
        de: &mut dyn erased_serde::Deserializer,
        dst_ptr: *mut u8,
    ) -> erased_serde::Result<()> {
        todo!()
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

impl GameComponent for RenderableVoxelEntity {
    fn clone_component(
        &self,
        ctx: &mut super::component::GameComponentContext<'_>,
        dst_ptr: *mut u8,
    ) {
        let dst_ptr = dst_ptr as *mut RenderableVoxelEntity;
        // Safety: dst_ptr should be allocated with the memory layout for this type.
        unsafe { dst_ptr.write(self.clone()) };
    }

    fn serialize_component(
        &self,
        ctx: super::component::GameComponentContext<'_>,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()> {
        todo!()
    }

    fn deserialize_component(
        &self,
        ctx: super::component::GameComponentContext<'_>,
        de: &mut dyn erased_serde::Deserializer,
        dst_ptr: *mut u8,
    ) -> erased_serde::Result<()> {
        todo!()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct EntityParent {
    pub parent: Entity,
}

impl GameComponent for EntityParent {
    fn clone_component(
        &self,
        ctx: &mut super::component::GameComponentContext<'_>,
        dst_ptr: *mut u8,
    ) {
        let dst_ptr = dst_ptr as *mut Self;
        // Safety: dst_ptr should be allocated with the memory layout for this type.
        unsafe { dst_ptr.write(self.clone()) };
    }

    fn serialize_component(
        &self,
        ctx: super::component::GameComponentContext<'_>,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()> {
        todo!()
    }

    fn deserialize_component(
        &self,
        ctx: super::component::GameComponentContext<'_>,
        de: &mut dyn erased_serde::Deserializer,
        dst_ptr: *mut u8,
    ) -> erased_serde::Result<()> {
        todo!()
    }
}

impl EntityParent {
    pub fn new(parent: Entity) -> Self {
        Self { parent: parent }
    }
}

#[derive(Clone, PartialEq)]
pub struct EntityChildren {
    pub children: HashSet<Entity>,
}

impl GameComponent for EntityChildren {
    fn clone_component(
        &self,
        ctx: &mut super::component::GameComponentContext<'_>,
        dst_ptr: *mut u8,
    ) {
        let dst_ptr = dst_ptr as *mut Self;
        // Safety: dst_ptr should be allocated with the memory layout for this type.
        unsafe { dst_ptr.write(self.clone()) };
    }

    fn serialize_component(
        &self,
        ctx: super::component::GameComponentContext<'_>,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()> {
        todo!()
    }

    fn deserialize_component(
        &self,
        ctx: super::component::GameComponentContext<'_>,
        de: &mut dyn erased_serde::Deserializer,
        dst_ptr: *mut u8,
    ) -> erased_serde::Result<()> {
        todo!()
    }
}
