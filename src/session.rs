use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    path::PathBuf,
    str::FromStr,
    sync::mpsc::{Receiver, Sender},
    time::Duration,
};

use hecs::With;
use rogue_macros::Resource;

use crate::{
    consts,
    engine::{
        asset::{
            asset::{AssetHandle, AssetPath, Assets},
            repr::{
                editor_settings::EditorSettingsAsset, project::EditorProjectAsset,
                voxel::any::VoxelModelAnyAsset,
            },
        },
        editor::editor::Editor,
        entity::{
            ecs_world::{ECSWorld, Entity},
            scripting::Scripts,
            GameEntity, RenderableVoxelEntity,
        },
        graphics::camera::{Camera, MainCamera},
        physics::{
            collider_registry::ColliderRegistry,
            physics_world::{self, PhysicsWorld},
            transform::Transform,
        },
        resource::{Res, ResMut},
        ui::UI,
        voxel::{
            flat::VoxelModelFlat,
            thc::VoxelModelTHCCompressed,
            voxel::{VoxelModel, VoxelModelType},
            voxel_world::VoxelWorld,
        },
        window::time::Timer,
    },
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionState {
    Editor,
    Game,
    GamePaused,
}

#[derive(Resource)]
pub struct Session {
    pub editor_settings: EditorSettingsAsset,
    pub project: EditorProjectAsset,

    pub autosave_timer: Timer,
    pub project_save_dir: Option<PathBuf>,
    pub terrain_dir: Option<PathBuf>,
    pub game_camera: Option<Entity>,

    pub session_state: SessionState,
    pub editor_ecs_world: Option<ECSWorld>,
    pub should_start_game: bool,
    pub should_stop_game: bool,

    pub loading_renderables: HashMap<Entity, AssetHandle>,
}

impl Session {
    pub fn new() -> Self {
        let mut editor_settings = Assets::load_asset_sync::<EditorSettingsAsset>(
            AssetPath::new_user_dir(consts::io::EDITOR_SETTINGS_FILE),
        )
        .unwrap_or(EditorSettingsAsset {
            last_project_dir: None,
        });

        if let Some(last_project_dir) = editor_settings.last_project_dir.as_ref() {
            if std::fs::read_dir(last_project_dir).is_err() {
                editor_settings.last_project_dir = None;
            }
        }

        let mut load_error = None;
        let project = editor_settings
            .last_project_dir
            .as_ref()
            .map(|last_project_dir| {
                let res = Assets::load_asset_sync::<EditorProjectAsset>(AssetPath::new(
                    last_project_dir.join("project.json"),
                ));
                if res.is_err() {
                    load_error = Some(res.as_ref().err().unwrap().to_string());
                }
                res.ok()
            })
            .unwrap_or_else(|| None)
            .unwrap_or_else(|| EditorProjectAsset::new_empty());

        let project_save_dir = if let Some(load_error) = load_error {
            log::error!(
                "Failed to load previous project data at {:?}, error: {}",
                editor_settings.last_project_dir.as_ref().unwrap(),
                load_error
            );
            None
        } else {
            editor_settings.last_project_dir.clone()
        };

        Self {
            editor_settings,
            project,
            session_state: SessionState::Editor,
            editor_ecs_world: None,
            should_start_game: false,
            should_stop_game: false,

            project_save_dir,
            terrain_dir: None,
            autosave_timer: Timer::new(Duration::from_secs(5)),
            game_camera: None,

            loading_renderables: HashMap::new(),
        }
    }

    pub fn project_assets_dir(&self) -> Option<PathBuf> {
        self.project_save_dir.as_ref().map(|p| p.join("assets"))
    }

    pub fn init_from_project_asset(
        mut session: ResMut<Session>,
        mut ecs_world: ResMut<ECSWorld>,
        mut assets: ResMut<Assets>,
        mut scripts: ResMut<Scripts>,
        mut physics_world: ResMut<PhysicsWorld>,
    ) {
        let session: &mut Session = &mut session;

        // Add all top-level game entities.
        let mut uuid_map = HashMap::new();
        let mut to_add_children = Vec::new();
        for entity in &session.project.game_entities {
            if entity.parent.is_some() {
                to_add_children.push(entity);
                continue;
            }
            let entity_id = entity.spawn(
                session.project_save_dir.clone().unwrap(),
                &mut ecs_world,
                &mut assets,
                &mut session.loading_renderables,
                &mut scripts,
            );
            uuid_map.insert(uuid::Uuid::from_str(&entity.uuid).unwrap(), entity_id);
            if Some(&entity.uuid) == session.project.game_camera.as_ref() {
                session.game_camera = Some(entity_id);
            }
        }

        // Collect collider information and load into physics world. Ensuring collider ids match
        // properly with what is referenced in entity components.
        physics_world.colliders = ColliderRegistry::from(&session.project.collider_registry);

        // Add children if their parent uuid is spawned in until we spawned all entities in the
        // project asset file.
        while !to_add_children.is_empty() {
            to_add_children = to_add_children
                .into_iter()
                .filter(|entity| {
                    let Some(parent) = uuid_map
                        .get(&uuid::Uuid::from_str(entity.parent.as_ref().unwrap()).unwrap())
                    else {
                        return true;
                    };
                    let entity_id = entity.spawn(
                        session.project_save_dir.clone().unwrap(),
                        &mut ecs_world,
                        &mut assets,
                        &mut session.loading_renderables,
                        &mut scripts,
                    );
                    ecs_world.set_parent(entity_id, *parent);
                    uuid_map.insert(uuid::Uuid::from_str(&entity.uuid).unwrap(), entity_id);
                    if Some(&entity.uuid) == session.project.game_camera.as_ref() {
                        session.game_camera = Some(entity_id);
                    }
                    return false;
                })
                .collect::<Vec<_>>();
        }

        // Assert that each entities colliders exist in the registry.
        physics_world.validate_colliders_exist(&mut ecs_world);
    }

    pub fn can_start_game(&self) -> bool {
        self.game_camera.is_some()
    }

    pub fn start_game(&mut self) {
        assert_eq!(self.session_state, SessionState::Editor);
        if !self.can_start_game() {
            return;
        }
        self.should_start_game = true;
    }

    pub fn stop_game(&mut self) {
        assert_ne!(self.session_state, SessionState::Editor);
        self.should_stop_game = true;
    }

    pub fn new_project(
        &mut self,
        ecs_world: &mut ECSWorld,
        new_project_path: PathBuf,
        voxel_world: &mut VoxelWorld,
    ) {
        let mut existing_entities_query = ecs_world.query::<With<(), &GameEntity>>();
        let existing_entities = existing_entities_query
            .into_iter()
            .map(|(entity_id, _)| entity_id)
            .collect::<Vec<_>>();
        drop(existing_entities_query);
        for id in existing_entities {
            ecs_world.despawn(id);
        }

        self.project_save_dir = Some(new_project_path.clone());
        self.editor_settings.last_project_dir = Some(new_project_path);
        self.project = EditorProjectAsset::new_empty();
        self.terrain_dir = None;
        self.game_camera = None;
        voxel_world.chunks.clear();
    }

    pub fn update(
        mut session: ResMut<Session>,
        mut assets: ResMut<Assets>,
        mut voxel_world: ResMut<VoxelWorld>,
        mut main_camera: ResMut<MainCamera>,
        mut ecs_world: ResMut<ECSWorld>,
        mut editor: ResMut<Editor>,
        mut scripts: ResMut<Scripts>,
        mut ui: ResMut<UI>,
        mut physics_world: ResMut<PhysicsWorld>,
    ) {
        let session: &mut Session = &mut session;

        let mut completed = HashSet::new();
        for (entity, asset_handle) in &session.loading_renderables {
            match assets.get_asset_status(asset_handle) {
                crate::engine::asset::asset::AssetStatus::Loaded => {}
                crate::engine::asset::asset::AssetStatus::NotFound => {
                    log::error!("Voxel model not found trying to load {:?}", asset_handle)
                }
                crate::engine::asset::asset::AssetStatus::Error(error) => log::error!(
                    "Voxel model had an error trying to load {:?}: {:?}",
                    asset_handle,
                    error
                ),
                _ => continue,
            }
            completed.insert(*entity);
            let Ok(mut renderable) = ecs_world.get::<&mut RenderableVoxelEntity>(*entity) else {
                continue;
            };

            let model = *assets
                .take_asset::<VoxelModelAnyAsset>(asset_handle)
                .unwrap();
            let model_id = voxel_world.registry.register_renderable_voxel_model_any(
                format!(
                    "asset_{:?}",
                    asset_handle.asset_path().asset_path.as_ref().unwrap()
                ),
                model,
            );
            voxel_world
                .registry
                .set_voxel_model_asset_path(model_id, Some(asset_handle.asset_path().clone()));
            log::info!(
                "Settings asset path voxel for {:?} {:?}",
                asset_handle.asset_path(),
                model_id
            );
            voxel_world.to_update_normals.insert(model_id);
            renderable.set_id(model_id);
        }
        for e in completed {
            session.loading_renderables.remove(&e);
        }

        if session.should_start_game {
            // Start the game.
            session.should_start_game = false;
            session.session_state = SessionState::Game;
            main_camera.set_camera(session.game_camera.as_ref().unwrap().clone(), "game_camera");
            session.editor_ecs_world = Some(ecs_world.clone_game_entities());
            scripts.run_setup(&mut ecs_world, &assets, &mut ui);
            physics_world.reset_last_timestep();
        }

        if session.should_stop_game {
            session.should_stop_game = false;
            session.session_state = SessionState::Editor;
            let (editor_camera, editor_transform) = ecs_world
                .query_one_mut::<(&Camera, &Transform)>(editor.editor_camera_entity.unwrap())
                .unwrap();
            let editor_camera = editor_camera.clone();
            let editor_transform = editor_transform.clone();
            // Reset the ECS world to the old state with just game entities and the editor camera.
            *ecs_world = session.editor_ecs_world.take().unwrap();
            editor.editor_camera_entity = Some(ecs_world.spawn((editor_camera, editor_transform)));
            main_camera.set_camera(
                editor
                    .editor_camera_entity
                    .expect("Editor camera should exist"),
                "Editor camera",
            );
            log::info!("Stopping game");
            // We must respawn the editor camera.
        }

        if session.autosave_timer.try_complete() {
            //assets.save_asset(
            //    AssetPath::new_user_dir(consts::io::EDITOR_SETTINGS_FILE),
            //    session.editor_settings.clone(),
            //);

            //if let Some(save_dir) = &session.project_save_dir {
            //    assets.save_asset(
            //        AssetPath::new(save_dir.join("project.json")),
            //        session
            //            .project
            //            .new_existing(&editor, &ecs_world, &voxel_world.registry),
            //    );
            //}
        }
    }

    pub fn save_project(
        &self,
        assets: &mut Assets,
        session: &Session,
        editor: &Editor,
        ecs_world: &ECSWorld,
        voxel_world: &VoxelWorld,
        physics_world: &PhysicsWorld,
    ) {
        assets.save_asset(
            AssetPath::new_user_dir(consts::io::EDITOR_SETTINGS_FILE),
            session.editor_settings.clone(),
        );

        if let Some(save_dir) = &session.project_save_dir {
            assets.save_asset(
                AssetPath::new(save_dir.join("project.json")),
                session.project.new_existing(
                    editor,
                    ecs_world,
                    voxel_world,
                    self.terrain_dir.clone(),
                    self.game_camera.clone(),
                    &physics_world.colliders,
                ),
            );
        }
    }
}

pub struct RenderableEntityLoad {
    pub asset_handle: AssetHandle,
    pub renderable_entity: Entity,
}
