use std::{collections::HashMap, f32, ops::Deref, path::PathBuf, str::FromStr};

use hecs::With;
use nalgebra::{Translation3, UnitQuaternion, Vector3};

use crate::{
    engine::{
        asset::{
            asset::{AssetHandle, AssetLoader, AssetPath, Assets},
            repr::{collider::ColliderRegistryAsset, game_entity::EditorGameEntityAsset},
        },
        editor::editor::Editor,
        entity::{
            ecs_world::{ECSWorld, Entity},
            scripting::{ScriptableEntity, Scripts},
            EntityChildren, EntityParent, GameEntity, RenderableVoxelEntity,
        },
        graphics::camera::Camera,
        physics::{
            capsule_collider::CapsuleCollider, collider_registry::ColliderRegistry,
            plane_collider::PlaneCollider, rigid_body::RigidBody, transform::Transform,
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

#[derive(serde::Serialize, serde::Deserialize)]
pub struct EditorProjectAsset {
    pub editor_camera_transform: TransformAsset,
    pub editor_camera: CameraAsset,

    pub rotation_anchor: Vector3<f32>,
    pub terrain_asset_path: Option<PathBuf>,
    pub game_camera: Option</*uuid=*/ String>,
    pub game_entities: Vec<EditorGameEntityAsset>,
    #[serde(default)]
    pub collider_registry: ColliderRegistryAsset,
}

impl EditorProjectAsset {
    pub fn new_empty() -> Self {
        Self {
            editor_camera_transform: TransformAsset {
                transform: Transform::with_translation(Translation3::new(-5.0, 5.0, -5.0)),
            },
            editor_camera: CameraAsset {
                camera: Camera::new(f32::consts::FRAC_PI_2),
            },
            terrain_asset_path: None,
            game_camera: None,
            rotation_anchor: Vector3::zeros(),
            game_entities: Vec::new(),
            collider_registry: ColliderRegistryAsset::new(),
        }
    }

    // Creates a project asset from the current world state.
    pub fn new_existing(
        &self,
        editor: &Editor,
        ecs_world: &ECSWorld,
        voxel_world: &VoxelWorld,
        terrain_asset_path: Option<PathBuf>,
        game_camera: Option<Entity>,
        collider_registry: &ColliderRegistry,
    ) -> Self {
        let game_entities = ecs_world
            .query::<With<(), &GameEntity>>()
            .into_iter()
            .map(|(id, _)| EditorGameEntityAsset::new(ecs_world, &voxel_world.registry, id))
            .collect::<Vec<_>>();

        let mut editor_camera_query = ecs_world
            .query_one::<(&mut Transform, &Camera)>(editor.editor_camera_entity.unwrap())
            .unwrap();
        let (mut editor_transform, editor_camera) = editor_camera_query.get().unwrap();

        let game_camera_uuid = game_camera.map(|e| {
            ecs_world
                .get::<&GameEntity>(e)
                .expect("Game camera should be valid entity.")
                .uuid
                .to_string()
        });
        Self {
            editor_camera_transform: TransformAsset {
                transform: editor_transform.clone(),
            },
            editor_camera: CameraAsset {
                camera: editor_camera.clone(),
            },
            rotation_anchor: editor.editor_camera.rotation_anchor,
            terrain_asset_path,
            game_entities,
            game_camera: game_camera_uuid,
            collider_registry: ColliderRegistryAsset::from(collider_registry),
        }
    }
}
