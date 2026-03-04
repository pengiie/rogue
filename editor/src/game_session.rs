use rogue_engine::entity::ecs_world::Entity;
use rogue_macros::Resource;

#[derive(Resource)]
pub struct GameSession {
    pub game_camera: Option<Entity>,
}
