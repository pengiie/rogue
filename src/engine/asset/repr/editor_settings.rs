use std::{collections::HashMap, f32, ops::Deref, path::PathBuf, str::FromStr};

use hecs::With;
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
            capsule_collider::CapsuleCollider, physics_world::Colliders,
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

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct EditorSettingsAsset {
    pub last_project_dir: Option<PathBuf>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct EditorSessionAsset {
    pub editor_camera_transform: TransformAsset,
    pub editor_camera: CameraAsset,
    pub rotation_anchor: Vector3<f32>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct EditorProjectAsset {
    pub editor_camera_transform: TransformAsset,
    pub editor_camera: CameraAsset,
    pub rotation_anchor: Vector3<f32>,
    pub terrain_asset_path: Option<PathBuf>,
    pub game_camera: Option</*uuid=*/ String>,
    pub game_entities: Vec<EditorGameEntityAsset>,
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
        }
    }

    pub fn new_existing(
        &self,
        editor: &Editor,
        ecs_world: &ECSWorld,
        voxel_world: &VoxelWorld,
        terrain_asset_path: Option<PathBuf>,
        game_camera: Option<Entity>,
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
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct EditorGameEntityAsset {
    pub name: String,
    pub uuid: String,
    pub parent: Option</*uuid*/ String>,
    pub children: Vec</*uuid*/ String>,
    pub components: Vec<EditorGameComponentAsset>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub enum EditorGameComponentAsset {
    Transform {
        position: Vector3<f32>,
        rotation: UnitQuaternion<f32>,
        scale: Vector3<f32>,
    },
    Camera {
        fov: f32,
    },
    RenderableVoxelEntity {
        model_asset_path: String,
    },
    ScriptableEntity {
        script_asset_paths: Vec<String>,
    },
    RigidBody {
        mass: f32,
    },
    Colliders(#[serde(default)] Colliders),
}

impl EditorGameEntityAsset {
    pub fn new(ecs_world: &ECSWorld, registry: &VoxelModelRegistry, entity_id: Entity) -> Self {
        let game_entity = ecs_world.get::<&GameEntity>(entity_id).unwrap();
        let mut map = serde_json::Map::new();

        let parent_uuid = ecs_world
            .get::<&EntityParent>(entity_id)
            .map(|parent| {
                ecs_world
                    .get::<&GameEntity>(parent.parent)
                    .map(|parent_game_entity| parent_game_entity.uuid.to_string())
                    .ok()
            })
            .unwrap_or(None);
        let children_uuids = ecs_world
            .get::<&EntityChildren>(entity_id)
            .map(|children| {
                let mut uuids = Vec::new();
                for child in children.children.iter() {
                    if let Ok(child_uuid) = ecs_world
                        .get::<&GameEntity>(*child)
                        .map(|child_game_entity| child_game_entity.uuid.to_string())
                    {
                        uuids.push(child_uuid);
                    }
                }
                uuids
            })
            .unwrap_or(Vec::new());

        let mut s = Self {
            name: game_entity.name.clone(),
            uuid: game_entity.uuid.to_string(),
            parent: parent_uuid,
            children: children_uuids,
            components: Vec::new(),
        };

        if let Ok(transform) = ecs_world.get::<&Transform>(entity_id) {
            s.components.push(EditorGameComponentAsset::Transform {
                position: transform.position,
                rotation: transform.rotation,
                scale: transform.scale,
            });
        }

        if let Ok(camera) = ecs_world.get::<&Camera>(entity_id) {
            s.components
                .push(EditorGameComponentAsset::Camera { fov: camera.fov() });
        }

        if let Ok(voxel_model_ref) = ecs_world.get::<&RenderableVoxelEntity>(entity_id) {
            if let Some(voxel_model_id) = voxel_model_ref.voxel_model_id() {
                let model_info = registry.get_model_info(voxel_model_id);
                if let Some(asset_path) = &model_info.asset_path {
                    s.components
                        .push(EditorGameComponentAsset::RenderableVoxelEntity {
                            model_asset_path: asset_path
                                .asset_path
                                .clone()
                                .expect("Entities should only have asset paths."),
                        });
                }
            }
        }

        if let Ok(scriptable) = ecs_world.get::<&ScriptableEntity>(entity_id) {
            s.components
                .push(EditorGameComponentAsset::ScriptableEntity {
                    script_asset_paths: scriptable
                        .scripts
                        .iter()
                        .map(|asset_path| asset_path.asset_path.clone().unwrap())
                        .collect::<Vec<_>>(),
                });
        }

        if let Ok(rigid_body) = ecs_world.get::<&RigidBody>(entity_id) {
            s.components.push(EditorGameComponentAsset::RigidBody {
                mass: rigid_body.mass(),
            });
        }

        if let Ok(colliders) = ecs_world.get::<&Colliders>(entity_id) {
            s.components.push(EditorGameComponentAsset::Colliders(
                colliders.deref().clone(),
            ));
        }

        return s;
    }

    pub fn spawn(
        &self,
        project_dir: PathBuf,
        mut ecs_world: &mut ECSWorld,
        assets: &mut Assets,
        loading_renderables: &mut HashMap<Entity, AssetHandle>,
        scripts: &mut Scripts,
    ) -> Entity {
        let uuid = uuid::Uuid::from_str(&self.uuid).unwrap();
        let id = ecs_world.spawn((GameEntity {
            uuid,
            name: self.name.clone(),
        },));

        for component in &self.components {
            match component {
                EditorGameComponentAsset::Transform {
                    position,
                    rotation,
                    scale,
                } => ecs_world
                    .insert_one(
                        id,
                        Transform {
                            position: position.clone(),
                            rotation: rotation.clone(),
                            scale: *scale,
                        },
                    )
                    .unwrap(),
                EditorGameComponentAsset::Camera { fov } => {
                    ecs_world.insert_one(id, Camera::new(*fov)).unwrap()
                }
                EditorGameComponentAsset::RenderableVoxelEntity { model_asset_path } => {
                    ecs_world.insert_one(id, RenderableVoxelEntity::new_null());
                    let asset_path =
                        AssetPath::new_project_dir(project_dir.clone(), model_asset_path.clone());
                    let asset_handle = assets.load_asset::<VoxelModelAnyAsset>(asset_path);
                    loading_renderables.insert(id, asset_handle);
                }
                EditorGameComponentAsset::ScriptableEntity {
                    script_asset_paths: script_project_paths,
                } => {
                    let mut asset_paths = Vec::new();
                    for asset_path in script_project_paths {
                        let asset_path =
                            AssetPath::new_project_dir(project_dir.clone(), asset_path.clone());
                        scripts.load_script(asset_path.clone());
                        asset_paths.push(asset_path);
                    }
                    ecs_world.insert_one(
                        id,
                        ScriptableEntity {
                            scripts: asset_paths,
                        },
                    );
                }
                EditorGameComponentAsset::RigidBody { mass } => {
                    ecs_world.insert_one(id, RigidBody::new(*mass));
                }
                EditorGameComponentAsset::Colliders(colliders) => {
                    ecs_world.insert_one(id, colliders.clone());
                }
            }
        }

        return id;
    }
}
