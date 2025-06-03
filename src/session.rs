use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    path::PathBuf,
    sync::mpsc::{Receiver, Sender},
    time::Duration,
};

use rogue_macros::Resource;

use crate::{
    consts,
    engine::{
        asset::{
            asset::{AssetHandle, AssetPath, Assets},
            repr::{
                editor_settings::{EditorProjectAsset, EditorSessionAsset, EditorSettingsAsset},
                world::voxel::VoxelModelAnyAsset,
            },
        },
        editor::editor::Editor,
        entity::{
            ecs_world::{ECSWorld, Entity},
            RenderableVoxelEntity,
        },
        resource::{Res, ResMut},
        voxel::{
            flat::VoxelModelFlat,
            thc::VoxelModelTHCCompressed,
            voxel::{VoxelModel, VoxelModelType},
            voxel_world::VoxelWorld,
        },
        window::time::Timer,
    },
};

#[derive(Resource)]
pub struct Session {
    pub editor_settings: EditorSettingsAsset,
    pub project: EditorProjectAsset,

    pub autosave_timer: Timer,
    pub project_save_dir: Option<PathBuf>,
    pub terrain_dir: Option<PathBuf>,
    pub game_camera: Option<Entity>,

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

        let project = editor_settings
            .last_project_dir
            .as_ref()
            .map(|last_project_dir| {
                Assets::load_asset_sync::<EditorProjectAsset>(AssetPath::new(
                    last_project_dir.join("project.json"),
                ))
                .ok()
            })
            .unwrap_or_else(|| None)
            .unwrap_or_else(|| EditorProjectAsset::new_empty());

        let project_save_dir = editor_settings.last_project_dir.clone();
        Self {
            editor_settings,
            project,

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

    pub fn init(
        mut session: ResMut<Session>,
        mut ecs_world: ResMut<ECSWorld>,
        mut assets: ResMut<Assets>,
    ) {
        let session: &mut Session = &mut session;
        for entity in &session.project.game_entities {
            let entity_id = entity.spawn(
                session.project_save_dir.clone().unwrap(),
                &mut ecs_world,
                &mut assets,
                &mut session.loading_renderables,
            );
            if Some(&entity.uuid) == session.project.game_camera.as_ref() {
                session.game_camera = Some(entity_id);
            }
        }
    }

    pub fn update(
        mut session: ResMut<Session>,
        mut assets: ResMut<Assets>,
        mut voxel_world: ResMut<VoxelWorld>,
        ecs_world: Res<ECSWorld>,
        editor: Res<Editor>,
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
                ),
            );
        }
    }
}

pub struct RenderableEntityLoad {
    pub asset_handle: AssetHandle,
    pub renderable_entity: Entity,
}
