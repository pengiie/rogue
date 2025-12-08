use std::time::Duration;

use crate::{app::App, game::player_controller::PlayerController};

pub fn on_game_physics_update(app: &App) {
    app.run_system(PlayerController::on_physics_update);
}

pub fn on_game_update(app: &App) {
    app.run_system(PlayerController::on_update);
}
