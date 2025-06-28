use crate::session::Session;
use crate::{app::App, settings::Settings};

use crate::{consts, engine};

use super::asset::asset::AssetPath;
use super::asset::repr::settings::SettingsAsset;
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
    app.insert_resource(Assets::new());
    app.insert_resource(Events::new());
    app.insert_resource(Session::new());
    app.insert_resource(Scripts::new());

    let settings = Settings::from(&match Assets::load_asset_sync::<SettingsAsset>(
        AssetPath::new_user_dir(consts::io::SETTINGS_FILE),
    ) {
        Ok(settings) => {
            log::info!("Using existing settings.");
            settings
        }
        Err(_) => {
            log::info!("Existing settings not found, creating new settings.");
            SettingsAsset::default()
        }
    });
    app.insert_resource(settings);
    app.insert_resource(ECSWorld::new());
    app.insert_resource(Input::new());
    app.insert_resource(Time::new());
    app.insert_resource(PhysicsWorld::new());
    app.insert_resource(Audio::new());
    app.insert_resource(MainCamera::new_empty());
}

/// The graphics `DeviceResource` has been inserted before this.
pub fn init_post_graphics(app: &mut App) {
    engine::ui::initialize_debug_ui_resource(app);

    app.insert_resource(Editor::new());
    app.insert_resource(DebugRenderer::new());
    engine::graphics::initialize_graphics_resources(app);
    engine::voxel::initialize_voxel_world_resources(app);

    app.run_system(Session::init);
}
