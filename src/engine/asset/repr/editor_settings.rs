use std::{collections::HashMap, f32, ops::Deref, path::PathBuf, str::FromStr};

use nalgebra::{Translation3, UnitQuaternion, Vector3};

use crate::{
    engine::{
        asset::asset::{impl_asset_load_save_serde, AssetHandle, AssetLoader, AssetPath, Assets},
        editor::editor::Editor,
        entity::{
            ecs_world::{ECSWorld, Entity},
            scripting::{ScriptableEntity, Scripts},
            EntityChildren, EntityParent, GameEntity, RenderableVoxelEntity,
        },
        graphics::camera::Camera,
        physics::{
            capsule_collider::CapsuleCollider, plane_collider::PlaneCollider,
            rigid_body::RigidBody, transform::Transform,
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

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct EditorUserSettingsAsset {
    pub last_project_dir: Option<PathBuf>,
}

impl_asset_load_save_serde!(EditorUserSettingsAsset);
