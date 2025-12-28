use crate::engine::asset::repr::editor_settings::{self, EditorUserSettingsAsset};
use crate::engine::asset::repr::project::EditorProjectAsset;
use crate::engine::entity::RenderableVoxelEntity;
use crate::engine::task::task_arbiter::TaskArbiter;
use crate::engine::voxel::voxel_events;
use crate::engine::world::World;
use crate::session::EditorSession;
use crate::{app::App, settings::Settings};

use crate::{consts, engine};

use super::asset::asset::AssetPath;
use super::asset::repr::settings::UserSettingsAsset;
use super::debug::DebugRenderer;
use super::editor::editor::Editor;
use super::entity::scripting::Scripts;
use super::graphics::camera::MainCamera;
use super::{
    asset::asset::Assets,
    audio::Audio,
    entity::ecs_world::ECSWorld,
    event::Events,
    input::Input,
    physics::physics_world::PhysicsWorld,
    window::{time::Time, window::Window},
};

/// We have the window resource inserted before this but that is it.
pub fn init_pre_graphics(app: &mut App) {
    app.insert_resource(TaskArbiter::new());
    app.insert_resource(Assets::new());
    app.insert_resource(Events::new());

    init_editor_project(app);

    app.insert_resource(Scripts::new());

    let asset_path = AssetPath::new_user_dir(consts::io::GAME_USER_SETTINGS_FILE);
    let settings = Settings::from(&match Assets::load_asset_sync::<UserSettingsAsset>(
        asset_path.clone(),
    ) {
        Ok(settings) => {
            log::info!("Using existing user settings.");
            settings
        }
        Err(_) => {
            log::info!(
                "Existing user settings not found, creating new user settings {:?}.",
                asset_path
            );
            UserSettingsAsset::default()
        }
    });
    app.insert_resource(settings);
    app.insert_resource(Input::new());
    app.insert_resource(Time::new());
    app.insert_resource(Audio::new());
    app.insert_resource(MainCamera::new_empty());
}

/// Initializes the ECSWorld, EditorSession, PhysicsWorld, and Editor.
pub fn init_editor_project(app: &mut App) {
    let mut editor_settings = Assets::load_asset_sync::<EditorUserSettingsAsset>(
        AssetPath::new_user_dir(consts::io::EDITOR_USER_SETTINGS_FILE),
    )
    .unwrap_or(EditorUserSettingsAsset {
        last_project_dir: None,
    });

    // Ensure last project still exists.
    if let Some(last_project_dir) = editor_settings.last_project_dir.as_ref() {
        if std::fs::read_dir(last_project_dir).is_err() {
            editor_settings.last_project_dir = None;
        }
    }

    let project = editor_settings
        .last_project_dir
        .as_ref()
        .map(|last_project_dir| {
            EditorProjectAsset::from_existing_raw(last_project_dir)
                .map_err(|err| {
                    log::error!(
                        "Error when trying to deserialize last project. Error: {:?}",
                        err
                    );
                    err
                })
                .ok()
        })
        .unwrap_or(None);

    if project.is_none() {
        editor_settings.last_project_dir = None;
    }

    let project = project.unwrap_or_else(|| EditorProjectAsset::new_empty());

    // Initialize project assets.
    let mut events = app.get_resource_mut::<Events>();
    for (entity, renderable) in project
        .ecs_world
        .query::<&RenderableVoxelEntity>()
        .into_iter()
    {
        events.push(voxel_events::EventVoxelRenderableEntityLoad {
            entity,
            reload: false,
        });
    }
    drop(events);

    app.insert_resource(project.ecs_world);
    app.insert_resource(project.physics_world);
    app.insert_resource(project.material_bank);
    app.insert_resource(Editor::new(project.editor_settings));
    app.insert_resource(EditorSession::new(editor_settings, project.settings));
}

/// The graphics `DeviceResource` has been inserted before this.
pub fn init_post_graphics(app: &mut App) {
    engine::ui::initialize_debug_ui_resource(app);

    app.insert_resource(DebugRenderer::new());
    engine::graphics::initialize_graphics_resources(app);
    app.insert_resource(MaterialBankGpu::new());

    app.insert_resource(World::new());
    engine::voxel::initialize_voxel_world_resources(app);
}
