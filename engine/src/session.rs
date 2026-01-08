use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    path::PathBuf,
    str::FromStr,
    sync::mpsc::{Receiver, Sender},
    time::Duration,
};

use nalgebra::{Translation3, Vector3};
use rogue_macros::Resource;
use serde::ser::SerializeStruct;
use uuid::Uuid;

use crate::{
    consts,
    engine::{
        asset::{
            asset::{impl_asset_load_save_serde, AssetHandle, AssetPath, Assets},
            repr::project::ProjectAsset,
        },
        editor::editor::Editor,
        entity::{
            component::GameComponentCloneContext,
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
        voxel::{flat::VoxelModelFlat, thc::VoxelModelTHCCompressed},
        window::time::Timer,
    },
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionState {
    Editor,
    Game,
    GamePaused,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ProjectEditorSettings {
    pub editor_camera_transform: Transform,
    pub editor_camera: Camera,
    pub editor_rotation_anchor: Vector3<f32>,
}

impl ProjectEditorSettings {
    pub fn new_empty() -> Self {
        Self {
            editor_camera_transform: Transform::with_translation(Translation3::new(
                -5.0, 5.0, -5.0,
            )),
            editor_camera: Camera::new(std::f32::consts::FRAC_PI_2),
            editor_rotation_anchor: Vector3::zeros(),
        }
    }
}

impl_asset_load_save_serde!(ProjectEditorSettings);

/// Manages the state of the project and handles the serialization/deserialization and
/// initialization of:
/// - editor
/// - ecs world
/// - voxel model
/// - terrain
/// - physics.
#[derive(Resource)]
pub struct EditorSession {
    pub autosave_timer: Timer,
    pub project_save_dir: Option<PathBuf>,

    /// The current project open in the editor.
    pub project: ProjectSettings,

    pub session_state: SessionState,
    pub editor_ecs_world: Option<ECSWorld>,
    pub should_start_game: bool,
    pub should_stop_game: bool,

    pub loading_renderables: HashMap<Entity, AssetHandle>,
}

impl EditorSession {
    pub fn new(
        editor_settings: EditorUserSettingsAsset,
        project_settings: ProjectSettings,
    ) -> Self {
        // let mut editor_settings = Assets::load_asset_sync::<EditorSettingsAsset>(
        //     AssetPath::new_user_dir(consts::io::EDITOR_SETTINGS_FILE),
        // )
        // .unwrap_or(EditorSettingsAsset {
        //     last_project_dir: None,
        // });

        // if let Some(last_project_dir) = editor_settings.last_project_dir.as_ref() {
        //     if std::fs::read_dir(last_project_dir).is_err() {
        //         editor_settings.last_project_dir = None;
        //     }
        // }

        // let project_save_dir = if let Some(load_error) = load_error {
        //     log::error!(
        //         "Failed to load previous project data at {:?}, error: {}",
        //         editor_settings.last_project_dir.as_ref().unwrap(),
        //         load_error
        //     );
        //     None
        // } else {
        //     editor_settings.last_project_dir.clone()
        // };

        Self {
            session_state: SessionState::Editor,
            editor_ecs_world: None,
            should_start_game: false,
            should_stop_game: false,

            project: project_settings,
            project_save_dir: editor_settings.last_project_dir,

            autosave_timer: Timer::new(Duration::from_secs(5)),

            loading_renderables: HashMap::new(),
        }
    }

    pub fn terrain_dir(&self) -> Option<&PathBuf> {
        self.project.terrain_asset_path.as_ref()
    }

    pub fn game_camera(&self) -> Option<Entity> {
        self.project.game_camera
    }

    pub fn project_assets_dir(&self) -> Option<PathBuf> {
        self.project_save_dir.as_ref().map(|p| p.join("assets"))
    }

    pub fn can_start_game(&self) -> bool {
        self.project.game_camera.is_some()
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

    // pub fn new_project(
    //     &mut self,
    //     ecs_world: &mut ECSWorld,
    //     new_project_path: PathBuf,
    //     voxel_world: &mut VoxelWorld,
    // ) {
    //     let mut existing_entities_query = ecs_world.query::<()>().with::<(GameEntity,)>();
    //     let existing_entities = existing_entities_query
    //         .into_iter()
    //         .map(|(entity_id, _)| entity_id)
    //         .collect::<Vec<_>>();
    //     for id in existing_entities {
    //         ecs_world.despawn(id);
    //     }

    //     self.project_save_dir = Some(new_project_path.clone());
    //     self.aa12!last_project_dir = Some(new_project_path);
    //     self.project = EditorProjectAsset::new_empty();
    //     self.terrain_dir = None;
    //     self.game_camera = None;
    //     voxel_world.chunks.clear();
    // }

    pub fn update(
        mut session: ResMut<EditorSession>,
        mut assets: ResMut<Assets>,
        mut voxel_world: ResMut<VoxelWorld>,
        mut main_camera: ResMut<MainCamera>,
        mut ecs_world: ResMut<ECSWorld>,
        mut editor: ResMut<Editor>,
        mut scripts: ResMut<Scripts>,
        mut ui: ResMut<UI>,
        mut physics_world: ResMut<PhysicsWorld>,
    ) {
        let session: &mut EditorSession = &mut session;

        if session.should_start_game {
            // Start the game.
            session.should_start_game = false;
            session.session_state = SessionState::Game;
            main_camera.set_camera(
                session.game_camera().as_ref().unwrap().clone(),
                "game_camera",
            );
            session.editor_ecs_world =
                Some(ecs_world.clone_game_entities(GameComponentCloneContext {
                    voxel_world: &mut voxel_world,
                    collider_registry: &mut physics_world.colliders,
                }));
            scripts.run_setup(&mut ecs_world, &assets, &mut ui);
            physics_world.reset_last_timestep();
        }

        if session.should_stop_game {
            session.should_stop_game = false;
            session.session_state = SessionState::Editor;
            let (editor_camera, editor_transform) = ecs_world
                .query_one::<(&Camera, &Transform)>(editor.editor_camera_entity.unwrap())
                .get()
                .unwrap();
            let editor_camera = editor_camera.clone();
            let editor_transform = editor_transform.clone();
            // Reset the ECS world to the old state with just game entities.
            *ecs_world = session.editor_ecs_world.take().unwrap();
            // Respawn editor camera.
            editor.editor_camera_entity = Some(ecs_world.spawn((editor_camera, editor_transform)));
            main_camera.set_camera(
                editor
                    .editor_camera_entity
                    .expect("Editor camera should exist"),
                "Editor camera",
            );

            if let Some(selected_entity) = editor.selected_entity {
                if !ecs_world.contains(selected_entity) {
                    log::info!(
                        "Deselecting entity {:?} as it no longer exists",
                        selected_entity
                    );
                    editor.selected_entity = None;
                }
            }
            editor.hovered_entity = None;
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
        session: &EditorSession,
        editor: &Editor,
        ecs_world: &ECSWorld,
        voxel_world: &mut VoxelWorld,
        physics_world: &mut PhysicsWorld,
        material_bank: &MaterialBank,
    ) {
        let Some(save_dir) = &session.project_save_dir else {
            return;
        };

        assets.save_asset(
            AssetPath::new_user_dir(consts::io::EDITOR_USER_SETTINGS_FILE),
            EditorUserSettingsAsset {
                last_project_dir: session.project_save_dir.clone(),
            },
        );

        let project_asset = match ProjectAsset::serialize(
            session,
            editor,
            ecs_world,
            physics_world,
            &voxel_world.registry,
            material_bank,
        ) {
            Ok(asset) => asset,
            Err(err) => {
                log::error!("Failed to save project, error: \n{:#?}", err);
                return;
            }
        };
        assets.save_asset(AssetPath::new(save_dir.join("project.json")), project_asset);
    }
}

pub struct RenderableEntityLoad {
    pub asset_handle: AssetHandle,
    pub renderable_entity: Entity,
}
