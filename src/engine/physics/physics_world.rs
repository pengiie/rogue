use rogue_macros::Resource;

use crate::engine::{ecs::ecs_world::ECSWorld, resource::ResMut};

#[derive(Resource)]
pub struct PhysicsWorld {}

impl PhysicsWorld {
    pub fn new() -> Self {
        Self {}
    }

    pub fn do_physics_update(physics_world: ResMut<PhysicsWorld>, ecs_world: ResMut<ECSWorld>) {}
}
