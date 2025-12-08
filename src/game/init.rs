use crate::{
    engine::entity::ecs_world::ECSWorld,
    game::{camera_controller::CameraController, player_controller::PlayerController},
};

/// The graphics context and project has been initialized before this.
pub fn init_post_graphics(app: &mut crate::app::App) {}

pub fn register_game_components(ecs_world: &mut ECSWorld) {
    ecs_world.register_game_component::<PlayerController>();
    ecs_world.register_game_component::<CameraController>();
}
