use rogue_macros::Resource;

use crate::engine::{entity::ecs_world::ECSWorld, resource::ResMut, window::time::Instant};

#[derive(Resource)]
pub struct PhysicsWorld {
    last_timestep: Instant,
}

impl PhysicsWorld {
    pub fn new() -> Self {
        Self {
            last_timestep: Instant::now(),
        }
    }

    pub fn do_physics_update(physics_world: ResMut<PhysicsWorld>, ecs_world: ResMut<ECSWorld>) {}
}
