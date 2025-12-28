use std::time::Duration;

use crate::{
    app::App,
    game::{
        camera_controller::CameraController, player_controller::PlayerController,
        spinning_platform::SpinningPlatform,
    },
};

pub fn on_game_physics_update(app: &App) {
    app.run_system(PlayerController::on_physics_update);
    app.run_system(SpinningPlatform::on_physics_update);
}

pub fn on_game_update(app: &App) {
    app.run_system(PlayerController::on_update);
}

pub fn on_game_post_physics_update(app: &App) {
    app.run_system(CameraController::on_update);
}
