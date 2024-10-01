use std::any::TypeId;

use hecs::{Entity, Query, QueryBorrow, QueryIter, With};
use rogue_macros::Resource;

use crate::{
    engine::{
        system::SystemParam,
        voxel::voxel::{VoxelModel, VoxelModelImpl},
    },
    game::player::player::Player,
};

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

    pub fn player_query<'a, Q: Query>(&'a self) -> PlayerQuery<Q> {
        PlayerQuery::new(
            self.query::<Q>().with::<&'a Player>() as QueryBorrow<'a, With<Q, &'a Player>>
        )
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

pub struct PlayerQuery<'a, Q: Query>(QueryBorrow<'a, With<Q, &'a Player>>);

impl<'a, Q: Query> PlayerQuery<'a, Q> {
    pub fn new(query: QueryBorrow<'a, With<Q, &'a Player>>) -> Self {
        Self(query)
    }

    pub fn player<'b>(&'b mut self) -> (Entity, Q::Item<'b>) {
        if self.0.iter().len() > 1 {
            panic!("More than one player spawned?");
        }
        self.0.iter().next().expect("Player was not spawned.")
    }
}
