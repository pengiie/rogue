use std::{any::TypeId, collections::HashMap, f32, ops::Deref, path::PathBuf, str::FromStr};

use nalgebra::{Translation3, UnitQuaternion, Vector3};
use uuid::Uuid;

use crate::{
    common::dyn_vec::TypeInfo,
    engine::{
        asset::asset::{AssetHandle, AssetLoader, AssetPath, Assets},
        editor::editor::Editor,
        entity::{
            component::GameComponentMethods,
            ecs_world::{ECSWorld, Entity},
            scripting::{ScriptableEntity, Scripts},
            EntityChildren, EntityParent, GameEntity, RenderableVoxelEntity,
        },
        graphics::camera::Camera,
        physics::{
            capsule_collider::CapsuleCollider,
            plane_collider::PlaneCollider,
            rigid_body::{RigidBody, RigidBodyCreateInfo, RigidBodyType},
            transform::Transform,
        },
        voxel::{
            voxel::VoxelModelImpl, voxel_registry::VoxelModelRegistry, voxel_world::VoxelWorld,
        },
    },
    session::{EditorSession, RenderableEntityLoad},
};

use super::{
    components::{CameraAsset, TransformAsset},
    voxel::any::VoxelModelAnyAsset,
};

/// A standalone game entity with all of its component data, essentially a prefab.
/// Any references to colliders or voxel models are also specific to this entity instance.
pub struct WorldGameEntityAsset {
    pub name: String,
    pub uuid: Uuid,
    pub parent: Option<Uuid>,
    pub children: Vec<Uuid>,
    pub components: HashMap<TypeId, WorldGameComponentAsset>,
}

pub struct WorldGameComponentAsset {
    type_info: TypeInfo,
    data: *mut u8,
}

impl WorldGameComponentAsset {
    pub unsafe fn new(type_info: TypeInfo, data: *mut u8) -> Self {
        Self { type_info, data }
    }

    pub fn take_data(&mut self) -> *mut u8 {
        let ptr = self.data;
        assert!(!self.data.is_null(), "Data is already taken.");
        self.data = std::ptr::null_mut();
        return ptr;
    }
}

impl Drop for WorldGameComponentAsset {
    fn drop(&mut self) {
        if !self.data.is_null() {
            // Safety: We check for null, and when ownership of the component data is transferred,
            // we set the pointer to null.
            unsafe { std::alloc::dealloc(self.data, self.type_info.layout(1)) };
        }
    }
}

impl WorldGameEntityAsset {}
