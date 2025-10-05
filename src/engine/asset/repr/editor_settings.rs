use std::{collections::HashMap, f32, ops::Deref, path::PathBuf, str::FromStr};

use nalgebra::{Translation3, UnitQuaternion, Vector3};

use crate::{
    engine::{
        asset::asset::{AssetHandle, AssetLoader, AssetPath, Assets},
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
    session::{RenderableEntityLoad, Session},
};

use super::{
    components::{CameraAsset, TransformAsset},
    voxel::any::VoxelModelAnyAsset,
};

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct EditorSettingsAsset {
    pub last_project_dir: Option<PathBuf>,
}
