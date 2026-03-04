use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use uuid::Uuid;

use crate::{material::MaterialBankDeserializer, world::region_map::RegionMap};

use crate::asset::{
    asset::{AssetPath, Assets},
    repr::TextAsset,
};
use crate::entity::{
    component::{GameComponentDeserializeContext, GameComponentSerializeContext},
    ecs_world::{ECSWorld, Entity, ProjectSceneEntitiesVisitor},
    EntityChildren, EntityParent, GameEntity,
};
use crate::graphics::camera::MainCamera;
use crate::material::{Material, MaterialBank};
use crate::physics::physics_world::PhysicsWorld;
use crate::voxel::voxel_registry::VoxelModelRegistry;
use serde::{ser::SerializeStruct, Deserializer};

#[derive(Clone)]
pub struct ProjectSettings {
    pub terrain_asset_path: Option<PathBuf>,
    pub game_camera: Option<Entity>,
}

impl ProjectSettings {
    pub fn new_empty() -> Self {
        Self {
            terrain_asset_path: None,
            game_camera: None,
        }
    }

    pub fn as_serializable(&self, ecs_world: &ECSWorld) -> ProjectSettingsSerializable {
        let game_camera_uuid = self
            .game_camera
            .map(|e| ecs_world.get::<&GameEntity>(e).unwrap().uuid.clone());
        ProjectSettingsSerializable {
            terrain_asset_path: self.terrain_asset_path.clone(),
            game_camera: game_camera_uuid,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ProjectSettingsSerializable {
    pub terrain_asset_path: Option<PathBuf>,
    pub game_camera: Option<Uuid>,
}

pub struct ProjectAssetRaw {
    data: String,
}

pub struct ProjectAsset {
    pub project_dir: Option<PathBuf>,
    pub settings: ProjectSettings,
    pub ecs_world: ECSWorld,
    pub physics_world: PhysicsWorld,
    pub voxel_registry: VoxelModelRegistry,
    pub material_bank: MaterialBank,
}

pub struct ProjectSerializeContext<'a> {
    pub ecs_world: &'a ECSWorld,
    pub physics_world: &'a PhysicsWorld,
    pub voxel_registry: &'a VoxelModelRegistry,
    pub material_bank: &'a MaterialBank,
    pub main_camera: &'a MainCamera,
    pub region_map: &'a RegionMap,
    pub game_camera: Option<Entity>,
}

impl ProjectAsset {
    pub fn new_empty(ecs_world: ECSWorld) -> Self {
        Self {
            project_dir: None,
            settings: ProjectSettings::new_empty(),
            ecs_world,
            physics_world: PhysicsWorld::new(),
            voxel_registry: VoxelModelRegistry::new(),
            material_bank: MaterialBank::new(),
        }
    }

    pub fn from_existing_raw(project_dir: &Path, ecs_world: ECSWorld) -> anyhow::Result<Self> {
        let json_text = Assets::load_asset_sync::<TextAsset>(AssetPath::new_project_file(
            project_dir.to_owned(),
        ))?;
        let mut de = serde_json::Deserializer::from_str(&json_text.contents);

        const FIELDS: [&str; 3] = ["materials", "project_settings", "scene"];
        Ok(de.deserialize_struct(
            "project",
            &FIELDS,
            ProjectVisitor {
                project_dir: project_dir.to_path_buf(),
                ecs_world,
            },
        )?)
    }

    pub fn serialize(context: ProjectSerializeContext<'_>) -> anyhow::Result<TextAsset> {
        let project_settings = ProjectSettings {
            terrain_asset_path: context.region_map.regions_data_path.clone(),
            game_camera: context.game_camera,
        };

        let mut str = serde_json::to_string_pretty(&ProjectSerializer {
            project_settings: project_settings.as_serializable(context.ecs_world),
            ecs_world: context.ecs_world,
            physics_world: context.physics_world,
            voxel_registry: context.voxel_registry,
            material_bank: context.material_bank,
        })?;
        return Ok(TextAsset { contents: str });
    }
}

struct ProjectSerializer<'a> {
    project_settings: ProjectSettingsSerializable,
    ecs_world: &'a ECSWorld,
    physics_world: &'a PhysicsWorld,
    voxel_registry: &'a VoxelModelRegistry,
    material_bank: &'a MaterialBank,
}

impl serde::Serialize for ProjectSerializer<'_> {
    fn serialize<S>(&self, mut ser: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = ser.serialize_struct("project", 2)?;
        s.serialize_field("project_settings", &self.project_settings);

        s.serialize_field("materials", &self.material_bank);

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
    ecs_world: ECSWorld,
}
#[derive(serde::Deserialize)]
#[serde(field_identifier, rename_all = "snake_case")]
enum ProjectField {
    ProjectSettings,
    Scene,
    Materials,
}

impl<'de> serde::de::Visitor<'de> for ProjectVisitor {
    type Value = ProjectAsset;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("project thingy")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut voxel_registry = VoxelModelRegistry::new();
        let mut ecs_world = self.ecs_world;
        let mut physics_world = PhysicsWorld::new();
        let mut material_bank = MaterialBank::new();
        let mut uuid_to_entity_map = HashMap::new();
        let mut project_settings_ser = None;
        let mut visited_scene = false;
        let mut visited_materials = false;
        while let Some(key) = map.next_key::<ProjectField>()? {
            match key {
                ProjectField::ProjectSettings => {
                    if project_settings_ser.is_some() {
                        return Err(serde::de::Error::duplicate_field("project_settings"));
                    }
                    project_settings_ser = Some(map.next_value::<ProjectSettingsSerializable>()?);
                }
                ProjectField::Scene => {
                    if visited_scene {
                        return Err(serde::de::Error::duplicate_field("scene"));
                    }
                    visited_scene = true;
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
                ProjectField::Materials => {
                    if visited_materials {
                        return Err(serde::de::Error::duplicate_field("materials"));
                    }
                    visited_materials = true;
                    map.next_value_seed(MaterialBankDeserializer {
                        material_bank: &mut material_bank,
                    })?;
                }
            }
        }

        let project_settings_ser = project_settings_ser
            .ok_or_else(|| serde::de::Error::missing_field("project_settings"))?;

        if !visited_materials {
            log::error!("Project is missing materials field parsing");
        }
        if !visited_scene {
            log::error!("Project is missing scene field while parsing");
        }

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

        Ok(ProjectAsset {
            project_dir: Some(self.project_dir),
            settings: project_settings,
            ecs_world,
            physics_world,
            voxel_registry,
            material_bank,
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
