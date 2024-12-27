use super::{player::player::Player, world::game_world::GameWorld};

/// The graphics `DeviceResource` has been inserted before this.
pub fn init_post_graphics(app: &mut crate::app::App) {
    app.insert_resource(GameWorld::new());
    app.run_system(GameWorld::spawn_player);
}
