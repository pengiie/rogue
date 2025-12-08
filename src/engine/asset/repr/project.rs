use std::{
    collections::HashMap,
    f32,
    ops::Deref,
    path::{Path, PathBuf},
    str::FromStr,
};

use nalgebra::{Translation3, UnitQuaternion, Vector3};
use uuid::Uuid;

use crate::{
    engine::{
        asset::{
            asset::{AssetFile, AssetHandle, AssetLoadError, AssetLoader, AssetPath, Assets},
            repr::{game_entity::WorldGameEntityAsset, TextAsset},
        },
        editor::editor::Editor,
        entity::{
            component::{self, GameComponentDeserializeContext, GameComponentSerializeContext},
            ecs_world::{self, ECSWorld, Entity, ProjectSceneEntitiesVisitor},
            scripting::{ScriptableEntity, Scripts},
            EntityChildren, EntityParent, GameEntity, RenderableVoxelEntity,
        },
        graphics::camera::Camera,
        physics::{
            capsule_collider::CapsuleCollider, collider_registry::ColliderRegistry,
            physics_world::PhysicsWorld, plane_collider::PlaneCollider, rigid_body::RigidBody,
            transform::Transform,
        },
        voxel::{
            voxel::VoxelModelImpl, voxel_registry::VoxelModelRegistry, voxel_world::VoxelWorld,
        },
    },
    session::{
        EditorSession, ProjectEditorSettings, ProjectSettings, ProjectSettingsSerializable,
        RenderableEntityLoad,
    },
};

use serde::{ser::SerializeStruct, Deserializer};

use super::{
    components::{CameraAsset, TransformAsset},
    voxel::any::VoxelModelAnyAsset,
};

pub struct EditorProjectRaw {
    data: String,
}

pub struct EditorProjectAsset {
    pub project_dir: Option<PathBuf>,
    pub editor_settings: ProjectEditorSettings,
    pub settings: ProjectSettings,
    pub ecs_world: ECSWorld,
    pub physics_world: PhysicsWorld,
    pub voxel_registry: VoxelModelRegistry,
}

impl EditorProjectAsset {
    pub fn new_empty() -> Self {
        Self {
            project_dir: None,
            editor_settings: ProjectEditorSettings::new_empty(),
            settings: ProjectSettings::new_empty(),
            ecs_world: ECSWorld::new(),
            physics_world: PhysicsWorld::new(),
            voxel_registry: VoxelModelRegistry::new(),
        }
    }

    pub fn from_existing_raw(project_dir: &Path) -> anyhow::Result<Self> {
        let json_text = Assets::load_asset_sync::<TextAsset>(AssetPath::new_project_file(
            project_dir.to_owned(),
        ))?;
        let mut de = serde_json::Deserializer::from_str(&json_text.contents);

        const FIELDS: [&str; 3] = ["editor_settings", "project_settings", "entities"];
        Ok(de.deserialize_struct(
            "project",
            &FIELDS,
            ProjectVisitor {
                project_dir: project_dir.to_path_buf(),
            },
        )?)
    }

    pub fn serialize(
        session: &EditorSession,
        editor: &Editor,
        ecs_world: &ECSWorld,
        physics_world: &PhysicsWorld,
        voxel_registry: &VoxelModelRegistry,
    ) -> anyhow::Result<TextAsset> {
        let mut str = serde_json::to_string_pretty(&ProjectSerializer {
            session,
            editor,
            ecs_world,
            physics_world,
            voxel_registry,
        })?;
        return Ok(TextAsset { contents: str });
    }
}

struct ProjectSerializer<'a> {
    session: &'a EditorSession,
    editor: &'a Editor,
    ecs_world: &'a ECSWorld,
    physics_world: &'a PhysicsWorld,
    voxel_registry: &'a VoxelModelRegistry,
}

impl serde::Serialize for ProjectSerializer<'_> {
    fn serialize<S>(&self, mut ser: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = ser.serialize_struct("project", 2)?;
        s.serialize_field(
            "editor_settings",
            &self.editor.editor_settings(self.ecs_world),
        );
        s.serialize_field(
            "project_settings",
            &self.session.project.serialize(self.ecs_world),
        );

        let entity_uuid_map = self
            .ecs_world
            .query::<&GameEntity>()
            .into_iter()
            .map(|(entity, game_entity)| (entity, game_entity.uuid))
            .collect::<HashMap<_, _>>();
        s.serialize_field(
            "scene",
            &self
                .ecs_world
                .serialize_world(&GameComponentSerializeContext {
                    voxel_registry: self.voxel_registry,
                    collider_registry: &self.physics_world.colliders,
                    entity_uuid_map: &entity_uuid_map,
                }),
        );
        s.end()
    }
}

struct ProjectVisitor {
    project_dir: PathBuf,
}
#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum ProjectField {
    ProjectSettings,
    EditorSettings,
    Scene,
}

impl<'de> serde::de::Visitor<'de> for ProjectVisitor {
    type Value = EditorProjectAsset;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("project thingy")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut voxel_registry = VoxelModelRegistry::new();
        let mut ecs_world = ECSWorld::new();
        let mut physics_world = PhysicsWorld::new();
        let mut uuid_to_entity_map = HashMap::new();
        let mut editor_settings = None;
        let mut project_settings_ser = None;
        while let Some(key) = map.next_key::<ProjectField>()? {
            match key {
                ProjectField::ProjectSettings => {
                    if project_settings_ser.is_some() {
                        return Err(serde::de::Error::duplicate_field("project_settings"));
                    }
                    project_settings_ser = Some(map.next_value::<ProjectSettingsSerializable>()?);
                }
                ProjectField::EditorSettings => {
                    if editor_settings.is_some() {
                        return Err(serde::de::Error::duplicate_field("editor_settings"));
                    }
                    editor_settings = Some(map.next_value::<ProjectEditorSettings>()?);
                }
                ProjectField::Scene => {
                    let visitor = ProjectSceneVisitor {
                        ctx: &mut ProjectSceneDeserializeContext {
                            ecs_world: &mut ecs_world,
                            uuid_to_entity_map: &mut uuid_to_entity_map,
                            // State used within deserialization, just hoisted up here for convenience.
                            to_parent_entities: &mut Vec::new(),
                            component_ctx: &mut GameComponentDeserializeContext {
                                voxel_registry: &mut voxel_registry,
                                collider_registry: &mut physics_world.colliders,
                                entity_parent: uuid::Uuid::nil(),
                            },
                        },
                    };
                    map.next_value_seed(visitor)?;
                }
            }
        }

        let editor_settings =
            editor_settings.ok_or_else(|| serde::de::Error::missing_field("editor_settings"))?;
        let project_settings_ser = project_settings_ser
            .ok_or_else(|| serde::de::Error::missing_field("project_settings"))?;

        let game_camera = project_settings_ser
            .game_camera
            .map(|uuid| {
                uuid_to_entity_map.get(&uuid).ok_or_else(|| {
                    serde::de::Error::custom(
                        "Game camera contains uuid of an entity that doesn't exist",
                    )
                })
            })
            .transpose()?
            .map(|e| *e);

        let project_settings = ProjectSettings {
            terrain_asset_path: project_settings_ser.terrain_asset_path,
            game_camera,
        };

        Ok(EditorProjectAsset {
            project_dir: Some(self.project_dir),
            editor_settings,
            settings: project_settings,
            ecs_world,
            physics_world,
            voxel_registry,
        })
    }
}

pub struct ProjectSceneDeserializeContext<'a> {
    pub ecs_world: &'a mut ECSWorld,
    pub component_ctx: &'a mut GameComponentDeserializeContext<'a>,
    pub uuid_to_entity_map: &'a mut HashMap<uuid::Uuid, Entity>,
    pub to_parent_entities: &'a mut Vec<(/*self*/ Entity /*parent*/, uuid::Uuid)>,
}

pub struct ProjectSceneVisitor<'a> {
    pub ctx: &'a mut ProjectSceneDeserializeContext<'a>,
}

#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum ProjectSceneField {
    Entities,
}

impl<'de> serde::de::DeserializeSeed<'de> for ProjectSceneVisitor<'_> {
    type Value = ();

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        const FIELDS: [&str; 1] = ["entities"];
        deserializer.deserialize_struct("Scene", &FIELDS, self)
    }
}

impl<'de> serde::de::Visitor<'de> for ProjectSceneVisitor<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("failed while visiting struct Scene")
    }

    fn visit_map<A>(mut self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut entities_visitor = ProjectSceneEntitiesVisitor { ctx: self.ctx };
        let mut parsed_entities = false;
        while let Some(key) = map.next_key::<ProjectSceneField>()? {
            match key {
                ProjectSceneField::Entities => {
                    if parsed_entities {
                        return Err(serde::de::Error::duplicate_field("entities"));
                    }
                    parsed_entities = true;
                    map.next_value_seed(&mut entities_visitor)?;
                }
            }
        }

        if !parsed_entities {
            return Err(serde::de::Error::custom(
                "Scene does not contain an `entities` field.",
            ));
        }

        // Populate entity_uuid_map via ECS query.
        for (entity, game_entity) in self.ctx.ecs_world.query::<&GameEntity>().into_iter() {
            let old = self.ctx.uuid_to_entity_map.insert(game_entity.uuid, entity);
            if old.is_some() {
                return Err(serde::de::Error::custom(format!(
                    "Scene contains duplicate entity uuid of {}.",
                    game_entity.uuid
                )));
            }
        }

        // Populate the EntityParent and EntityChildren entity references.
        for (child_entity, parent_uuid) in self.ctx.to_parent_entities.drain(..) {
            let mut parent_component = self.ctx.ecs_world.get::<&mut EntityParent>(child_entity)
                .expect("If entity is in to_parent_entities but doesnt have an EntityParent component something logic wise went wrong.");
            let parent_entity_id = self
                .ctx
                .uuid_to_entity_map
                .get(&parent_uuid)
                .unwrap_or_else(|| {
                    panic!(
                        "Entity references parent with uuid {} but that doesn't exist",
                        parent_uuid.to_string()
                    )
                });
            parent_component.set_parent(*parent_entity_id);

            let mut children_component = self.ctx.ecs_world.get::<&mut EntityChildren>(*parent_entity_id)
                .expect("If entity is a parent in to_parent_entities but doesnt have an EntityChildren component something logic wise went wrong.");
            children_component.children.insert(child_entity);
        }

        Ok(())
    }
}
