use std::collections::HashSet;

use erased_serde::Serialize;
use rogue_macros::game_component;

use super::ecs_world::Entity;
use crate::engine::entity::component::GameComponentSerializeContext;
use crate::engine::{
    asset::asset::{AssetHandle, AssetPath, GameAssetPath},
    entity::component::{GameComponent, GameComponentDeserializeContext},
    voxel::voxel_registry::VoxelModelId,
};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[game_component(name = "GameEntity")]
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

    /// Keeps the name, appending/incrementing a number suffix.
    /// Creates a new Uuid.
    pub fn duplicate(&self) -> Self {
        let name = format!("{} copy", self.name);

        Self {
            uuid: uuid::Uuid::new_v4(),
            name,
        }
    }
}

#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[game_component(name = "RenderableVoxelEntity")]
pub struct RenderableVoxelEntity {
    /// The asset path of the model, optional since the model
    /// may be procedurally generated.
    model_asset_path: Option<GameAssetPath>,

    /// Defaults to false.
    // TODO: make default const function util for bools.
    #[serde(default)]
    is_dynamic: bool,

    /// An ID which is null when the model is not yet loaded for this entity.
    /// Can be requested to load via `EventVoxelRenderableEntityLoad`.
    #[serde(skip)]
    #[serde(default = "VoxelModelId::null")]
    voxel_model_id: VoxelModelId,
}

impl RenderableVoxelEntity {
    pub fn new(
        model_asset_path: Option<GameAssetPath>,
        is_dynamic: bool,
        voxel_model_id: VoxelModelId,
    ) -> Self {
        Self {
            model_asset_path,
            is_dynamic,
            voxel_model_id,
        }
    }

    pub fn set_model(&mut self, model_asset_path: Option<GameAssetPath>, id: VoxelModelId) {
        self.model_asset_path = model_asset_path;
        self.voxel_model_id = id;
    }

    pub fn new_null() -> Self {
        Self {
            model_asset_path: None,
            is_dynamic: false,
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

    pub fn model_asset_path(&self) -> Option<&GameAssetPath> {
        self.model_asset_path.as_ref()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct EntityParent {
    pub parent: Entity,
}

impl GameComponent for EntityParent {
    const NAME: &str = "EntityParent";

    fn clone_component(
        &self,
        ctx: &mut super::component::GameComponentCloneContext<'_>,
        dst_ptr: *mut u8,
    ) {
        let dst_ptr = dst_ptr as *mut Self;
        // Safety: dst_ptr should be allocated with the memory layout for this type.
        unsafe { dst_ptr.write(self.clone()) };
    }

    fn serialize_component(
        &self,
        ctx: &GameComponentSerializeContext<'_>,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()> {
        todo!()
    }

    unsafe fn deserialize_component(
        ctx: &mut GameComponentDeserializeContext<'_>,
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
    const NAME: &str = "EntityChildren";

    fn clone_component(
        &self,
        ctx: &mut super::component::GameComponentCloneContext<'_>,
        dst_ptr: *mut u8,
    ) {
        let dst_ptr = dst_ptr as *mut Self;
        // Safety: dst_ptr should be allocated with the memory layout for this type.
        unsafe { dst_ptr.write(std::clone::Clone::clone(self)) };
    }

    fn serialize_component(
        &self,
        ctx: &GameComponentSerializeContext<'_>,
        ser: &mut dyn erased_serde::Serializer,
    ) -> erased_serde::Result<()> {
        todo!()
    }

    unsafe fn deserialize_component(
        ctx: &mut GameComponentDeserializeContext<'_>,
        de: &mut dyn erased_serde::Deserializer,
        dst_ptr: *mut u8,
    ) -> erased_serde::Result<()> {
        todo!()
    }
}
