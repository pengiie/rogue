use crate::{app::App, settings::Settings};

use crate::engine;

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
    app.insert_resource(Events::new());
    app.insert_resource(Settings::default());
    app.insert_resource(ECSWorld::new());
    app.insert_resource(Input::new());
    app.insert_resource(Time::new());
    app.insert_resource(Assets::new());
    app.insert_resource(PhysicsWorld::new());
    app.insert_resource(Audio::new());
}

/// The graphics `DeviceResource` has been inserted before this.
pub fn init_post_graphics(app: &mut App) {
    engine::ui::initialize_debug_ui_resource(app);

    engine::graphics::initialize_graphics_resources(app);
    engine::voxel::initialize_voxel_world_resources(app);
}
