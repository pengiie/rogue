use crate::{app::App, settings::Settings};

use crate::{consts, engine};

use super::asset::asset::AssetPath;
use super::asset::repr::settings::SettingsAsset;
use super::graphics::camera::MainCamera;
use super::world::game_world::GameWorld;
use super::{
    asset::asset::Assets,
    audio::Audio,
    ecs::ecs_world::ECSWorld,
    event::Events,
    input::Input,
    physics::physics_world::PhysicsWorld,
    window::{time::Time, window::Window},
};

/// We have the window resource inserted before this but that is it.
pub fn init_pre_graphics(app: &mut App) {
    app.insert_resource(Assets::new());
    app.insert_resource(Events::new());

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
    app.insert_resource(MainCamera::new());
    let game_world = GameWorld::new(&app.get_resource::<Settings>());
    app.insert_resource(game_world);
}

/// The graphics `DeviceResource` has been inserted before this.
pub fn init_post_graphics(app: &mut App) {
    engine::ui::initialize_debug_ui_resource(app);

    engine::graphics::initialize_graphics_resources(app);
    engine::voxel::initialize_voxel_world_resources(app);
}
