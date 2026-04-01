#![allow(warnings)]
use rogue_engine::{
    app::App, entity::ecs_world::ECSWorld, resource::ResourceBank, system::SystemErased,
};

use crate::{
    player::{
        player_camera_controller::PlayerCameraController, player_controller::PlayerController,
    },
    weapon::WeaponComponent,
};

pub mod player;
pub mod weapon;

pub fn register_game_types(ecs_world: &mut ECSWorld) {
    ecs_world.register_game_component::<PlayerCameraController>();
    ecs_world.register_game_component::<PlayerController>();
    ecs_world.register_game_component::<WeaponComponent>();
}

pub fn add_init_resources(app: &mut App) {}

pub fn on_init(rb: &ResourceBank) {}

pub fn on_update(rb: &ResourceBank) {
    rb.run_system(PlayerCameraController::on_update);
    rb.run_system(PlayerController::on_update);
}

pub fn on_fixed_update(rb: &ResourceBank) {
    rb.run_system(PlayerController::on_fixed_update);
}
