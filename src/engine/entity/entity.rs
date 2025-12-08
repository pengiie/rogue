use std::collections::HashSet;

use erased_serde::Serialize;
use rogue_macros::game_component;
use serde::de::DeserializeSeed;
use uuid::serde::braced::serialize;

use super::ecs_world::Entity;
use crate::engine::entity::component::GameComponentSerializeContext;
use crate::engine::{
    asset::asset::{AssetHandle, AssetPath, GameAssetPath},
    entity::component::{GameComponent, GameComponentDeserializeContext},
    voxel::voxel_registry::VoxelModelId,
};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[game_component(name = "GameEntity", constructible = false)]
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

impl Default for RenderableVoxelEntity {
    fn default() -> Self {
        Self::new_null()
    }
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

    pub fn set_model_id(&mut self, id: VoxelModelId) {
        assert!(!id.is_null());
        self.voxel_model_id = id;
    }

    pub fn is_dynamic(&self) -> bool {
        self.is_dynamic
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
    parent: Entity,
}

impl EntityParent {
    pub fn new(parent: Entity) -> Self {
        assert!(!parent.is_null(), "Parent cannot be null.");
        Self { parent }
    }

    pub fn parent(&self) -> Entity {
        self.parent
    }

    pub fn set_parent(&mut self, parent: Entity) {
        assert!(!parent.is_null(), "Parent cannot be null.");
        self.parent = parent;
    }
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
        let parent_uuid = ctx.entity_uuid_map.get(&self.parent).ok_or_else(|| {
            erased_serde::convert_ser_error(serde::ser::Error::custom(format!(
                "Parent {:?} doesn't exist when it is referenced.",
                self.parent
            )))
        })?;
        let serializable = EntityParentSerializable { parent_uuid };
        serializable.erased_serialize(ser)
    }

    unsafe fn deserialize_component(
        ctx: &mut GameComponentDeserializeContext<'_>,
        de: &mut dyn erased_serde::Deserializer,
        dst_ptr: *mut u8,
    ) -> erased_serde::Result<()> {
        // This will write the parent uuid which will be retrieved later and associated with the
        // spawned entity after deserializing.
        (EntityParentDerializable {
            parent_uuid: &mut ctx.entity_parent,
        })
        .deserialize(de)?;

        // Write the dangling pointer for now and we will populate later after
        // reading all entities.
        // Safety: dst_ptr should be allocated with the memory layout for this type.
        let dst_ptr = dst_ptr as *mut Self;
        unsafe {
            dst_ptr.write(Self {
                parent: Entity::DANGLING,
            })
        };

        Ok(())
    }
}

struct EntityParentDerializable<'a> {
    parent_uuid: &'a mut uuid::Uuid,
}

impl<'de> serde::de::DeserializeSeed<'de> for EntityParentDerializable<'_> {
    type Value = ();

    fn deserialize<D>(self, de: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        de.deserialize_struct("EntityParent", &["uuid"], self)
    }
}

#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum EntityParentField {
    Uuid,
}

impl<'de> serde::de::Visitor<'de> for EntityParentDerializable<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("struct EntityParent")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut parsed_uuid = false;
        while let Some(key) = map.next_key::<EntityParentField>()? {
            match key {
                EntityParentField::Uuid => {
                    if parsed_uuid {
                        return Err(serde::de::Error::duplicate_field("uuid"));
                    }
                    *self.parent_uuid = map.next_value::<uuid::Uuid>()?;
                    parsed_uuid = true;
                }
            }
        }
        if !parsed_uuid {
            return Err(serde::de::Error::missing_field("uuid"));
        }

        Ok(())
    }
}

#[derive(serde::Serialize)]
#[serde(rename = "EntityParent")]
struct EntityParentSerializable<'a> {
    #[serde(rename = "uuid")]
    parent_uuid: &'a uuid::Uuid,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq)]
#[game_component(name = "EntityChildren", constructible = false)]
pub struct EntityChildren {
    /// Don't serialize the children since they can be derived when
    /// deserializing the entity parents.
    #[serde(skip)]
    pub children: HashSet<Entity>,
}

impl Drop for EntityChildren {
    fn drop(&mut self) {
        log::debug!(
            "Dropping EntityChildren with {:?} children.",
            &self.children
        );
    }
}
