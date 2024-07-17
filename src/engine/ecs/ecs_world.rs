use hecs::{Query, QueryBorrow, With};
use rogue_macros::Resource;

use crate::engine::system::SystemParam;

#[derive(Resource)]
pub struct ECSWorld {
    world: hecs::World,
}

impl ECSWorld {
    pub fn new() -> ECSWorld {
        ECSWorld {
            world: hecs::World::new(),
        }
    }

    pub fn world_mut(&mut self) -> &mut hecs::World {
        &mut self.world
    }
}

impl std::ops::Deref for ECSWorld {
    type Target = hecs::World;

    fn deref(&self) -> &Self::Target {
        &self.world
    }
}

impl std::ops::DerefMut for ECSWorld {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.world
    }
}
